//! Windows game detection via Toolhelp32 process enumeration and Win32 foreground window APIs.

use std::path::PathBuf;

use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, INVALID_HANDLE_VALUE, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
};

use crate::config::GameEntry;
use super::game_detector::{
    entry_matches, extract_game_from_path, is_clip_recorder, is_ignored_game_process, match_game_signature,
    strip_exe, DetectedGame, RunningApp,
};

/// Information about a running process.
#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub exe_name: String,
}

/// Lists running processes via Toolhelp32 snapshot.
pub fn list_processes() -> Vec<ProcessInfo> {
    let mut procs = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return procs;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                procs.push(ProcessInfo {
                    pid: entry.th32ProcessID,
                    parent_pid: entry.th32ParentProcessID,
                    exe_name,
                });
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    procs
}

/// Resolves the full executable path of a process via `QueryFullProcessImageNameW`.
pub fn process_exe_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok != 0 && size > 0 {
            Some(PathBuf::from(String::from_utf16_lossy(&buf[..size as usize])))
        } else {
            None
        }
    }
}

/// Information about the current foreground window.
pub struct ForegroundWindowInfo {
    pub pid: u32,
    pub title: String,
    pub exe_name: String,
    pub exe_path: Option<PathBuf>,
    pub is_fullscreen: bool,
}

/// Checks if a window covers its whole monitor.
fn window_covers_monitor(hwnd: HWND) -> bool {
    unsafe {
        let mut win_rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut win_rect) == 0 {
            return false;
        }
        if win_rect.right <= win_rect.left || win_rect.bottom <= win_rect.top {
            return false;
        }

        let hmonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if hmonitor.is_null() {
            return false;
        }

        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmonitor, &mut mi) == 0 {
            return false;
        }

        win_rect.left <= mi.rcMonitor.left
            && win_rect.top <= mi.rcMonitor.top
            && win_rect.right >= mi.rcMonitor.right
            && win_rect.bottom >= mi.rcMonitor.bottom
    }
}

/// Retrieves details about the active foreground window.
pub fn get_foreground_window_info() -> Option<ForegroundWindowInfo> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 || pid == std::process::id() {
            return None;
        }

        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
        let title = if len > 0 {
            String::from_utf16_lossy(&title_buf[..len as usize])
        } else {
            String::new()
        };

        let exe_path = process_exe_path(pid);
        let exe_name = exe_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|f| f.to_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let is_fullscreen = window_covers_monitor(hwnd);

        Some(ForegroundWindowInfo {
            pid,
            title,
            exe_name,
            exe_path,
            is_fullscreen,
        })
    }
}

/// Detects the currently running game. Foreground games take priority over background ones.
pub fn detect_current_game(custom: &[GameEntry]) -> Option<DetectedGame> {
    let entries: Vec<&GameEntry> = custom.iter().filter(|g| usable(g)).collect();
    custom_in_foreground(&entries)
        .or_else(raw_foreground)
        .or_else(|| {
            running_custom(&entries).map(|(name, pid)| DetectedGame {
                name,
                is_focused: false,
                pid: Some(pid),
            })
        })
        .or_else(raw_running)
        .map(|mut g| {
            g.name = crate::hardware::steam::sanitize_folder_name(&g.name);
            g
        })
}

/// Matches the foreground window against user-added games.
fn custom_in_foreground(entries: &[&GameEntry]) -> Option<DetectedGame> {
    if entries.is_empty() {
        return None;
    }
    let f = get_foreground_window_info()?;
    // UWP games run hosted in ApplicationFrameHost; allow matching them by window title only.
    let is_uwp = strip_exe(&f.exe_name).eq_ignore_ascii_case("applicationframehost");
    if never_a_game(&f.exe_name) && !is_uwp {
        return None;
    }
    let path_str = f.exe_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let (comm, path) = if is_uwp {
        ("", "")
    } else {
        (f.exe_name.as_str(), path_str.as_str())
    };
    let e = entries.iter().find(|e| entry_matches(e.detect_word(), comm, path, "", &f.title))?;
    Some(DetectedGame {
        name: e.name.clone(),
        is_focused: true,
        pid: Some(f.pid),
    })
}

