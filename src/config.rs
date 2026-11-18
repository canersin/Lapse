use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::Result;
use directories::ProjectDirs;
use global_hotkey::hotkey::Code;

use crate::hotkeys::HotkeyEvent;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub save_path: PathBuf,
    pub recorder_path: String,
    pub replay_seconds: u32,
    pub hotkey_replay: Code,
    pub hotkey_replay_mod: u32,
    pub hotkey_record: Code,
    pub hotkey_record_mod: u32,
    /// Saves only the last `short_save_seconds` (keeps the buffer).
    pub hotkey_short: Code,
    pub hotkey_short_mod: u32,
    pub short_save_seconds: u32,
    /// Saves a PNG of the recorded monitor into the clip folder.
    pub hotkey_screenshot: Code,
    pub hotkey_screenshot_mod: u32,
    /// Marks the current moment; markers show on the trimmer timeline.
    pub hotkey_marker: Code,
    pub hotkey_marker_mod: u32,
    /// Bind the three shortcuts above (save last part, screenshot, marker).
    pub extra_hotkeys: bool,
    /// Moves the audio of saved clips (ms, positive = later) to fix A/V offset.
    pub audio_offset_ms: i32,
    /// Record in HDR (10-bit AV1/HEVC). Needs an HDR monitor and monitor capture.
    pub hdr: bool,
    /// Discord webhook URL for "Send to Discord" in the trimmer.
    pub discord_webhook: String,
    /// Show "<game> with Lapse" on the user's Discord profile while a game is open.
    pub discord_rpc: bool,
    /// Adds a "Get Lapse" button (to the GitHub page) to that Discord status.
    pub discord_rpc_button: bool,
    /// Save a replay with a button combination on a gamepad.
    pub gamepad_hotkey: bool,
    /// Linux input button codes that must be held together (default Select + R3).
    pub gamepad_buttons: Vec<u16>,
    pub audio_output: String,
    pub audio_input: String,
    pub quality: String,
    pub bitrate_kbps: u32,
    pub fps: u32,
    pub resolution: String,
    pub monitor: String,
    /// Encoder GPU: the card gpu-screen-recorder captures and encodes on.
    pub gpu: String,
    /// Dual GPU mode: the card games are rendered on (via `Lapse run -- %command%`).
    pub dual_gpu: bool,
    pub game_gpu: String,
    pub theme_palette: String,
    /// "ram" keeps the replay buffer in memory, "disk" writes it to the save folder.
    pub replay_storage: String,
    /// Record desktop audio and microphone as two separate audio tracks.
    pub separate_audio_tracks: bool,
    /// Record only these applications' audio instead of the whole desktop output.
    pub audio_apps: Vec<String>,
    /// Delete the oldest clips when the clip folder grows past `cleanup_max_gb`.
    pub cleanup_enabled: bool,
    pub cleanup_max_gb: u32,
    /// "auto" uses the Wayland GlobalShortcuts portal when available plus X11 grabs.
    pub hotkey_backend: String,
    /// Share: fall back to third-party SSH tunnels when Cloudflare doesn't work.
    pub share_allow_ssh_fallback: bool,
    /// UI language: "en" or "tr".
    pub language: String,
    pub first_run_done: bool,
    /// Clear the replay buffer after every save. Off: back-to-back clips may overlap.
    pub restart_replay_on_save: bool,
    /// Play the shutter sound / show a desktop notification when a clip is saved.
    pub save_sound: bool,
    /// Sound file played instead of the built-in shutter; empty = shutter.
    pub save_sound_path: String,
    pub save_notification: bool,
    /// "desktop": the replay buffer always runs. "game": it only runs while a game is open.
    pub capture_mode: String,
    /// Games added by the user, and per-game recording profiles.
    pub games: Vec<GameEntry>,
    /// Counter-Strike 2: save a clip automatically after a multi-kill round.
    pub cs2_auto_clip: bool,
    pub cs2_min_kills: u32,
    /// CS2 auto clips start this many seconds before the first kill.
    pub cs2_lead_in: u32,
    /// Dota 2: save a clip automatically after a multi-kill (double kill and up).
    pub dota_auto_clip: bool,
    pub dota_min_kills: u32,
    /// Dota 2 auto clips start this many seconds before the first kill.
    pub dota_lead_in: u32,
    pub video_codec: String,
    pub record_only_game: bool,
    pub autostart: bool,
    pub check_updates: bool,
    /// Width of the sidebar while it is expanded.
    pub sidebar_width: f32,
    /// Sidebar shrunk to icons only.
    pub sidebar_collapsed: bool,
}

