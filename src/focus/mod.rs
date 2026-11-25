//! Games only mode on Linux: which window has the focus, the monitor the game is on, and
//! the card gpu-screen-recorder shows instead of the picture while the player is elsewhere.

mod card;
mod clients;
mod kwin;
mod matcher;
mod shared;
mod wayland;
mod x11;

pub use card::{plugin_supported, Card};
pub use shared::ENV as CARD_ENV;

use crate::config::GameEntry;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Condvar, Mutex, Once};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Window {
    pub pid: Option<u32>,
    /// X11 class or Wayland app id.
    pub class: String,
    pub title: String,
    /// Connector name, spelled the way its source does.
    pub monitor: Option<String>,
    /// X11 window id.
    pub x11: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Report {
    Window(Window),
    /// No window has the focus, or none this source can see.
    Nothing,
}

#[derive(Clone, Copy)]
enum Source {
    X11,
    /// The compositor itself: it knows X11 and Wayland windows.
    Compositor,
}

#[derive(Default)]
struct Reports {
    x11: Option<Report>,
    compositor: Option<Report>,
    generation: u64,
}

static REPORTS: Mutex<Reports> = Mutex::new(Reports { x11: None, compositor: None, generation: 0 });
static CHANGED: Condvar = Condvar::new();

/// The game's X11 window, 0: none.
static GAME_WINDOW: AtomicU32 = AtomicU32::new(0);
static GAME_HIDDEN: AtomicBool = AtomicBool::new(false);
/// Monitor the game was last focused on.
static GAME_MONITOR: Mutex<Option<String>> = Mutex::new(None);
/// Monitors seen by X11, 0: unknown.
static X11_MONITORS: AtomicU32 = AtomicU32::new(0);
/// Monitors seen by the compositor, 0: unknown.
static COMPOSITOR_MONITORS: AtomicU32 = AtomicU32::new(0);
/// Games the user added.
static GAMES: Mutex<Vec<GameEntry>> = Mutex::new(Vec::new());
/// Process of the game the daemon's game watcher sees, 0: none.
static GAME_PID: AtomicU32 = AtomicU32::new(0);
/// The windows that were open when the Wayland clients were last listed, and the clients.
/// A newer window's program may be missing from the list, so nothing is concluded about it.
static CLIENTS: Mutex<(Vec<clients::Toplevel>, Vec<clients::Client>)> = Mutex::new((Vec::new(), Vec::new()));
/// Toplevel windows reported by the Wayland compositor.
static TOPLEVELS: Mutex<Vec<clients::Toplevel>> = Mutex::new(Vec::new());

/// Starts watching the focus; the watchers stay for the life of the daemon.
pub fn start() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(x11::run);
        std::thread::spawn(wayland::run);
        std::thread::spawn(kwin::run);
    });
}

fn report(source: Source, report: Option<Report>) {
    let mut r = REPORTS.lock().unwrap_or_else(|e| e.into_inner());
    match source {
        Source::X11 => r.x11 = report,
        Source::Compositor => r.compositor = report,
    }
    r.generation += 1;
    CHANGED.notify_all();
}

/// The focused window, or None when nothing can tell.
fn focused() -> Option<Report> {
    let r = REPORTS.lock().unwrap_or_else(|e| e.into_inner());
    merge(r.x11.as_ref(), r.compositor.as_ref())
}

/// The compositor decides; X11 adds what it knows about the same window.
fn merge(x11: Option<&Report>, compositor: Option<&Report>) -> Option<Report> {
    let Some(compositor) = compositor else { return x11.cloned() };
    if let (Report::Window(c), Some(Report::Window(x))) = (compositor, x11) {
        if c.class.eq_ignore_ascii_case(&x.class) && !c.class.is_empty() {
            return Some(Report::Window(Window { pid: x.pid.or(c.pid), x11: x.x11, ..c.clone() }));
        }
    }
    Some(compositor.clone())
}

