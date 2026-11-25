//! Drives the card: decides when the player is away and draws the pictures the plugin shows.

use super::shared::*;
use super::{matcher, Report};
use crate::recorder_windows::focus_card::{self, FocusCard};
use crate::recorder_windows::focus_gate::{Foreground, FocusState};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::*};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

static PLUGIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/gsr_plugin.so"));

/// A window that wasn't a game may become one.
const RECHECK: Duration = Duration::from_secs(2);

/// Whether this gpu-screen-recorder takes plugins.
pub fn plugin_supported(recorder: &str) -> bool {
    static KNOWN: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let mut known = KNOWN.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    *known.entry(recorder.to_string()).or_insert_with(|| {
        std::process::Command::new(recorder).arg("--help").output().is_ok_and(|o| {
            [o.stdout, o.stderr].iter().any(|text| String::from_utf8_lossy(text).contains("[-p <plugin_path>]"))
        })
    })
}

pub struct Card {
    pub plugin: PathBuf,
    pub file: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Card {
    /// `dir` is where the plugin file is kept.
    pub fn start(dir: &Path, monitor: String, title: String) -> anyhow::Result<Self> {
        let plugin = dir.join("gsr-plugin.so");
        if std::fs::read(&plugin).ok().as_deref() != Some(PLUGIN) {
            std::fs::create_dir_all(dir)?;
            // Replaced, not overwritten: a running recorder may have the old one loaded.
            let new = dir.join("gsr-plugin.so.new");
            std::fs::write(&new, PLUGIN)?;
            std::fs::rename(&new, &plugin)?;
        }
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let file = runtime.join(format!("lapse-card-{}", std::process::id()));
        let shared = Shared::create(&file)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::Builder::new().name("focus-card".into()).spawn(move || run(shared, monitor, title, &stopped))?;
        Ok(Self { plugin, file, stop, thread: Some(thread) })
    }
}

impl Drop for Card {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = std::fs::remove_file(&self.file);
    }
}

struct Shared(*mut u8);

unsafe impl Send for Shared {}

impl Shared {
    fn create(path: &Path) -> anyhow::Result<Self> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let _ = std::fs::remove_file(path);
        let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(path)?;
        file.set_len(FILE_SIZE as u64)?;
        let map = unsafe {
            libc::mmap(std::ptr::null_mut(), FILE_SIZE, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, file.as_raw_fd(), 0)
        };
        anyhow::ensure!(map != libc::MAP_FAILED, "mapping {} failed", path.display());
        let shared = Self(map.cast());
        let [b, g, r, _] = focus_card::BACKGROUND;
        shared.field(BACKGROUND).store(u32::from_be_bytes([0, r, g, b]), Relaxed);
        shared.field(MAGIC_AT).store(MAGIC, Release);
        Ok(shared)
    }

    fn field(&self, i: usize) -> &AtomicU32 {
        unsafe { &*(self.0 as *const AtomicU32).add(i) }
    }

    /// Puts a picture into the buffer the plugin isn't reading.
    fn show(&self, (w, h): (u32, u32), pixels: &[u8]) {
        if pixels.len() > BUFFER_SIZE {
            return;
        }
        let seq = self.field(SEQ).load(Relaxed).wrapping_add(1).max(1);
        unsafe {
            let to = self.0.add(HEADER_SIZE + (seq & 1) as usize * BUFFER_SIZE);
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), to, pixels.len());
        }
        self.field(CARD_W).store(w, Relaxed);
        self.field(CARD_H).store(h, Relaxed);
        self.field(SEQ).store(seq, Release);
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.0.cast(), FILE_SIZE) };
    }
}

