use crate::config::{Config, GameProfile};
use crate::ipc::StatusResponse;
use anyhow::Result;
#[cfg(target_os = "linux")]
use anyhow::Context;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RecordingMode {
    None,
    Replay,
    Continuous,
}

/// Parses a resolution string like "1920x1080" into (width, height).
#[allow(dead_code)]
pub fn parse_resolution(res: &str) -> Option<(u32, u32)> {
    let (w, h) = res.split_once('x')?;
    let w = w.trim().parse::<u32>().ok()?;
    let h = h.trim().parse::<u32>().ok()?;
    if w > 0 && h > 0 {
        Some((w, h))
    } else {
        None
    }
}

/// Clamps the target resolution to fit within the monitor dimensions while
/// preserving aspect ratio, never upscaling past the requested size.
#[allow(dead_code)]
pub fn clamp_resolution_to_monitor(
    target_w: u32,
    target_h: u32,
    monitor_w: u32,
    monitor_h: u32,
) -> (u32, u32) {
    if target_w == 0 || target_h == 0 || monitor_w == 0 || monitor_h == 0 {
        return (monitor_w, monitor_h);
    }
    let scale = (monitor_w as f64 / target_w as f64)
        .min(monitor_h as f64 / target_h as f64)
        .min(1.0);
    let mut w = ((target_w as f64 * scale).round() as u32) & !1;
    let mut h = ((target_h as f64 * scale).round() as u32) & !1;
    if w > monitor_w {
        w = monitor_w & !1;
    }
    if h > monitor_h {
        h = monitor_h & !1;
    }
    (w.max(2), h.max(2))
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime};

/// State written by the stdout/stderr reader threads of the running recorder.
#[derive(Default)]
struct Shared {
    /// Most recent error-looking line from gpu-screen-recorder's stderr.
    last_stderr_error: Mutex<Option<String>>,
    encode_card: Mutex<String>,
    last_saved: Mutex<Option<String>>,
    save_counter: AtomicU64,
    /// Game detected when the save hotkey was pressed; the replay is moved into its folder.
    pending_game: Mutex<Option<String>>,
    /// Exact length asked for by `save_last`; gsr only saves 10/30/60/300 s, so the
    /// saved file is cut down to this.
    pending_trim: Mutex<Option<u32>>,
    /// Moments marked with the marker hotkey (last two hours).
    markers: Mutex<Vec<(SystemTime, crate::markers::MarkKind)>>,
    /// Shifts the audio of saved clips by this many ms (positive = later).
    audio_offset_ms: std::sync::atomic::AtomicI32,
    /// Name suffix for the next saved clip ("dust2_3K"); set by auto clips.
    pending_label: Mutex<Option<String>>,
    /// Show a desktop notification for each saved clip.
    notify: AtomicBool,
    /// Track names for the saved clip (e.g. ["game", "chat", "mic"]).
    track_names: Mutex<Vec<String>>,
}

pub struct Recorder {
    config: Config,
    process: Option<Child>,
    /// What is currently running.
    mode: RecordingMode,
    /// What the user asked for. The watchdog brings `mode` back to this after a crash.
    desired: RecordingMode,
    installed: bool,
    shared: Arc<Shared>,
    started_at: Instant,
    restarts: u32,
    consecutive_failures: u32,
    next_restart: Option<Instant>,
    last_error: Option<String>,
    capture_target: String,
    warnings: Vec<String>,
    current_record_path: Option<PathBuf>,
    record_started: Option<SystemTime>,
    /// Game seen by the last `on_game_tick`.
    current_game: Option<String>,
    /// Per-game (fps, kbps, resolution) overrides in effect for the running replay buffer.
    profile: Option<GameProfile>,
    /// Game mode with no game open: the buffer is off until one starts.
    waiting_for_game: bool,
    no_game_ticks: u32,
    denoise_available: bool,
    last_monitor_check: Instant,
    /// The config asks for a RAM buffer, but memory was too low: it is kept on disk for now.
    memory_fallback: bool,
    last_memory_check: Instant,
    last_app_check: Instant,
    /// An app without its own track started playing: restart the buffer once the next clip
    /// has been saved (the save counter passes this value), so nothing buffered is lost.
    restart_after_save: Option<u64>,
    /// Shown instead of the picture while the player is out of the game (Games only mode).
    card: Option<crate::focus::Card>,
    /// How often in a row the game was seen on another monitor than the recorded one.
    moved_ticks: u32,
    /// Since when the recorded monitor is gone.
    monitor_gone: Option<Instant>,
}

/// Free memory (% of total) below which a running RAM buffer moves to disk.
const LOW_MEMORY_PCT: f64 = 15.0;
/// Free memory (% of total) needed before a fallen-back buffer may use RAM again.
const RECOVERED_PCT: f64 = 30.0;

/// Whether the replay buffer should be kept on disk. `fallback`: it already moved there
/// because memory ran low; it stays until memory has clearly recovered, so it doesn't
/// bounce between RAM and disk (each switch restarts the buffer).
fn buffer_on_disk(config: &Config, fallback: bool, buffer_mb: f64, avail_mb: f64, total_mb: f64) -> bool {
    if config.replay_storage == "disk" {
        return true;
    }
    if !config.low_memory_guard || avail_mb <= 0.0 || total_mb <= 0.0 {
        return false;
    }
    if fallback && avail_mb / total_mb * 100.0 < RECOVERED_PCT {
        return true;
    }
    buffer_mb > 0.5 * avail_mb
}

/// Monitor capture in the Flatpak needs gpu-screen-recorder's host helper, which only
/// runs from a system-wide install; otherwise Lapse records through the portal.
fn kms_capture_available() -> bool {
    if !crate::hardware::procs::in_flatpak() {
        return true;
    }
    let Ok(info) = std::fs::read_to_string("/.flatpak-info") else {
        return false;
    };
    for line in info.lines() {
        if let Some(path) = line.strip_prefix("app-path=") {
            return path.starts_with("/var/lib/flatpak/");
        }
    }
    false
}

/// True unless the monitor list is readable and lacks `name`.
fn monitor_connected(name: &str) -> bool {
    let monitors = crate::hardware::monitor::get_monitors();
    monitors.is_empty() || monitors.iter().any(|m| m.name == name)
}

const MAX_CONSECUTIVE_FAILURES: u32 = 6;

fn notify_async(summary: &str, body: &str) {
    crate::utils::send_notification(summary, body, "media-record");
}

fn resolve_app_name(configured: &str, running_apps: &[String]) -> String {
    for running in running_apps {
        if running.eq_ignore_ascii_case(configured) {
            return running.clone();
        }
    }
    configured.to_string()
}

pub fn is_pipewire_running() -> bool {
    let output = std::process::Command::new("pactl")
        .arg("info")
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.to_lowercase().contains("pipewire")
        }
        Err(_) => false,
    }
}

pub fn list_running_audio_apps() -> Vec<String> {
    let output = std::process::Command::new("gpu-screen-recorder")
        .arg("--list-application-audio")
        .output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !l.contains("[lapse]"))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Apps that get a track of their own: the ones the user listed, plus (with automatic
/// detection) the ones heard before, minus "Never record". First spelling wins.
fn separated_apps(config: &Config, heard_apps: &[String]) -> Vec<String> {
    let auto: &[String] = if config.auto_detect_apps { heard_apps } else { &[] };
    let mut apps: Vec<String> = Vec::new();
    for app in config.separate_apps.iter().chain(auto) {
        let excluded = config.excluded_apps.iter().any(|e| e.eq_ignore_ascii_case(app));
        if !excluded && !apps.iter().any(|a| a.eq_ignore_ascii_case(app)) {
            apps.push(app.clone());
        }
    }
    apps
}

pub fn build_gsr_audio_args(
    config: &Config,
    denoise_available: bool,
    running_audio_apps: &[String],
    heard_apps: &[String],
    is_pipewire: bool,
) -> (Vec<String>, Vec<String>) {
    let separate_apps = separated_apps(config, heard_apps);
    let enabled = |v: &str| !v.is_empty() && v != "None";
    let mic_enabled = enabled(&config.audio_input);
    let mic_source = if mic_enabled {
        let use_pipe = (config.noise_suppression || (config.mic_gain - 1.0).abs() >= 0.01) && denoise_available;
        if use_pipe {
            Some("lapse_denoise".to_string())
        } else {
            Some(config.audio_input.clone())
        }
    } else {
        None
    };

    if config.separate_app_audio {
        if !is_pipewire {
            eprintln!("PipeWire is not running (checked via pactl info); falling back to default output for separate app audio");
            let desktop = if enabled(&config.audio_output) {
                config.audio_output.clone()
            } else {
                "default_output".to_string()
            };
            if let Some(m) = mic_source {
                if config.separate_audio_tracks {
                    (vec![desktop, m], vec!["game".to_string(), "mic".to_string()])
                } else {
                    (vec![format!("{}|{}", desktop, m)], vec!["game".to_string()])
                }
            } else {
                (vec![desktop], vec!["game".to_string()])
            }
        } else {
            let mut combined = Vec::new();
            for app in separate_apps.iter().chain(config.excluded_apps.iter()) {
                let resolved = resolve_app_name(app, running_audio_apps);
                if !combined.iter().any(|e: &String| e.eq_ignore_ascii_case(&resolved)) {
                    combined.push(resolved);
                }
            }

            let mut args = Vec::new();
            let mut tracks = Vec::new();

            // Track A: Game & desktop
            let track_a = if !combined.is_empty() {
                combined
                    .iter()
                    .map(|a| format!("app-inverse:{}", a))
                    .collect::<Vec<_>>()
                    .join("|")
            } else if enabled(&config.audio_output) {
                config.audio_output.clone()
            } else {
                "default_output".to_string()
            };
            args.push(track_a);
            tracks.push("game".to_string());

            // Track B: Each chosen app on its own track
            let mut seen_apps = Vec::new();
            for app in &separate_apps {
                let resolved = resolve_app_name(app, running_audio_apps);
                if !seen_apps.iter().any(|e: &String| e.eq_ignore_ascii_case(&resolved)) {
                    seen_apps.push(resolved.clone());
                    args.push(format!("app:{}", resolved));
                    tracks.push(format!("app:{}", app));
                }
            }

            // Track C: Microphone
            if let Some(m) = mic_source {
                args.push(m);
                tracks.push("mic".to_string());
            }

            (args, tracks)
        }
    } else if !config.excluded_apps.is_empty() {
        if !is_pipewire {
            eprintln!("PipeWire is not running (checked via pactl info); falling back to default output for excluded apps");
            let desktop = if enabled(&config.audio_output) {
                config.audio_output.clone()
            } else {
                "default_output".to_string()
            };
            if let Some(m) = mic_source {
                if config.separate_audio_tracks {
                    (vec![desktop, m], vec!["game".to_string(), "mic".to_string()])
                } else {
                    (vec![format!("{}|{}", desktop, m)], vec!["game".to_string()])
                }
            } else {
                (vec![desktop], vec!["game".to_string()])
            }
        } else {
            let mut excluded = Vec::new();
            for app in &config.excluded_apps {
                let resolved = resolve_app_name(app, running_audio_apps);
                if !excluded.iter().any(|e: &String| e.eq_ignore_ascii_case(&resolved)) {
                    excluded.push(resolved);
                }
            }
            let track_a = excluded
                .iter()
                .map(|a| format!("app-inverse:{}", a))
                .collect::<Vec<_>>()
                .join("|");

            let mut args = vec![track_a];
            let mut tracks = vec!["game".to_string()];

            if let Some(m) = mic_source {
                if config.separate_audio_tracks {
                    args.push(m);
                    tracks.push("mic".to_string());
                } else {
                    args[0] = format!("{}|{}", args[0], m);
                }
            }

            (args, tracks)
        }
    } else {
        let desktop = if !config.audio_apps.is_empty() {
            Some(
                config
                    .audio_apps
                    .iter()
                    .map(|a| format!("app:{}", a))
                    .collect::<Vec<_>>()
                    .join("|"),
            )
        } else {
            enabled(&config.audio_output).then(|| config.audio_output.clone())
        };

        let mut sources = Vec::new();
        let mut tracks = Vec::new();
        if let Some(d) = desktop {
            sources.push(d);
            tracks.push("game".to_string());
        }
        if let Some(m) = mic_source {
            sources.push(m);
            tracks.push("mic".to_string());
        }

        if config.separate_audio_tracks || sources.len() < 2 {
            (sources, tracks)
        } else {
            (vec![sources.join("|")], vec!["game".to_string()])
        }
    }
}

fn detect_installed(config: &Config) -> bool {
    std::path::Path::new("/usr/local/bin/gpu-screen-recorder").exists()
        || std::path::Path::new("/usr/bin/gpu-screen-recorder").exists()
        || crate::utils::find_in_path(&config.recorder_path).is_some()
}

fn state_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "lapse", "lapse")
        .and_then(|d| d.state_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::temp_dir().join("lapse"))
}

