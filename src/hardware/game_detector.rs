
use crate::config::GameEntry;
use super::procs;

#[derive(Clone, Debug, PartialEq)]
pub struct DetectedGame {
    pub name: String,
    pub is_focused: bool,
    pub pid: Option<u32>,
}

const STEAM_APP_MAP: &[(u32, &str)] = &[
    (730, "Counter-Strike 2"),
    (220, "Half-Life 2"),
    (362890, "Black Mesa"),
    (753640, "Outer Wilds"),
    (570, "Dota 2"),
    (1091500, "Cyberpunk 2077"),
    (1245620, "Elden Ring"),
    (292030, "The Witcher 3"),
    (271590, "Grand Theft Auto V"),
    (252490, "Rust"),
    (1172470, "Apex Legends"),
    (252950, "Rocket League"),
    (1426210, "Deadlock"),
    (1145360, "Hades"),
    (413150, "Stardew Valley"),
    (550, "Left 4 Dead 2"),
    (440, "Team Fortress 2"),
    (892970, "Valheim"),
    (105600, "Terraria"),
    (289070, "Civilization VI"),
];

const PROCESS_GAME_MAP: &[(&str, &str)] = &[
    ("cs2", "Counter-Strike 2"),
    ("hl2_linux", "Half-Life 2"),
    ("hl2.exe", "Half-Life 2"),
    ("hl2", "Half-Life 2"),
    ("bms.exe", "Black Mesa"),
    ("bms", "Black Mesa"),
    ("outerwilds.exe", "Outer Wilds"),
    ("outerwilds", "Outer Wilds"),
    ("dota2", "Dota 2"),
    ("deadlock.exe", "Deadlock"),
    ("deadlock", "Deadlock"),
    ("r5apex.exe", "Apex Legends"),
    ("apex_legends", "Apex Legends"),
    ("valorant.exe", "Valorant"),
    ("rocketleague.exe", "Rocket League"),
    ("overwatch.exe", "Overwatch 2"),
    ("leagueclient.exe", "League of Legends"),
    ("minecraft", "Minecraft"),
    ("osu!.exe", "osu!"),
    ("osu!", "osu!"),
    ("gta5.exe", "Grand Theft Auto V"),
    ("cyberpunk2077.exe", "Cyberpunk 2077"),
    ("eldenring.exe", "Elden Ring"),
];

/// Detects the running game. Games the user added (`custom`) win over built-in detection.
pub fn detect_current_game(custom: &[GameEntry]) -> Option<DetectedGame> {
    detect_custom(custom).or_else(detect_raw).map(|mut g| {
        g.name = super::steam::sanitize_folder_name(&g.name);
        g
    })
}

fn detect_raw() -> Option<DetectedGame> {
    let focused = get_active_window_info();

    // Steam games carry their app id in the environment; resolve any installed game by name.
    if let Some(app_id) = focused.as_ref().and_then(|f| f.pid).and_then(super::steam::app_id_of) {
        if let Some(name) = super::steam::app_name(app_id) {
            if !super::steam::is_tool(app_id, &name) {
                return Some(DetectedGame { name, is_focused: true, pid: focused.as_ref().and_then(|f| f.pid) });
            }
        }
    }
    if let Some((_, name, pid)) = super::steam::running_game() {
        return Some(DetectedGame { name, is_focused: false, pid: Some(pid) });
    }

    if let Some(focused) = focused {
        if let Some(game_name) = match_game_signature(&focused.comm, &focused.class, &focused.title, focused.pid) {
            return Some(DetectedGame {
                name: game_name,
                is_focused: true,
                pid: focused.pid,
            });
        }
    }

    if let Some((game_name, pid)) = scan_running_game_processes() {
        return Some(DetectedGame {
            name: game_name,
            is_focused: false,
            pid: Some(pid),
        });
    }

    None
}

fn read_cmdline(pid: u32) -> String {
    procs::cmdline(pid)
}

/// Processes that mention every game name and must never count as one.
fn is_ignored_process(comm: &str) -> bool {
    let c = comm.to_ascii_lowercase();
    c == "lapse" || c.starts_with("gpu-screen-rec") || matches!(c.as_str(), "grep" | "rg" | "pgrep" | "ps" | "top" | "htop")
}

/// True when `matcher` appears in the process name, command line or window class/title.
pub fn entry_matches(matcher: &str, comm: &str, cmdline: &str, class: &str, title: &str) -> bool {
    let m = matcher.trim().to_lowercase();
    if m.is_empty() {
        return false;
    }
    [comm, cmdline, class, title].iter().any(|s| s.to_lowercase().contains(&m))
}