fn run(shared: Shared, monitor: String, title: String, stop: &AtomicBool) {
    let mut gate = Gate::new(monitor);
    let mut card: Option<FocusCard> = None;
    let mut away_since: Option<Instant> = None;
    let mut seen = 0;
    while !stop.load(Relaxed) {
        // The plugin tells the frame's size once the recording runs.
        let (w, h) = (shared.field(FRAME_W).load(Acquire), shared.field(FRAME_H).load(Relaxed));
        if card.is_none() && w > 0 && h > 0 {
            card = FocusCard::new(w, h, &title).ok();
            if let Some(card) = &mut card {
                prepare(&shared, card);
            }
        }
        let away = gate.update();
        match (away, away_since) {
            (true, None) => {
                shared.field(AWAY).store(1, Release);
                away_since = Some(Instant::now());
            }
            (false, Some(_)) => {
                shared.field(AWAY).store(0, Release);
                away_since = None;
                if let Some(card) = &mut card {
                    prepare(&shared, card);
                }
            }
            _ => {}
        }
        if let (Some(since), Some(card)) = (away_since, &mut card) {
            let (_, _, w, h) = card.content_rect();
            shared.show((w, h), card.draw(since.elapsed().as_secs_f32()));
        }
        gate.recheck();
        super::wait(&mut seen, Duration::from_millis(if away_since.is_some() { 16 } else { 50 }));
    }
    shared.field(AWAY).store(0, Release);
}

/// Has the card's first picture ready, so the next time starts with it.
fn prepare(shared: &Shared, card: &mut FocusCard) {
    let (_, _, w, h) = card.content_rect();
    shared.show((w, h), card.draw(0.0));
}

struct Gate {
    monitor: String,
    state: FocusState,
    started: Instant,
    /// Verdicts per window identity and when they were decided.
    known: HashMap<(Option<u32>, String), (Option<bool>, Instant)>,
    /// The game they were decided for.
    game: u32,
}

impl Gate {
    fn new(monitor: String) -> Self {
        super::GAME_WINDOW.store(0, Relaxed);
        Self { monitor, state: FocusState::default(), started: Instant::now(), known: HashMap::new(), game: 0 }
    }

    fn update(&mut self) -> bool {
        let foreground = match super::focused() {
            None => Foreground::Unknown,
            Some(Report::Nothing) => self.elsewhere(None),
            Some(Report::Window(w)) => match self.is_game(&w) {
                None => Foreground::Unknown,
                Some(true) => {
                    super::GAME_WINDOW.store(w.x11.unwrap_or(0), Relaxed);
                    Foreground::Game
                }
                Some(false) => self.elsewhere(w.monitor.as_deref()),
            },
        };
        let hidden = super::GAME_HIDDEN.load(Relaxed) && super::game_window().is_some();
        self.state.update(foreground, hidden, self.started.elapsed().as_millis() as u64)
    }

    fn is_game(&mut self, window: &super::Window) -> Option<bool> {
        let game = super::GAME_PID.load(Acquire);
        if self.game != game {
            self.game = game;
            self.known.clear();
        }
        let key = (window.pid, window.class.clone());
        if let Some((verdict, _)) = self.known.get(&key) {
            return *verdict;
        }
        let verdict = matcher::is_game(window);
        if self.known.len() > 256 {
            self.known.clear();
        }
        self.known.insert(key, (verdict, Instant::now()));
        verdict
    }

    /// A focused window that wasn't a game may become one: re-checks it once RECHECK has
    /// passed, as long as it wasn't already confirmed to be a game.
    fn recheck(&mut self) {
        if let Some(Report::Window(w)) = super::focused() {
            let key = (w.pid, w.class.clone());
            if let Some((verdict, at)) = self.known.get_mut(&key) {
                if *verdict != Some(true) && at.elapsed() >= RECHECK {
                    *verdict = matcher::is_game(&w);
                    *at = Instant::now();
                }
            }
        }
    }

