//! Automatic clips through Valve's Game State Integration (GSI).
//!
//! Supported games post their state to a local HTTP endpoint described by a cfg file in
//! the game folder. Each game uses a tracker for its kind of play (`rounds.rs`, `streaks.rs`) that decides
//! when something clip-worthy happened; Lapse then saves the last part of the replay
//! buffer without clearing it.

pub mod lol;
mod rounds;
mod streaks;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::recorder::Recorder;

pub const PORT: u16 = 27532;
const CFG_NAME: &str = "gamestate_integration_lapse.cfg";

/// What one game state update means for a tracker.
#[derive(Debug, PartialEq)]
pub enum Action {
    Nothing,
    /// Save from the first kill (`secs` ago; the configured lead-in is added);
    /// `label` goes into the file name.
    Save { secs: u32, kills: u32, label: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Game {
    Cs2,
    Dota2,
    Lol,
}

pub static LAST_UPDATE: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

impl Game {
    pub fn name(self) -> &'static str {
        match self {
            Game::Cs2 => "Counter-Strike 2",
            Game::Dota2 => "Dota 2",
            Game::Lol => "League of Legends",
        }
    }

    fn token(self) -> &'static str {
        match self {
            Game::Cs2 => "lapse-gsi",
            Game::Dota2 => "lapse-gsi-dota",
            Game::Lol => "",
        }
    }

    fn from_token(token: &str) -> Option<Game> {
        [Game::Cs2, Game::Dota2].into_iter().find(|g| g.token() == token)
    }

    fn index(self) -> usize {
        self as usize
    }

    /// (enabled, minimum kills) from the config.
    fn settings(self, config: &Config) -> (bool, u32) {
        match self {
            Game::Cs2 => (config.cs2_auto_clip, config.cs2_min_kills),
            Game::Dota2 => (config.dota_auto_clip, config.dota_min_kills),
            Game::Lol => (config.lol_auto_clip, config.lol_min_kills),
        }
    }

    /// Seconds the clip keeps from before the first kill.
    fn lead_in(self, config: &Config) -> u32 {
        match self {
            Game::Cs2 => config.cs2_lead_in,
            Game::Dota2 => config.dota_lead_in,
            Game::Lol => config.lol_lead_in,
        }
    }

    /// The game's folder (inside any Steam library) and where its GSI cfg goes.
    fn paths(self) -> (&'static str, &'static str) {
        match self {
            Game::Cs2 => ("Counter-Strike Global Offensive/game/csgo/cfg", ""),
            Game::Dota2 => ("dota 2 beta/game/dota/cfg", "gamestate_integration"),
            Game::Lol => ("", ""),
        }
    }

    /// Folder the cfg is written to, if the game is installed.
    pub fn cfg_dir(self) -> Option<PathBuf> {
        if self == Game::Lol {
            return None;
        }
        let (cfg, sub) = self.paths();
        crate::hardware::steam::library_dirs()
            .into_iter()
            .map(|lib| lib.join("steamapps/common").join(cfg))
            .find(|p| p.is_dir())
            .map(|p| if sub.is_empty() { p } else { p.join(sub) })
    }

    pub fn is_installed(self) -> bool {
        if self == Game::Lol {
            return false;
        }
        self.cfg_dir().map(|d| d.join(CFG_NAME).is_file()).unwrap_or(false)
    }

    fn data_section(self) -> &'static str {
        match self {
            Game::Cs2 => "    \"provider\" \"1\"\n    \"map\" \"1\"\n    \"round\" \"1\"\n    \"player_id\" \"1\"\n    \"player_state\" \"1\"\n    \"player_match_stats\" \"1\"\n",
            Game::Dota2 => "    \"provider\" \"1\"\n    \"map\" \"1\"\n    \"player\" \"1\"\n    \"hero\" \"1\"\n",
            Game::Lol => "",
        }
    }

    fn cfg_text(self) -> String {
        format!(
            "\"Lapse\"\n{{\n  \"uri\" \"http://127.0.0.1:{PORT}/\"\n  \"timeout\" \"1.0\"\n  \"buffer\" \"0.0\"\n  \"throttle\" \"0.2\"\n  \"heartbeat\" \"30.0\"\n  \"auth\" {{ \"token\" \"{}\" }}\n  \"data\"\n  {{\n{}  }}\n}}\n",
            self.token(),
            self.data_section()
        )
    }

    /// Writes the GSI cfg into the game's folder. The game reads it on its next start.
    pub fn install(self) -> anyhow::Result<PathBuf> {
        let dir = self
            .cfg_dir()
            .ok_or_else(|| anyhow::anyhow!("{} was not found in your Steam libraries", self.name()))?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(CFG_NAME);
        std::fs::write(&path, self.cfg_text())?;
        Ok(path)
    }

    /// Brings a cfg written by an older Lapse up to date (the game picks it up on its next start).
    fn refresh_installed(self) {
        if let Some(path) = self.cfg_dir().map(|d| d.join(CFG_NAME)) {
            if path.is_file() && std::fs::read_to_string(&path).ok().as_deref() != Some(self.cfg_text().as_str()) {
                let _ = std::fs::write(&path, self.cfg_text());
            }
        }
    }

    pub fn uninstall(self) {
        if let Some(dir) = self.cfg_dir() {
            let _ = std::fs::remove_file(dir.join(CFG_NAME));
        }
    }

    /// Seconds since the game last sent data, if it ever did (heartbeat: every 30 s).
    pub fn seconds_since_last_update(self) -> Option<u64> {
        let last = LAST_UPDATE[self.index()].load(Ordering::Relaxed);
        (last > 0).then(|| unix_now().saturating_sub(last))
    }
}

