use crate::config::Config;
use crate::hotkeys::{HotkeyEvent, HotkeyManager};
use crate::recorder::{Recorder, RecordingMode};
use crate::{ipc, tray};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::time::Duration;

/// Runs a hotkey action. Also used by the tray menu.
pub fn handle_hotkey(recorder: &Arc<Mutex<Recorder>>, event: HotkeyEvent) {
    let Ok(mut rec) = recorder.lock() else { return };
    match event {
        HotkeyEvent::SaveReplay => {
            #[cfg(windows)]
            {
                let rx = match rec.queue_save_replay() {
                    Ok(rx) => rx,
                    Err(e) => {
                        crate::utils::send_notification("Lapse", &e.to_string(), "dialog-warning");
                        return;
                    }
                };
                drop(rec);
                std::thread::spawn(move || {
                    match rx.recv_timeout(Duration::from_secs(120)) {
                        Ok(Ok(_path)) => {}
                        Ok(Err(e)) => {
                            crate::utils::send_notification("Lapse", &e, "dialog-warning");
                        }
                        Err(_) => {
                            crate::utils::send_notification(
                                "Lapse",
                                "Timed out waiting for replay clip to save",
                                "dialog-warning",
                            );
                        }
                    }
                });
            }
            #[cfg(not(windows))]
            {
                if let Err(e) = rec.save_replay() {
                    crate::utils::send_notification("Lapse", &e.to_string(), "dialog-warning");
                }
            }
        }
        HotkeyEvent::SaveShort => {
            let secs = rec.config().short_save_seconds;
            if let Err(e) = rec.save_last(secs, None) {
                crate::utils::send_notification("Lapse", &e.to_string(), "dialog-warning");
            }
        }
        HotkeyEvent::Screenshot => {
            if let Err(e) = rec.take_screenshot() {
                crate::utils::send_notification("Lapse", &e.to_string(), "dialog-warning");
            }
        }
        HotkeyEvent::Marker => match rec.add_marker() {
            Ok(()) => {
                crate::utils::send_notification("Lapse", "Marker added", "bookmark-new");
            }
            Err(e) => {
                crate::utils::send_notification("Lapse", &e.to_string(), "dialog-warning");
            }
        },
        HotkeyEvent::ToggleRecord => {
            if rec.current_mode() == RecordingMode::Continuous {
                let _ = rec.stop();
                let _ = rec.start_replay();
            } else {
                let _ = rec.start_recording();
            }
        }
    }
}

fn state_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "lapse", "lapse")
        .and_then(|d| d.state_dir().or_else(|| Some(d.data_local_dir())).map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::temp_dir().join("lapse"))
}

/// Path to the daemon log file.
pub fn log_path() -> PathBuf {
    state_dir().join("daemon.log")
}

/// Redirects stderr to daemon.log when running non-interactively.
fn redirect_stderr() {
    #[cfg(unix)]
    unsafe {
        if libc::isatty(2) != 0 {
            return;
        }
    }
    // The CRT's isatty() is true for NUL too, which a detached daemon gets, so ask
    // the console itself whether stderr is one.
    #[cfg(windows)]
    unsafe {
        extern "system" {
            fn GetStdHandle(n: u32) -> *mut std::ffi::c_void;
            fn GetConsoleMode(h: *mut std::ffi::c_void, mode: *mut u32) -> i32;
        }
        let mut mode = 0u32;
        if GetConsoleMode(GetStdHandle(-12i32 as u32), &mut mode) != 0 {
            return;
        }
    }

    let log = log_path();
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let too_big = std::fs::metadata(&log)
        .map(|m| m.len() > 2 * 1024 * 1024)
        .unwrap_or(false);

    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(&log)
    {
        Ok(f) => f,
        Err(_) => return,
    };

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::dup2(file.as_raw_fd(), 2);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::IntoRawHandle;
        extern "system" {
            fn SetStdHandle(nStdHandle: u32, hHandle: *mut std::ffi::c_void) -> i32;
        }
        const STD_ERROR_HANDLE: u32 = -12i32 as u32;
        let raw = file.into_raw_handle();
        unsafe {
            SetStdHandle(STD_ERROR_HANDLE, raw as *mut _);
        }
    }

    eprintln!(
        "--- {} Lapse daemon started (v{}) ---",
        chrono::Local::now().format("%F %T"),
        env!("CARGO_PKG_VERSION")
    );
    use std::io::Write;
    let _ = std::io::stderr().flush();
}

