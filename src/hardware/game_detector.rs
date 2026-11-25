
#[cfg(not(windows))]
use crate::config::GameEntry;
#[cfg(not(windows))]
use super::procs;

#[cfg(windows)]
pub use super::game_detector_windows::{detect_current_game, is_running, open_programs, running_apps};

#[derive(Clone, Debug, PartialEq)]
pub struct DetectedGame {
    pub name: String,
    pub is_focused: bool,
    pub pid: Option<u32>,
}

#[cfg(not(windows))]
pub const STEAM_APP_MAP: &[(u32, &str)] = &[
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

pub const PROCESS_GAME_MAP: &[(&str, &str)] = &[
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
    ("r5apex", "Apex Legends"),
    ("valorant.exe", "Valorant"),
    ("valorant", "Valorant"),
    ("valorant-win64-shipping", "Valorant"),
    ("rocketleague.exe", "Rocket League"),
    ("rocketleague", "Rocket League"),
    ("overwatch.exe", "Overwatch 2"),
    ("overwatch", "Overwatch 2"),
    ("leagueclient.exe", "League of Legends"),
    ("leagueclient", "League of Legends"),
    ("league of legends", "League of Legends"),
    ("minecraft", "Minecraft"),
    ("osu!.exe", "osu!"),
    ("osu!", "osu!"),
    ("gta5.exe", "Grand Theft Auto V"),
    ("gta5", "Grand Theft Auto V"),
    ("gta5_enhanced", "Grand Theft Auto V"),
    ("cyberpunk2077.exe", "Cyberpunk 2077"),
    ("cyberpunk2077", "Cyberpunk 2077"),
    ("eldenring.exe", "Elden Ring"),
    ("eldenring", "Elden Ring"),
    ("fc25.exe", "EA SPORTS FC 25"),
    ("fc25", "EA SPORTS FC 25"),
    ("fc24.exe", "EA SPORTS FC 24"),
    ("fc24", "EA SPORTS FC 24"),
    ("fifa23.exe", "FIFA 23"),
    ("fifa23", "FIFA 23"),
    ("fifa22.exe", "FIFA 22"),
    ("fifa22", "FIFA 22"),
    ("fortniteclient-win64-shipping", "Fortnite"),
    ("tslgame", "PUBG"),
    ("rainbowsix", "Rainbow Six Siege"),
    ("robloxplayerbeta", "Roblox"),
    ("sober", "Roblox"),
];

/// Strips `.exe` or `.EXE` from an executable name.
#[allow(dead_code)]
pub fn strip_exe(name: &str) -> &str {
    let s = name.trim();
    if s.len() >= 4 && s[s.len() - 4..].eq_ignore_ascii_case(".exe") {
        &s[..s.len() - 4]
    } else {
        s
    }
}

/// Known roots where Windows games are installed.
#[allow(dead_code)]
const GAME_PATH_ROOTS: &[&str] = &[
    "steamapps/common/",
    "epic games/",
    "ea games/",
    "electronic arts/",
    "riot games/",
    "ubisoft game launcher/games/",
    "xboxgames/",
    "gog games/",
];

/// Extracts the game folder name directly under a recognized game root folder.
#[allow(dead_code)]
pub fn extract_game_from_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();

    for &root in GAME_PATH_ROOTS {
        let mut search_from = 0;
        while let Some(rel_idx) = lower[search_from..].find(root) {
            let idx = search_from + rel_idx;
            if idx == 0 || normalized.as_bytes()[idx - 1] == b'/' {
                let start_of_folder = idx + root.len();
                let remaining = &normalized[start_of_folder..];
                if let Some(slash_pos) = remaining.find('/') {
                    let folder = remaining[..slash_pos].trim();
                    if !folder.is_empty() {
                        return Some(folder.to_string());
                    }
                }
            }
            search_from = idx + 1;
        }
    }
    None
}

/// Medal, another clip recorder, which stays in the background.
#[allow(dead_code)]
pub fn is_clip_recorder(name: &str) -> bool {
    matches!(strip_exe(name).to_ascii_lowercase().as_str(), "medal" | "medalencoder")
}

