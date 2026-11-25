//! Tray icon for Lapse.
//!
//! On Linux, provides a StatusNotifierItem (D-Bus) tray via `ksni`.
//! On Windows, provides a native Win32 system tray icon with a hidden top-level window.

use crate::recorder::Recorder;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayAction {
    Show,
    SaveReplay,
    ToggleRecord,
    OpenFolder,
    Quit,
}

const MENU: &[Option<(&str, TrayAction)>] = &[
    Some(("Show Lapse", TrayAction::Show)),
    None,
    Some(("Save Replay Now", TrayAction::SaveReplay)),
    Some(("Start / Stop Recording", TrayAction::ToggleRecord)),
    Some(("Open Clips Folder", TrayAction::OpenFolder)),
    None,
    Some(("Quit Lapse", TrayAction::Quit)),
];

fn run(recorder: &Arc<Mutex<Recorder>>, action: TrayAction) {
    match action {
        TrayAction::Show => show_window(),
        TrayAction::SaveReplay => {
            crate::daemon::handle_hotkey(recorder, crate::hotkeys::HotkeyEvent::SaveReplay);
        }
        TrayAction::ToggleRecord => {
            crate::daemon::handle_hotkey(recorder, crate::hotkeys::HotkeyEvent::ToggleRecord);
        }
        TrayAction::OpenFolder => {
            if let Ok(cfg) = crate::config::Config::load() {
                let _ = open::that(&cfg.save_path);
            }
        }
        TrayAction::Quit => {
            if let Ok(mut rec) = recorder.lock() {
                let _ = rec.stop();
            }
            let _ = std::fs::remove_file(crate::ipc::socket_path());
            crate::gui::single_instance::close_window();
            std::process::exit(0);
        }
    }
}