/// A game the user added by hand (e.g. Minecraft, which runs as "java"), and/or recording
/// settings for one game. Clips of a detected game are saved in a folder named `name`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct GameEntry {
    pub name: String,
    /// Matched (case-insensitive) against process names, command lines and window titles.
    /// Filled in when the game is picked from the running programs; empty: the name is used.
    pub matcher: String,
    /// 0 keeps the global setting.
    pub fps: u32,
    pub bitrate_kbps: u32,
}

impl GameEntry {
    /// The word that finds the game: the matcher, or the name when there is none.
    pub fn detect_word(&self) -> &str {
        if self.matcher.trim().is_empty() { self.name.trim() } else { self.matcher.trim() }
    }
}

impl Config {
    /// Every bound global shortcut: (action, key, modifier bits).
    pub fn hotkey_bindings(&self) -> Vec<(HotkeyEvent, Code, u32)> {
        let mut list = vec![
            (HotkeyEvent::SaveReplay, self.hotkey_replay, self.hotkey_replay_mod),
            (HotkeyEvent::ToggleRecord, self.hotkey_record, self.hotkey_record_mod),
        ];
        if self.extra_hotkeys {
            list.push((HotkeyEvent::SaveShort, self.hotkey_short, self.hotkey_short_mod));
            list.push((HotkeyEvent::Screenshot, self.hotkey_screenshot, self.hotkey_screenshot_mod));
            list.push((HotkeyEvent::Marker, self.hotkey_marker, self.hotkey_marker_mod));
        }
        list
    }

    /// The key and modifier fields of one shortcut, for rebinding it.
    pub fn hotkey_mut(&mut self, event: HotkeyEvent) -> (&mut Code, &mut u32) {
        match event {
            HotkeyEvent::SaveReplay => (&mut self.hotkey_replay, &mut self.hotkey_replay_mod),
            HotkeyEvent::ToggleRecord => (&mut self.hotkey_record, &mut self.hotkey_record_mod),
            HotkeyEvent::SaveShort => (&mut self.hotkey_short, &mut self.hotkey_short_mod),
            HotkeyEvent::Screenshot => (&mut self.hotkey_screenshot, &mut self.hotkey_screenshot_mod),
            HotkeyEvent::Marker => (&mut self.hotkey_marker, &mut self.hotkey_marker_mod),
        }
    }

    pub fn is_game_mode(&self) -> bool {
        self.capture_mode == "game"
    }

    /// Recording profile (fps, kbps) for a game, if one is set.
    pub fn profile_for(&self, game: &str) -> Option<(u32, u32)> {
        self.games
            .iter()
            .find(|g| g.name.eq_ignore_ascii_case(game) && (g.fps > 0 || g.bitrate_kbps > 0))
            .map(|g| (g.fps, g.bitrate_kbps))
    }
}

pub fn get_default_save_path() -> PathBuf {
    if let Some(user_dirs) = directories::UserDirs::new() {
        if let Some(video_dir) = user_dirs.video_dir() {
            return video_dir.join("Lapse");
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(&home);
        let user_dirs_file = home_path.join(".config/user-dirs.dirs");
        if let Ok(content) = std::fs::read_to_string(user_dirs_file) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("XDG_VIDEOS_DIR=") {
                    let val = trimmed.trim_start_matches("XDG_VIDEOS_DIR=").trim_matches('"');
                    let expanded = if val.starts_with("$HOME/") {
                        home_path.join(&val[6..])
                    } else if val == "$HOME" {
                        home_path.clone()
                    } else {
                        PathBuf::from(val)
                    };
                    return expanded.join("Lapse");
                }
            }
        }

        for candidate in ["Videolar", "Videos", "Vidéos", "Vídeos", "Video"] {
            let p = home_path.join(candidate);
            if p.is_dir() {
                return p.join("Lapse");
            }
        }

        return home_path.join("Videos/Lapse");
    }

    PathBuf::from("/tmp/Lapse")
}

/// Turkish when the system locale is Turkish, English otherwise.
fn default_language() -> String {
    let lang = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();
    if lang.starts_with("tr") { "tr".into() } else { "en".into() }
}

/// Rough size of the replay buffer in megabytes for the current settings.
pub fn estimated_buffer_mb(config: &Config, width: u32, height: u32) -> f64 {
    let kbps = if config.quality.starts_with("qp_") {
        // Constant quality: scale a 1080p60 baseline by pixel rate.
        let base = match config.quality.as_str() {
            "qp_ultra" => 40000.0,
            "qp_very_high" => 25000.0,
            "qp_high" => 15000.0,
            _ => 10000.0,
        };
        let pixels = (width.max(1) as f64 * height.max(1) as f64) / (1920.0 * 1080.0);
        base * pixels.max(0.25) * (config.fps as f64 / 60.0).max(0.25)
    } else {
        config.bitrate_kbps as f64
    };
    // Video + ~256 kbps of audio.
    (kbps + 256.0) * 1000.0 / 8.0 * config.replay_seconds as f64 / (1024.0 * 1024.0)
}