/// "de_dust2" -> "dust2", "npc_dota_hero_axe" -> "axe".
fn short_name(raw: &str, prefixes: &[&str]) -> String {
    let mut name = raw.rsplit('/').next().unwrap_or(raw);
    for p in prefixes {
        if let Some(rest) = name.strip_prefix(p) {
            name = rest;
        }
    }
    name.to_string()
}

/// File name label: "dust2_3K", or just "3K" without a map name.
fn label(name: &str, what: &str) -> String {
    if name.is_empty() { what.to_string() } else { format!("{}_{}", name, what) }
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One tracker per game.
#[derive(Default)]
struct Trackers {
    cs2: rounds::RoundTracker,
    dota2: streaks::StreakTracker,
}

impl Trackers {
    fn update(&mut self, game: Game, state: &serde_json::Value, min_kills: u32, now: Instant) -> Action {
        match game {
            Game::Cs2 => self.cs2.update(state, min_kills, now),
            Game::Dota2 => self.dota2.update(state, min_kills, now),
            Game::Lol => Action::Nothing,
        }
    }

    fn take_new_kills(&mut self, game: Game) -> u32 {
        match game {
            Game::Cs2 => self.cs2.take_new_kills(),
            Game::Dota2 => self.dota2.take_new_kills(),
            Game::Lol => 0,
        }
    }

    fn poll(&mut self, game: Game, min_kills: u32, now: Instant) -> Action {
        match game {
            Game::Cs2 => self.cs2.poll(min_kills, now),
            Game::Dota2 => self.dota2.poll(min_kills, now),
            Game::Lol => Action::Nothing,
        }
    }
}

pub(crate) fn run(game: Game, action: Action, recorder: &Arc<Mutex<Recorder>>) {
    if let Action::Save { secs, kills, label } = action {
        if let Ok(mut rec) = recorder.lock() {
            let secs = secs + game.lead_in(rec.config());
            let label = (!label.is_empty()).then_some(label);
            match rec.save_last_named(secs, Some(game.name().into()), label) {
                Ok(()) => eprintln!("Lapse: {} auto clip ({} kills, last {} s)", game.name(), kills, secs),
                Err(e) => eprintln!("Lapse: {} auto clip failed: {}", game.name(), e),
            }
        }
    }
}

fn settings(recorder: &Arc<Mutex<Recorder>>, game: Game) -> Option<(bool, u32)> {
    let rec = recorder.lock().ok()?;
    Some(game.settings(rec.config()))
}

/// Reads one HTTP POST and returns its JSON body.
fn read_request(stream: &mut TcpStream) -> Option<serde_json::Value> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().ok()?;
            }
        }
    }
    if length == 0 || length > 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

/// Listens on 127.0.0.1 for game state. Costs nothing while no supported game runs.
pub fn start_server(recorder: Arc<Mutex<Recorder>>) {
    lol::start_poller(Arc::clone(&recorder));
    let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Lapse: auto clips unavailable (port {} busy: {})", PORT, e);
            return;
        }
    };
    for game in [Game::Cs2, Game::Dota2] {
        game.refresh_installed();
    }
    let trackers = Arc::new(Mutex::new(Trackers::default()));

    // Trackers that wait for a quiet moment after the last kill are checked every second.
    let (rec_poll, trackers_poll) = (Arc::clone(&recorder), Arc::clone(&trackers));
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        for game in [Game::Cs2, Game::Dota2] {
            let Some((enabled, min_kills)) = settings(&rec_poll, game) else { return };
            if enabled {
                let action = trackers_poll.lock().map(|mut t| t.poll(game, min_kills, Instant::now())).unwrap_or(Action::Nothing);
                run(game, action, &rec_poll);
            }
        }
    });

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let body = read_request(&mut stream);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let Some(state) = body else { continue };
            let Some(game) = state.pointer("/auth/token").and_then(|v| v.as_str()).and_then(Game::from_token) else {
                continue;
            };
            LAST_UPDATE[game.index()].store(unix_now(), Ordering::Relaxed);
            let Some((enabled, min_kills)) = settings(&recorder, game) else { return };
            if !enabled {
                continue;
            }
            let (action, kills) = trackers
                .lock()
                .map(|mut t| (t.update(game, &state, min_kills, Instant::now()), t.take_new_kills(game)))
                .unwrap_or((Action::Nothing, 0));
            if kills > 0 {
                if let Ok(mut rec) = recorder.lock() {
                    rec.add_kill_marks(kills);
                }
            }
            run(game, action, &recorder);
        }
    });
}