    /// The focus is on something that isn't the game. Where that is isn't always known: with
    /// several monitors it may be beside the game, whose picture then stays.
    fn elsewhere(&self, monitor: Option<&str>) -> Foreground {
        let same_monitor = match monitor {
            Some(name) => super::same_monitor(name, &self.monitor),
            None => super::single_monitor(),
        };
        Foreground::Other { same_monitor }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_window_is_decided_at_once() {
        let mut gate = Gate::new("monitor".into());
        let class = "no_such_game_class_12345".to_string();
        gate.known.insert((None, class.clone()), (Some(true), Instant::now()));
        let window = super::super::Window { class, ..Default::default() };
        assert_eq!(gate.is_game(&window), Some(true));
    }

    /// Prints when the card comes and goes in the running session: LAPSE_LIVE_SECS seconds
    /// for the monitor LAPSE_LIVE_MONITOR.
    #[test]
    #[ignore]
    fn watch_live() {
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        let now = || {
            let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
            t.tv_sec as f64 + t.tv_nsec as f64 / 1e9
        };
        crate::focus::start();
        let dir = std::env::temp_dir().join(format!("lapse-card-test-{}", std::process::id()));
        let card = Card::start(&dir, var("LAPSE_LIVE_MONITOR"), "Oyun ekranında değil".into()).unwrap();
        let shared = {
            use std::os::fd::AsRawFd;
            let file = std::fs::OpenOptions::new().read(true).write(true).open(&card.file).unwrap();
            let map = unsafe { libc::mmap(std::ptr::null_mut(), FILE_SIZE, 3, libc::MAP_SHARED, file.as_raw_fd(), 0) };
            Shared(map.cast())
        };
        shared.field(FRAME_H).store(720, Relaxed);
        shared.field(FRAME_W).store(1280, Release);
        let pid_file = std::env::var_os("LAPSE_LIVE_PID_FILE").filter(|f| !f.is_empty());
        let mut last_pid_read = Instant::now() - Duration::from_secs(1);
        let end = Instant::now() + Duration::from_secs(var("LAPSE_LIVE_SECS").parse().unwrap_or(10));
        let (mut away, mut report) = (0, None);
        while Instant::now() < end {
            if let Some(file) = &pid_file {
                if last_pid_read.elapsed() >= Duration::from_secs(1) {
                    let pid = std::fs::read_to_string(file).ok().and_then(|s| s.trim().parse::<u32>().ok());
                    crate::focus::update_game(pid, &[]);
                    last_pid_read = Instant::now();
                }
            }
            let found = super::super::focused();
            if found != report {
                println!("{:.4} focus {found:?}", now());
                report = found;
            }
            let a = shared.field(AWAY).load(Acquire);
            if a != away {
                println!("{:.4} AWAY {a} (picture {})", now(), shared.field(SEQ).load(Acquire));
                away = a;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(card);
        println!("{:.4} end, AWAY {}", now(), shared.field(AWAY).load(Acquire));
    }

    /// Records the monitor LAPSE_LIVE_MONITOR with the real gpu-screen-recorder into
    /// LAPSE_LIVE_DIR, with a made-up focus: game, other window, game.
    #[test]
    #[ignore]
    fn record_live() {
        use super::super::{report, Source, Window};
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        let config = crate::config::Config {
            monitor: var("LAPSE_LIVE_MONITOR"),
            save_path: var("LAPSE_LIVE_DIR").into(),
            capture_mode: "game".into(),
            language: "tr".into(),
            video_codec: var("LAPSE_LIVE_CODEC"),
            resolution: var("LAPSE_LIVE_SIZE"),
            replay_seconds: 30,
            save_sound: false,
            save_notification: false,
            ..Default::default()
        };
        let focus = |class: &str| {
            let w = Window { class: class.into(), monitor: Some(var("LAPSE_LIVE_MONITOR")), ..Default::default() };
            report(Source::X11, Some(Report::Window(w)));
        };
        let sleep = |ms| std::thread::sleep(Duration::from_millis(ms));
        let mut recorder = crate::recorder::Recorder::new(config);
        recorder.enter_game_wait();
        recorder.on_game_tick(Some("Test".into()));
        assert!(recorder.is_running());
        sleep(3000);
        focus("steam_app_1");
        sleep(2000);
        for _ in 0..3 {
            focus("other");
            sleep(2000);
            focus("steam_app_1");
            sleep(1500);
        }
        assert!(recorder.is_running());
        recorder.save_replay().unwrap();
        sleep(4000);
        recorder.stop().unwrap();
    }
}