/// First user-added game that is running, preferring the focused window.
fn detect_custom(custom: &[GameEntry]) -> Option<DetectedGame> {
    let entries: Vec<&GameEntry> = custom.iter().filter(|g| !g.detect_word().is_empty()).collect();
    if entries.is_empty() {
        return None;
    }
    if let Some(f) = get_active_window_info() {
        let cmdline = f.pid.map(read_cmdline).unwrap_or_default();
        if let Some(e) = entries.iter().find(|e| entry_matches(e.detect_word(), &f.comm, &cmdline, &f.class, &f.title)) {
            return Some(DetectedGame { name: e.name.clone(), is_focused: true, pid: f.pid });
        }
    }
    running_custom(&entries).map(|(name, pid)| DetectedGame { name, is_focused: false, pid: Some(pid) })
}

fn running_custom(entries: &[&GameEntry]) -> Option<(String, u32)> {
    let me = std::process::id();
    for pid in procs::pids() {
        if pid == me {
            continue;
        }
        let comm = read_proc_comm(pid);
        if comm.is_empty() || is_ignored_process(&comm) {
            continue;
        }
        let cmdline = read_cmdline(pid);
        if let Some(e) = entries.iter().find(|e| entry_matches(e.detect_word(), &comm, &cmdline, "", "")) {
            return Some((e.name.clone(), pid));
        }
    }
    None
}

/// Whether a single user-added game is running right now (for the settings list).
pub fn is_running(entry: &GameEntry) -> bool {
    !entry.detect_word().is_empty() && running_custom(&[entry]).is_some()
}

struct WindowInfo {
    comm: String,
    class: String,
    title: String,
    pid: Option<u32>,
}

fn get_active_window_info() -> Option<WindowInfo> {
    
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        if let Ok(output) = procs::host_command("hyprctl").args(["activewindow", "-j"]).output() {
            if output.status.success() {
                if let Ok(json_str) = String::from_utf8(output.stdout) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        let class = val.get("class").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let title = val.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let pid = val.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32);
                        let comm = pid.map(read_proc_comm).unwrap_or_default();
                        return Some(WindowInfo { comm, class, title, pid });
                    }
                }
            }
        }
    }

    if std::env::var_os("DISPLAY").is_some() {
        if let Ok(out) = procs::host_command("xprop").args(["-root", "_NET_ACTIVE_WINDOW"]).output() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(id_str) = s.split('#').nth(1) {
                let win_id = id_str.trim();
                if win_id != "0x0" && !win_id.is_empty() {
                    let mut info = WindowInfo {
                        comm: String::new(),
                        class: String::new(),
                        title: String::new(),
                        pid: None,
                    };

                    if let Ok(prop_out) = procs::host_command("xprop")
                        .args(["-id", win_id, "_NET_WM_PID", "WM_CLASS", "_NET_WM_NAME"])
                        .output()
                    {
                        let prop_s = String::from_utf8_lossy(&prop_out.stdout);
                        for line in prop_s.lines() {
                            if line.contains("_NET_WM_PID") {
                                if let Some(p) = line.split('=').nth(1) {
                                    info.pid = p.trim().parse::<u32>().ok();
                                }
                            } else if line.contains("WM_CLASS") {
                                info.class = xprop_value(line);
                            } else if line.contains("_NET_WM_NAME") {
                                info.title = xprop_value(line);
                            }
                        }
                    }

                    if let Some(p) = info.pid {
                        info.comm = read_proc_comm(p);
                    }
                    return Some(info);
                }
            }
        }
    }

    None
}

/// `_NET_WM_NAME(UTF8_STRING) = "Game Title"` -> `Game Title`
fn xprop_value(line: &str) -> String {
    line.split_once('=')
        .map(|(_, v)| v.trim().trim_matches('"').replace("\", \"", ", "))
        .unwrap_or_default()
}

fn read_proc_comm(pid: u32) -> String {
    procs::comm(pid)
}

fn read_steam_app_id(pid: u32) -> Option<u32> {
    procs::steam_app_id(pid)
}