/// Starts the GUI; a second window is refused there and raises the open one instead.
fn show_window() {
    let Ok(exe) = std::env::current_exe() else { return };
    if let Ok(mut child) = std::process::Command::new(exe).arg("--gui").spawn() {
        // Reap it so a closed window doesn't stay behind as a zombie of the daemon.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(target_os = "linux")]
const ICON_NAME: &str = "lapse-symbolic";
#[cfg(target_os = "linux")]
const SYMBOLIC_SVG: &[u8] = include_bytes!("../assets/lapse-symbolic.svg");
#[cfg(target_os = "linux")]
const FALLBACK_PNG: &[u8] = include_bytes!("../assets/tray.png");

#[cfg(target_os = "linux")]
struct LapseTray {
    recorder: Arc<Mutex<Recorder>>,
    icon_dir: Option<PathBuf>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LapseTray {
    fn id(&self) -> String {
        "lapse".into()
    }

    fn title(&self) -> String {
        "Lapse".into()
    }

    fn icon_name(&self) -> String {
        ICON_NAME.into()
    }

    fn icon_theme_path(&self) -> String {
        self.icon_dir.as_ref().map(|d| d.to_string_lossy().to_string()).unwrap_or_default()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        fallback_pixmap().into_iter().collect()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "Lapse".into(), ..Default::default() }
    }

    /// Left click opens (or raises) the window.
    fn activate(&mut self, _x: i32, _y: i32) {
        show_window();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};
        MENU.iter()
            .map(|entry| match entry {
                Some((label, action)) => {
                    let action = *action;
                    StandardItem {
                        label: (*label).into(),
                        activate: Box::new(move |t: &mut Self| {
                            run(&t.recorder, action);
                        }),
                        ..Default::default()
                    }
                    .into()
                }
                None => MenuItem::Separator,
            })
            .collect()
    }
}

/// Writes the symbolic icon where the panel can find it without an installed
/// icon theme: as a loose file and inside a hicolor layout.
#[cfg(target_os = "linux")]
fn install_icon() -> Option<PathBuf> {
    let dir = directories::ProjectDirs::from("com", "lapse", "lapse")?.data_dir().join("icons");
    let file = format!("{}.svg", ICON_NAME);
    for target in [dir.join(&file), dir.join("hicolor/scalable/apps").join(&file)] {
        std::fs::create_dir_all(target.parent()?).ok()?;
        if std::fs::read(&target).ok().as_deref() != Some(SYMBOLIC_SVG) {
            std::fs::write(&target, SYMBOLIC_SVG).ok()?;
        }
    }
    Some(dir)
}

/// The white PNG as ARGB32 in network byte order.
#[cfg(target_os = "linux")]
fn fallback_pixmap() -> Option<ksni::Icon> {
    let img = image::load_from_memory(FALLBACK_PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    let data = img.pixels().flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
    Some(ksni::Icon { width: width as i32, height: height as i32, data })
}

#[cfg(target_os = "linux")]
pub fn setup_and_run(recorder: Arc<Mutex<Recorder>>) -> anyhow::Result<()> {
    use ksni::blocking::TrayMethods;
    let tray = LapseTray { recorder, icon_dir: install_icon() };
    // Flatpak can't own the well-known D-Bus name the spec asks for.
    let sandboxed = std::env::var_os("FLATPAK_ID").is_some();
    match tray.disable_dbus_name(sandboxed).spawn() {
        // The tray service runs on its own thread; keep the daemon's main thread alive.
        Ok(_handle) => loop {
            std::thread::park();
        },
        Err(e) => {
            eprintln!("Warning: no system tray ({}). Continuing without tray.", e);
            loop {
                std::thread::park();
            }
        }
    }
}

#[cfg(windows)]
mod win32 {
    use super::*;
    use std::cell::RefCell;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, LoadIconW,
        PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
        TrackPopupMenu, TranslateMessage, HICON, ICONINFO, IDI_APPLICATION, MF_SEPARATOR,
        MF_STRING, MSG, SM_CXSMICON, SM_CYSMICON, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
        WINDOW_EX_STYLE, WM_APP, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
        WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };

    const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
    const TRAY_ICON_ID: u32 = 1;

    struct TrayState {
        recorder: Arc<Mutex<Recorder>>,
        icon: HICON,
        taskbar_created_msg: u32,
    }

    thread_local! {
        static TRAY_STATE: RefCell<Option<TrayState>> = const { RefCell::new(None) };
    }

    unsafe fn create_tray_icon() -> HICON {
        try_create_icon().unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default())
    }

    unsafe fn try_create_icon() -> anyhow::Result<HICON> {
        let raw = include_bytes!("../assets/icon.png");
        let img = image::load_from_memory(raw)?.into_rgba8();
        let cx = GetSystemMetrics(SM_CXSMICON);
        let cy = GetSystemMetrics(SM_CYSMICON);
        if cx <= 0 || cy <= 0 {
            anyhow::bail!("invalid icon metrics");
        }
        let resized = image::imageops::resize(
            &img,
            cx as u32,
            cy as u32,
            image::imageops::FilterType::Lanczos3,
        );

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: cx,
                biHeight: -cy,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbm_color = CreateDIBSection(
            None,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?;
        if bits.is_null() {
            let _ = DeleteObject(hbm_color.into());
            anyhow::bail!("CreateDIBSection bits null");
        }

        let pixel_count = (cx * cy) as usize;
        let bgra_slice = std::slice::from_raw_parts_mut(bits as *mut u8, pixel_count * 4);
        for (src, dst) in resized.as_raw().chunks_exact(4).zip(bgra_slice.chunks_exact_mut(4)) {
            dst[0] = src[2]; // B
            dst[1] = src[1]; // G
            dst[2] = src[0]; // R
            dst[3] = src[3]; // A
        }

        let row_bytes = ((cx + 15) / 16) * 2;
        let mask_bytes = vec![0u8; (row_bytes * cy) as usize];
        let hbm_mask = CreateBitmap(cx, cy, 1, 1, Some(mask_bytes.as_ptr() as *const _));
        if hbm_mask.is_invalid() {
            let _ = DeleteObject(hbm_color.into());
            anyhow::bail!("CreateBitmap failed");
        }

        let icon_info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm_color,
        };
        let hicon_res = CreateIconIndirect(&icon_info);
        let _ = DeleteObject(hbm_color.into());
        let _ = DeleteObject(hbm_mask.into());
        let hicon = hicon_res?;
        Ok(hicon)
    }

    unsafe fn make_notify_icon_data(hwnd: HWND, icon: HICON) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY_CALLBACK,
            hIcon: icon,
            ..Default::default()
        };
        let tip = "Lapse";
        for (i, c) in tip.encode_utf16().enumerate() {
            if i < nid.szTip.len() - 1 {
                nid.szTip[i] = c;
            }
        }
        nid
    }

    unsafe fn add_tray_icon(hwnd: HWND, icon: HICON) -> bool {
        let nid = make_notify_icon_data(hwnd, icon);
        Shell_NotifyIconW(NIM_ADD, &nid).as_bool()
    }

    unsafe fn delete_tray_icon(hwnd: HWND) {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }

    unsafe fn show_context_menu(hwnd: HWND, recorder: &Arc<Mutex<Recorder>>) {
        let Ok(menu) = CreatePopupMenu() else { return };
        for (i, entry) in MENU.iter().enumerate() {
            let id = i + 1;
            match entry {
                Some((label, _)) => {
                    let wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
                    let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(wide.as_ptr()));
                }
                None => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
            }
        }

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        if cmd.0 > 0 {
            let idx = (cmd.0 as usize) - 1;
            if let Some(Some((_, action))) = MENU.get(idx) {
                let action = *action;
                if action == TrayAction::Quit {
                    delete_tray_icon(hwnd);
                }
                run(recorder, action);
            }
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let mut taskbar_msg = 0;
        let mut recorder = None;
        let mut icon = HICON::default();

        TRAY_STATE.with(|state| {
            if let Some(s) = state.borrow().as_ref() {
                taskbar_msg = s.taskbar_created_msg;
                recorder = Some(s.recorder.clone());
                icon = s.icon;
            }
        });

        if msg == WM_TRAY_CALLBACK {
            let event = (lparam.0 as usize as u32) & 0xffff;
            match event {
                WM_LBUTTONUP => {
                    if let Some(ref rec) = recorder {
                        run(rec, TrayAction::Show);
                    }
                    return LRESULT(0);
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    if let Some(ref rec) = recorder {
                        show_context_menu(hwnd, rec);
                    }
                    return LRESULT(0);
                }
                _ => {}
            }
        } else if taskbar_msg != 0 && msg == taskbar_msg {
            add_tray_icon(hwnd, icon);
            return LRESULT(0);
        } else if msg == WM_DESTROY {
            delete_tray_icon(hwnd);
            PostQuitMessage(0);
            return LRESULT(0);
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    pub fn setup_and_run(recorder: Arc<Mutex<Recorder>>) -> anyhow::Result<()> {
        match setup_tray(recorder) {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("Warning: no system tray ({}). Continuing without tray.", e);
                loop {
                    std::thread::park();
                }
            }
        }
    }

    fn setup_tray(recorder: Arc<Mutex<Recorder>>) -> anyhow::Result<()> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)
                .map_err(|e| anyhow::anyhow!("GetModuleHandleW failed: {e}"))?
                .into();
            let class_name = w!("LapseTrayWindow");
            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance,
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassW(&wnd_class);

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                w!("Lapse"),
                WS_OVERLAPPEDWINDOW,
                windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
                windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
                windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
                windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
                None,
                None,
                Some(instance),
                None,
            )?;

            let icon = create_tray_icon();
            let taskbar_created_msg = RegisterWindowMessageW(w!("TaskbarCreated"));

            TRAY_STATE.with(|s| {
                *s.borrow_mut() = Some(TrayState {
                    recorder,
                    icon,
                    taskbar_created_msg,
                });
            });

            // When Lapse starts with Windows the taskbar may not exist yet: keep trying for a
            // minute, after that the "TaskbarCreated" message re-adds the icon.
            let mut added = add_tray_icon(hwnd, icon);
            for _ in 0..30 {
                if added {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
                added = add_tray_icon(hwnd, icon);
            }
            if !added {
                eprintln!("Warning: tray icon not added yet; waiting for the taskbar.");
            }

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            loop {
                std::thread::park();
            }
        }
    }
}

#[cfg(windows)]
pub use win32::setup_and_run;

#[cfg(not(any(target_os = "linux", windows)))]
pub fn setup_and_run(_recorder: Arc<Mutex<Recorder>>) -> anyhow::Result<()> {
    loop {
        std::thread::park();
    }
}
