//! Decides whether the player is away from the game (alt-tabbed) in Games only mode.

/// How long the game must be back before its picture is shown again: Windows animates
/// windows away for a moment after the game already has the focus.
const BACK_MS: u64 = 400;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Foreground {
    /// No foreground window, or one whose program can't be read.
    Unknown,
    Game,
    Other { same_monitor: bool },
}

/// Fails open: whenever it isn't sure, the real picture stays.
#[derive(Debug, Default)]
pub struct FocusState {
    away: bool,
    /// A game has had the focus, so it is known what "away" means.
    armed: bool,
    back_since: Option<u64>,
}

impl FocusState {
    pub fn update(&mut self, fg: Foreground, game_minimized: bool, now_ms: u64) -> bool {
        match fg {
            Foreground::Unknown => {}
            Foreground::Game => {
                self.armed = true;
                let since = *self.back_since.get_or_insert(now_ms);
                if now_ms.saturating_sub(since) >= BACK_MS {
                    self.away = false;
                }
            }
            Foreground::Other { same_monitor } => {
                self.back_since = None;
                // Focus on another monitor leaves the game visible, unless it minimized itself.
                if self.armed && (same_monitor || game_minimized) {
                    self.away = true;
                }
            }
        }
        self.away
    }
}

#[cfg(windows)]
pub use gate::{FocusGate, FocusSettings};

#[cfg(windows)]
mod gate {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::time::Instant;

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow};

    use super::{Foreground, FocusState};
    use crate::config::GameEntry;
    use crate::hardware::game_detector_windows::{foreground_game_pid, process_exe_path};

    /// A window that wasn't a game may become one (it goes fullscreen).
    const RECHECK_MS: u64 = 500;

    pub struct FocusSettings {
        /// HMONITOR being recorded.
        pub monitor: isize,
        pub games: Vec<GameEntry>,
        pub title: String,
    }

    pub struct FocusGate {
        monitor: isize,
        games: Vec<GameEntry>,
        state: FocusState,
        started: Instant,
        game_pids: HashSet<u32>,
        game_dirs: Vec<PathBuf>,
        game_window: isize,
        /// pid -> (window, when checked, unreadable) of programs that aren't games.
        others: HashMap<u32, (isize, u64, bool)>,
    }

    impl FocusGate {
        pub fn new(monitor: isize, games: Vec<GameEntry>) -> Self {
            Self {
                monitor,
                games,
                state: FocusState::default(),
                started: Instant::now(),
                game_pids: HashSet::new(),
                game_dirs: Vec::new(),
                game_window: 0,
                others: HashMap::new(),
            }
        }

        /// Whether the player is away from the game right now; called for every frame.
        pub fn update(&mut self) -> bool {
            let now = self.started.elapsed().as_millis() as u64;
            let hwnd = unsafe { GetForegroundWindow() };
            let fg = self.classify(hwnd, now);
            let minimized = self.game_window != 0
                && matches!(fg, Foreground::Other { same_monitor: false })
                && unsafe { IsWindow(self.game_window as HWND) != 0 && IsIconic(self.game_window as HWND) != 0 };
            self.state.update(fg, minimized, now)
        }

        fn classify(&mut self, hwnd: HWND, now: u64) -> Foreground {
            if hwnd.is_null() {
                return Foreground::Unknown;
            }
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if pid == 0 || pid == std::process::id() {
                return Foreground::Unknown;
            }
            if !self.game_pids.contains(&pid) {
                match self.others.get(&pid) {
                    Some(&(window, checked, unreadable)) if window == hwnd as isize && now - checked < RECHECK_MS => {
                        return if unreadable { Foreground::Unknown } else { self.other(hwnd) };
                    }
                    _ => {}
                }
                let path = process_exe_path(pid);
                let is_game = foreground_game_pid(&self.games) == Some(pid)
                    || path.as_ref().is_some_and(|p| self.game_dirs.iter().any(|d| p.starts_with(d)));
                if !is_game {
                    self.others.insert(pid, (hwnd as isize, now, path.is_none()));
                    return if path.is_none() { Foreground::Unknown } else { self.other(hwnd) };
                }
                self.others.remove(&pid);
                self.game_pids.insert(pid);
                if let Some(dir) = path.as_ref().and_then(|p| p.parent()).filter(|d| d.components().count() > 3) {
                    if !self.game_dirs.iter().any(|d| dir.starts_with(d)) {
                        self.game_dirs.push(dir.to_path_buf());
                    }
                }
            }
            self.game_window = hwnd as isize;
            Foreground::Game
        }

        fn other(&self, hwnd: HWND) -> Foreground {
            let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
            Foreground::Other { same_monitor: monitor as isize == self.monitor }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAME: Foreground = Foreground::Other { same_monitor: true };
    const ELSEWHERE: Foreground = Foreground::Other { same_monitor: false };

    #[test]
    fn test_never_away_before_a_game_had_focus() {
        let mut s = FocusState::default();
        assert!(!s.update(SAME, false, 0));
        assert!(!s.update(ELSEWHERE, true, 10));
    }

    #[test]
    fn test_away_at_once_and_back_after_a_moment() {
        let mut s = FocusState::default();
        s.update(Foreground::Game, false, 0);
        assert!(s.update(SAME, false, 100));
        assert!(s.update(Foreground::Game, false, 200));
        // Interrupted: the wait starts over.
        assert!(s.update(SAME, false, 500));
        assert!(s.update(Foreground::Game, false, 700));
        assert!(s.update(Foreground::Game, false, 1099));
        assert!(!s.update(Foreground::Game, false, 1100));
    }

    #[test]
    fn test_unknown_keeps_the_state() {
        let mut s = FocusState::default();
        s.update(Foreground::Game, false, 0);
        assert!(!s.update(Foreground::Unknown, false, 10));
        s.update(SAME, false, 20);
        assert!(s.update(Foreground::Unknown, false, 30));
    }

    #[test]
    fn test_other_monitor_only_counts_when_the_game_minimized() {
        let mut s = FocusState::default();
        s.update(Foreground::Game, false, 0);
        assert!(!s.update(ELSEWHERE, false, 10));
        assert!(s.update(ELSEWHERE, true, 20));
        // Whatever covered the game's monitor is still there.
        assert!(s.update(ELSEWHERE, false, 30));
    }
}