fn match_game_signature(comm: &str, class: &str, title: &str, pid: Option<u32>) -> Option<String> {
    
    if let Some(p) = pid {
        if let Some(app_id) = read_steam_app_id(p) {
            for &(id, name) in STEAM_APP_MAP {
                if id == app_id {
                    return Some(name.to_string());
                }
            }
        }
    }

    let lower_comm = comm.to_lowercase();
    let lower_class = class.to_lowercase();
    let lower_title = title.to_lowercase();

    for &(bin_name, game_title) in PROCESS_GAME_MAP {
        if lower_comm == bin_name || lower_class.contains(bin_name) || lower_title.contains(&game_title.to_lowercase()) {
            return Some(game_title.to_string());
        }
    }

    if lower_comm.contains("wine") || lower_comm.contains("proton") || lower_class.contains("steam_app") {
        if !lower_title.is_empty() {
            let clean_title = title.trim_matches('"').trim();
            if !clean_title.is_empty() {
                return Some(clean_title.to_string());
            }
        }
    }

    None
}

/// Minecraft runs as `java`; its command line names the game (any launcher: vanilla,
/// Prism, OneClient, Modrinth, ...). Servers don't count.
fn is_minecraft_client(cmdline: &str) -> bool {
    let c = cmdline.to_ascii_lowercase();
    c.contains("minecraft") && !c.contains("nogui") && !c.contains("minecraft_server") && !c.contains("minecraft-server")
}

fn scan_running_game_processes() -> Option<(String, u32)> {
    for pid in procs::pids() {
        let lower_comm = read_proc_comm(pid).to_lowercase();
        if let Some(app_id) = read_steam_app_id(pid) {
            if let Some(&(_, name)) = STEAM_APP_MAP.iter().find(|(id, _)| *id == app_id) {
                return Some((name.to_string(), pid));
            }
        }
        if let Some(&(_, game_title)) = PROCESS_GAME_MAP.iter().find(|(bin, _)| lower_comm == *bin) {
            return Some((game_title.to_string(), pid));
        }
        if lower_comm.starts_with("java") && is_minecraft_client(&read_cmdline(pid)) {
            return Some(("Minecraft".to_string(), pid));
        }
    }
    None
}

/// A program that could be a game, for "pick a running game" in the settings.
#[derive(Clone, Debug, PartialEq)]
pub struct RunningApp {
    pub name: String,
    /// Word that finds it again (process name, .exe name, ...), for `GameEntry::matcher`.
    pub matcher: String,
    /// It is a Steam game (Lapse detects those by itself).
    pub steam: bool,
}

/// Desktop parts that draw with the GPU but are never games.
fn is_desktop_process(comm: &str) -> bool {
    const SKIP: &[&str] = &[
        "gnome-shell", "mutter-x11-fram", "xwayland", "plasmashell", "kwin_wayland", "kwin_x11", "hyprland", "sway",
        "niri", "river", "labwc", "wayfire", "weston", "cosmic-comp", "cosmic-panel", "waybar", "quickshell", "qs",
        "eww", "ags", "swaybg", "hyprpaper", "mako", "dunst", "swaync", "ibus-x11", "ibus-ui-gtk3", "fcitx5",
        "xdg-desktop-por", "gsd-xsettings", "gnome-software", "kded6", "kded5", "krunner", "steamwebhelper", "steam",
        "gjs", "xorg", "x", "polkit-gnome-au", "nm-applet", "blueman-applet", "pipewire", "wireplumber",
    ];
    let c = comm.to_ascii_lowercase();
    is_ignored_process(&c) || c.starts_with("gsr") || SKIP.contains(&c.as_str())
}

/// Everyday programs that use the GPU but aren't games.
fn is_common_app(comm: &str) -> bool {
    const APPS: &[&str] = &[
        "firefox", "firefox-bin", "librewolf", "floorp", "zen", "chrome", "chromium", "brave", "vivaldi-bin", "opera", "thorium",
        "electron", "code", "codium", "cursor", "zed", "discord", "vesktop", "legcord", "webcord", "spotify", "telegram-deskto",
        "signal-desktop", "slack", "teams-for-linux", "thunderbird", "obs", "kdenlive", "gimp", "krita", "inkscape", "mpv",
        "vlc", "celluloid", "totem", "nautilus", "dolphin", "thunar", "nemo", "kitty", "alacritty", "wezterm-gui", "foot",
        "ghostty", "konsole", "gnome-terminal-", "ptyxis", "blackbox", "evince", "okular", "loupe", "eog", "fdm",
        "lutris", "heroic", "bottles", "prismlauncher", "oneclient", "modrinth-app", "atlauncher", "mangohud",
    ];
    let c = comm.to_ascii_lowercase();
    c.starts_with("claude") || c.starts_with("cowork") || c.starts_with("disclaimer") || APPS.contains(&c.as_str())
}