/// Total RAM in megabytes, from /proc/meminfo.
pub fn total_ram_mb() -> f64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<f64>().ok())
        })
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            save_path: get_default_save_path(),
            recorder_path: "gpu-screen-recorder".into(),
            replay_seconds: 60,
            hotkey_replay: Code::F10,
            hotkey_replay_mod: 0,
            hotkey_record: Code::F9,
            hotkey_record_mod: 0,
            hotkey_short: Code::F10,
            hotkey_short_mod: 1 << 2,
            short_save_seconds: 30,
            hotkey_screenshot: Code::F10,
            hotkey_screenshot_mod: 1 << 1,
            // Alt+F10 is a window-manager shortcut on GNOME, so not that.
            hotkey_marker: Code::F9,
            hotkey_marker_mod: 1 << 2,
            extra_hotkeys: true,
            audio_offset_ms: 0,
            hdr: false,
            discord_webhook: String::new(),
            discord_rpc: false,
            discord_rpc_button: true,
            gamepad_hotkey: false,
            gamepad_buttons: vec![314, 318],
            audio_output: "default_output".into(),
            audio_input: "None".into(),
            quality: "high".into(),
            bitrate_kbps: 25000,
            fps: 60,
            resolution: "Native".into(),
            monitor: "focused".into(),
            gpu: "Auto".into(),
            dual_gpu: false,
            game_gpu: "Auto".into(),
            theme_palette: "Lavender".into(),
            replay_storage: "ram".into(),
            separate_audio_tracks: true,
            audio_apps: Vec::new(),
            cleanup_enabled: false,
            cleanup_max_gb: 50,
            hotkey_backend: "auto".into(),
            share_allow_ssh_fallback: false,
            language: default_language(),
            first_run_done: false,
            restart_replay_on_save: false,
            save_sound: true,
            save_sound_path: String::new(),
            save_notification: true,
            capture_mode: "desktop".into(),
            games: Vec::new(),
            cs2_auto_clip: false,
            cs2_min_kills: 3,
            cs2_lead_in: 8,
            dota_auto_clip: false,
            dota_min_kills: 3,
            dota_lead_in: 15,
            video_codec: "auto".into(),
            record_only_game: false,
            autostart: true,
            check_updates: true,
            sidebar_width: 260.0,
            sidebar_collapsed: false,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;
        if !config_path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }
        let content = std::fs::read_to_string(&config_path)?;
        let mut config: Config = match toml::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                // Never crash on a broken config: keep a backup and fall back to defaults.
                eprintln!("Lapse: config is invalid ({}), using defaults", e);
                let _ = std::fs::copy(&config_path, config_path.with_extension("toml.bak"));
                Self::default()
            }
        };
        if config.fps == 0 {
            config.fps = 60;
        }
        config.replay_seconds = config.replay_seconds.clamp(5, 3600);
        config.cs2_min_kills = config.cs2_min_kills.clamp(2, 5);
        config.audio_offset_ms = config.audio_offset_ms.clamp(-500, 500);
        if config.gamepad_buttons.is_empty() {
            config.gamepad_buttons = vec![314, 318];
        }
        if ![10, 30, 60, 300].contains(&config.short_save_seconds) {
            config.short_save_seconds = 30;
        }
        config.dota_min_kills = config.dota_min_kills.clamp(2, 5);
        config.cs2_lead_in = config.cs2_lead_in.clamp(3, 60);
        config.dota_lead_in = config.dota_lead_in.clamp(3, 60);
        config.games.retain(|g| !g.name.trim().is_empty());
        if !config.sidebar_width.is_finite() {
            config.sidebar_width = 260.0;
        }
        if config.bitrate_kbps < 500 {
            config.bitrate_kbps = match config.quality.as_str() {
                "ultra" => 50000,
                "high" => 25000,
                "medium" => 15000,
                "low" => 8000,
                _ => 25000,
            };
        }
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::get_config_path()?;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        // Write to a temp file and rename so a crash mid-write can't corrupt the config.
        let tmp = config_path.with_extension("toml.tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, &config_path)?;
        Self::sync_autostart(self.autostart);
        Ok(())
    }

    pub fn sync_autostart(enable: bool) {
        crate::init_system::sync_autostart(enable);
    }

    pub fn get_config_path() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "lapse", "lapse")
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
        Ok(proj_dirs.config_dir().join("config.toml"))
    }
}