fn restart_voice_listener(
    recorder: &Arc<Mutex<Recorder>>,
    listener_holder: &Arc<Mutex<Option<crate::voice::VoiceListener>>>,
) {
    if let Ok(mut lock) = listener_holder.lock() {
        if let Some(mut old) = lock.take() {
            old.stop();
        }

        let (enabled, sens, input_dev) = match recorder.lock() {
            Ok(rec) => (
                rec.config().voice_trigger_enabled,
                rec.config().voice_sensitivity,
                rec.config().audio_input.clone(),
            ),
            Err(_) => return,
        };

        if !enabled {
            return;
        }

        let templates_path = match Config::get_voice_templates_path() {
            Ok(p) => p,
            Err(_) => return,
        };

        if !templates_path.exists() {
            return;
        }

        let templates = match crate::voice::VoiceTemplates::load(&templates_path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Lapse: failed to load voice templates: {}", e);
                return;
            }
        };

        let recorder_voice = Arc::clone(recorder);
        let on_trigger = move || {
            let (voice_beep, prev_save_sound, sound_path) = match recorder_voice.lock() {
                Ok(rec) => (
                    rec.config().voice_beep,
                    rec.config().save_sound,
                    rec.config().save_sound_path.clone(),
                ),
                Err(_) => return,
            };

            if !voice_beep && prev_save_sound {
                if let Ok(mut rec) = recorder_voice.lock() {
                    let mut temp_cfg = rec.config().clone();
                    temp_cfg.save_sound = false;
                    let _ = rec.update_config(temp_cfg);
                }
            }

            handle_hotkey(&recorder_voice, HotkeyEvent::SaveReplay);

            if !voice_beep && prev_save_sound {
                if let Ok(mut rec) = recorder_voice.lock() {
                    let mut temp_cfg = rec.config().clone();
                    temp_cfg.save_sound = prev_save_sound;
                    let _ = rec.update_config(temp_cfg);
                }
            } else if voice_beep && !prev_save_sound {
                crate::sound::play_save_sound(&sound_path);
            }
        };

        match crate::voice::VoiceListener::start(templates, input_dev, sens, on_trigger) {
            Ok(listener) => {
                *lock = Some(listener);
            }
            Err(e) => {
                eprintln!("Lapse: failed to start voice trigger listener: {}", e);
            }
        }
    }
}