/// A helper of a program already listed (browser tabs, Electron helpers): it runs
/// the same executable as its parent.
fn is_child_of_same_program(pid: u32) -> bool {
    let Some(ppid) = procs::ppid(pid) else { return false };
    match (procs::exe(pid), procs::exe(ppid)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}


/// Name and matcher from the command line: "Game.exe" under Wine, Minecraft under Java.
fn describe(comm: &str, cmdline: &str) -> (String, String) {
    if let Some(exe) = cmdline.split([' ', '\\', '/']).find(|w| w.to_ascii_lowercase().ends_with(".exe")) {
        let stem = exe.trim_end_matches(".exe").trim_end_matches(".EXE");
        return (stem.to_string(), stem.to_string());
    }
    if comm.starts_with("java") && is_minecraft_client(cmdline) {
        return ("Minecraft".into(), "minecraft".into());
    }
    let mut name = comm.to_string();
    if let Some(first) = name.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    (name, comm.to_string())
}

/// Programs of this user that draw with the GPU, Steam games first. Works on every
/// desktop, because it reads /proc instead of asking the window manager.
pub fn running_apps() -> Vec<RunningApp> {
    let mut out: Vec<RunningApp> = Vec::new();
    for pid in procs::pids() {
        let comm = read_proc_comm(pid);
        if comm.is_empty() || is_desktop_process(&comm) || is_common_app(&comm) || !procs::uses_gpu(pid) || is_child_of_same_program(pid) {
            continue;
        }
        let app = match super::steam::app_id_of(pid).and_then(|id| super::steam::app_name(id).map(|n| (id, n))) {
            Some((id, name)) if !super::steam::is_tool(id, &name) => RunningApp { name, matcher: comm.clone(), steam: true },
            _ => {
                let (name, matcher) = describe(&comm, &read_cmdline(pid));
                RunningApp { name, matcher, steam: false }
            }
        };
        if !out.iter().any(|a| a.name.eq_ignore_ascii_case(&app.name)) {
            out.push(app);
        }
    }
    out.sort_by(|a, b| b.steam.cmp(&a.steam).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_programs_get_readable_names() {
        assert_eq!(describe("wine64-preloade", "C:\\Games\\Hollow\\Hollow.exe -windowed"), ("Hollow".into(), "Hollow".into()));
        assert_eq!(describe("java", "/usr/bin/java -cp minecraft.jar net.minecraft.client.main.Main"), ("Minecraft".into(), "minecraft".into()));
        assert_eq!(describe("peak", "/games/PEAK/peak"), ("Peak".into(), "peak".into()));
        assert!(is_desktop_process("gnome-shell") && !is_desktop_process("firefox"));
        let oneclient = "/home/u/.local/share/oneclient/java/bin/java -Xmx4G -cp /x/libraries/net/minecraft/client/1.21.4/client.jar net.fabricmc.loader.impl.launch.knot.KnotClient --gameDir /x/instances/a";
        assert!(is_minecraft_client(oneclient));
        assert!(!is_minecraft_client("java -Xmx8G -jar minecraft_server.1.21.jar nogui"));
        // Typed by hand (no matcher): the name finds it.
        let typed = GameEntry { name: "Hollow Knight".into(), ..Default::default() };
        assert_eq!(typed.detect_word(), "Hollow Knight");
        let picked = GameEntry { name: "Minecraft".into(), matcher: "minecraft".into(), ..Default::default() };
        assert_eq!(picked.detect_word(), "minecraft");
    }

    #[test]
    fn custom_matcher_finds_java_games() {
        let cmd = "/usr/lib/jvm/java-21/bin/java -Xmx4G net.minecraft.client.main.Main --version 1.21";
        assert!(entry_matches("minecraft", "java", cmd, "", ""));
        assert!(entry_matches("Minecraft", "java", "", "", "Minecraft* 1.21"));
        assert!(!entry_matches("minecraft", "java", "java -jar server.jar", "", ""));
        assert!(!entry_matches("  ", "java", cmd, "", ""));
    }
}

/// What Lapse sees right now; also run inside a Flatpak sandbox to check the host
/// lookup: `cargo test print_detection -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn print_detection() {
    println!("flatpak: {}", procs::in_flatpak());
    println!("processes: {}", procs::pids().len());
    println!("game: {:?}", detect_current_game(&[]).map(|g| g.name));
    for app in running_apps() {
        println!("app: {} ({})", app.name, app.matcher);
    }
}
