use crate::config::Config;
use crate::hotkeys::{HotkeyEvent, HotkeyManager};
use crate::recorder::{Recorder, RecordingMode};
use crate::{ipc, tray};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::time::Duration;

/// Runs a hotkey action. Also used by the tray menu.
pub fn handle_hotkey(recorder: &Arc<Mutex<Recorder>>, event: HotkeyEvent) {
    let Ok(mut rec) = recorder.lock() else { return };
    match event {
        HotkeyEvent::SaveReplay => {
            if let Err(e) = rec.save_replay() {
                crate::utils::send_notification("Lapse", &e.to_string(), "dialog-warning");
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
        HotkeyEvent::Marker => {
            if let Err(e) = rec.add_marker() {
                crate::utils::send_notification("Lapse", &e.to_string(), "dialog-warning");
            }
        }
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

pub fn run_daemon() -> anyhow::Result<()> {
    if ipc::daemon_alive() {
        eprintln!("Lapse daemon is already running ({}).", ipc::socket_path().display());
        return Ok(());
    }

    stop_legacy_daemon();
    crate::recorder::cleanup_orphan();

    let config = Config::load()?;
    let recorder = Arc::new(Mutex::new(Recorder::new(config.clone())));

    let (tx_hotkey, rx_hotkey) = mpsc::channel();
    let hotkey_manager = Arc::new(HotkeyManager::new(tx_hotkey));

    hotkey_manager.register_from_config(&config);

    let recorder_clone = Arc::clone(&recorder);
    std::thread::spawn(move || {
        if let Ok(mut rec) = recorder_clone.lock() {
            if rec.config().is_game_mode() {
                rec.enter_game_wait();
            } else {
                let _ = rec.start_replay();
            }
        }
        while let Ok(event) = rx_hotkey.recv() {
            handle_hotkey(&recorder_clone, event);
        }
    });

    // Game watcher: starts/stops the buffer in game mode and applies per-game profiles.
    let recorder_games = Arc::clone(&recorder);
    std::thread::spawn(move || loop {
        let games = match recorder_games.lock() {
            Ok(rec) => rec.config().games.clone(),
            Err(_) => return,
        };
        // Detection reads /proc; do it without holding the recorder lock.
        let game = crate::hardware::game_detector::detect_current_game(&games).map(|g| g.name);
        if let Ok(mut rec) = recorder_games.lock() {
            rec.on_game_tick(game);
        }
        std::thread::sleep(Duration::from_secs(3));
    });

    crate::autoclip::start_server(Arc::clone(&recorder));
    crate::discord_rpc::start(Arc::clone(&recorder));

    // Watchdog: restarts gpu-screen-recorder if it crashes (driver reset, monitor hotplug, ...).
    let recorder_watch = Arc::clone(&recorder);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(mut rec) = recorder_watch.lock() {
            rec.tick();
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
    let recorder_sig = Arc::clone(&recorder);
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
                let _ = std::fs::remove_file(ipc::socket_path());
                crate::gui::single_instance::close_window();
                std::process::exit(0);
            }
        });
    }

    ipc::start_server(Arc::clone(&recorder), Arc::clone(&hotkey_manager))?;

    tray::setup_and_run(Arc::clone(&recorder))?;

    Ok(())
}

/// Versions before 2.1 listened on /tmp/lapse.sock. If one is still running after an
/// upgrade, stop its recorder and exit it so two daemons never record at once.
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
