//! Whether a window belongs to a game. Stricter than the game detector: a browser tab
//! named after a game is not the game.

use super::{clients, Window};
use crate::config::GameEntry;
use crate::hardware::game_detector::{entry_matches, match_game_signature};
use crate::hardware::{procs, steam};
use std::sync::atomic::Ordering;

/// None: a window that says nothing about where the player is (Lapse's own).
pub fn is_game(window: &Window) -> Option<bool> {
    let pid = window.pid.or_else(|| compositor_pid(&window.class));
    let comm = pid.map(procs::comm).unwrap_or_default();
    if pid == Some(std::process::id()) || comm.eq_ignore_ascii_case("lapse") || is_lapse(&window.class) {
        return None;
    }
    // X11 windows on niri and driftwm all belong to this helper.
    let pid = pid.filter(|_| comm != "xwayland-satell" && comm != "Xwayland");
    let game_pid = Some(super::GAME_PID.load(Ordering::Relaxed)).filter(|p| *p != 0);
    let games = super::GAMES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Some(decide(window, pid, &comm, &games, game_pid))
}

fn is_lapse(class: &str) -> bool {
    class.eq_ignore_ascii_case("lapse") || class.eq_ignore_ascii_case("io.github.canersin.Lapse")
}

fn decide(window: &Window, pid: Option<u32>, comm: &str, games: &[GameEntry], game_pid: Option<u32>) -> bool {
    let class = window.class.trim();
    if pid.is_some() && pid == game_pid {
        return true;
    }
    if class.to_ascii_lowercase().starts_with("steam_app_") {
        return true;
    }
    if class.eq_ignore_ascii_case("gamescope") || comm.starts_with("gamescope") {
        return game_pid.is_some_and(runs_in_gamescope);
    }
    if let Some(pid) = pid {
        if let Some(app) = steam::app_id_of(pid) {
            if steam::app_name(app).is_some_and(|name| !steam::is_tool(app, &name)) {
                return true;
            }
        }
    }
    // Java programs share a process name, so there the title has to tell.
    let title = if comm.starts_with("java") { window.title.as_str() } else { "" };
    if match_game_signature(comm, class, title, None).is_some() {
        return true;
    }
    let cmdline = pid.map(procs::cmdline).unwrap_or_default();
    if games.iter().any(|g| entry_matches(g.detect_word(), comm, &cmdline, class, "")) {
        return true;
    }
    // Without a pid: named after the running game's program, or the one window left over.
    let (None, Some(game)) = (pid, game_pid) else { return false };
    same_program(class, &procs::comm(game)) || {
        let toplevels = super::TOPLEVELS.lock().unwrap_or_else(|e| e.into_inner());
        let listed = super::CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
        let (seen, clients) = &*listed;
        seen.iter().any(|t| t.class == class)
            && !procs::comm(game).is_empty()
            && clients::is_game_window(class, &toplevels, clients, |c| {
                c.to_ascii_lowercase().starts_with("steam_app_") || match_game_signature("", c, "", None).is_some()
            })
    }
}

/// Process names are cut off after 15 bytes.
fn same_program(class: &str, comm: &str) -> bool {
    let stem = |s: &str| {
        let s = s.to_ascii_lowercase();
        s.strip_suffix(".exe").unwrap_or(&s).to_string()
    };
    let (class, comm) = (class.to_ascii_lowercase(), comm.to_ascii_lowercase());
    !comm.is_empty() && (stem(&class) == stem(&comm) || (comm.len() == 15 && class.starts_with(&comm)))
}

fn runs_in_gamescope(mut pid: u32) -> bool {
    for _ in 0..12 {
        match procs::ppid(pid) {
            Some(parent) if parent > 1 => pid = parent,
            _ => return false,
        }
        if procs::comm(pid).starts_with("gamescope") {
            return true;
        }
    }
    false
}

/// The focused window's process, from compositors whose own commands tell.
fn compositor_pid(class: &str) -> Option<u32> {
    let ask = |program: &str, args: &[&str]| -> Option<serde_json::Value> {
        let out = procs::host_command(program).args(args).output().ok()?;
        serde_json::from_slice(&out.stdout).ok()
    };
    let env = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    let window = if env("HYPRLAND_INSTANCE_SIGNATURE") {
        ask("hyprctl", &["activewindow", "-j"])?
    } else if env("NIRI_SOCKET") {
        ask("niri", &["msg", "--json", "focused-window"])?
    } else if env("SWAYSOCK") {
        focused_node(&ask("swaymsg", &["-t", "get_tree", "-r"])?)?.clone()
    } else {
        return None;
    };
    let named = ["class", "app_id"].iter().find_map(|k| window.get(k)?.as_str()).or_else(|| window.get("window_properties")?.get("class")?.as_str());
    if !named.is_some_and(|n| n.eq_ignore_ascii_case(class)) {
        return None;
    }
    window.get("pid")?.as_u64().map(|p| p as u32).filter(|p| *p > 0)
}

fn focused_node(node: &serde_json::Value) -> Option<&serde_json::Value> {
    if node.get("focused")?.as_bool()? {
        return Some(node);
    }
    ["nodes", "floating_nodes"].iter().filter_map(|k| node.get(k)?.as_array()).flatten().find_map(focused_node)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(class: &str, title: &str) -> Window {
        Window { class: class.into(), title: title.into(), ..Default::default() }
    }

    #[test]
    fn test_proton_and_unrelated_windows() {
        assert!(decide(&window("steam_app_1422450", "Deadlock"), None, "", &[], None));
        assert!(decide(&window("steam_app_default", ""), None, "", &[], None));
        assert!(!decide(&window("firefox", "Counter-Strike 2 - YouTube"), None, "", &[], None));
        assert!(!decide(&window("steam", "Steam"), None, "", &[], None));
        assert!(!decide(&window("discord", "Discord"), None, "", &[], Some(std::process::id())));
        assert!(!decide(&window("gamescope", ""), None, "", &[], None));
    }

    #[test]
    fn test_program_names() {
        assert!(same_program("Deadlock.exe", "deadlock.exe"));
        assert!(same_program("hl2_linux", "hl2_linux"));
        assert!(same_program("averyveryverylonggame.exe", "averyveryverylo"));
        assert!(!same_program("", ""));
        assert!(!same_program("firefox", "fire"));
    }

    #[test]
    fn test_focused_sway_node() {
        let tree = serde_json::json!({"focused": false, "nodes": [
            {"focused": false, "nodes": [], "floating_nodes": [{"focused": true, "pid": 5, "app_id": "game"}]}]});
        assert_eq!(focused_node(&tree).and_then(|n| n.get("pid")).and_then(|p| p.as_u64()), Some(5));
    }
}