pub fn run_daemon() -> anyhow::Result<()> {
    redirect_stderr();

    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".into());
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            *s
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.as_str()
        } else {
            "unknown panic"
        };
        eprintln!("Daemon panic at {}: {}", location, msg);
        let _ = std::io::stderr().flush();
    }));

    if ipc::daemon_alive() {
        eprintln!("Lapse daemon is already running ({}).", ipc::socket_path().display());
        let _ = std::io::stderr().flush();
        return Ok(());
    }

    stop_legacy_daemon();
    crate::recorder::cleanup_orphan();

    let config = Config::load()?;
    let recorder = Arc::new(Mutex::new(Recorder::new(config.clone())));

    let denoiser: Arc<Mutex<Option<crate::denoise::Denoiser>>> = Arc::new(Mutex::new(None));
    let denoise_needed = config.noise_suppression || (config.mic_gain - 1.0).abs() >= 0.01;
    if denoise_needed {
        match crate::denoise::Denoiser::start(
            &config.noise_source,
            config.noise_as_default,
            config.noise_suppression,
            config.mic_gain,
        ) {
            Ok(d) => {
                if let Ok(mut rec) = recorder.lock() {
                    rec.set_denoise_available(true);
                }
                if let Ok(mut lock) = denoiser.lock() {
                    *lock = Some(d);
                }
            }
            Err(e) => {
                eprintln!("Lapse: failed to start noise suppression: {}", e);
                let _ = std::io::stderr().flush();
            }
        }
    }

    let voice_listener: Arc<Mutex<Option<crate::voice::VoiceListener>>> = Arc::new(Mutex::new(None));
    restart_voice_listener(&recorder, &voice_listener);

    let (tx_hotkey, rx_hotkey) = mpsc::channel();
    let hotkey_manager = Arc::new(HotkeyManager::new(tx_hotkey));

    hotkey_manager.register_from_config(&config);

    let hotkey_capture = Arc::new(Mutex::new(ipc::HotkeyCapture::default()));

    let recorder_clone = Arc::clone(&recorder);
    let capture_clone = Arc::clone(&hotkey_capture);
    std::thread::spawn(move || {
        if let Ok(mut rec) = recorder_clone.lock() {
            if rec.config().is_game_mode() {
                rec.enter_game_wait();
            } else {
                let _ = rec.start_replay();
            }
        }
        while let Ok(event) = rx_hotkey.recv() {
            let captured = if let Ok(mut cap) = capture_clone.lock() {
                if cap.is_active() {
                    cap.last = Some(event);
                    cap.seq += 1;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if !captured {
                handle_hotkey(&recorder_clone, event);
            }
        }
    });

    // Game watcher: starts/stops the buffer in game mode and applies per-game profiles.
    let recorder_games = Arc::clone(&recorder);
    std::thread::spawn(move || loop {
        let games = match recorder_games.lock() {
            Ok(rec) => {
                #[cfg(target_os = "linux")]
                if rec.follows_game() {
                    crate::focus::start();
                }
                rec.config().games.clone()
            }
            Err(_) => return,
        };
        // Detection reads /proc; do it without holding the recorder lock.
        let game = crate::hardware::game_detector::detect_current_game(&games);
        #[cfg(target_os = "linux")]
        crate::focus::update_game(game.as_ref().and_then(|g| g.pid), &games);
        let game = game.map(|g| g.name);
        if let Ok(mut rec) = recorder_games.lock() {
            rec.on_game_tick(game);
        }
        std::thread::sleep(Duration::from_secs(3));
    });

    crate::autoclip::start_server(Arc::clone(&recorder));
    crate::discord_rpc::start(Arc::clone(&recorder));

    // Watchdog: restarts gpu-screen-recorder if it crashes (driver reset, monitor hotplug, ...).
    let recorder_watch = Arc::clone(&recorder);
    let denoiser_watch = Arc::clone(&denoiser);
    let voice_listener_watch = Arc::clone(&voice_listener);
    let mut last_suppression = config.noise_suppression;
    let mut last_mic_gain = config.mic_gain;
    let mut last_source = config.noise_source.clone();
    let mut last_as_default = config.noise_as_default;
    let mut last_denoise_retry = std::time::Instant::now();
    let mut last_voice_enabled = config.voice_trigger_enabled;
    let mut last_voice_sens = config.voice_sensitivity;
    let mut last_voice_input = config.audio_input.clone();
    let mut last_voice_mtime = Config::get_voice_templates_path()
        .ok()
        .and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    #[cfg(windows)]
    let mut last_mem_log = std::time::Instant::now();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        #[cfg(windows)]
        if last_mem_log.elapsed() >= Duration::from_secs(30) {
            use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
            use windows_sys::Win32::System::Threading::GetCurrentProcess;
            unsafe {
                let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
                let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, cb) != 0 {
                    let ws_mb = pmc.WorkingSetSize as f64 / (1024.0 * 1024.0);
                    eprintln!("Memory working set: {:.1} MB", ws_mb);
                    use std::io::Write;
                    let _ = std::io::stderr().flush();
                }
            }
            last_mem_log = std::time::Instant::now();
        }
        let cur_mtime = Config::get_voice_templates_path()
            .ok()
            .and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        let (suppression_changed, cur_suppression, cur_gain, cur_source, cur_as_default, voice_changed, cur_voice_enabled, cur_voice_sens, cur_voice_input) =
            if let Ok(mut rec) = recorder_watch.lock() {
                rec.tick();
                let c = rec.config();
                let cur_supp = c.noise_suppression;
                let cur_g = c.mic_gain;
                let cur_src = c.noise_source.clone();
                let cur_def = c.noise_as_default;
                let cur_needed = cur_supp || (cur_g - 1.0).abs() >= 0.01;
                let last_needed = last_suppression || (last_mic_gain - 1.0).abs() >= 0.01;
                let v_changed = c.voice_trigger_enabled != last_voice_enabled
                    || (c.voice_sensitivity - last_voice_sens).abs() > 0.001
                    || c.audio_input != last_voice_input
                    || cur_mtime != last_voice_mtime;
                (
                    cur_needed != last_needed
                        || (cur_g - last_mic_gain).abs() >= 0.01
                        || cur_supp != last_suppression
                        || cur_src != last_source
                        || cur_def != last_as_default,
                    cur_supp,
                    cur_g,
                    cur_src,
                    cur_def,
                    v_changed,
                    c.voice_trigger_enabled,
                    c.voice_sensitivity,
                    c.audio_input.clone(),
                )
            } else {
                continue;
            };

        if voice_changed {
            last_voice_enabled = cur_voice_enabled;
            last_voice_sens = cur_voice_sens;
            last_voice_input = cur_voice_input;
            last_voice_mtime = cur_mtime;
            restart_voice_listener(&recorder_watch, &voice_listener_watch);
        }

        // Bring the virtual mic back if it died (audio server restart) or never started.
        let denoiser_needed = cur_suppression || (cur_gain - 1.0).abs() >= 0.01;
        let denoiser_dead = cfg!(target_os = "linux")
            && denoiser_needed
            && last_denoise_retry.elapsed() >= Duration::from_secs(10)
            && denoiser_watch.lock().map(|mut d| !d.as_mut().is_some_and(|x| x.is_alive())).unwrap_or(false);
        // A gain-only change is applied live: restarting the pipeline would
        // restart the recorder and drop the replay buffer.
        let gain_only = suppression_changed
            && !denoiser_dead
            && denoiser_needed
            && cur_suppression == last_suppression
            && cur_source == last_source
            && cur_as_default == last_as_default
            && ((last_mic_gain - 1.0).abs() >= 0.01 || last_suppression)
            && denoiser_watch.lock().map(|d| d.as_ref().map(|x| x.set_gain(cur_gain)).is_some()).unwrap_or(false);
        if gain_only {
            last_mic_gain = cur_gain;
        } else if suppression_changed || denoiser_dead {
            last_denoise_retry = std::time::Instant::now();
            last_suppression = cur_suppression;
            last_mic_gain = cur_gain;
            last_source = cur_source.clone();
            last_as_default = cur_as_default;

            let denoiser_ok = if let Ok(mut d) = denoiser_watch.lock() {
                if let Some(old) = d.take() {
                    old.stop();
                }
                if denoiser_needed {
                    match crate::denoise::Denoiser::start(&cur_source, cur_as_default, cur_suppression, cur_gain) {
                        Ok(new_denoiser) => {
                            *d = Some(new_denoiser);
                            true
                        }
                        Err(e) => {
                            eprintln!("Lapse: failed to start noise suppression/gain pipeline: {}", e);
                            let _ = std::io::stderr().flush();
                            false
                        }
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if let Ok(mut rec) = recorder_watch.lock() {
                rec.set_denoise_available(denoiser_ok);
            }
        }
    });

    // Storage limit: check after every save and every 10 minutes.
    let recorder_clean = Arc::clone(&recorder);
    std::thread::spawn(move || {
        let mut last_counter = 0u64;
        let mut last_run = std::time::Instant::now() - Duration::from_secs(3600);
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let counter = match recorder_clean.lock() {
                Ok(mut rec) => rec.status().save_counter,
                Err(_) => continue,
            };
            if counter == last_counter && last_run.elapsed() < Duration::from_secs(600) {
                continue;
            }
            last_counter = counter;
            last_run = std::time::Instant::now();
            if let Ok(cfg) = Config::load() {
                if let Some(r) = crate::cleanup::enforce(&cfg) {
                    crate::utils::send_notification(
                        "Lapse - Storage limit",
                        &format!(
                            "Removed {} old clip(s), freed {:.1} GB",
                            r.removed,
                            r.freed_bytes as f64 / 1024.0 / 1024.0 / 1024.0
                        ),
                        "user-trash",
                    );
                }
            }
        }
    });

    // Finish recordings cleanly when the session logs out or the service is stopped.
    #[cfg(unix)]
    {
        let recorder_sig = Arc::clone(&recorder);
        let denoiser_sig = Arc::clone(&denoiser);
        let voice_sig = Arc::clone(&voice_listener);
        if let Ok(mut signals) = signal_hook::iterator::Signals::new([
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGHUP,
        ]) {
            std::thread::spawn(move || {
                if signals.forever().next().is_some() {
                    if let Ok(mut rec) = recorder_sig.lock() {
                        let _ = rec.stop();
                    }
                    if let Ok(mut d) = denoiser_sig.lock() {
                        if let Some(denoiser) = d.take() {
                            denoiser.stop();
                        }
                    }
                    if let Ok(mut vl) = voice_sig.lock() {
                        if let Some(mut listener) = vl.take() {
                            listener.stop();
                        }
                    }
                    let _ = std::fs::remove_file(ipc::socket_path());
                    crate::gui::single_instance::close_window();
                    std::process::exit(0);
                }
            });
        }
    }

    ipc::start_server(
        Arc::clone(&recorder),
        Arc::clone(&hotkey_manager),
        Arc::clone(&hotkey_capture),
    )?;

    let tray_res = tray::setup_and_run(Arc::clone(&recorder));

    if let Ok(mut d) = denoiser.lock() {
        if let Some(denoiser) = d.take() {
            denoiser.stop();
        }
    }

    if let Ok(mut vl) = voice_listener.lock() {
        if let Some(mut listener) = vl.take() {
            listener.stop();
        }
    }

    tray_res
}

/// Versions before 2.1 listened on /tmp/lapse.sock. If one is still running after an
/// upgrade, stop its recorder and exit it so two daemons never record at once.
#[cfg(target_os = "linux")]
fn stop_legacy_daemon() {
    use std::io::{Read, Write};
    let legacy = std::path::Path::new("/tmp/lapse.sock");
    if legacy == ipc::socket_path() {
        return;
    }
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(legacy) else {
        return;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.write_all(b"\"Stop\"");
    let mut buf = [0u8; 256];
    let _ = stream.read(&mut buf);

    let me = std::process::id();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if pid == me {
                continue;
            }
            let comm = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
            let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
            let cmdline = String::from_utf8_lossy(&cmdline);
            if comm.trim().eq_ignore_ascii_case("lapse") && cmdline.contains("--daemon") {
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGTERM);
                }
            }
        }
    }
    let _ = std::fs::remove_file(legacy);
}

#[cfg(not(target_os = "linux"))]
fn stop_legacy_daemon() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_log_path() {
        let p = log_path();
        assert!(p.ends_with("daemon.log"));
    }
}