/// Searches running processes for a user-added game.
fn running_custom(entries: &[&GameEntry]) -> Option<(String, u32)> {
    if entries.is_empty() {
        return None;
    }
    let me = std::process::id();
    for proc in list_processes() {
        if proc.pid == me || proc.pid == 0 || never_a_game(&proc.exe_name) {
            continue;
        }
        let exe_path_str = process_exe_path(proc.pid)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        if let Some(e) = entries.iter().find(|e| {
            entry_matches(e.detect_word(), &proc.exe_name, &exe_path_str, "", "")
        }) {
            return Some((e.name.clone(), proc.pid));
        }
    }
    None
}

/// Built-in game detection on the foreground window.
fn raw_foreground() -> Option<DetectedGame> {
    let f = get_foreground_window_info()?;
    if !is_ignored_game_process(&f.exe_name) {
        // Known game signature match on foreground window
        if let Some(game_name) = match_game_signature(&f.exe_name, "", &f.title, Some(f.pid)) {
            return Some(DetectedGame {
                name: game_name,
                is_focused: true,
                pid: Some(f.pid),
            });
        }

        // Generic path root rule for fullscreen foreground window
        if f.is_fullscreen {
            if let Some(path_str) = f.exe_path.as_ref().and_then(|p| p.to_str()) {
                if let Some(folder_name) = extract_game_from_path(path_str) {
                    return Some(DetectedGame {
                        name: folder_name,
                        is_focused: true,
                        pid: Some(f.pid),
                    });
                }
            }
        }
    }
    None
}

/// Built-in game detection scanning running processes when foreground is not a game.
fn raw_running() -> Option<DetectedGame> {
    let me = std::process::id();
    for proc in list_processes() {
        if proc.pid == me || proc.pid == 0 || is_ignored_game_process(&proc.exe_name) {
            continue;
        }
        if let Some(game_name) = match_game_signature(&proc.exe_name, "", "", Some(proc.pid)) {
            return Some(DetectedGame {
                name: game_name,
                is_focused: false,
                pid: Some(proc.pid),
            });
        }
    }
    None
}

/// Whether a single user-added game is running right now.
pub fn is_running(entry: &GameEntry) -> bool {
    usable(entry) && running_custom(&[entry]).is_some()
}

/// Process of the foreground window when that is a game. Stricter than `detect_current_game`
/// about titles: a browser tab named after a game is not that game.
pub fn foreground_game_pid(custom: &[GameEntry]) -> Option<u32> {
    let f = get_foreground_window_info()?;
    if never_a_game(&f.exe_name) {
        return None;
    }
    let path = f.exe_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let java = matches!(f.exe_name.to_ascii_lowercase().as_str(), "java.exe" | "javaw.exe");
    let title = if f.is_fullscreen || java { f.title.as_str() } else { "" };
    let is_game = custom
        .iter()
        .any(|e| usable(e) && entry_matches(e.detect_word(), &f.exe_name, &path, "", title))
        || match_game_signature(&f.exe_name, "", title, Some(f.pid)).is_some()
        || (f.is_fullscreen && extract_game_from_path(&path).is_some());
    is_game.then_some(f.pid)
}

/// HMONITOR the game in the foreground is on.
pub fn foreground_game_monitor(custom: &[GameEntry]) -> Option<isize> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if foreground_game_pid(custom)? != pid {
        return None;
    }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    (!monitor.is_null()).then_some(monitor as isize)
}

/// Collects running applications for the picker.
struct AppCollector {
    apps: Vec<RunningApp>,
    /// Game picker: hide launchers, browsers and chat apps. The audio app picker wants those.
    games_only: bool,
}