pub fn log_path() -> PathBuf {
    state_dir().join("recorder.log")
}

fn pid_file() -> PathBuf {
    crate::ipc::socket_path().with_extension("gsr.pid")
}

fn send_signal(pid: u32, sig: libc::c_int) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, sig) == 0 }
}

/// Kills a gpu-screen-recorder left behind by a previous daemon that died without cleaning up
/// (otherwise it keeps the encoder busy and the new one fails to start).
pub fn cleanup_orphan() {
    let path = pid_file();
    if let Ok(pid_str) = std::fs::read_to_string(&path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            let comm = std::fs::read_to_string(format!("/proc/{}/comm", pid)).unwrap_or_default();
            if comm.trim().starts_with("gpu-screen-reco") {
                send_signal(pid, libc::SIGINT);
                let deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < deadline && Path::new(&format!("/proc/{}", pid)).exists() {
                    std::thread::sleep(Duration::from_millis(50));
                }
                if Path::new(&format!("/proc/{}", pid)).exists() {
                    send_signal(pid, libc::SIGKILL);
                }
            }
        }
        let _ = std::fs::remove_file(path);
    }
}

fn generate_thumbnail(base_save_path: &Path, video: &Path) {
    let out = crate::clip_index::thumb_path_for(base_save_path, video);
    crate::clip_index::generate_thumbnail(video, &out);
}

/// Called from the stdout reader thread when gpu-screen-recorder reports a saved replay.
fn handle_saved_replay(shared: &Shared, base_save_path: &Path, saved: PathBuf) {
    // The clip ends now; the steps below take a moment.
    let saved_at = SystemTime::now();
    let notify = shared.notify.load(Ordering::Relaxed);
    let game = shared.pending_game.lock().ok().and_then(|mut g| g.take());
    if let Some(secs) = shared.pending_trim.lock().ok().and_then(|mut t| t.take()) {
        trim_to_last(&saved, secs);
    }
    let offset = shared.audio_offset_ms.load(Ordering::Relaxed);
    if offset != 0 {
        shift_audio(&saved, offset);
    }
    let label = shared.pending_label.lock().ok().and_then(|mut l| l.take());
    let saved = match label.map(|l| labelled_name(&saved, &l)) {
        Some(named) if std::fs::rename(&saved, &named).is_ok() => named,
        _ => saved,
    };
    let filename = saved
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let (final_path, shown) = match game {
        Some(game_name) => {
            let target_dir = base_save_path.join(&game_name);
            let _ = std::fs::create_dir_all(&target_dir);
            let rest = filename.strip_prefix("Replay_").unwrap_or(&filename);
            let name = crate::clip_index::game_file_name(&game_name, rest);
            let target = crate::clip_index::free_path(target_dir.join(name));
            if std::fs::rename(&saved, &target).is_ok() {
                let shown_name = target.file_name().unwrap_or_default().to_string_lossy().to_string();
                (target, format!("{}/{}", game_name, shown_name))
            } else {
                (saved, filename)
            }
        }
        None => (saved, filename),
    };
    if notify {
        crate::utils::send_clip_notification("Lapse - Replay Saved", &format!("Saved to {}", shown), final_path.clone());
    }

    if let Ok(marks) = shared.markers.lock() {
        let duration = crate::clip_index::probe_duration(&final_path);
        let offsets = crate::markers::offsets_in_clip(&marks, saved_at, duration);
        crate::markers::save(base_save_path, &final_path, &offsets);
    }
    if let Ok(tracks) = shared.track_names.lock() {
        if !tracks.is_empty() {
            crate::tracks::save(base_save_path, &final_path, &tracks);
        }
    }
    generate_thumbnail(base_save_path, &final_path);
    if let Ok(mut last) = shared.last_saved.lock() {
        *last = Some(final_path.to_string_lossy().to_string());
    }
    shared.save_counter.fetch_add(1, Ordering::SeqCst);
}

/// ".Replay_x.mp4.trim.mp4" next to the clip: hidden, so the library ignores it.
fn hidden_temp(path: &Path, tag: &str) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{}.{}.mp4", name, tag))
}

/// "Replay_2026-11-16_00-47-30.mp4" + "dust2_3K" -> "Replay_2026-11-16_00-47-30_dust2_3K.mp4".
fn labelled_name(path: &Path, label: &str) -> PathBuf {
    let clean: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{}_{}.mp4", stem, clean.trim_matches('_')))
}

