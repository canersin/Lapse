//! Wayland socket peers and elimination-based game window matching.
//! Recognises the running game's window on Wayland compositors that provide no PIDs.

use crate::hardware::{procs, steam};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const NETLINK_SOCK_DIAG: libc::c_int = 4;
const SOCK_DIAG_BY_FAMILY: u16 = 20;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_DUMP: u16 = 0x300;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const UDIAG_SHOW_NAME: u32 = 1;
const UDIAG_SHOW_PEER: u32 = 4;
const UDIAG_NAME: u16 = 0;
const UDIAG_PEER: u16 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Client {
    pub pid: u32,
    pub names: Vec<String>,
    pub game: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toplevel {
    pub class: String,
    pub fullscreen: bool,
}

fn trim_nul(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && b[end - 1] == 0 {
        end -= 1;
    }
    &b[..end]
}

/// Parses one received netlink buffer; returns true when the dump is done or errored.
pub fn parse(buf: &[u8], path: &[u8], peers: &mut HashSet<u32>) -> bool {
    let mut offset = 0;
    let target = trim_nul(path);
    while offset + 16 <= buf.len() {
        let len = u32::from_ne_bytes(buf[offset..offset + 4].try_into().unwrap()) as usize;
        if len < 16 || offset + len > buf.len() {
            return false;
        }
        let msg_type = u16::from_ne_bytes(buf[offset + 4..offset + 6].try_into().unwrap());
        if msg_type == NLMSG_DONE || msg_type == NLMSG_ERROR {
            return true;
        }
        if len >= 32 {
            let mut rta_offset = offset + 32;
            let msg_end = offset + len;
            let mut name: Option<&[u8]> = None;
            let mut peer: Option<u32> = None;
            while rta_offset + 4 <= msg_end {
                let rta_len = u16::from_ne_bytes(buf[rta_offset..rta_offset + 2].try_into().unwrap()) as usize;
                let rta_type = u16::from_ne_bytes(buf[rta_offset + 2..rta_offset + 4].try_into().unwrap());
                if rta_len < 4 || rta_offset + rta_len > msg_end {
                    break;
                }
                let rta_data = &buf[rta_offset + 4..rta_offset + rta_len];
                if rta_type == UDIAG_NAME {
                    name = Some(rta_data);
                } else if rta_type == UDIAG_PEER && rta_data.len() >= 4 {
                    peer = Some(u32::from_ne_bytes(rta_data[..4].try_into().unwrap()));
                }
                let rta_aligned = (rta_len + 3) & !3;
                if rta_aligned == 0 {
                    break;
                }
                rta_offset += rta_aligned;
            }
            if let (Some(name), Some(peer)) = (name, peer) {
                if trim_nul(name) == target && peer != 0 {
                    peers.insert(peer);
                }
            }
        }
        let aligned = (len + 3) & !3;
        if aligned == 0 {
            break;
        }
        offset += aligned;
    }
    false
}

/// Sockets connected to the listening Unix domain socket at `path`.
fn socket_peers(path: &str) -> Option<HashSet<u32>> {
    let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_RAW | libc::SOCK_CLOEXEC, NETLINK_SOCK_DIAG) };
    if fd < 0 {
        return None;
    }
    struct Fd(libc::c_int);
    impl Drop for Fd {
        fn drop(&mut self) {
            unsafe { libc::close(self.0) };
        }
    }
    let _guard = Fd(fd);

    let tv = libc::timeval { tv_sec: 1, tv_usec: 0 };
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            &tv as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        );
    }

    let mut req = [0u8; 40];
    req[0..4].copy_from_slice(&40u32.to_ne_bytes());
    req[4..6].copy_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
    req[6..8].copy_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
    req[8..12].copy_from_slice(&1u32.to_ne_bytes());
    req[16] = libc::AF_UNIX as u8;
    req[20..24].copy_from_slice(&(!0u32).to_ne_bytes());
    req[28..32].copy_from_slice(&(UDIAG_SHOW_NAME | UDIAG_SHOW_PEER).to_ne_bytes());

    let sent = unsafe { libc::send(fd, req.as_ptr() as *const libc::c_void, req.len(), 0) };
    if sent < 0 {
        return None;
    }

    let mut buf = vec![0u8; 65536];
    let mut peers = HashSet::new();
    loop {
        let n = unsafe { libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0) };
        if n <= 0 {
            break;
        }
        if parse(&buf[..n as usize], path.as_bytes(), &mut peers) {
            break;
        }
    }
    Some(peers)
}