/// Windows' own shell windows (text input, Start, search...), never worth a track or a game entry.
fn is_shell_process(exe_name: &str) -> bool {
    let stem = exe_name.trim_end_matches(".exe").trim_end_matches(".EXE").to_ascii_lowercase();
    matches!(
        stem.as_str(),
        "explorer" | "textinputhost" | "applicationframehost" | "shellexperiencehost"
            | "startmenuexperiencehost" | "searchhost" | "searchapp" | "lockapp"
    ) || stem == "lapse"
}

/// Launchers, background recorders, and Windows shell processes that are never games.
fn never_a_game(exe_name: &str) -> bool {
    is_ignored_game_process(exe_name) || is_shell_process(exe_name)
}

/// Whether an entry of the games list can find a game. The picker used to offer these always
/// running programs; "Medal" would still match its helpers by the folder they are in.
fn usable(entry: &GameEntry) -> bool {
    let word = entry.detect_word();
    !word.is_empty() && !is_shell_process(word) && !is_clip_recorder(word)
}

unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let collector = &mut *(lparam as *mut AppCollector);

    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }

    if !GetWindow(hwnd, GW_OWNER).is_null() {
        return 1;
    }

    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if (ex_style & WS_EX_TOOLWINDOW) != 0 {
        return 1;
    }

    let mut rect: RECT = std::mem::zeroed();
    if GetWindowRect(hwnd, &mut rect) == 0 || rect.right <= rect.left || rect.bottom <= rect.top {
        return 1;
    }

    let len = GetWindowTextLengthW(hwnd);
    if len == 0 {
        return 1;
    }

    let mut title_buf = vec![0u16; (len + 1) as usize];
    let read_len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
    let title = if read_len > 0 {
        String::from_utf16_lossy(&title_buf[..read_len as usize]).trim().to_string()
    } else {
        String::new()
    };
    if title.is_empty() {
        return 1;
    }

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 || pid == std::process::id() {
        return 1;
    }

    let exe_path = process_exe_path(pid);
    let exe_name = exe_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|f| f.to_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let skip = if collector.games_only { never_a_game(&exe_name) } else { is_shell_process(&exe_name) };
    if skip {
        return 1;
    }

    let stem = exe_name
        .trim_end_matches(".exe")
        .trim_end_matches(".EXE")
        .to_string();
    if stem.is_empty() {
        return 1;
    }

    let path_str = exe_path.as_ref().and_then(|p| p.to_str()).unwrap_or("");
    let name = match_game_signature(&exe_name, "", &title, Some(pid))
        .or_else(|| extract_game_from_path(path_str))
        .unwrap_or_else(|| {
            if collector.games_only {
                title.clone()
            } else {
                // Browser and chat window titles change with every tab; the program name doesn't.
                let mut c = stem.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
            }
        });

    let lower_path = path_str.to_ascii_lowercase();
    let steam = lower_path.contains("steamapps/common") || lower_path.contains(r"steamapps\common");

    if !collector.apps.iter().any(|a| a.name.eq_ignore_ascii_case(&name) || a.matcher.eq_ignore_ascii_case(&stem)) {
        collector.apps.push(RunningApp {
            name,
            matcher: stem,
            steam,
        });
    }

    1
}

/// Programs on Windows with a visible top-level window and a readable name.
pub fn running_apps() -> Vec<RunningApp> {
    collect_windows(true)
}

/// Every program with an open window (browsers and chat apps included), for picking an app
/// to give its own audio track.
pub fn open_programs() -> Vec<RunningApp> {
    collect_windows(false)
}

fn collect_windows(games_only: bool) -> Vec<RunningApp> {
    let mut collector = AppCollector { apps: Vec::new(), games_only };
    unsafe {
        EnumWindows(Some(enum_windows_callback), &mut collector as *mut _ as LPARAM);
    }
    collector.apps.sort_by(|a, b| {
        b.steam.cmp(&a.steam).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    collector.apps
}
