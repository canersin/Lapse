use crate::config::Config;
use crate::ipc::StatusResponse;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingMode {
    None,
    Replay,
    Continuous,
}

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
    /// Per-game (fps, kbps) overrides in effect for the running replay buffer.
    profile: Option<(u32, u32)>,
    /// Game mode with no game open: the buffer is off until one starts.
    waiting_for_game: bool,
    no_game_ticks: u32,
}

const MAX_CONSECUTIVE_FAILURES: u32 = 6;

fn notify_async(summary: &str, body: &str) {
    crate::utils::send_notification(summary, body, "media-record");
}

fn detect_installed(config: &Config) -> bool {
    std::path::Path::new("/usr/local/bin/gpu-screen-recorder").exists()
        || std::path::Path::new("/usr/bin/gpu-screen-recorder").exists()
        || Command::new("which")
            .arg(&config.recorder_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

fn state_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "lapse", "lapse")
        .and_then(|d| d.state_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("/tmp/lapse"))
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
            let new_path = target_dir.join(&filename);
            if std::fs::rename(&saved, &new_path).is_ok() {
                (new_path, format!("{}/{}", game_name, filename))
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
    let ok = Command::new("ffmpeg")
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
fn trim_to_last(path: &Path, secs: u32) {
    let duration = crate::clip_index::probe_duration(path);
    if duration <= secs as f64 + 2.0 {
        return;
    }
    let tmp = hidden_temp(path, "trim");
    let ok = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-sseof"])
        .arg(format!("-{}", secs))
        .arg("-i")
        .arg(path)
        .args(["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero"])
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
        let desired = self.desired;
        // Only settings that change the gpu-screen-recorder command line require a restart;
        // theme, hotkeys, autostart etc. must not wipe the replay buffer.
        let recording_view = |c: &Config| {
            let mut c = c.clone();
            let d = Config::default();
            c.theme_palette = d.theme_palette;
            c.autostart = d.autostart;
            c.init_system = d.init_system;
            c.check_updates = d.check_updates;
            c.discord_rpc = d.discord_rpc;
            c.discord_rpc_button = d.discord_rpc_button;
            c.hotkey_replay = d.hotkey_replay;
            c.hotkey_replay_mod = d.hotkey_replay_mod;
            c.hotkey_record = d.hotkey_record;
            c.hotkey_record_mod = d.hotkey_record_mod;
            c.game_gpu = d.game_gpu;
            c.cleanup_enabled = d.cleanup_enabled;
            c.cleanup_max_gb = d.cleanup_max_gb;
            c.hotkey_backend = d.hotkey_backend;
            c.share_allow_ssh_fallback = d.share_allow_ssh_fallback;
            c.language = d.language;
            c.first_run_done = d.first_run_done;
            c.save_sound = d.save_sound;
            c.save_sound_path = d.save_sound_path.clone();
            c.save_notification = d.save_notification;
            c.capture_mode = d.capture_mode;
            c.games = d.games;
            c.cs2_auto_clip = d.cs2_auto_clip;
            c.cs2_min_kills = d.cs2_min_kills;
            c.dota_auto_clip = d.dota_auto_clip;
            c.dota_min_kills = d.dota_min_kills;
            c.cs2_lead_in = d.cs2_lead_in;
            c.dota_lead_in = d.dota_lead_in;
            c.hotkey_short = d.hotkey_short;
            c.hotkey_short_mod = d.hotkey_short_mod;
            c.short_save_seconds = d.short_save_seconds;
            c.hotkey_screenshot = d.hotkey_screenshot;
            c.hotkey_screenshot_mod = d.hotkey_screenshot_mod;
            c.hotkey_marker = d.hotkey_marker;
            c.hotkey_marker_mod = d.hotkey_marker_mod;
            c.extra_hotkeys = d.extra_hotkeys;
            c.audio_offset_ms = d.audio_offset_ms;
            c.discord_webhook = d.discord_webhook.clone();
            c.gamepad_hotkey = d.gamepad_hotkey;
            c.gamepad_buttons = d.gamepad_buttons.clone();
            c
        };
        let needs_restart = recording_view(&self.config) != recording_view(&config);
        let mode_changed = self.config.capture_mode != config.capture_mode;

        self.config = config;
        self.shared.notify.store(self.config.save_notification, Ordering::Relaxed);
        self.shared.audio_offset_ms.store(self.config.audio_offset_ms, Ordering::Relaxed);
        self.installed = detect_installed(&self.config);

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

        if needs_restart && self.process.is_some() {
            // Keep a continuous recording running untouched; only restart the replay buffer.
            if self.mode == RecordingMode::Continuous {
                return Ok(());
            }
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
        let profile_kbps = self.profile.map(|p| p.1).filter(|k| *k > 0);
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
        let enabled = |v: &str| !v.is_empty() && v != "None";
        let desktop = if !self.config.audio_apps.is_empty() {
            Some(
                self.config
                    .audio_apps
                    .iter()
                    .map(|a| format!("app:{}", a))
                    .collect::<Vec<_>>()
                    .join("|"),
            )
        } else {
            enabled(&self.config.audio_output).then(|| self.config.audio_output.clone())
        };
        let mic = enabled(&self.config.audio_input).then(|| self.config.audio_input.clone());

        let sources: Vec<String> = desktop.into_iter().chain(mic).collect();
        if self.config.separate_audio_tracks || sources.len() < 2 {
            sources
        } else {
            vec![sources.join("|")]
        }
    }

    pub fn is_running(&mut self) -> bool {
        self.process
            .as_mut()
            .map(|p| p.try_wait().map(|s| s.is_none()).unwrap_or(false))
            .unwrap_or(false)
    }

    fn resolve_target_monitor(&self) -> String {
        let is_auto = self.config.monitor.is_empty()
            || self.config.monitor.eq_ignore_ascii_case("auto")
            || self.config.monitor.eq_ignore_ascii_case("screen")
            || self.config.monitor.eq_ignore_ascii_case("focused");

        if !is_auto {
            if self.follows_game() {
                return crate::hardware::monitor::get_game_or_primary_monitor(&self.config.monitor);
            }
            return self.config.monitor.clone();
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
                if self.follows_game() {
                    return crate::hardware::monitor::get_game_or_primary_monitor(first_mon);
                }
                return first_mon.clone();
            }
        }

        let monitors = crate::hardware::monitor::get_monitors();
        if let Some(mon) = monitors.first() {
            if self.follows_game() {
                return crate::hardware::monitor::get_game_or_primary_monitor(&mon.name);
            }
            return mon.name.clone();
        }

        "focused".to_string()
    }

    /// Record the monitor the game is on (game mode or the "record only game" option).
    fn follows_game(&self) -> bool {
        self.config.record_only_game || self.config.is_game_mode()
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
            .arg(self.profile.map(|p| p.0).filter(|f| *f > 0).unwrap_or(self.config.fps).to_string());

        if self.config.resolution != "Native" {
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

        let target_monitor = self.resolve_target_monitor();
        self.warnings = self.compute_warnings(&target_monitor);
        self.capture_target = target_monitor.clone();

        let mut cmd = self.build_command(&target_monitor);
        cmd.arg("-r").arg(self.config.replay_seconds.to_string());
        if self.config.replay_storage == "disk" {
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
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let detected = crate::hardware::game_detector::detect_current_game(&self.config.games);
        let filename = if let Some(game) = detected {
            let game_dir = self.config.save_path.join(&game.name);
            let _ = std::fs::create_dir_all(&game_dir);
            format!("{}/record_{}.mp4", game.name, timestamp)
        } else {
            format!("record_{}.mp4", timestamp)
        };
        let save_target = self.config.save_path.join(&filename);
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
        }
    }

    pub fn save_replay(&mut self) -> Result<()> {
        if let Ok(mut t) = self.shared.pending_trim.lock() {
            *t = None;
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
        }
    }
}