/// Common launchers, overlays, and system utilities that must not be counted as games.
#[allow(dead_code)]
pub fn is_ignored_game_process(comm: &str) -> bool {
    let stem = strip_exe(comm);
    let lower = stem.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return true;
    }
    if lower == "lapse" || lower.starts_with("lapse-") || lower.starts_with("lapse_") {
        return true;
    }
    is_clip_recorder(&lower) || matches!(
        lower.as_str(),
        "steam"
            | "steamwebhelper"
            | "epicgameslauncher"
            | "epicwebhelper"
            | "eadesktop"
            | "easervices"
            | "eaconnect_unreal"
            | "riotclientservices"
            | "riotclientux"
            | "discord"
            | "discordcanary"
            | "discordptb"
            | "vesktop"
            | "explorer"
            | "chrome"
            | "firefox"
            | "firefox-bin"
            | "msedge"
            | "edge"
            | "brave"
            | "opera"
            | "opera_gx"
            | "vivaldi"
            | "vivaldi-bin"
            | "librewolf"
            | "zen"
            | "waterfox"
            | "thorium"
            | "arc"
    )
}

/// Matches a process against known game signatures.
pub fn match_game_signature(comm: &str, class: &str, title: &str, _pid: Option<u32>) -> Option<String> {
    #[cfg(target_os = "linux")]
    if let Some(p) = _pid {
        if let Some(app_id) = read_steam_app_id(p) {
            for &(id, name) in STEAM_APP_MAP {
                if id == app_id {
                    return Some(name.to_string());
                }
            }
        }
    }

    let comm_stem = strip_exe(comm);
    let lower_comm = comm_stem.to_ascii_lowercase();
    let lower_class = class.to_ascii_lowercase();
    let lower_title = title.to_ascii_lowercase();

    if (lower_comm == "javaw" || lower_comm == "java") && lower_title.contains("minecraft") {
        return Some("Minecraft".to_string());
    }

    for &(bin_name, game_title) in PROCESS_GAME_MAP {
        let bin_stem = strip_exe(bin_name);
        if lower_comm == bin_stem
            || (!bin_stem.is_empty() && lower_class.contains(bin_stem))
            || lower_title.contains(&game_title.to_ascii_lowercase())
        {
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

/// True when `matcher` appears in the process name, command line or window class/title.
pub fn entry_matches(matcher: &str, comm: &str, cmdline: &str, class: &str, title: &str) -> bool {
    let m = matcher.trim().to_lowercase();
    if m.is_empty() {
        return false;
    }
    [comm, cmdline, class, title].iter().any(|s| s.to_lowercase().contains(&m))
}

#[cfg(not(windows))]
pub fn detect_current_game(custom: &[GameEntry]) -> Option<DetectedGame> {
    detect_custom(custom).or_else(detect_raw).map(|mut g| {
        g.name = super::steam::sanitize_folder_name(&g.name);
        g
    })
}

#[cfg(not(windows))]
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
        let title = if is_common_app(&focused.comm) { "" } else { &focused.title };
        if let Some(game_name) = match_game_signature(&focused.comm, &focused.class, title, focused.pid) {
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

#[cfg(not(windows))]
fn read_cmdline(pid: u32) -> String {
    procs::cmdline(pid)
}

#[cfg(not(windows))]
fn is_ignored_process(comm: &str) -> bool {
    let c = comm.to_ascii_lowercase();
    c == "lapse" || c.starts_with("gpu-screen-rec") || matches!(c.as_str(), "grep" | "rg" | "pgrep" | "ps" | "top" | "htop")
}

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
pub fn is_running(entry: &GameEntry) -> bool {
    !entry.detect_word().is_empty() && running_custom(&[entry]).is_some()
}

#[cfg(not(windows))]
struct WindowInfo {
    comm: String,
    class: String,
    title: String,
    pid: Option<u32>,
}

#[cfg(not(windows))]
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

#[cfg(not(windows))]
fn xprop_value(line: &str) -> String {
    line.split_once('=')
        .map(|(_, v)| v.trim().trim_matches('"').replace("\", \"", ", "))
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn read_proc_comm(pid: u32) -> String {
    procs::comm(pid)
}

#[cfg(not(windows))]
fn read_steam_app_id(pid: u32) -> Option<u32> {
    procs::steam_app_id(pid)
}

/// Minecraft runs as `java`; its command line names the game (any launcher: vanilla,
/// Prism, OneClient, Modrinth, ...). Servers don't count.
#[cfg(not(windows))]
pub fn is_minecraft_client(cmdline: &str) -> bool {
    let c = cmdline.to_ascii_lowercase();
    c.contains("minecraft") && !c.contains("nogui") && !c.contains("minecraft_server") && !c.contains("minecraft-server")
}

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
fn is_child_of_same_program(pid: u32) -> bool {
    let Some(ppid) = procs::ppid(pid) else { return false };
    match (procs::exe(pid), procs::exe(ppid)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

#[cfg(not(windows))]
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

#[cfg(not(windows))]
pub fn running_apps() -> Vec<RunningApp> {
    list_programs(true)
}

/// Every open program with a window (browsers and chat apps included), for picking an
/// app to give its own audio track.
#[cfg(not(windows))]
pub fn open_programs() -> Vec<RunningApp> {
    list_programs(false)
}

#[cfg(not(windows))]
fn list_programs(games_only: bool) -> Vec<RunningApp> {
    let mut out: Vec<RunningApp> = Vec::new();
    for pid in procs::pids() {
        let comm = read_proc_comm(pid);
        if comm.is_empty() || is_desktop_process(&comm) || (games_only && is_common_app(&comm)) || !procs::uses_gpu(pid) || is_child_of_same_program(pid) {
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

    #[test]
    fn known_game_signatures_match_exe_names() {
        assert_eq!(match_game_signature("FC25.exe", "", "", None), Some("EA SPORTS FC 25".into()));
        assert_eq!(match_game_signature("fc25", "", "", None), Some("EA SPORTS FC 25".into()));
        assert_eq!(match_game_signature("FC24.exe", "", "", None), Some("EA SPORTS FC 24".into()));
        assert_eq!(match_game_signature("FIFA23.exe", "", "", None), Some("FIFA 23".into()));
        assert_eq!(match_game_signature("FIFA22.exe", "", "", None), Some("FIFA 22".into()));
        assert_eq!(match_game_signature("VALORANT-Win64-Shipping.exe", "", "", None), Some("Valorant".into()));
        assert_eq!(match_game_signature("cs2.exe", "", "", None), Some("Counter-Strike 2".into()));
        assert_eq!(match_game_signature("cs2", "", "", None), Some("Counter-Strike 2".into()));
        assert_eq!(match_game_signature("FortniteClient-Win64-Shipping.exe", "", "", None), Some("Fortnite".into()));
        assert_eq!(match_game_signature("League of Legends.exe", "", "", None), Some("League of Legends".into()));
        assert_eq!(match_game_signature("dota2.exe", "", "", None), Some("Dota 2".into()));
        assert_eq!(match_game_signature("r5apex.exe", "", "", None), Some("Apex Legends".into()));
        assert_eq!(match_game_signature("GTA5.exe", "", "", None), Some("Grand Theft Auto V".into()));
        assert_eq!(match_game_signature("GTA5_Enhanced.exe", "", "", None), Some("Grand Theft Auto V".into()));
        assert_eq!(match_game_signature("RocketLeague.exe", "", "", None), Some("Rocket League".into()));
        assert_eq!(match_game_signature("Overwatch.exe", "", "", None), Some("Overwatch 2".into()));
        assert_eq!(match_game_signature("TslGame.exe", "", "", None), Some("PUBG".into()));
        assert_eq!(match_game_signature("RainbowSix.exe", "", "", None), Some("Rainbow Six Siege".into()));
        assert_eq!(match_game_signature("RobloxPlayerBeta.exe", "", "", None), Some("Roblox".into()));
        assert_eq!(match_game_signature("sober", "", "", None), Some("Roblox".into()));
        assert_eq!(match_game_signature("sober", "org.vinegarhq.Sober", "", None), Some("Roblox".into()));
        // Minecraft via javaw with window title
        assert_eq!(match_game_signature("javaw.exe", "", "Minecraft* 1.21.4", None), Some("Minecraft".into()));
        assert_eq!(match_game_signature("javaw", "", "Minecraft 1.20", None), Some("Minecraft".into()));
        assert_eq!(match_game_signature("javaw.exe", "", "IntelliJ IDEA", None), None);
        // Unknown process
        assert_eq!(match_game_signature("notepad.exe", "", "Untitled - Notepad", None), None);
    }

    #[test]
    fn path_root_rule_extracts_game_folder() {
        assert_eq!(
            extract_game_from_path(r"C:\Program Files (x86)\Steam\steamapps\common\EA SPORTS FC 25\FC25.exe"),
            Some("EA SPORTS FC 25".into())
        );
        assert_eq!(
            extract_game_from_path(r"D:\Games\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe"),
            Some("Cyberpunk 2077".into())
        );
        assert_eq!(
            extract_game_from_path(r"C:\Program Files\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe"),
            Some("Fortnite".into())
        );
        assert_eq!(
            extract_game_from_path(r"C:\Program Files\EA Games\Battlefield 2042\BF2042.exe"),
            Some("Battlefield 2042".into())
        );
        assert_eq!(
            extract_game_from_path(r"C:\Program Files\Electronic Arts\Dead Space\DeadSpace.exe"),
            Some("Dead Space".into())
        );
        assert_eq!(
            extract_game_from_path(r"C:\Riot Games\VALORANT\live\ShooterGame\Binaries\Win64\VALORANT-Win64-Shipping.exe"),
            Some("VALORANT".into())
        );
        assert_eq!(
            extract_game_from_path(r"C:\Program Files (x86)\Ubisoft\Ubisoft Game Launcher\games\Tom Clancy's Rainbow Six Siege\RainbowSix.exe"),
            Some("Tom Clancy's Rainbow Six Siege".into())
        );
        assert_eq!(
            extract_game_from_path(r"C:\XboxGames\Halo Infinite\Content\HaloInfinite.exe"),
            Some("Halo Infinite".into())
        );
        assert_eq!(
            extract_game_from_path(r"C:\GOG Games\The Witcher 3 Wild Hunt\bin\x64\witcher3.exe"),
            Some("The Witcher 3 Wild Hunt".into())
        );
        // Forward slashes and case variations
        assert_eq!(
            extract_game_from_path("E:/SteamLibrary/STEAMAPPS/COMMON/Hades/Hades.exe"),
            Some("Hades".into())
        );
        // Not a game root
        assert_eq!(extract_game_from_path(r"C:\Windows\System32\notepad.exe"), None);
        // Directly in root without subfolder
        assert_eq!(extract_game_from_path(r"C:\XboxGames\installer.exe"), None);
    }

    #[test]
    fn ignore_list_filters_launchers_and_utilities() {
        assert!(is_ignored_game_process("Steam.exe"));
        assert!(is_ignored_game_process("steam"));
        assert!(is_ignored_game_process("steamwebhelper.exe"));
        assert!(is_ignored_game_process("EpicGamesLauncher.exe"));
        assert!(is_ignored_game_process("epicgameslauncher"));
        assert!(is_ignored_game_process("EADesktop.exe"));
        assert!(is_ignored_game_process("eadesktop"));
        assert!(is_ignored_game_process("RiotClientServices.exe"));
        assert!(is_ignored_game_process("riotclientservices"));
        assert!(is_ignored_game_process("Discord.exe"));
        assert!(is_ignored_game_process("discord"));
        assert!(is_ignored_game_process("explorer.exe"));
        assert!(is_ignored_game_process("explorer"));
        assert!(is_ignored_game_process("Lapse.exe"));
        assert!(is_ignored_game_process("lapse"));
        assert!(is_ignored_game_process("chrome.exe"));
        assert!(is_ignored_game_process("firefox.exe"));
        assert!(is_ignored_game_process("msedge.exe"));
        // Background clip recorder
        assert!(is_ignored_game_process("Medal.exe"));
        assert!(is_ignored_game_process("medal"));
        assert!(is_ignored_game_process("MedalEncoder.exe"));
        assert!(is_ignored_game_process("medalencoder"));

        // Real games are not ignored
        assert!(!is_ignored_game_process("FC25.exe"));
        assert!(!is_ignored_game_process("cs2.exe"));
        assert!(!is_ignored_game_process("VALORANT-Win64-Shipping.exe"));
        assert!(!is_ignored_game_process("FortniteClient-Win64-Shipping.exe"));
        assert!(!is_ignored_game_process("RainbowSix.exe"));
        assert!(!is_ignored_game_process("TslGame.exe"));
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