/// PIDs of processes holding a connection to the Wayland compositor socket.
pub fn connected() -> Vec<u32> {
    if procs::in_flatpak() {
        return Vec::new();
    }
    let display = match std::env::var("WAYLAND_DISPLAY").ok().filter(|d| !d.is_empty()) {
        Some(d) => d,
        None => return Vec::new(),
    };
    let path = if Path::new(&display).is_absolute() {
        display
    } else {
        match std::env::var("XDG_RUNTIME_DIR").ok().filter(|d| !d.is_empty()) {
            Some(dir) => PathBuf::from(dir).join(display).to_string_lossy().into_owned(),
            None => return Vec::new(),
        }
    };
    let Some(peers) = socket_peers(&path) else { return Vec::new() };
    if peers.is_empty() {
        return Vec::new();
    }

    let mut pids = Vec::new();
    for pid in procs::pids() {
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else { continue };
        for fd in fds.flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else { continue };
            let s = target.to_string_lossy();
            if let Some(ino_str) = s.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']')) {
                if let Ok(ino) = ino_str.parse::<u32>() {
                    if peers.contains(&ino) {
                        pids.push(pid);
                        break;
                    }
                }
            }
        }
    }
    pids
}

/// Strips trailing `.exe`, keeps only lower-case alphanumerics.
pub fn plain(s: &str) -> String {
    let s = s.to_ascii_lowercase();
    let s = s.strip_suffix(".exe").unwrap_or(&s);
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Whether client process names plausibly match a window class.
pub fn owns(names: &[String], class: &str) -> bool {
    let last = class.rsplit('.').next().unwrap_or(class);
    for part in [plain(class), plain(last)] {
        if part.len() < 3 {
            continue;
        }
        for name in names {
            if part == *name || (name.len() >= 3 && (part.contains(name) || name.contains(&part))) {
                return true;
            }
        }
    }
    false
}

fn descends_from(mut current: u32, target: u32) -> bool {
    if target == 0 {
        return false;
    }
    for _ in 0..12 {
        match procs::ppid(current) {
            Some(parent) if parent > 1 => {
                if parent == target {
                    return true;
                }
                current = parent;
            }
            _ => return false,
        }
    }
    false
}

/// Identifies connected Wayland clients and whether they belong to `game`.
pub fn collect(game: u32) -> Vec<Client> {
    let game_app = steam::app_id_of(game);
    connected()
        .into_iter()
        .map(|pid| {
            let mut names = Vec::new();
            let comm = procs::comm(pid);
            let exe_name = procs::exe(pid).and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()));
            let cmdline = procs::cmdline(pid);
            let first_word_name = cmdline.split_whitespace().next().map(|w| {
                Path::new(w).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| w.to_string())
            });
            for raw in [Some(comm), exe_name, first_word_name].into_iter().flatten() {
                let p = plain(&raw);
                if !p.is_empty() && !names.contains(&p) {
                    names.push(p);
                }
            }
            let is_game = pid == game
                || descends_from(pid, game)
                || (game_app.is_some() && game_app == steam::app_id_of(pid));
            Client { pid, names, game: is_game }
        })
        .collect()
}