/// Waits until the focus may have changed, at most `timeout`.
fn wait(seen: &mut u64, timeout: Duration) {
    let r = REPORTS.lock().unwrap_or_else(|e| e.into_inner());
    let r = CHANGED.wait_timeout_while(r, timeout, |r| r.generation == *seen).map(|(r, _)| r).unwrap_or_else(|e| e.into_inner().0);
    *seen = r.generation;
}

fn game_window() -> Option<u32> {
    Some(GAME_WINDOW.load(Ordering::Relaxed)).filter(|w| *w != 0)
}

fn set_game_hidden(hidden: bool) {
    GAME_HIDDEN.store(hidden, Ordering::Relaxed);
}

fn set_monitor_count(source: Source, count: usize) {
    let target = match source {
        Source::X11 => &X11_MONITORS,
        Source::Compositor => &COMPOSITOR_MONITORS,
    };
    target.store(count as u32, Ordering::Relaxed);
}

/// Whether there is nowhere else to look than the recorded monitor.
fn single_monitor() -> bool {
    let count = match COMPOSITOR_MONITORS.load(Ordering::Relaxed) {
        0 => X11_MONITORS.load(Ordering::Relaxed),
        n => n,
    };
    match count {
        0 => crate::hardware::monitor::connected_count() == 1,
        n => n == 1,
    }
}

/// What the daemon's game watcher sees; also follows the game's monitor.
pub fn update_game(pid: Option<u32>, games: &[GameEntry]) {
    *GAMES.lock().unwrap_or_else(|e| e.into_inner()) = games.to_vec();
    let seen = TOPLEVELS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let list = match pid {
        Some(game) if !seen.is_empty() => clients::collect(game),
        _ => Vec::new(),
    };
    *CLIENTS.lock().unwrap_or_else(|e| e.into_inner()) = (seen, list);
    // Last: the card forgets its verdicts when this changes.
    GAME_PID.store(pid.unwrap_or(0), Ordering::Release);
    let monitor = match (pid, focused()) {
        (None, _) => None,
        (_, Some(Report::Window(w))) if w.monitor.is_some() && matcher::is_game(&w) == Some(true) => w.monitor,
        _ => return,
    };
    *GAME_MONITOR.lock().unwrap_or_else(|e| e.into_inner()) = monitor;
}

fn set_toplevels(list: Vec<clients::Toplevel>) {
    *TOPLEVELS.lock().unwrap_or_else(|e| e.into_inner()) = list;
}

/// Which of `monitors` the game is on.
pub fn game_monitor(monitors: &[String]) -> Option<String> {
    let name = GAME_MONITOR.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    monitors.iter().find(|m| same_monitor(m, &name)).cloned()
}

/// GNOME's X11 names leave out the connector's type letter (HDMI-2 for HDMI-A-2).
fn same_monitor(a: &str, b: &str) -> bool {
    let plain = |name: &str| name.to_ascii_uppercase().replace("HDMI-A-", "HDMI-").replace("HDMI-B-", "HDMI-");
    plain(a) == plain(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(class: &str, pid: Option<u32>) -> Report {
        Report::Window(Window { pid, class: class.into(), monitor: Some("DP-1".into()), ..Default::default() })
    }

    #[test]
    fn test_compositor_wins_and_x11_adds_the_pid() {
        let x = window("game", Some(7));
        assert_eq!(merge(Some(&x), None), Some(x.clone()));
        assert_eq!(merge(Some(&x), Some(&window("Game", None))), Some(window("Game", Some(7))));
        // A Wayland window has the focus: what X11 last saw doesn't matter.
        assert_eq!(merge(Some(&x), Some(&window("firefox", None))), Some(window("firefox", None)));
        assert_eq!(merge(Some(&x), Some(&Report::Nothing)), Some(Report::Nothing));
        assert_eq!(merge(None, None), None);
    }

    #[test]
    fn test_monitor_names() {
        assert!(same_monitor("HDMI-A-2", "HDMI-2"));
        assert!(same_monitor("DP-1", "DP-1"));
        assert!(!same_monitor("DP-1", "DP-2"));
        assert!(!same_monitor("HDMI-A-1", "HDMI-2"));
    }
}