/// Moves every audio track by `ms` to fix audio that runs ahead (positive: play later)
/// or behind. The picture is copied; only the sound is encoded again, which is quick.
/// (A plain `-itsoffset` stream copy doesn't survive in MP4.)
fn shift_audio(path: &Path, ms: i32) {
    let filter = if ms > 0 {
        format!("adelay={}:all=1", ms)
    } else {
        format!("atrim=start={:.3},asetpts=PTS-STARTPTS", -ms as f64 / 1000.0)
    };
    let tmp = hidden_temp(path, "sync");
    let ok = crate::utils::ffmpeg_cmd()
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-map", "0:v", "-map", "0:a?", "-c:v", "copy", "-af", &filter, "-c:a", "libopus", "-b:a", "160k"])
        .arg(&tmp)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !(ok && std::fs::rename(&tmp, path).is_ok()) {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Keeps only the last `secs` seconds (stream copy, so it starts at the keyframe before).
pub(crate) fn trim_to_last(path: &Path, secs: u32) {
    let duration = crate::clip_index::probe_duration(path);
    if duration <= secs as f64 + 0.05 {
        return;
    }
    let tmp = hidden_temp(path, "trim");
    // Stream copy starts at the keyframe before the cut; the mp4 edit list ffmpeg writes hides
    // the frames before it, so the clip is exactly `secs` long. make_zero would undo that.
    let ok = crate::utils::ffmpeg_cmd()
        .args(["-y", "-loglevel", "error", "-sseof"])
        .arg(format!("-{}", secs))
        .arg("-i")
        .arg(path)
        .args(["-map", "0", "-c", "copy"])
        .arg(&tmp)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok && std::fs::rename(&tmp, path).is_ok() {
        return;
    }
    let _ = std::fs::remove_file(&tmp);
}

fn is_error_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    if l.contains("trying ") || l.contains("ignoring") || l.starts_with("gsr info") {
        return false;
    }
    l.starts_with("gsr error") || l.contains("error:") || l.contains(" failed")
}

impl Recorder {
    pub fn new(config: Config) -> Self {
        let installed = detect_installed(&config);
        Self {
            config,
            process: None,
            mode: RecordingMode::None,
            desired: RecordingMode::None,
            installed,
            shared: Arc::new(Shared::default()),
            started_at: Instant::now(),
            restarts: 0,
            consecutive_failures: 0,
            next_restart: None,
            last_error: None,
            capture_target: String::new(),
            warnings: Vec::new(),
            current_record_path: None,
            record_started: None,
            current_game: None,
            profile: None,
            waiting_for_game: false,
            no_game_ticks: 0,
            denoise_available: false,
            last_monitor_check: Instant::now(),
            memory_fallback: false,
            last_memory_check: Instant::now(),
            last_app_check: Instant::now(),
            restart_after_save: None,
            card: None,
            moved_ticks: 0,
            monitor_gone: None,
        }
        .with_flags()
    }

    fn with_flags(self) -> Self {
        self.shared.notify.store(self.config.save_notification, Ordering::Relaxed);
        self.shared.audio_offset_ms.store(self.config.audio_offset_ms, Ordering::Relaxed);
        self
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn update_config(&mut self, config: Config) -> Result<()> {
        // Only settings that change the gpu-screen-recorder command line require a restart;
        // theme, hotkeys, autostart etc. must not wipe the replay buffer.
        let recording_view = |c: &Config| {
            let mut c = c.clone();
            let d = Config::default();
            c.theme_palette = d.theme_palette;
            c.autostart = d.autostart;
            c.check_updates = d.check_updates;
            c.sidebar_width = d.sidebar_width;
            c.sidebar_collapsed = d.sidebar_collapsed;
            c.discord_rpc = d.discord_rpc;
            c.discord_rpc_button = d.discord_rpc_button;
            c.hotkey_replay = d.hotkey_replay;
            c.hotkey_replay_mod = d.hotkey_replay_mod;
            c.hotkey_record = d.hotkey_record;
            c.hotkey_record_mod = d.hotkey_record_mod;
            c.game_gpu = d.game_gpu;
            c.cleanup_enabled = d.cleanup_enabled;
            c.cleanup_max_gb = d.cleanup_max_gb;
            c.cleanup_old_enabled = d.cleanup_old_enabled;
            c.cleanup_old_days = d.cleanup_old_days;
            c.hotkey_backend = d.hotkey_backend;
            c.share_allow_ssh_fallback = d.share_allow_ssh_fallback;
            c.language = d.language;
            c.first_run_done = d.first_run_done;
            c.save_sound = d.save_sound;
            c.save_sound_path = d.save_sound_path.clone();
            c.save_notification = d.save_notification;
            c.capture_mode = d.capture_mode;
            // Per-game settings are applied through `profile` below.
            c.games = d.games;
            c.auto_game_profiles = d.auto_game_profiles;
            // Read when the buffer next starts; switching it alone must not drop the buffer.
            c.low_memory_guard = d.low_memory_guard;
            c.cs2_auto_clip = d.cs2_auto_clip;
            c.cs2_min_kills = d.cs2_min_kills;
            c.dota_auto_clip = d.dota_auto_clip;
            c.dota_min_kills = d.dota_min_kills;
            c.lol_auto_clip = d.lol_auto_clip;
            c.lol_min_kills = d.lol_min_kills;
            c.cs2_lead_in = d.cs2_lead_in;
            c.dota_lead_in = d.dota_lead_in;
            c.lol_lead_in = d.lol_lead_in;
            c.hotkey_short = d.hotkey_short;
            c.hotkey_short_mod = d.hotkey_short_mod;
            c.short_save_seconds = d.short_save_seconds;
            c.hotkey_screenshot = d.hotkey_screenshot;
            c.hotkey_screenshot_mod = d.hotkey_screenshot_mod;
            c.hotkey_marker = d.hotkey_marker;
            c.hotkey_marker_mod = d.hotkey_marker_mod;
            c.extra_hotkeys = d.extra_hotkeys;
            // The gain is applied live in the mic pipe; only whether the pipe is used matters.
            c.mic_gain = if !c.noise_suppression && (c.mic_gain - 1.0).abs() >= 0.01 { 2.0 } else { 1.0 };
            c.audio_offset_ms = d.audio_offset_ms;
            c.discord_webhook = d.discord_webhook.clone();
            c.gamepad_hotkey = d.gamepad_hotkey;
            c.gamepad_buttons = d.gamepad_buttons.clone();
            c.noise_source = d.noise_source.clone();
            c.voice_trigger_enabled = d.voice_trigger_enabled;
            c.voice_sensitivity = d.voice_sensitivity;
            c.voice_beep = d.voice_beep;
            c
        };
        let needs_restart = recording_view(&self.config) != recording_view(&config);
        let mode_changed = self.config.capture_mode != config.capture_mode;

        self.config = config;
        self.shared.notify.store(self.config.save_notification, Ordering::Relaxed);
        self.shared.audio_offset_ms.store(self.config.audio_offset_ms, Ordering::Relaxed);
        self.installed = detect_installed(&self.config);

        // The running game's profile was edited, or automatic profiles were switched on/off:
        // apply it now instead of when the game next opens or closes.
        let profile = self.current_game.as_deref().and_then(|g| self.config.profile_for(g));
        let profile_changed = profile != self.profile && self.mode == RecordingMode::Replay;
        self.profile = profile;

        if mode_changed && self.mode != RecordingMode::Continuous {
            if self.config.is_game_mode() && self.current_game.is_none() {
                // Switched to game mode on the desktop: turn the buffer off until a game opens.
                self.enter_game_wait();
                return Ok(());
            }
            if !self.config.is_game_mode() && self.waiting_for_game {
                self.waiting_for_game = false;
                return self.start_replay();
            }
        }

        // The card comes with the buffer's start; leaving game mode only takes it away.
        let needs_card = mode_changed && self.config.is_game_mode();
        if mode_changed {
            self.card = None;
        }
        if needs_restart || profile_changed || needs_card {
            self.restart()?;
        }
        Ok(())
    }

    pub fn set_denoise_available(&mut self, ok: bool) {
        if self.denoise_available != ok {
            self.denoise_available = ok;
            if self.is_running() {
                let _ = self.restart();
            }
        }
    }

    /// Restarts the running replay buffer so new settings take effect.
    pub fn restart(&mut self) -> Result<()> {
        if self.mode == RecordingMode::Continuous {
            return Ok(());
        }
        if self.process.is_some() {
            let desired = self.desired;
            self.kill_process(Duration::from_secs(3));
            self.mode = RecordingMode::None;
            self.desired = desired;
            if desired == RecordingMode::Replay {
                self.start_replay()?;
            }
        }
        Ok(())
    }

    fn base_command(&self) -> Command {
        let mut cmd = Command::new(&self.config.recorder_path);

        // Inside Flatpak the sandbox already sets up Wayland and the libraries.
        if !Path::new("/.flatpak-info").exists() {
            // Some distros ship an ffmpeg6 compat dir; add it without clobbering the user's path.
            let mut paths: Vec<String> = ["/usr/lib64/ffmpeg6", "/usr/lib/ffmpeg6"]
                .iter()
                .filter(|p| Path::new(p).is_dir())
                .map(|p| p.to_string())
                .collect();
            if !paths.is_empty() {
                if let Ok(existing) = std::env::var("LD_LIBRARY_PATH") {
                    if !existing.is_empty() {
                        paths.push(existing);
                    }
                }
                cmd.env("LD_LIBRARY_PATH", paths.join(":"));
            }
        }
        // gpu-screen-recorder refuses prime-run style variables; make sure a game launch
        // environment inherited by the daemon can't leak into it.
        cmd.env_remove("DRI_PRIME");
        cmd.env_remove("MESA_VK_DEVICE_SELECT");
        cmd
    }

    fn apply_quality_and_bitrate_args(&self, cmd: &mut Command) {
        let profile_kbps = self.profile.as_ref().map(|p| p.bitrate_kbps).filter(|k| *k > 0);
        let kbps = profile_kbps.unwrap_or(self.config.bitrate_kbps).clamp(1000, 250000);
        // A game profile with its own bitrate always uses that bitrate as the limit.
        let quality = if profile_kbps.is_some() { "custom" } else { self.config.quality.as_str() };
        match quality {
            "qp_ultra" => {
                cmd.arg("-bm").arg("qp").arg("-q").arg("ultra");
            }
            "qp_very_high" => {
                cmd.arg("-bm").arg("qp").arg("-q").arg("very_high");
            }
            "qp_high" => {
                cmd.arg("-bm").arg("qp").arg("-q").arg("high");
            }
            "qp_medium" => {
                cmd.arg("-bm").arg("qp").arg("-q").arg("medium");
            }
            _ => {
                cmd.arg("-bm").arg("cbr").arg("-q").arg(kbps.to_string());
            }
        }
    }

    /// gpu-screen-recorder `-a` values. Sources joined with `|` are mixed into one track;
    /// each separate `-a` becomes its own audio track.
    fn audio_args(&self) -> Vec<String> {
        let running_apps = list_running_audio_apps();
        let is_pw = is_pipewire_running();
        let heard = if self.config.separate_app_audio && self.config.auto_detect_apps {
            crate::audio_apps::remember(&running_apps);
            crate::audio_apps::load()
        } else {
            Vec::new()
        };
        // Apps picked from open windows are stored by program name; record their audio name.
        let mut cfg = self.config.clone();
        for list in [&mut cfg.separate_apps, &mut cfg.excluded_apps] {
            for app in list.iter_mut() {
                *app = crate::audio_apps::resolve_key(app);
            }
        }
        let (args, tracks) = build_gsr_audio_args(&cfg, self.denoise_available, &running_apps, &heard, is_pw);
        if let Ok(mut t) = self.shared.track_names.lock() {
            *t = tracks;
        }
        args
    }

    pub fn is_running(&mut self) -> bool {
        self.process
            .as_mut()
            .map(|p| p.try_wait().map(|s| s.is_none()).unwrap_or(false))
            .unwrap_or(false)
    }

    fn resolve_target_monitor(&self) -> String {
        if !kms_capture_available() {
            return "portal".to_string();
        }
        if let Some(m) = self.game_monitor() {
            return m;
        }
        let is_auto = self.config.monitor.is_empty()
            || self.config.monitor.eq_ignore_ascii_case("auto")
            || self.config.monitor.eq_ignore_ascii_case("screen")
            || self.config.monitor.eq_ignore_ascii_case("focused");

        if !is_auto {
            // A switched-off monitor is gone for gpu-screen-recorder too: record
            // another one until it is back (`tick` then switches back).
            if self.config.monitor == "portal" || monitor_connected(&self.config.monitor) {
                return self.config.monitor.clone();
            }
        }

        // Auto: record a monitor that is plugged into the encoder GPU, because
        // gpu-screen-recorder encodes on whichever GPU the monitor is connected to.
        let gpus = crate::hardware::gpu::get_gpus();
        let target_gpu = if !self.config.gpu.eq_ignore_ascii_case("auto") {
            gpus.iter()
                .find(|g| g.matches(&self.config.gpu) && !g.connected_monitors.is_empty())
                .or_else(|| gpus.iter().find(|g| g.is_primary))
                .or_else(|| gpus.first())
        } else {
            gpus.iter().find(|g| g.is_primary).or_else(|| gpus.first())
        };

        if let Some(gpu) = target_gpu {
            if let Some(first_mon) = gpu.connected_monitors.first() {
                return first_mon.clone();
            }
        }

        let monitors = crate::hardware::monitor::get_monitors();
        if let Some(mon) = monitors.first() {
            return mon.name.clone();
        }

        "focused".to_string()
    }

    /// The monitor the game is on, if that is the one to record.
    fn game_monitor(&self) -> Option<String> {
        if !self.follows_game() || self.config.monitor == "portal" {
            return None;
        }
        let names: Vec<String> = crate::hardware::monitor::get_monitors().into_iter().map(|m| m.name).collect();
        crate::focus::game_monitor(&names)
    }

    /// Starts the card that replaces the picture while the player is out of the game.
    fn start_card(&mut self, cmd: &mut Command, monitor: &str) {
        self.card = None;
        if !self.config.is_game_mode() || matches!(monitor, "portal" | "focused") || !crate::focus::plugin_supported(&self.config.recorder_path) {
            return;
        }
        let title = crate::gui::i18n::tr_in(&self.config.language, "Not on the game screen");
        match crate::focus::Card::start(&state_dir(), monitor.to_string(), title.to_string()) {
            Ok(card) => {
                cmd.arg("-p").arg(&card.plugin).env(crate::focus::CARD_ENV, &card.file);
                self.card = Some(card);
            }
            Err(e) => eprintln!("Lapse: focus card: {e}"),
        }
    }

    /// Record the monitor the game is on (game mode or the "record only game" option).
    pub fn follows_game(&self) -> bool {
        self.config.record_only_game || self.config.is_game_mode()
    }

    /// Roughly how big the replay buffer for `target` gets, with the game profile applied.
    fn estimated_buffer_mb(&self, target: &str) -> f64 {
        let mut cfg = self.config.clone();
        let profile = self.profile.clone().unwrap_or_default();
        if profile.fps > 0 {
            cfg.fps = profile.fps;
        }
        if profile.bitrate_kbps > 0 {
            cfg.bitrate_kbps = profile.bitrate_kbps;
            cfg.quality = "custom".into();
        }
        let size = parse_resolution(&profile.resolution)
            .or_else(|| parse_resolution(&self.config.resolution))
            .or_else(|| {
                crate::hardware::monitor::get_monitors().iter().find(|m| m.name == target).and_then(|m| parse_resolution(&m.resolution))
            })
            .unwrap_or((1920, 1080));
        crate::config::estimated_buffer_mb(&cfg, size.0, size.1)
    }

    fn compute_warnings(&self, target: &str) -> Vec<String> {
        let mut warnings = Vec::new();
        let gpus = crate::hardware::gpu::get_gpus();
        if self.config.dual_gpu {
            let report = crate::hardware::gpu::dual_gpu_report(
                &gpus,
                &self.config.game_gpu,
                &self.config.gpu,
                target,
            );
            warnings.extend(report.problems);
        } else if let Some(enc) = crate::hardware::gpu::find_gpu(&gpus, &self.config.gpu) {
            if let Some(mon_gpu) = crate::hardware::gpu::gpu_for_monitor(&gpus, target) {
                if mon_gpu.pci_id != enc.pci_id {
                    warnings.push(format!(
                        "{} is connected to {}, so encoding happens there instead of {}.",
                        target,
                        mon_gpu.short_name(),
                        enc.short_name()
                    ));
                }
            }
        }
        warnings
    }

    fn build_command(&self, target_monitor: &str) -> Command {
        let mut cmd = self.base_command();
        cmd.arg("-w")
            .arg(target_monitor)
            .arg("-f")
            .arg(self.profile.as_ref().map(|p| p.fps).filter(|f| *f > 0).unwrap_or(self.config.fps).to_string());

        if target_monitor == "portal" {
            // Restore portal session so the desktop remembers permission and only asks once.
            cmd.arg("-restore-portal-session").arg("yes");
        }

        if let Some(res) = self.profile.as_ref().map(|p| p.resolution.as_str()).filter(|s| !s.is_empty()) {
            cmd.arg("-s").arg(res);
        } else if self.config.resolution != "Native" {
            cmd.arg("-s").arg(&self.config.resolution);
        }

        let codec = self.config.video_codec.as_str();
        // HDR needs a 10-bit codec and monitor (KMS) capture; the portal only gives SDR.
        let hdr = self.config.hdr && target_monitor != "portal";
        match (hdr, codec) {
            (true, "av1") => {
                cmd.arg("-k").arg("av1_hdr");
            }
            (true, _) => {
                cmd.arg("-k").arg("hevc_hdr");
            }
            (false, c) if !c.is_empty() && c != "auto" => {
                cmd.arg("-k").arg(c);
            }
            _ => {}
        }

        self.apply_quality_and_bitrate_args(&mut cmd);
        cmd.arg("-c").arg("mp4");
        for audio in self.audio_args() {
            cmd.arg("-a").arg(audio);
        }
        cmd
    }

    /// Spawns gpu-screen-recorder with piped output and the reader threads that watch it.
    fn spawn(&mut self, mut cmd: Command, mode: RecordingMode) -> Result<()> {
        let _ = std::fs::create_dir_all(&self.config.save_path);
        let _ = std::fs::create_dir_all(state_dir());

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Ok(mut guard) = self.shared.last_stderr_error.lock() {
            *guard = None;
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to launch {}", self.config.recorder_path))?;

        let _ = std::fs::write(pid_file(), child.id().to_string());

        if let Some(stdout) = child.stdout.take() {
            let shared = Arc::clone(&self.shared);
            let base = self.config.save_path.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                    let p = PathBuf::from(line.trim());
                    let is_video = matches!(
                        p.extension().and_then(|e| e.to_str()),
                        Some("mp4") | Some("mkv")
                    );
                    if is_video && p.is_file() {
                        handle_saved_replay(&shared, &base, p);
                    }
                }
            });
        }

        if let Some(stderr) = child.stderr.take() {
            let shared = Arc::clone(&self.shared);
            let log = log_path();
            std::thread::spawn(move || {
                // Start a fresh log when it grows too large.
                let too_big = std::fs::metadata(&log).map(|m| m.len() > 2 * 1024 * 1024).unwrap_or(false);
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(!too_big)
                    .write(true)
                    .truncate(too_big)
                    .open(&log)
                    .ok();
                if let Some(f) = file.as_mut() {
                    let _ = writeln!(f, "--- {} gpu-screen-recorder started ---", chrono::Local::now().format("%F %T"));
                }
                for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
                    if line.contains("update fps:") {
                        continue;
                    }
                    if let Some(f) = file.as_mut() {
                        let _ = writeln!(f, "{}", line);
                    }
                    // "the monitor "DP-1" is connected to /dev/dri/card1, capturing and encoding on that gpu"
                    if let Some(idx) = line.find("connected to /dev/dri/") {
                        let rest = &line[idx + "connected to /dev/dri/".len()..];
                        let card: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                        if let Ok(mut c) = shared.encode_card.lock() {
                            *c = card;
                        }
                    }
                    if is_error_line(&line) {
                        if let Ok(mut e) = shared.last_stderr_error.lock() {
                            *e = Some(line.trim().to_string());
                        }
                    }
                }
            });
        }

        self.process = Some(child);
        self.mode = mode;
        self.started_at = Instant::now();
        self.next_restart = None;
        Ok(())
    }

    pub fn start_replay(&mut self) -> Result<()> {
        if self.mode == RecordingMode::Replay && self.is_running() {
            return Ok(());
        }
        if self.process.is_some() {
            self.stop_process()?;
        }

        self.desired = RecordingMode::Replay;
        self.waiting_for_game = false;
        self.last_error = None;
        self.restart_after_save = None;

        let target_monitor = self.resolve_target_monitor();
        self.warnings = self.compute_warnings(&target_monitor);
        self.capture_target = target_monitor.clone();

        let mut cmd = self.build_command(&target_monitor);
        self.start_card(&mut cmd, &target_monitor);
        cmd.arg("-r").arg(self.config.replay_seconds.to_string());
        let on_disk = buffer_on_disk(
            &self.config,
            self.memory_fallback,
            self.estimated_buffer_mb(&target_monitor),
            crate::config::available_ram_mb(),
            crate::config::total_ram_mb(),
        );
        self.memory_fallback = on_disk && self.config.replay_storage != "disk";
        if self.memory_fallback {
            eprintln!("Lapse: not enough free memory for a RAM replay buffer, keeping it on disk");
            self.warnings.push("Low memory: the replay buffer is kept on disk for now.".into());
        }
        if on_disk {
            cmd.arg("-replay-storage").arg("disk");
        }
        let restart = if self.config.restart_replay_on_save { "yes" } else { "no" };
        cmd.arg("-restart-replay-on-save").arg(restart);
        cmd.arg("-o").arg(&self.config.save_path);

        let res = self.spawn(cmd, RecordingMode::Replay);
        if let Err(e) = &res {
            self.last_error = Some(e.to_string());
            self.schedule_restart();
        }
        res
    }

    pub fn start_recording(&mut self) -> Result<()> {
        if self.process.is_some() {
            self.stop_process()?;
        }

        self.desired = RecordingMode::Continuous;
        self.last_error = None;

        let target_monitor = self.resolve_target_monitor();
        self.warnings = self.compute_warnings(&target_monitor);
        self.capture_target = target_monitor.clone();

        let mut cmd = self.build_command(&target_monitor);

        let _ = std::fs::create_dir_all(&self.config.save_path);
        let detected = crate::hardware::game_detector::detect_current_game(&self.config.games);
        let save_target = if let Some(game) = detected {
            let game_dir = self.config.save_path.join(&game.name);
            let _ = std::fs::create_dir_all(&game_dir);
            let rest = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S.mp4").to_string();
            let name = crate::clip_index::game_file_name(&game.name, &rest);
            crate::clip_index::free_path(game_dir.join(name))
        } else {
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            self.config.save_path.join(format!("record_{}.mp4", timestamp))
        };
        cmd.arg("-o").arg(&save_target);
        self.current_record_path = Some(save_target);
        self.record_started = Some(SystemTime::now());

        let res = self.spawn(cmd, RecordingMode::Continuous);
        if let Err(e) = &res {
            self.last_error = Some(e.to_string());
            // Fall back to the replay buffer instead of leaving nothing running.
            self.desired = RecordingMode::Replay;
            self.schedule_restart();
        }
        res
    }

    /// Sends SIGINT, waits for a clean exit and escalates to SIGKILL after `grace`.
    fn kill_process(&mut self, grace: Duration) -> bool {
        let Some(mut child) = self.process.take() else {
            return false;
        };
        let graceful = send_signal(child.id(), libc::SIGINT);
        let start = Instant::now();
        let mut exited_cleanly = false;
        while start.elapsed() < grace {
            if let Ok(Some(status)) = child.try_wait() {
                exited_cleanly = status.success() || status.code().is_none();
                break;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
            exited_cleanly = false;
        }
        let _ = child.wait();
        self.card = None;
        let _ = std::fs::remove_file(pid_file());
        graceful && exited_cleanly
    }

    /// Stops whatever is running without changing what the user asked for.
    fn stop_process(&mut self) -> Result<()> {
        let was = self.mode;
        // Finalizing a long mp4 can take a while; don't cut it off early.
        let grace = if was == RecordingMode::Continuous {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(3)
        };
        let clean = self.kill_process(grace);
        self.mode = RecordingMode::None;

        if was == RecordingMode::Continuous {
            if let Some(path) = self.current_record_path.take() {
                if clean && path.is_file() {
                    notify_async("Lapse", &format!("Recording saved: {}", path.file_name().unwrap_or_default().to_string_lossy()));
                    let base = self.config.save_path.clone();
                    let shared = Arc::clone(&self.shared);
                    let started = self.record_started.take();
                    std::thread::spawn(move || {
                        if let (Some(start), Ok(marks)) = (started, shared.markers.lock()) {
                            let duration = crate::clip_index::probe_duration(&path);
                            let offsets = crate::markers::offsets_since(&marks, start, duration);
                            crate::markers::save(&base, &path, &offsets);
                        }
                        if let Ok(tracks) = shared.track_names.lock() {
                            if !tracks.is_empty() {
                                crate::tracks::save(&base, &path, &tracks);
                            }
                        }
                        generate_thumbnail(&base, &path);
                        if let Ok(mut last) = shared.last_saved.lock() {
                            *last = Some(path.to_string_lossy().to_string());
                        }
                        shared.save_counter.fetch_add(1, Ordering::SeqCst);
                    });
                } else if !clean {
                    notify_async("Lapse", "Recording was interrupted; the file may be incomplete.");
                }
            }
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.desired = RecordingMode::None;
        self.next_restart = None;
        self.stop_process()
    }

    fn schedule_restart(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures > MAX_CONSECUTIVE_FAILURES {
            self.desired = RecordingMode::None;
            self.next_restart = None;
            notify_async(
                "Lapse - Recorder stopped",
                &format!(
                    "gpu-screen-recorder keeps crashing. {}",
                    self.last_error.clone().unwrap_or_default()
                ),
            );
            return;
        }
        // 1s, 2s, 4s, 8s ... capped at 30s.
        let delay = Duration::from_secs((1u64 << (self.consecutive_failures - 1).min(5)).min(30));
        self.next_restart = Some(Instant::now() + delay);
    }

    /// Watchdog step, called periodically by the daemon. Detects a crashed
    /// recorder and restarts it with exponential backoff.
    pub fn tick(&mut self) {
        if let Some(child) = self.process.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                self.process = None;
                self.card = None;
                let was = self.mode;
                self.mode = RecordingMode::None;
                let _ = std::fs::remove_file(pid_file());

                // A long healthy run resets the failure streak.
                if self.started_at.elapsed() > Duration::from_secs(60) {
                    self.consecutive_failures = 0;
                }
                let stderr_err = self
                    .shared
                    .last_stderr_error
                    .lock()
                    .ok()
                    .and_then(|e| e.clone());
                let reason = stderr_err.unwrap_or_else(|| format!("gpu-screen-recorder exited ({})", status));
                self.last_error = Some(reason.clone());
                eprintln!("Lapse: recorder exited unexpectedly: {}", reason);

                if was == RecordingMode::Continuous {
                    notify_async("Lapse - Recording stopped", &reason);
                    self.current_record_path = None;
                    // Don't silently start a new continuous file; go back to the replay buffer.
                    self.desired = RecordingMode::Replay;
                }
                if self.desired != RecordingMode::None {
                    self.restarts += 1;
                    self.schedule_restart();
                }
            }
        }

        if self.process.is_none() && self.desired != RecordingMode::None {
            if let Some(at) = self.next_restart {
                if Instant::now() >= at {
                    self.next_restart = None;
                    let _ = match self.desired {
                        RecordingMode::Replay => self.start_replay(),
                        RecordingMode::Continuous => self.start_recording(),
                        RecordingMode::None => Ok(()),
                    };
                }
            }
        }

        // Automatic app tracks: notice apps that started playing without a track of their own.
        if self.mode == RecordingMode::Replay
            && self.process.is_some()
            && self.config.separate_app_audio
            && self.config.auto_detect_apps
            && self.last_app_check.elapsed() >= Duration::from_secs(10)
        {
            self.last_app_check = Instant::now();
            let running = list_running_audio_apps();
            crate::audio_apps::remember(&running);
            let tracks = self.shared.track_names.lock().map(|t| t.clone()).unwrap_or_default();
            let has_track = |app: &str| tracks.iter().any(|t| t.strip_prefix("app:").is_some_and(|n| n.eq_ignore_ascii_case(app)));
            let excluded = |app: &str| self.config.excluded_apps.iter().any(|e| crate::audio_apps::resolve_key(e).eq_ignore_ascii_case(app));
            let new_app = running.iter().any(|a| !crate::audio_apps::is_ignored(a) && !excluded(a) && !has_track(a));
            if new_app && self.restart_after_save.is_none() {
                self.restart_after_save = Some(self.shared.save_counter.load(Ordering::SeqCst));
            }
        }
        if let Some(at) = self.restart_after_save {
            if self.shared.save_counter.load(Ordering::SeqCst) > at {
                eprintln!("Lapse: restarting the replay buffer to give newly heard apps their own track");
                let _ = self.restart();
            }
        }

        // Memory running low with the buffer in RAM: move it to disk before the system
        // starts killing programs (likely the game). Going back to RAM waits for the next
        // buffer start, since every switch drops the buffered seconds.
        if self.mode == RecordingMode::Replay
            && self.process.is_some()
            && self.config.replay_storage != "disk"
            && self.config.low_memory_guard
            && !self.memory_fallback
            && self.last_memory_check.elapsed() >= Duration::from_secs(5)
        {
            self.last_memory_check = Instant::now();
            let (avail, total) = (crate::config::available_ram_mb(), crate::config::total_ram_mb());
            if total > 0.0 && avail > 0.0 && avail / total * 100.0 < LOW_MEMORY_PCT {
                self.memory_fallback = true;
                let _ = self.stop_process();
                let _ = self.start_replay();
                crate::utils::send_notification(
                    "Lapse",
                    "Memory is running low, so the replay buffer moved to disk. Clips are still saved normally.",
                    "dialog-warning",
                );
            }
        }

        self.check_monitor_gone();

        // Recording a stand-in monitor: go back to the chosen one once it is on again.
        if self.mode == RecordingMode::Replay
            && self.process.is_some()
            && !self.follows_game()
            && self.capture_target != self.config.monitor
            && self.last_monitor_check.elapsed() >= Duration::from_secs(5)
        {
            self.last_monitor_check = Instant::now();
            if self.resolve_target_monitor() == self.config.monitor {
                let _ = self.stop_process();
                let _ = self.start_replay();
            }
        }
    }

    /// The recorded monitor was switched off or unplugged: gpu-screen-recorder keeps
    /// running without a picture, so another monitor is recorded until it is back.
    fn check_monitor_gone(&mut self) {
        let watched = self.mode == RecordingMode::Replay && self.process.is_some();
        if !watched || crate::hardware::monitor::connector_connected(&self.capture_target) {
            self.monitor_gone = None;
            return;
        }
        // Monitors drop out for a moment when they change their mode or wake up.
        if self.monitor_gone.get_or_insert_with(Instant::now).elapsed() < Duration::from_secs(10) {
            return;
        }
        self.monitor_gone = None;
        let next = self.resolve_target_monitor();
        if next == self.capture_target || !crate::hardware::monitor::connector_connected(&next) {
            return;
        }
        let body = format!("{} is off or unplugged, so {} is recorded for now.", self.capture_target, next);
        eprintln!("Lapse: {body}");
        let _ = self.stop_process();
        let _ = self.start_replay();
        notify_async("Lapse", &body);
    }

    pub fn current_mode(&self) -> RecordingMode {
        self.mode
    }

    pub fn status(&mut self) -> StatusResponse {
        let running = self.is_running();
        StatusResponse {
            recording: running && self.mode != RecordingMode::None,
            mode: format!("{:?}", if running { self.mode } else { RecordingMode::None }),
            is_installed: self.installed,
            last_error: self.last_error.clone(),
            warnings: self.warnings.clone(),
            encode_card: self.shared.encode_card.lock().map(|c| c.clone()).unwrap_or_default(),
            capture_target: self.capture_target.clone(),
            restarts: self.restarts,
            last_saved: self.shared.last_saved.lock().ok().and_then(|l| l.clone()),
            save_counter: self.shared.save_counter.load(Ordering::SeqCst),
            game: self.current_game.clone(),
            waiting_for_game: self.waiting_for_game,
            cs2_data_age: crate::autoclip::Game::Cs2.seconds_since_last_update(),
            dota_data_age: crate::autoclip::Game::Dota2.seconds_since_last_update(),
            lol_data_age: crate::autoclip::Game::Lol.seconds_since_last_update(),
            captured_hotkey: None,
            captured_hotkey_seq: 0,
        }
    }

    pub fn save_replay(&mut self) -> Result<()> {
        // gsr can hand back far more than `-r` (seen: 70 s for 30 s), so cut to the setting.
        if let Ok(mut t) = self.shared.pending_trim.lock() {
            *t = Some(self.config.replay_seconds);
        }
        if let Ok(mut l) = self.shared.pending_label.lock() {
            *l = None;
        }
        self.save_with_signal(libc::SIGUSR1, None)
    }

    /// Saves only the last `seconds` of the buffer (rounded up to 10, 30, 60 or 300 s, the
    /// lengths gpu-screen-recorder offers). Never clears the buffer. Used by auto clips.
    pub fn save_last(&mut self, seconds: u32, game: Option<String>) -> Result<()> {
        self.save_last_named(seconds, game, None)
    }

    /// Like `save_last`, and the clip's file name ends with `label` (e.g. "dust2_3K").
    pub fn save_last_named(&mut self, seconds: u32, game: Option<String>, label: Option<String>) -> Result<()> {
        if let Ok(mut l) = self.shared.pending_label.lock() {
            *l = label;
        }
        let offset = match seconds {
            0..=10 => 1,
            11..=30 => 2,
            31..=60 => 3,
            _ => 4,
        };
        if let Ok(mut t) = self.shared.pending_trim.lock() {
            *t = Some(seconds.min(300));
        }
        self.save_with_signal(libc::SIGRTMIN() + offset, game)
    }

    fn save_with_signal(&mut self, signal: libc::c_int, game: Option<String>) -> Result<()> {
        if self.waiting_for_game {
            anyhow::bail!("No game is running. In Game mode the replay buffer starts when a game opens.");
        }
        if self.mode != RecordingMode::Replay || !self.is_running() {
            anyhow::bail!("Replay buffer is not running");
        }
        let pid = self.process.as_ref().map(|c| c.id()).unwrap_or(0);

        let game = game.or_else(|| {
            crate::hardware::game_detector::detect_current_game(&self.config.games).map(|g| g.name)
        });
        if let Ok(mut g) = self.shared.pending_game.lock() {
            *g = game;
        }

        if !send_signal(pid, signal) {
            anyhow::bail!("Failed to signal gpu-screen-recorder");
        }
        if self.config.save_sound {
            crate::sound::play_save_sound(&self.config.save_sound_path);
        }
        Ok(())
    }

    /// Remembers this moment; it shows as a marker in clips that contain it.
    pub fn add_marker(&mut self) -> Result<()> {
        if !self.is_running() {
            anyhow::bail!("Nothing is being recorded");
        }
        self.push_marks(crate::markers::MarkKind::Marker, 1);
        if self.config.save_notification {
            notify_async("Lapse", "Marker added");
        }
        Ok(())
    }

    /// Kills reported by the game; they show on the timeline of clips that contain them.
    pub fn add_kill_marks(&mut self, kills: u32) {
        self.push_marks(crate::markers::MarkKind::Kill, kills);
    }

    fn push_marks(&mut self, kind: crate::markers::MarkKind, count: u32) {
        let now = SystemTime::now();
        if let Ok(mut marks) = self.shared.markers.lock() {
            marks.retain(|(m, _)| now.duration_since(*m).map(|d| d.as_secs() < 7200).unwrap_or(false));
            marks.extend(std::iter::repeat((now, kind)).take(count as usize));
        }
    }

    /// Saves a PNG of the recorded monitor into the clip folder (in the background).
    pub fn take_screenshot(&mut self) -> Result<()> {
        let target = if self.capture_target.is_empty() { self.resolve_target_monitor() } else { self.capture_target.clone() };
        if target == "portal" || target.is_empty() {
            anyhow::bail!("Screenshots need a monitor to capture; choose one in Settings > Recording.");
        }
        let game = self
            .current_game
            .clone()
            .or_else(|| crate::hardware::game_detector::detect_current_game(&self.config.games).map(|g| g.name));
        let dir = match &game {
            Some(g) => self.config.save_path.join(g),
            None => self.config.save_path.clone(),
        };
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("Screenshot_{}.png", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")));
        let mut cmd = self.base_command();
        cmd.arg("-w").arg(&target).arg("-o").arg(&path).stdout(Stdio::null()).stderr(Stdio::null());
        let (sound, notify) = (self.config.save_sound, self.config.save_notification);
        let sound_file = self.config.save_sound_path.clone();
        std::thread::spawn(move || {
            let ok = cmd.status().map(|s| s.success()).unwrap_or(false) && path.is_file();
            if ok {
                if sound {
                    crate::sound::play_save_sound(&sound_file);
                }
                if notify {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    notify_async("Lapse - Screenshot", &format!("Saved {}", name));
                }
            } else {
                notify_async("Lapse", "Screenshot failed");
            }
        });
        Ok(())
    }

    /// Game mode with nothing to record: stop the buffer and wait for a game.
    pub fn enter_game_wait(&mut self) {
        if self.mode == RecordingMode::Continuous {
            return;
        }
        let _ = self.stop_process();
        self.desired = RecordingMode::None;
        self.next_restart = None;
        self.waiting_for_game = true;
        self.capture_target.clear();
    }

    /// Called every few seconds by the daemon with the game that is running (if any).
    /// Starts/stops the buffer in game mode and applies per-game profiles.
    pub fn on_game_tick(&mut self, game: Option<String>) {
        if game.is_some() {
            self.no_game_ticks = 0;
        } else {
            self.no_game_ticks += 1;
        }
        // Ignore a single missed detection (e.g. while the game is loading).
        let game = if game.is_none() && self.no_game_ticks < 2 { self.current_game.clone() } else { game };
        let changed = game != self.current_game;
        self.current_game = game.clone();

        if self.mode == RecordingMode::Continuous {
            return;
        }

        if self.config.is_game_mode() {
            match (&game, self.waiting_for_game) {
                (Some(_), true) => {
                    self.profile = game.as_deref().and_then(|g| self.config.profile_for(g));
                    let _ = self.start_replay();
                    return;
                }
                (None, false) if self.desired == RecordingMode::Replay => {
                    self.enter_game_wait();
                    return;
                }
                _ => {}
            }
        }

        let profile = game.as_deref().and_then(|g| self.config.profile_for(g));
        if changed && profile != self.profile && self.mode == RecordingMode::Replay {
            // Different fps/bitrate for this game: restart the buffer with them.
            self.profile = profile;
            self.kill_process(Duration::from_secs(3));
            self.mode = RecordingMode::None;
            let _ = self.start_replay();
            return;
        }

        // The game moved to another monitor, or was first focused after the buffer started.
        let moved = self.mode == RecordingMode::Replay
            && self.process.is_some()
            && self.game_monitor().is_some_and(|m| m != self.capture_target);
        self.moved_ticks = if moved { self.moved_ticks + 1 } else { 0 };
        if self.moved_ticks >= 2 {
            self.moved_ticks = 0;
            let _ = self.restart();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_replay_is_named_after_the_game() {
        let dir = std::env::temp_dir().join(format!("lapse_named_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save = |game: Option<&str>, label: Option<&str>| {
            let file = dir.join("Replay_2026-11-25_00-12-01.mp4");
            std::fs::write(&file, b"clip").unwrap();
            let shared = Shared::default();
            *shared.pending_game.lock().unwrap() = game.map(String::from);
            *shared.pending_label.lock().unwrap() = label.map(String::from);
            handle_saved_replay(&shared, &dir, file);
            let last = shared.last_saved.lock().unwrap().clone().unwrap();
            assert!(Path::new(&last).exists());
            PathBuf::from(last).strip_prefix(&dir).unwrap().to_string_lossy().to_string()
        };
        assert_eq!(save(Some("Roblox"), None), "Roblox/Roblox_2026-11-25_00-12-01.mp4");
        // Never over an older clip.
        assert_eq!(save(Some("Roblox"), None), "Roblox/Roblox_2026-11-25_00-12-01_2.mp4");
        assert_eq!(save(Some("Deadlock"), Some("3K")), "Deadlock/Deadlock_2026-11-25_00-12-01_3K.mp4");
        assert_eq!(save(None, None), "Replay_2026-11-25_00-12-01.mp4");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_audio_args_noise_suppression() {
        let mut cfg = Config::default();
        cfg.audio_input = "default_input".to_string();
        cfg.audio_output = "".to_string();
        cfg.separate_audio_tracks = true;

        // Suppression off: uses raw mic.
        cfg.noise_suppression = false;
        let rec = Recorder::new(cfg.clone());
        assert_eq!(rec.audio_args(), vec!["default_input"]);

        // Suppression on, but denoise unavailable: falls back to raw mic.
        cfg.noise_suppression = true;
        let mut rec = Recorder::new(cfg.clone());
        assert_eq!(rec.audio_args(), vec!["default_input"]);

        // Suppression on and denoise available: uses lapse_denoise.
        rec.set_denoise_available(true);
        assert_eq!(rec.audio_args(), vec!["lapse_denoise"]);

        // Denoise available set back to false: falls back to raw mic.
        rec.set_denoise_available(false);
        assert_eq!(rec.audio_args(), vec!["default_input"]);

        // Mic disabled ("" or "None"): no mic included even when suppression is on and denoise available.
        cfg.audio_input = "".to_string();
        let mut rec = Recorder::new(cfg.clone());
        rec.set_denoise_available(true);
        assert!(rec.audio_args().is_empty());

        cfg.audio_input = "None".to_string();
        let mut rec = Recorder::new(cfg.clone());
        rec.set_denoise_available(true);
        assert!(rec.audio_args().is_empty());
    }

    #[test]
    fn test_update_config_noise_suppression_triggers_restart() {
        let mut cfg = Config::default();
        cfg.noise_suppression = false;
        cfg.noise_source = "source_1".to_string();

        let mut rec = Recorder::new(cfg.clone());

        // Changing only noise_source does not trigger restart.
        let mut cfg_src = cfg.clone();
        cfg_src.noise_source = "source_2".to_string();
        assert!(rec.update_config(cfg_src).is_ok());

        // Toggling noise_suppression does trigger restart logic.
        let mut cfg_supp = cfg.clone();
        cfg_supp.noise_suppression = true;
        assert!(rec.update_config(cfg_supp).is_ok());
    }

    #[test]
    fn test_build_gsr_audio_args_default() {
        let mut cfg = Config::default();
        // In default Config, audio_input is "None", so only game track is captured:
        let (args, tracks) = build_gsr_audio_args(&cfg, true, &[], &[], true);
        assert_eq!(args, vec!["default_output"]);
        assert_eq!(tracks, vec!["game"]);

        // When mic is enabled:
        cfg.audio_input = "default_input".into();
        let (args, tracks) = build_gsr_audio_args(&cfg, true, &[], &[], true);
        assert_eq!(args, vec!["default_output", "default_input"]);
        assert_eq!(tracks, vec!["game", "mic"]);
    }

    #[test]
    fn test_separated_apps_auto() {
        let mut cfg = Config::default();
        cfg.separate_app_audio = true;
        cfg.separate_apps = vec!["Discord".into()];
        cfg.excluded_apps = vec!["spotify".into()];
        let heard = vec!["discord".to_string(), "Helium".to_string(), "Spotify".to_string()];
        assert_eq!(separated_apps(&cfg, &heard), vec!["Discord", "Helium"]);
        let (args, tracks) = build_gsr_audio_args(&cfg, true, &[], &heard, true);
        assert_eq!(args[0], "app-inverse:Discord|app-inverse:Helium|app-inverse:spotify");
        assert_eq!(&args[1..3], &["app:Discord", "app:Helium"]);
        assert_eq!(&tracks[..3], &["game", "app:Discord", "app:Helium"]);
        cfg.auto_detect_apps = false;
        assert_eq!(separated_apps(&cfg, &heard), vec!["Discord"]);
    }

    #[test]
    fn test_buffer_on_disk() {
        let mut cfg = Config::default();
        cfg.replay_storage = "ram".into();
        // 32 GB machine, 16 GB free: a 7 GB buffer fits, a 9 GB one doesn't.
        assert!(!buffer_on_disk(&cfg, false, 7000.0, 16000.0, 32000.0));
        assert!(buffer_on_disk(&cfg, false, 9000.0, 16000.0, 32000.0));
        // After a low-memory switch it stays on disk until 30 % is free again.
        assert!(buffer_on_disk(&cfg, true, 1000.0, 6400.0, 32000.0));
        assert!(!buffer_on_disk(&cfg, true, 1000.0, 11200.0, 32000.0));
        // Unknown memory or guard off: what the config says.
        assert!(!buffer_on_disk(&cfg, true, 9000.0, 0.0, 0.0));
        cfg.low_memory_guard = false;
        assert!(!buffer_on_disk(&cfg, true, 9000.0, 1000.0, 32000.0));
        cfg.replay_storage = "disk".into();
        assert!(buffer_on_disk(&cfg, false, 0.0, 16000.0, 32000.0));
    }

    #[test]
    fn test_build_gsr_audio_args_separate_app_audio() {
        let mut cfg = Config::default();
        cfg.separate_app_audio = true;
        cfg.separate_apps = vec!["discord".into(), "spotify".into()];
        cfg.excluded_apps = vec!["obs".into()];
        cfg.audio_input = "default_input".into();

        let (args, tracks) = build_gsr_audio_args(&cfg, true, &[], &[], true);
        assert_eq!(
            args,
            vec![
                "app-inverse:discord|app-inverse:spotify|app-inverse:obs",
                "app:discord",
                "app:spotify",
                "default_input",
            ]
        );
        assert_eq!(tracks, vec!["game", "app:discord", "app:spotify", "mic"]);
    }

    #[test]
    fn test_build_gsr_audio_args_case_matching() {
        let mut cfg = Config::default();
        cfg.separate_app_audio = true;
        cfg.separate_apps = vec!["discord".into(), "spotify".into()];
        cfg.audio_input = "default_input".into();
        let running = vec!["Discord".to_string(), "Spotify".to_string(), "firefox".to_string()];

        let (args, tracks) = build_gsr_audio_args(&cfg, true, &running, &[], true);
        assert_eq!(
            args,
            vec![
                "app-inverse:Discord|app-inverse:Spotify",
                "app:Discord",
                "app:Spotify",
                "default_input",
            ]
        );
        assert_eq!(tracks, vec!["game", "app:discord", "app:spotify", "mic"]);
    }

    #[test]
    fn test_build_gsr_audio_args_no_pipewire_fallback() {
        let mut cfg = Config::default();
        cfg.separate_app_audio = true;
        cfg.audio_output = "my_sink".into();
        cfg.audio_input = "default_input".into();

        let (args, tracks) = build_gsr_audio_args(&cfg, true, &[], &[], false);
        assert_eq!(args, vec!["my_sink", "default_input"]);
        assert_eq!(tracks, vec!["game", "mic"]);
    }

    #[test]
    fn test_build_gsr_audio_args_excluded_apps_only() {
        let mut cfg = Config::default();
        cfg.separate_app_audio = false;
        cfg.excluded_apps = vec!["discord".into()];
        cfg.audio_input = "default_input".into();

        let (args, tracks) = build_gsr_audio_args(&cfg, true, &[], &[], true);
        assert_eq!(args, vec!["app-inverse:discord", "default_input"]);
        assert_eq!(tracks, vec!["game", "mic"]);
    }

    #[test]
    fn test_build_gsr_audio_args_mic_gain() {
        let mut cfg = Config::default();
        cfg.audio_input = "default_input".into();
        cfg.mic_gain = 2.0;

        let (args, tracks) = build_gsr_audio_args(&cfg, true, &[], &[], true);
        assert_eq!(args, vec!["default_output", "lapse_denoise"]);
        assert_eq!(tracks, vec!["game", "mic"]);
    }

    #[test]
    fn test_parse_resolution() {
        assert_eq!(parse_resolution("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_resolution(" 2560 x 1440 "), Some((2560, 1440)));
        assert_eq!(parse_resolution(""), None);
        assert_eq!(parse_resolution("invalid"), None);
        assert_eq!(parse_resolution("0x1080"), None);
        assert_eq!(parse_resolution("1920x0"), None);
    }

    #[test]
    fn test_clamp_resolution_to_monitor() {
        // Same size: stays same
        assert_eq!(clamp_resolution_to_monitor(1920, 1080, 1920, 1080), (1920, 1080));
        // Never upscale
        assert_eq!(clamp_resolution_to_monitor(1280, 720, 1920, 1080), (1280, 720));
        // Downscale clamped to monitor keeping aspect ratio
        assert_eq!(clamp_resolution_to_monitor(3840, 2160, 1920, 1080), (1920, 1080));
        assert_eq!(clamp_resolution_to_monitor(2560, 1440, 1920, 1080), (1920, 1080));
        // Monitor with different aspect ratio (1920x1200 16:10), target 2560x1440 (16:9)
        // scale is 1920 / 2560 = 0.75, target_h * 0.75 = 1080 -> (1920, 1080)
        assert_eq!(clamp_resolution_to_monitor(2560, 1440, 1920, 1200), (1920, 1080));
        // Ultrawide monitor (3440x1440), target 4K (3840x2160)
        // scale is 1440 / 2160 = 2/3, 3840 * 2/3 = 2560 -> (2560, 1440)
        assert_eq!(clamp_resolution_to_monitor(3840, 2160, 3440, 1440), (2560, 1440));
        // Non-standard monitor size: ensure even dimensions and bounds check
        let (w, h) = clamp_resolution_to_monitor(1920, 1080, 1365, 767);
        assert!(w <= 1365 && w % 2 == 0);
        assert!(h <= 767 && h % 2 == 0);
    }
}
}
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime};
    use crate::recorder_windows::audio::AudioMixer;
    use crate::recorder_windows::capture::{
        resolve_monitor, take_one_shot_screenshot, CaptureFlags,
        CaptureHandler, EncoderConfig, SaveRequest, ScreenshotRequest, SessionMode, SharedCaptureState,
    };
    use crate::recorder_windows::segment::get_buffer_dir;
    use windows_capture::capture::{CaptureControl, GraphicsCaptureApiHandler};
    use windows_capture::settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, SecondaryWindowSettings, Settings,
    };

    pub fn log_path() -> PathBuf {
        directories::ProjectDirs::from("com", "lapse", "lapse")
            .and_then(|d| d.state_dir().or_else(|| Some(d.data_local_dir())).map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::env::temp_dir().join("lapse"))
            .join("recorder.log")
    }

    pub fn cleanup_orphan() {}

    #[allow(dead_code)]
    pub(crate) fn trim_to_last(path: &Path, secs: u32) {
        let duration = crate::clip_index::probe_duration(path);
        if duration <= secs as f64 + 2.0 {
            return;
        }
        let tmp = path.with_extension("trim.tmp.mp4");
        if crate::recorder_windows::remux::join_segments(&[path.to_path_buf()], &tmp, Some(secs as f64)).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    fn notify_async(summary: &str, body: &str) {
        crate::utils::send_notification(summary, body, "media-record");
    }

    pub struct Recorder {
        config: Config,
        mode: RecordingMode,
        desired: RecordingMode,
        capture_control: Option<
            CaptureControl<CaptureHandler, Box<dyn std::error::Error + Send + Sync>>,
        >,
        shared: Arc<SharedCaptureState>,
        audio_mixer: Option<Arc<AudioMixer>>,
        last_error: Option<String>,
        warnings: Vec<String>,
        /// HMONITOR being recorded, and for how many ticks the game has been on another one.
        capture_monitor: isize,
        moved_ticks: u32,
        capture_target: String,
        current_game: Option<String>,
        waiting_for_game: bool,
        no_game_ticks: u32,
        profile: Option<GameProfile>,
        current_record_path: Option<PathBuf>,
        record_started: Option<SystemTime>,
        restarts: u32,
        next_restart: Option<Instant>,
    }

    impl Recorder {
        pub fn new(config: Config) -> Self {
            let shared = Arc::new(SharedCaptureState::new(
                config.save_path.clone(),
                config.save_sound,
                config.save_sound_path.clone(),
            ));
            Self {
                config,
                mode: RecordingMode::None,
                desired: RecordingMode::None,
                capture_control: None,
                shared,
                audio_mixer: None,
                last_error: None,
                warnings: Vec::new(),
                capture_monitor: 0,
                moved_ticks: 0,
                capture_target: String::new(),
                current_game: None,
                waiting_for_game: false,
                no_game_ticks: 0,
                profile: None,
                current_record_path: None,
                record_started: None,
                restarts: 0,
                next_restart: None,
            }
        }

        pub fn config(&self) -> &Config {
            &self.config
        }

        pub fn update_config(&mut self, config: Config) -> Result<()> {
            // The capture session reads these only when it starts.
            let (a, b) = (&self.config, &config);
            let needs_restart = a.video_codec != b.video_codec
                || a.audio_output != b.audio_output
                || a.audio_input != b.audio_input
                || a.separate_audio_tracks != b.separate_audio_tracks
                || a.separate_app_audio != b.separate_app_audio
                || a.separate_apps != b.separate_apps
                || a.excluded_apps != b.excluded_apps
                || a.capture_mode != b.capture_mode
                || (b.is_game_mode() && (a.games != b.games || a.language != b.language));
            self.config = config;
            if needs_restart && self.is_running() {
                self.restart()?;
            }
            Ok(())
        }

        pub fn set_denoise_available(&mut self, _ok: bool) {}

        #[allow(dead_code)]
        pub fn restart(&mut self) -> Result<()> {
            match self.desired {
                RecordingMode::Replay => self.start_replay(),
                RecordingMode::Continuous => self.start_recording(),
                RecordingMode::None => self.stop(),
            }
        }

        #[allow(dead_code)]
        fn _suppress_cross_platform_unused() {
            let _ = crate::utils::find_in_path("");
            let _ = crate::utils::has_notification_server();
            let _ = crate::denoise::FRAME_SIZE;
            let _ = crate::denoise::process_frame_dsp;
            let _ = crate::hotkeys::HotkeyEvent::SaveReplay.portal_info();
            let _ = crate::hotkeys::portal_available();
        }

        pub fn is_running(&mut self) -> bool {
            if let Some(control) = &self.capture_control {
                if control.is_finished() {
                    self.capture_control = None;
                    self.mode = RecordingMode::None;
                    if let Some(mixer) = self.audio_mixer.take() {
                        mixer.stop();
                    }
                    false
                } else {
                    true
                }
            } else {
                false
            }
        }

        /// Games only mode records the monitor the game is on.
        fn replay_monitor(&self) -> Result<windows_capture::monitor::Monitor> {
            if self.config.is_game_mode() {
                let game = crate::hardware::game_detector_windows::foreground_game_monitor(&self.config.games);
                let monitors = windows_capture::monitor::Monitor::enumerate().unwrap_or_default();
                if let Some(m) = monitors.into_iter().find(|m| Some(m.as_raw_hmonitor() as isize) == game) {
                    return Ok(m);
                }
            }
            resolve_monitor(&self.config.monitor)
        }

        pub fn start_replay(&mut self) -> Result<()> {
            if self.capture_control.is_some() {
                self.stop()?;
            }

            self.desired = RecordingMode::Replay;
            self.last_error = None;

            let monitor = match self.replay_monitor() {
                Ok(m) => m,
                Err(e) => {
                    let err = e.to_string();
                    self.last_error = Some(err.clone());
                    anyhow::bail!("{err}");
                }
            };

            let monitor_name = monitor
                .name()
                .unwrap_or_else(|_| "Primary Monitor".into());
            self.capture_monitor = monitor.as_raw_hmonitor() as isize;
            self.moved_ticks = 0;
            self.capture_target = monitor_name;

            let monitor_w = monitor.width().unwrap_or(1920);
            let monitor_h = monitor.height().unwrap_or(1080);
            let fps = self
                .profile
                .as_ref()
                .map(|p| p.fps)
                .filter(|f| *f > 0)
                .unwrap_or(self.config.fps);
            let bitrate = self
                .profile
                .as_ref()
                .map(|p| p.bitrate_kbps)
                .filter(|k| *k > 0)
                .unwrap_or(self.config.bitrate_kbps)
                * 1000;

            let profile_res = self
                .profile
                .as_ref()
                .and_then(|p| parse_resolution(&p.resolution));
            let (width, height) = if let Some((tw, th)) = profile_res {
                clamp_resolution_to_monitor(tw, th, monitor_w, monitor_h)
            } else if monitor_h > 1080 {
                crate::recorder_windows::segment::get_cached_downscale(
                    monitor_w,
                    monitor_h,
                    "Media Foundation",
                )
                .unwrap_or((monitor_w, monitor_h))
            } else {
                (monitor_w, monitor_h)
            };

            let buffer_dir = get_buffer_dir();
            let _ = std::fs::create_dir_all(&buffer_dir);
            if let Ok(entries) = std::fs::read_dir(&buffer_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| s.starts_with("seg_") && (s.ends_with(".mp4") || s.ends_with(".m4a")))
                            .unwrap_or(false)
                    {
                        let _ = std::fs::remove_file(p);
                    }
                }
            }

            self.shared.current_segment_index.store(1, Ordering::SeqCst);
            if let Ok(mut segs) = self.shared.segments.lock() {
                segs.clear();
            }

            let initial_seg_path = buffer_dir.join("seg_1.mp4");

            let audio_mixer = match AudioMixer::new(&self.config) {
                Ok(m) => Some(Arc::new(m)),
                Err(e) => {
                    eprintln!("Warning: Audio mixer initialization failed: {e}");
                    use std::io::Write;
                    let _ = std::io::stderr().flush();
                    None
                }
            };
            self.audio_mixer = audio_mixer.clone();

            let mic_info = if self.config.audio_input.is_empty() {
                "none"
            } else {
                &self.config.audio_input
            };
            if height < monitor_h {
                eprintln!(
                    "Recorder started: monitor=\"{}\", resolution={}x{} (downscaled from {}x{}), fps={}, encoder=\"Media Foundation\", audio=[desktop: \"default loopback\", mic: \"{}\"]",
                    self.capture_target, width, height, monitor_w, monitor_h, fps, mic_info
                );
            } else {
                eprintln!(
                    "Recorder started: monitor=\"{}\", resolution={}x{}, fps={}, encoder=\"Media Foundation\", audio=[desktop: \"default loopback\", mic: \"{}\"]",
                    self.capture_target, width, height, fps, mic_info
                );
            }
            use std::io::Write;
            let _ = std::io::stderr().flush();

            let enc_config = EncoderConfig {
                width,
                height,
                fps,
                bitrate,
                codec: self.config.video_codec.clone(),
                audio_enabled: false,
            };

            let is_ram = crate::recorder_windows::segment::decide_replay_storage(
                &self.config.replay_storage,
                self.config.low_memory_guard,
                true,
                crate::config::available_ram_mb(),
                crate::config::total_ram_mb(),
            );

            let (initial_encoder, initial_ram_stream) = match crate::recorder_windows::capture::create_encoder_for_target(&enc_config, &initial_seg_path, is_ram) {
                Ok(enc) => enc,
                Err(e) => {
                    let err = e.to_string();
                    self.last_error = Some(err.clone());
                    anyhow::bail!("{err}");
                }
            };

            let mode = SessionMode::Replay {
                replay_seconds: self.config.replay_seconds,
                buffer_dir: buffer_dir.clone(),
                replay_storage: self.config.replay_storage.clone(),
                low_memory_guard: self.config.low_memory_guard,
            };

            let flags = CaptureFlags {
                capture_width: monitor_w,
                capture_height: monitor_h,
                initial_encoder,
                initial_ram_stream,
                initial_segment_path: initial_seg_path,
                encoder_config: enc_config,
                mode,
                shared: self.shared.clone(),
                audio_mixer,
                focus: self.config.is_game_mode().then(|| crate::recorder_windows::focus_gate::FocusSettings {
                    monitor: monitor.as_raw_hmonitor() as isize,
                    games: self.config.games.clone(),
                    title: crate::gui::i18n::tr_in(&self.config.language, "Not on the game screen").to_string(),
                }),
            };

            let border_setting = crate::recorder_windows::capture::border_draw_setting();
            let settings = Settings::new(
                monitor,
                CursorCaptureSettings::Default,
                border_setting,
                SecondaryWindowSettings::Default,
                crate::recorder_windows::capture::update_interval_setting(fps),
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                flags,
            );

            let control = match CaptureHandler::start_free_threaded(settings) {
                Ok(c) => c,
                Err(e) => {
                    let err = format!("Failed to start screen capture: {e:?}");
                    self.last_error = Some(err.clone());
                    anyhow::bail!("{err}");
                }
            };

            self.capture_control = Some(control);
            self.mode = RecordingMode::Replay;
            self.waiting_for_game = false;
            Ok(())
        }

        pub fn start_recording(&mut self) -> Result<()> {
            if self.capture_control.is_some() {
                self.stop()?;
            }

            self.desired = RecordingMode::Continuous;
            self.last_error = None;

            let monitor = match resolve_monitor(&self.config.monitor) {
                Ok(m) => m,
                Err(e) => {
                    let err = e.to_string();
                    self.last_error = Some(err.clone());
                    anyhow::bail!("{err}");
                }
            };

            let monitor_name = monitor
                .name()
                .unwrap_or_else(|_| "Primary Monitor".into());
            self.capture_target = monitor_name;

            let monitor_w = monitor.width().unwrap_or(1920);
            let monitor_h = monitor.height().unwrap_or(1080);
            let fps = self.config.fps;
            let bitrate = self.config.bitrate_kbps * 1000;

            let (width, height) = if monitor_h > 1080 {
                crate::recorder_windows::segment::get_cached_downscale(
                    monitor_w,
                    monitor_h,
                    "Media Foundation",
                )
                .unwrap_or((monitor_w, monitor_h))
            } else {
                (monitor_w, monitor_h)
            };

            let _ = std::fs::create_dir_all(&self.config.save_path);
            let detected = crate::hardware::game_detector::detect_current_game(&self.config.games);
            let save_target = if let Some(ref game) = detected {
                let game_dir = self.config.save_path.join(&game.name);
                let _ = std::fs::create_dir_all(&game_dir);
                let rest = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S.mp4").to_string();
                let name = crate::clip_index::game_file_name(&game.name, &rest);
                crate::clip_index::free_path(game_dir.join(name))
            } else {
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                self.config.save_path.join(format!("record_{}.mp4", timestamp))
            };
            self.current_record_path = Some(save_target.clone());
            self.record_started = Some(SystemTime::now());

            let audio_mixer = match AudioMixer::new(&self.config) {
                Ok(m) => Some(Arc::new(m)),
                Err(e) => {
                    eprintln!("Warning: Audio mixer initialization failed: {e}");
                    use std::io::Write;
                    let _ = std::io::stderr().flush();
                    None
                }
            };
            self.audio_mixer = audio_mixer.clone();

            let mic_info = if self.config.audio_input.is_empty() {
                "none"
            } else {
                &self.config.audio_input
            };
            eprintln!(
                "Recorder started: monitor=\"{}\", resolution={}x{}, fps={}, encoder=\"Media Foundation\", audio=[desktop: \"default loopback\", mic: \"{}\"]",
                self.capture_target, width, height, fps, mic_info
            );
            use std::io::Write;
            let _ = std::io::stderr().flush();

            let enc_config = EncoderConfig {
                width,
                height,
                fps,
                bitrate,
                codec: self.config.video_codec.clone(),
                audio_enabled: false,
            };

            let (encoder, ram_stream) = match crate::recorder_windows::capture::create_encoder_for_target(&enc_config, &save_target, false) {
                Ok(enc) => enc,
                Err(e) => {
                    let err = e.to_string();
                    self.last_error = Some(err.clone());
                    anyhow::bail!("{err}");
                }
            };

            let flags = CaptureFlags {
                capture_width: monitor_w,
                capture_height: monitor_h,
                initial_encoder: encoder,
                initial_ram_stream: ram_stream,
                initial_segment_path: save_target,
                encoder_config: enc_config,
                mode: SessionMode::Continuous,
                shared: self.shared.clone(),
                audio_mixer,
                focus: None,
            };

            let border_setting = crate::recorder_windows::capture::border_draw_setting();
            let settings = Settings::new(
                monitor,
                CursorCaptureSettings::Default,
                border_setting,
                SecondaryWindowSettings::Default,
                crate::recorder_windows::capture::update_interval_setting(fps),
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                flags,
            );

            let control = match CaptureHandler::start_free_threaded(settings) {
                Ok(c) => c,
                Err(e) => {
                    let err = format!("Failed to start continuous screen capture: {e:?}");
                    self.last_error = Some(err.clone());
                    anyhow::bail!("{err}");
                }
            };

            self.capture_control = Some(control);
            self.mode = RecordingMode::Continuous;
            self.waiting_for_game = false;
            Ok(())
        }

        pub fn stop(&mut self) -> Result<()> {
            let was = self.mode;
            self.desired = RecordingMode::None;
            self.next_restart = None;

            if let Some(control) = self.capture_control.take() {
                let _ = control.stop();
            }
            if let Some(mixer) = self.audio_mixer.take() {
                mixer.stop();
            }

            self.mode = RecordingMode::None;
            crate::recorder_windows::mem_store::clear();

            if was == RecordingMode::Continuous {
                if let Some(path) = self.current_record_path.take() {
                    if path.is_file() {
                        notify_async(
                            "Lapse",
                            &format!(
                                "Recording saved: {}",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ),
                        );
                        let base = self.config.save_path.clone();
                        let shared = Arc::clone(&self.shared);
                        let started = self.record_started.take();
                        std::thread::spawn(move || {
                            let m4a = path.with_extension("m4a");
                            let chat_m4a = path.with_extension("chat.m4a");
                            let mic_m4a = path.with_extension("mic.m4a");
                            let has_app_m4a = (0..16).any(|i| path.with_extension(format!("app-{}.m4a", i)).exists());
                            if m4a.exists() || chat_m4a.exists() || mic_m4a.exists() || has_app_m4a {
                                let tmp = path.with_extension("muxing.mp4");
                                match crate::recorder_windows::remux::join_segments(&[path.clone()], &tmp, None) {
                                    Ok(()) => {
                                        if let Err(e) = std::fs::rename(&tmp, &path) {
                                            eprintln!("Failed to replace recording with muxed file: {e}");
                                            let _ = std::fs::remove_file(&tmp);
                                        } else {
                                            let _ = std::fs::remove_file(&m4a);
                                            let _ = std::fs::remove_file(&chat_m4a);
                                            let _ = std::fs::remove_file(&mic_m4a);
                                            let _ = std::fs::remove_file(crate::tracks::path_for_clip(&path));
                                            let _ = std::fs::remove_file(path.with_extension("tracks.json"));
                                            for i in 0..16 {
                                                let _ = std::fs::remove_file(path.with_extension(format!("app-{}.m4a", i)));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to mux continuous recording audio: {e}");
                                        let _ = std::fs::remove_file(&tmp);
                                    }
                                }
                            }
                            if let (Some(start), Ok(marks)) = (started, shared.markers.lock()) {
                                let duration = crate::clip_index::probe_duration(&path);
                                let offsets = crate::markers::offsets_since(&marks, start, duration);
                                crate::markers::save(&base, &path, &offsets);
                            }
                            if let Ok(mut last) = shared.last_saved.lock() {
                                *last = Some(path.to_string_lossy().to_string());
                            }
                            shared.save_counter.fetch_add(1, Ordering::SeqCst);
                        });
                    }
                }
            }

            Ok(())
        }

        pub fn tick(&mut self) {
            let running = self.is_running();
            if !running && self.desired != RecordingMode::None {
                if let Some(at) = self.next_restart {
                    if Instant::now() >= at {
                        self.next_restart = None;
                        let _ = match self.desired {
                            RecordingMode::Replay => self.start_replay(),
                            RecordingMode::Continuous => self.start_recording(),
                            RecordingMode::None => Ok(()),
                        };
                    }
                }
            }
        }

        pub fn current_mode(&self) -> RecordingMode {
            self.mode
        }

        pub fn status(&mut self) -> StatusResponse {
            let running = self.is_running();
            StatusResponse {
                recording: running && self.mode != RecordingMode::None,
                mode: format!("{:?}", if running { self.mode } else { RecordingMode::None }),
                is_installed: true,
                last_error: self.last_error.clone().or_else(|| self.shared.last_error.lock().ok().and_then(|e| e.clone())),
                warnings: self.warnings.clone(),
                encode_card: "Media Foundation".into(),
                capture_target: self.capture_target.clone(),
                restarts: self.restarts,
                last_saved: self.shared.last_saved.lock().ok().and_then(|l| l.clone()),
                save_counter: self.shared.save_counter.load(Ordering::SeqCst),
                game: self.current_game.clone(),
                waiting_for_game: self.waiting_for_game,
                cs2_data_age: crate::autoclip::Game::Cs2.seconds_since_last_update(),
                dota_data_age: crate::autoclip::Game::Dota2.seconds_since_last_update(),
                lol_data_age: crate::autoclip::Game::Lol.seconds_since_last_update(),
                captured_hotkey: None,
                captured_hotkey_seq: 0,
            }
        }

        pub fn queue_save_replay(
            &mut self,
        ) -> Result<std::sync::mpsc::Receiver<std::result::Result<PathBuf, String>>> {
            if self.waiting_for_game {
                anyhow::bail!("No game is running. In Game mode the replay buffer starts when a game opens.");
            }
            if self.mode != RecordingMode::Replay || !self.is_running() {
                anyhow::bail!("Replay buffer is not running");
            }
            let game = crate::hardware::game_detector::detect_current_game(&self.config.games)
                .map(|g| g.name);
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let req = SaveRequest {
                requested_seconds: self.config.replay_seconds,
                game,
                label: None,
                response_tx: Some(tx),
            };
            if let Ok(mut slot) = self.shared.save_request.lock() {
                *slot = Some(req);
            }
            Ok(rx)
        }

        #[allow(dead_code)]
        pub fn save_replay(&mut self) -> Result<()> {
            let rx = self.queue_save_replay()?;
            match rx.recv_timeout(Duration::from_secs(120)) {
                Ok(Ok(_path)) => Ok(()),
                Ok(Err(e)) => anyhow::bail!("{e}"),
                Err(_) => anyhow::bail!("Timed out waiting for replay clip to save"),
            }
        }

        pub fn queue_save_last(
            &mut self,
            seconds: u32,
            game: Option<String>,
        ) -> Result<std::sync::mpsc::Receiver<std::result::Result<PathBuf, String>>> {
            self.queue_save_last_named(seconds, game, None)
        }

        pub fn queue_save_last_named(
            &mut self,
            seconds: u32,
            game: Option<String>,
            label: Option<String>,
        ) -> Result<std::sync::mpsc::Receiver<std::result::Result<PathBuf, String>>> {
            if self.waiting_for_game {
                anyhow::bail!("No game is running. In Game mode the replay buffer starts when a game opens.");
            }
            if self.mode != RecordingMode::Replay || !self.is_running() {
                anyhow::bail!("Replay buffer is not running");
            }
            let game = game.or_else(|| {
                crate::hardware::game_detector::detect_current_game(&self.config.games)
                    .map(|g| g.name)
            });
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let req = SaveRequest {
                requested_seconds: seconds,
                game,
                label,
                response_tx: Some(tx),
            };
            if let Ok(mut slot) = self.shared.save_request.lock() {
                *slot = Some(req);
            }
            Ok(rx)
        }

        pub fn save_last(&mut self, seconds: u32, game: Option<String>) -> Result<()> {
            self.save_last_named(seconds, game, None)
        }

        pub fn save_last_named(
            &mut self,
            seconds: u32,
            game: Option<String>,
            label: Option<String>,
        ) -> Result<()> {
            let rx = self.queue_save_last_named(seconds, game, label)?;
            match rx.recv_timeout(Duration::from_secs(15)) {
                Ok(Ok(_path)) => Ok(()),
                Ok(Err(e)) => anyhow::bail!("{e}"),
                Err(_) => anyhow::bail!("Timed out waiting for replay clip to save"),
            }
        }

        pub fn add_marker(&mut self) -> Result<()> {
            if !self.is_running() {
                anyhow::bail!("Nothing is being recorded");
            }
            self.push_marks(crate::markers::MarkKind::Marker, 1);
            if self.config.save_notification {
                notify_async("Lapse", "Marker added");
            }
            Ok(())
        }

        pub fn add_kill_marks(&mut self, kills: u32) {
            self.push_marks(crate::markers::MarkKind::Kill, kills);
        }

        fn push_marks(&mut self, kind: crate::markers::MarkKind, count: u32) {
            let now = SystemTime::now();
            if let Ok(mut marks) = self.shared.markers.lock() {
                marks.retain(|(m, _)| {
                    now.duration_since(*m)
                        .map(|d| d.as_secs() < 7200)
                        .unwrap_or(false)
                });
                marks.extend(std::iter::repeat((now, kind)).take(count as usize));
            }
        }

        pub fn queue_screenshot(
            &mut self,
        ) -> Result<std::sync::mpsc::Receiver<std::result::Result<PathBuf, String>>> {
            let monitor = resolve_monitor(&self.config.monitor)?;
            let game = self.current_game.clone().or_else(|| {
                crate::hardware::game_detector::detect_current_game(&self.config.games)
                    .map(|g| g.name)
            });
            let dir = match &game {
                Some(g) => self.config.save_path.join(g),
                None => self.config.save_path.clone(),
            };
            let _ = std::fs::create_dir_all(&dir);
            let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
            let path = dir.join(format!("Screenshot_{}.png", timestamp));

            let (tx, rx) = std::sync::mpsc::sync_channel(1);

            if self.is_running() {
                let req = ScreenshotRequest {
                    path,
                    response_tx: Some(tx),
                };
                if let Ok(mut slot) = self.shared.screenshot_request.lock() {
                    if let Some(old) = slot.take() {
                        if let Some(old_tx) = old.response_tx {
                            let _ = old_tx.send(Err("Superceded by newer screenshot request".into()));
                        }
                    }
                    *slot = Some(req);
                } else {
                    anyhow::bail!("Failed to acquire screenshot lock");
                }
            } else {
                let res = take_one_shot_screenshot(
                    monitor,
                    path.clone(),
                    self.config.save_sound,
                    self.config.save_sound_path.clone(),
                    self.config.save_notification,
                );
                let _ = tx.send(res.map(|_| path).map_err(|e| e.to_string()));
            }
            Ok(rx)
        }

        pub fn take_screenshot(&mut self) -> Result<()> {
            let rx = self.queue_screenshot()?;
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(e)) => anyhow::bail!("{e}"),
                Err(_) => anyhow::bail!("Timed out waiting for screenshot to save"),
            }
        }

        pub fn enter_game_wait(&mut self) {
            if self.mode == RecordingMode::Continuous {
                return;
            }
            let _ = self.stop();
            self.desired = RecordingMode::None;
            self.next_restart = None;
            self.waiting_for_game = true;
            self.capture_target.clear();
        }

        pub fn on_game_tick(&mut self, game: Option<String>) {
            if game.is_some() {
                self.no_game_ticks = 0;
            } else {
                self.no_game_ticks += 1;
            }
            let game = if game.is_none() && self.no_game_ticks < 2 {
                self.current_game.clone()
            } else {
                game
            };
            let changed = game != self.current_game;
            self.current_game = game.clone();

            if self.mode == RecordingMode::Continuous {
                return;
            }

            if self.config.is_game_mode() {
                match (&game, self.waiting_for_game) {
                    (Some(_), true) => {
                        self.profile = game.as_deref().and_then(|g| self.config.profile_for(g));
                        let _ = self.start_replay();
                        return;
                    }
                    (None, false) if self.desired == RecordingMode::Replay => {
                        self.enter_game_wait();
                        return;
                    }
                    _ => {}
                }
                if self.mode == RecordingMode::Replay {
                    match crate::hardware::game_detector_windows::foreground_game_monitor(&self.config.games) {
                        Some(m) if m != self.capture_monitor => self.moved_ticks += 1,
                        Some(_) => self.moved_ticks = 0,
                        None => {}
                    }
                    if self.moved_ticks >= 2 {
                        let _ = self.start_replay();
                        return;
                    }
                }
            }

            let profile = game.as_deref().and_then(|g| self.config.profile_for(g));
            if changed && profile != self.profile && self.mode == RecordingMode::Replay {
                self.profile = profile;
                let _ = self.stop();
                let _ = self.start_replay();
            }
        }
    }
}
#[cfg(windows)]
pub use windows::*;