/// Recognises whether `class` belongs to the running game by eliminating non-game windows.
pub fn is_game_window(class: &str, toplevels: &[Toplevel], clients: &[Client], known_game: impl Fn(&str) -> bool) -> bool {
    if class.is_empty() {
        return false;
    }
    if !clients.iter().any(|c| c.game) {
        return false;
    }
    if clients.iter().any(|c| !c.game && owns(&c.names, class)) {
        return false;
    }
    if toplevels.iter().any(|t| known_game(&t.class)) {
        return false;
    }
    // A window named after the game's program is the game's; no other one can be.
    let named = |class: &str| clients.iter().any(|c| c.game && owns(&c.names, class));
    if toplevels.iter().any(|t| named(&t.class)) {
        return named(class);
    }
    let candidates: Vec<&Toplevel> = toplevels
        .iter()
        .filter(|t| !t.class.is_empty() && !clients.iter().any(|c| !c.game && owns(&c.names, &t.class)))
        .collect();
    if candidates.is_empty() {
        return false;
    }
    if candidates.iter().all(|t| t.class == class) {
        return true;
    }
    toplevels.iter().any(|t| t.class == class && t.fullscreen)
        && !candidates.iter().any(|t| t.class != class && t.fullscreen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(name: &[u8], peer: u32) -> Vec<u8> {
        let mut msg = Vec::new();
        let mut rta_name = Vec::new();
        let name_len = (4 + name.len()) as u16;
        rta_name.extend_from_slice(&name_len.to_ne_bytes());
        rta_name.extend_from_slice(&0u16.to_ne_bytes());
        rta_name.extend_from_slice(name);
        while rta_name.len() % 4 != 0 {
            rta_name.push(0);
        }

        let mut rta_peer = Vec::new();
        let peer_len = 8u16;
        rta_peer.extend_from_slice(&peer_len.to_ne_bytes());
        rta_peer.extend_from_slice(&2u16.to_ne_bytes());
        rta_peer.extend_from_slice(&peer.to_ne_bytes());

        let total_len = (16 + 16 + rta_name.len() + rta_peer.len()) as u32;
        msg.extend_from_slice(&total_len.to_ne_bytes());
        msg.extend_from_slice(&20u16.to_ne_bytes());
        msg.extend_from_slice(&0u16.to_ne_bytes());
        msg.extend_from_slice(&0u32.to_ne_bytes());
        msg.extend_from_slice(&0u32.to_ne_bytes());

        msg.extend_from_slice(&[1, 0, 0, 0]);
        msg.extend_from_slice(&100u32.to_ne_bytes());
        msg.extend_from_slice(&[0u8; 8]);

        msg.extend_from_slice(&rta_name);
        msg.extend_from_slice(&rta_peer);
        msg
    }

    fn make_done() -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&16u32.to_ne_bytes());
        msg.extend_from_slice(&3u16.to_ne_bytes());
        msg.extend_from_slice(&0u16.to_ne_bytes());
        msg.extend_from_slice(&0u32.to_ne_bytes());
        msg.extend_from_slice(&0u32.to_ne_bytes());
        msg
    }

    #[test]
    fn test_parse() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&make_msg(b"/tmp/wayland-0\0", 12345));
        buf.extend_from_slice(&make_msg(b"/tmp/other-socket\0", 99999));
        buf.extend_from_slice(&make_done());

        let mut peers = HashSet::new();
        let done = parse(&buf, b"/tmp/wayland-0", &mut peers);
        assert!(done);
        assert_eq!(peers.len(), 1);
        assert!(peers.contains(&12345));
        assert!(!peers.contains(&99999));
    }

    #[test]
    fn test_plain() {
        assert_eq!(plain("Deadlock.exe"), "deadlock");
        assert_eq!(plain("Game.EXE"), "game");
        assert_eq!(plain("org.gnome.Calculator"), "orggnomecalculator");
        assert_eq!(plain("steam_app_123"), "steamapp123");
    }

    #[test]
    fn test_owns() {
        assert!(owns(&["firefox".into()], "org.mozilla.firefox"));
        assert!(owns(&["steamwebhelper".into()], "steam"));
        assert!(!owns(&["spotify".into()], "discord"));
        assert!(!owns(&["go".into()], "go"));
    }

    #[test]
    fn test_is_game_window() {
        let clients = vec![
            Client { pid: 10, names: vec!["mygame".into()], game: true },
            Client { pid: 20, names: vec!["firefox".into()], game: false },
        ];

        // Single candidate
        let toplevels = vec![
            Toplevel { class: "mygame".into(), fullscreen: false },
            Toplevel { class: "firefox".into(), fullscreen: false },
        ];
        assert!(is_game_window("mygame", &toplevels, &clients, |_| false));

        // Ambiguous without fullscreen
        let toplevels_ambiguous = vec![
            Toplevel { class: "game1".into(), fullscreen: false },
            Toplevel { class: "game2".into(), fullscreen: false },
        ];
        assert!(!is_game_window("game1", &toplevels_ambiguous, &clients, |_| false));

        // Ambiguous with fullscreen
        let toplevels_fullscreen = vec![
            Toplevel { class: "game1".into(), fullscreen: true },
            Toplevel { class: "game2".into(), fullscreen: false },
        ];
        assert!(is_game_window("game1", &toplevels_fullscreen, &clients, |_| false));
        assert!(!is_game_window("game2", &toplevels_fullscreen, &clients, |_| false));

        // No game client
        let no_game_clients = vec![Client { pid: 10, names: vec!["mygame".into()], game: false }];
        assert!(!is_game_window("mygame", &toplevels, &no_game_clients, |_| false));

        // Class owned by another program
        assert!(!is_game_window("firefox", &toplevels, &clients, |_| false));

        // The game's own name beats a fullscreen stranger
        let named = vec![
            Toplevel { class: "org.mygame".into(), fullscreen: false },
            Toplevel { class: "stranger".into(), fullscreen: true },
        ];
        assert!(is_game_window("org.mygame", &named, &clients, |_| false));
        assert!(!is_game_window("stranger", &named, &clients, |_| false));

        // Another window already known as the game
        let toplevels_with_known = vec![
            Toplevel { class: "known_game".into(), fullscreen: false },
            Toplevel { class: "mygame".into(), fullscreen: false },
        ];
        assert!(!is_game_window("mygame", &toplevels_with_known, &clients, |c| c == "known_game"));
    }
}
