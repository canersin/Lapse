use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::config::Config;
#[cfg(not(windows))]
use crate::hardware::audio::get_audio_devices;
#[cfg(not(windows))]
use crate::hardware::gpu::get_gpus;
#[cfg(not(windows))]
use crate::hardware::monitor::get_monitors;
use crate::init_system::sync_autostart;
use crate::ipc;

pub fn handle_cli(args: &[String]) -> anyhow::Result<bool> {
    if args.is_empty() {
        return Ok(false);
    }

    let cmd = args[0].to_lowercase();
    match cmd.as_str() {
        "start" | "daemon" => {
            let no_gui = args.iter().any(|a| a == "--no-gui" || a == "--no-open-ui");
            handle_start(!no_gui)
        }
        "stop" => handle_stop(),
        "clip" | "save" => handle_clip(),
        "save-last" => {
            let secs = match args.get(1).map(|a| a.trim_end_matches('s').parse::<u32>()) {
                None => Config::load().map(|c| c.short_save_seconds).unwrap_or(30),
                Some(Ok(s)) => s,
                Some(Err(_)) => {
                    eprintln!("Usage: lapse save-last [seconds]   (e.g. lapse save-last 30)");
                    return Ok(true);
                }
            };
            simple_command(ipc::Command::SaveLast(secs), &format!("Saved the last {} seconds.", secs))
        }
        "screenshot" | "shot" => simple_command(ipc::Command::Screenshot, "Screenshot saved next to your clips."),
        "mark" | "marker" => simple_command(ipc::Command::Mark, "Marker added; it shows in the trimmer of the next saved clip."),
        "status" => handle_status(),
        "logs" | "log" => {
            let follow = args.iter().skip(1).any(|a| a == "-f" || a == "--follow");
            handle_logs(follow)
        }
        "reload" => handle_reload(),
        "gui" | "ui" => handle_gui(),
        "clips" | "list" => handle_clips(),
        "share" => {
            #[cfg(windows)]
            {
                println!("Live stream sharing is not supported on Windows.");
                Ok(true)
            }
            #[cfg(not(windows))]
            {
                let target = args.get(1).map(|s| s.as_str());
                handle_share(target)
            }
        }
        "config" => handle_config(),
        "open-config" => handle_open_config(),
        "list-keys" | "keys" => handle_list_keys(),
        "doctor" => handle_doctor(),
        "run" => {
            #[cfg(windows)]
            {
                println!("The 'run' command is for dual-GPU Linux setups and not supported on Windows.");
                Ok(true)
            }
            #[cfg(not(windows))]
            handle_run(&args[1..])
        }
        "gpus" | "dual-gpu" => {
            #[cfg(windows)]
            {
                println!("Dual-GPU management is not supported on Windows.");
                Ok(true)
            }
            #[cfg(not(windows))]
            handle_gpus()
        }
        "uninstall" => handle_uninstall(),
        "voice-test" => {
            let secs = match args.get(1).map(|a| a.trim_end_matches('s').parse::<u64>()) {
                None => 15,
                Some(Ok(s)) => s,
                Some(Err(_)) => {
                    eprintln!("Usage: lapse voice-test [seconds]   (e.g. lapse voice-test 15)");
                    return Ok(true);
                }
            };
            handle_voice_test(secs)
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn send_ipc(command: ipc::Command) -> Option<ipc::Response> {
    ipc::send(command)
}

/// `Lapse run -- %command%`: launches a game on the configured game GPU so the
/// encoder GPU stays free for recording. Meant for Steam/Lutris launch options.
#[cfg(not(windows))]
fn handle_run(rest: &[String]) -> anyhow::Result<bool> {
    let cmd_args: Vec<&String> = match rest.iter().position(|a| a == "--") {
        Some(i) => rest[i + 1..].iter().collect(),
        None => rest.iter().collect(),
    };
    if cmd_args.is_empty() {
        eprintln!("Usage: Lapse run -- <command>   (Steam: Lapse run -- %command%)");
        std::process::exit(2);
    }

    let mut command = Command::new(cmd_args[0]);
    command.args(&cmd_args[1..]);

    if let Ok(cfg) = Config::load() {
        if cfg.dual_gpu {
            let gpus = get_gpus();
            let report = crate::hardware::gpu::dual_gpu_report(&gpus, &cfg.game_gpu, &cfg.gpu, &cfg.monitor);
            if let Some(game_gpu) = report.game_gpu.as_ref().filter(|_| report.is_split()) {
                for (k, v) in crate::hardware::gpu::render_env_for(game_gpu) {
                    command.env(k, v);
                }
                eprintln!("Lapse: rendering on {}", game_gpu.short_name());
            }
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = command.exec();
        eprintln!("Lapse: failed to launch {}: {}", cmd_args[0], err);
        std::process::exit(127);
    }
    #[cfg(not(unix))]
    {
        match command.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(0)),
            Err(err) => {
                eprintln!("Lapse: failed to launch {}: {}", cmd_args[0], err);
                std::process::exit(127);
            }
        }
    }
}

#[cfg(not(windows))]
fn handle_gpus() -> anyhow::Result<bool> {
    let cfg = Config::load().unwrap_or_default();
    let gpus = get_gpus();
    println!("\x1b[1;35m--- GPUs ---\x1b[0m");
    for g in &gpus {
        println!(
            "  {} [{}] {}  monitors: {}",
            g.card_name(),
            g.pci_id,
            g.short_name(),
            if g.connected_monitors.is_empty() { "-".to_string() } else { g.connected_monitors.join(", ") }
        );
    }
    let report = crate::hardware::gpu::dual_gpu_report(&gpus, &cfg.game_gpu, &cfg.gpu, &cfg.monitor);
    println!("\n\x1b[1;35m--- Dual GPU ---\x1b[0m");
    println!("  Enabled:       {}", if cfg.dual_gpu { "yes" } else { "no" });
    println!("  Game GPU:      {}", report.game_gpu.as_ref().map(|g| g.short_name()).unwrap_or("-".into()));
    println!("  Encoder GPU:   {}", report.encode_gpu.as_ref().map(|g| g.short_name()).unwrap_or("-".into()));
    for p in &report.problems {
        println!("  \x1b[1;33m[!]\x1b[0m {}", p);
    }
    println!("\n  Steam launch option:  Lapse run -- %command%");
    Ok(true)
}

fn handle_start(open_gui: bool) -> anyhow::Result<bool> {
    if ipc::daemon_alive() {
        println!("\x1b[1;32m[OK]\x1b[0m Lapse daemon is already active and capturing.");
        if open_gui {
            return crate::client::run_gui_client().map(|_| true);
        }
        return Ok(true);
    }

    println!("\x1b[1;34m[INFO]\x1b[0m Launching Lapse background daemon...");
    let child = crate::utils::spawn_daemon();

    match child {
        Ok(_) => {
            println!("\x1b[1;32m[OK]\x1b[0m Lapse daemon started in background.");
            println!("Logs: lapse logs -f");
            std::thread::sleep(Duration::from_millis(600));
            if open_gui {
                return crate::client::run_gui_client().map(|_| true);
            }
            Ok(true)
        }
        Err(e) => {
            eprintln!("\x1b[1;31m[ERROR]\x1b[0m Failed to spawn daemon: {}", e);
            Ok(true)
        }
    }
}

fn handle_stop() -> anyhow::Result<bool> {
    match send_ipc(ipc::Command::Stop) {
        Some(ipc::Response::Ok) => {
            println!("\x1b[1;32m[OK]\x1b[0m Background recording daemon stopped.");
        }
        Some(ipc::Response::Error(e)) => {
            eprintln!("\x1b[1;31m[ERROR]\x1b[0m Daemon returned error: {}", e);
        }
        _ => {
            println!("\x1b[1;33m[WARN]\x1b[0m No active Lapse daemon found on {}.", ipc::socket_path().display());
        }
    }
    Ok(true)
}

fn handle_clip() -> anyhow::Result<bool> {
    println!("\x1b[1;34m[INFO]\x1b[0m Requesting instant replay save from daemon...");
    match ipc::send_detailed(ipc::Command::SaveReplay) {
        Ok(ipc::Response::Ok) => {
            println!("\x1b[1;32m[OK]\x1b[0m Replay buffer saved successfully! Check your clips directory.");
        }
        Ok(ipc::Response::Error(e)) => {
            if e.contains("Timed out") {
                println!("\x1b[1;33m[WARN]\x1b[0m Save is taking longer than expected. Check your clips directory shortly.");
            } else {
                eprintln!("\x1b[1;31m[ERROR]\x1b[0m Failed to save replay: {}", e);
            }
        }
        Err(ipc::IpcError::Timeout) => {
            println!("\x1b[1;33m[WARN]\x1b[0m Save is taking longer than expected. Check your clips directory shortly.");
        }
        Err(ipc::IpcError::NotRunning) => {
            eprintln!("\x1b[1;31m[ERROR]\x1b[0m Lapse daemon is not currently running. Start it with lapse start.");
        }
        Err(ipc::IpcError::Other(e)) => {
            eprintln!("\x1b[1;31m[ERROR]\x1b[0m Failed to communicate with daemon: {}", e);
        }
        _ => {
            eprintln!("\x1b[1;31m[ERROR]\x1b[0m Lapse daemon returned an unexpected response.");
        }
    }
    Ok(true)
}

/// Sends one command to the daemon and prints the outcome.
fn simple_command(command: ipc::Command, ok: &str) -> anyhow::Result<bool> {
    match ipc::send_detailed(command) {
        Ok(ipc::Response::Ok) => println!("\x1b[1;32m[OK]\x1b[0m {}", ok),
        Ok(ipc::Response::Error(e)) => {
            if e.contains("Timed out") {
                println!("\x1b[1;33m[WARN]\x1b[0m Save is taking longer than expected.");
            } else {
                eprintln!("\x1b[1;31m[ERROR]\x1b[0m {}", e);
            }
        }
        Err(ipc::IpcError::Timeout) => {
            println!("\x1b[1;33m[WARN]\x1b[0m Save is taking longer than expected.");
        }
        Err(ipc::IpcError::NotRunning) => {
            eprintln!("\x1b[1;31m[ERROR]\x1b[0m Lapse daemon is not currently running. Start it with lapse start.");
        }
        Err(ipc::IpcError::Other(e)) => {
            eprintln!("\x1b[1;31m[ERROR]\x1b[0m {}", e);
        }
        _ => eprintln!("\x1b[1;31m[ERROR]\x1b[0m Lapse daemon returned an unexpected response."),
    }
    Ok(true)
}

fn handle_reload() -> anyhow::Result<bool> {
    match send_ipc(ipc::Command::ReloadConfig) {
        Some(ipc::Response::Ok) => println!("\x1b[1;32m[OK]\x1b[0m Daemon reloaded the configuration."),
        Some(ipc::Response::Error(e)) => eprintln!("\x1b[1;31m[ERROR]\x1b[0m Reload failed: {}", e),
        _ => eprintln!("\x1b[1;31m[ERROR]\x1b[0m Lapse daemon is not currently running. Start it with lapse start."),
    }
    Ok(true)
}

/// Bitrate actually used by the newest saved replays (probes up to 3 files).
fn recent_clips_kbps(save_path: &std::path::Path) -> Option<f64> {
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let dirs = std::iter::once(save_path.to_path_buf())
        .chain(std::fs::read_dir(save_path).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    for dir in dirs {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !crate::clip_index::is_recording(&name) {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                files.push((entry.path(), meta.len(), meta.modified().unwrap_or(std::time::UNIX_EPOCH)));
            }
        }
    }
    files.sort_by(|a, b| b.2.cmp(&a.2));
    let samples: Vec<(f64, f64)> = files
        .iter()
        .take(3)
        .map(|(p, len, _)| (*len as f64, crate::clip_index::probe_duration(p)))
        .filter(|(_, secs)| *secs >= 30.0)
        .collect();
    crate::clip_index::average_kbps(samples.into_iter())
}

fn handle_status() -> anyhow::Result<bool> {
    println!("\x1b[1;35m--- Lapse Daemon Status ---\x1b[0m");

    let socket_active = ipc::daemon_alive();
    if !socket_active {
        println!("  Status:        \x1b[1;31mInactive (Stopped)\x1b[0m");
        println!("  Socket:        {} [Not connected]", ipc::socket_path().display());
    } else {
        println!("  Status:        \x1b[1;32mRunning (Active)\x1b[0m");
        println!("  Socket:        {} [Connected]", ipc::socket_path().display());

        if let Some(ipc::Response::Status(st)) = send_ipc(ipc::Command::GetStatus) {
            println!(
                "  Recording:     {}",
                if st.recording {
                    "\x1b[1;32mYES\x1b[0m"
                } else {
                    "\x1b[1;33mNO (Idle)\x1b[0m"
                }
            );
            println!("  Mode:          {}", st.mode);
        }
    }

    if let Ok(cfg) = Config::load() {
        let mb_min = (cfg.bitrate_kbps as f64) * 60.0 / 8.0 / 1024.0;
        println!("  Buffer:        {} seconds", cfg.replay_seconds);
        println!("  Framerate:     {} fps", cfg.fps);
        println!(
            "  Bitrate:       up to {:.1} Mbps (at most ~{:.0} MB/min)",
            cfg.bitrate_kbps as f64 / 1000.0,
            mb_min
        );
        if let Some(kbps) = recent_clips_kbps(&cfg.save_path) {
            println!(
                "  Real average:  {:.1} Mbps in your latest clips (~{:.0} MB/min)",
                kbps / 1000.0,
                kbps * 60.0 / 8.0 / 1024.0
            );
        }
        println!("  Quality:       {}", cfg.quality);
        println!("  Encoder:       {}", cfg.video_codec);
        println!("  GPU Target:    {}", cfg.gpu);
        println!(
            "  Monitor:       {}",
            if cfg.monitor.is_empty() {
                "Auto / Focused"
            } else {
                &cfg.monitor
            }
        );
        println!("  Save Path:     {}", cfg.save_path.display());
        println!(
            "  Autostart:     {}",
            if cfg.autostart { "Enabled" } else { "Disabled" }
        );
    }

    if let Ok(st) = crate::share::GLOBAL_SHARE_STATE.lock() {
        if st.is_sharing {
            if let Some(ref url) = st.public_url {
                println!("  Live Share:    \x1b[1;36m{}\x1b[0m", url);
            }
        }
    }

    println!("\x1b[1;35m---------------------------\x1b[0m");
    Ok(true)
}

fn handle_gui() -> anyhow::Result<bool> {
    crate::client::run_gui_client().map(|_| true)
}

fn handle_clips() -> anyhow::Result<bool> {
    let cfg = Config::load()?;
    let path = &cfg.save_path;

    if !path.exists() {
        println!("No clips directory found at {}", path.display());
        return Ok(true);
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "mp4").unwrap_or(false))
        .collect();

    entries.sort_by_key(|p| {
        p.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    entries.reverse();

    println!(
        "\x1b[1;34mSaved Gameplay Clips in {}\x1b[0m (Total: {})",
        path.display(),
        entries.len()
    );
    println!(
        "{:<4} {:<34} {:<12} {:<20}",
        "#", "File Name", "Size", "Modified"
    );
    println!("{:-<76}", "");

    for (i, p) in entries.iter().take(25).enumerate() {
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        let size_str = p
            .metadata()
            .map(|m| {
                let mb = m.len() as f64 / (1024.0 * 1024.0);
                format!("{:.1} MB", mb)
            })
            .unwrap_or_else(|_| "0 MB".into());

        let time_str = p
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .map(|t| {
                let dur = t.elapsed().unwrap_or_default();
                if dur.as_secs() < 3600 {
                    format!("{}m ago", dur.as_secs() / 60)
                } else if dur.as_secs() < 86400 {
                    format!("{}h ago", dur.as_secs() / 3600)
                } else {
                    format!("{}d ago", dur.as_secs() / 86400)
                }
            })
            .unwrap_or_else(|| "unknown".into());

        println!(
            "{:<4} {:<34} {:<12} {:<20}",
            i + 1,
            name,
            size_str,
            time_str
        );
    }

    Ok(true)
}

#[cfg(not(windows))]
fn handle_share(target: Option<&str>) -> anyhow::Result<bool> {
    let cfg = Config::load()?;
    let file_path = if let Some(t) = target {
        let p = PathBuf::from(t);
        if p.exists() {
            p
        } else {
            cfg.save_path.join(t)
        }
    } else {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&cfg.save_path)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "mp4").unwrap_or(false))
            .collect();
        entries.sort_by_key(|p| {
            p.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        match entries.pop() {
            Some(last) => last,
            None => {
                eprintln!(
                    "\x1b[1;31m[ERROR]\x1b[0m No MP4 clips found in {}",
                    cfg.save_path.display()
                );
                return Ok(true);
            }
        }
    };

    if !file_path.exists() {
        eprintln!(
            "\x1b[1;31m[ERROR]\x1b[0m Clip file does not exist: {}",
            file_path.display()
        );
        return Ok(true);
    }

    println!(
        "\x1b[1;34m[INFO]\x1b[0m Initializing live P2P streaming server for: {}",
        file_path.file_name().unwrap_or_default().to_string_lossy()
    );
    crate::share::start_live_share(file_path);

    println!("\x1b[1;33m[INFO]\x1b[0m Connecting SSH reverse proxy tunnel (localhost.run)...");
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(12) {
        if let Ok(st) = crate::share::GLOBAL_SHARE_STATE.lock() {
            if let Some(ref url) = st.public_url {
                println!(
                    "\x1b[1;32m[LIVE STREAM READY]\x1b[0m \x1b[1;36m{}\x1b[0m",
                    url
                );
                println!("  - Direct stream link copied to clipboard.");
                println!("  - Serving live HTML5 web player with seekable range streaming.");
                println!("  - Press Ctrl+C in terminal to stop sharing.");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn handle_config() -> anyhow::Result<bool> {
    let path = Config::get_config_path()?;
    println!("\x1b[1;34mConfig path:\x1b[0m {}", path.display());
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        println!("{}", content);
    } else {
        println!("Config file does not exist yet; default configuration is used.");
    }
    Ok(true)
}

fn handle_open_config() -> anyhow::Result<bool> {
    let path = Config::get_config_path()?;
    if !path.exists() {
        let cfg = Config::load()?;
        let _ = cfg.save();
    }

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "nano".into());

    println!("Opening config with: {} {}", editor, path.display());
    if Command::new(&editor).arg(&path).status().is_err() {
        let _ = open::that(&path);
    }
    Ok(true)
}

fn handle_list_keys() -> anyhow::Result<bool> {
    println!("\x1b[1;35m--- Lapse Supported Hotkey Names ---\x1b[0m");
    println!("  Function Keys:  F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12");
    println!("  Navigation:     Insert, Delete, Home, End, PageUp, PageDown");
    println!("  Control:        Pause, PrintScreen, ScrollLock, Space, Tab");
    println!("  Modifiers:      Ctrl, Alt, Shift, Super");
    println!("");
    println!("  Examples in config.json:");
    println!("    \"replay_hotkey\": \"F10\"");
    println!("    \"replay_hotkey\": \"Ctrl+F9\"");
    println!("    \"replay_hotkey\": \"Alt+F10\"");
    println!("\x1b[1;35m-------------------------------------\x1b[0m");
    Ok(true)
}

#[cfg(windows)]
fn handle_doctor() -> anyhow::Result<bool> {
    use std::path::Path;

    println!("\x1b[1;35m=======================================\x1b[0m");
    println!("\x1b[1;35m       LAPSE STARTUP DIAGNOSTICS       \x1b[0m");
    println!("\x1b[1;35m=======================================\x1b[0m");

    println!("\x1b[1;34m[1/5] Windows System:\x1b[0m");
    let win_ver = Command::new("cmd")
        .args(["/c", "ver"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string()))
        .unwrap_or_else(|| "Windows 10/11".to_string());
    println!("  OS Version:    {}", win_ver);

    println!("\x1b[1;34m[2/5] Video Encoder:\x1b[0m");
    println!("  Encoder:       \x1b[1;32mMedia Foundation (H.264)\x1b[0m");

    println!("\x1b[1;34m[3/5] Audio System (WASAPI):\x1b[0m");
    let (renders, captures) = {
        let _ = wasapi::initialize_mta();
        if let Ok(enumerator) = wasapi::DeviceEnumerator::new() {
            let r = enumerator
                .get_device_collection(wasapi::Direction::Render)
                .and_then(|c| c.get_nbr_devices())
                .unwrap_or(0);
            let c = enumerator
                .get_device_collection(wasapi::Direction::Capture)
                .and_then(|c| c.get_nbr_devices())
                .unwrap_or(0);
            (r, c)
        } else {
            (0, 0)
        }
    };
    println!("  Output Sinks:  {} detected", renders);
    println!("  Input Sources: {} detected", captures);
    let playing: Vec<String> = crate::recorder_windows::audio::playing_apps().into_iter().map(|(name, _)| name).collect();
    println!("  Playing now:   {}", if playing.is_empty() { "-".to_string() } else { playing.join(", ") });

    println!("\x1b[1;34m[4/5] FFmpeg Tool:\x1b[0m");
    let ffmpeg_found = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("ffmpeg.exe")))
        .map(|p| p.is_file())
        .unwrap_or(false)
        || Path::new("ffmpeg.exe").is_file();
    if ffmpeg_found {
        println!("  ffmpeg.exe:    \x1b[1;32mFound next to lapse.exe\x1b[0m");
    } else {
        println!("  ffmpeg.exe:    \x1b[1;33mNot found next to lapse.exe\x1b[0m");
    }

    println!("\x1b[1;34m[5/5] Autostart Status:\x1b[0m");
    let autostart_ok = Command::new("reg")
        .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "Lapse"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if autostart_ok {
        println!("  Run Key:       \x1b[1;32mEnabled (HKCU Run key)\x1b[0m");
    } else {
        println!("  Run Key:       Disabled (not in HKCU Run key)");
    }

    println!("\x1b[1;32mAll diagnostic checks completed successfully.\x1b[0m\n");
    Ok(true)
}

#[cfg(not(windows))]
fn handle_doctor() -> anyhow::Result<bool> {
    println!("\x1b[1;35m=======================================\x1b[0m");
    println!("\x1b[1;35m       LAPSE STARTUP DIAGNOSTICS       \x1b[0m");
    println!("\x1b[1;35m=======================================\x1b[0m");

    println!("\x1b[1;34m[1/6] Desktop Session:\x1b[0m");
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into());
    println!("  Session:       {} ({})", session, desktop);

    println!("\x1b[1;34m[2/6] GPU Acceleration & Codecs:\x1b[0m");
    let gpus = get_gpus();
    if gpus.is_empty() {
        println!("  \x1b[1;33mWarning: No GPU detected. CPU fallback will be used.\x1b[0m");
    } else {
        for g in gpus {
            let primary_tag = if g.is_primary {
                " [Active Display]"
            } else {
                ""
            };
            println!(
                "  GPU Device:    {} ({}, {}){}",
                g.name, g.pci_id, g.card_path, primary_tag
            );
        }
    }

    println!("\x1b[1;34m[3/6] Display Monitors & DRM:\x1b[0m");
    let monitors = get_monitors();
    if monitors.is_empty() {
        println!("  Monitors:      Default primary display");
    } else {
        for m in monitors {
            println!("  Output:        {} ({})", m.name, m.resolution);
        }
    }

    println!("\x1b[1;34m[4/6] Audio System (PipeWire/Pulse):\x1b[0m");
    let (outputs, inputs) = get_audio_devices();
    println!("  Output Sinks:  {} detected", outputs.len());
    println!("  Input Sources: {} detected", inputs.len());

    println!("\x1b[1;34m[5/6] Network Tunnel Tools:\x1b[0m");
    let ssh_ok = Command::new("ssh").arg("-V").output().is_ok()
        || (crate::utils::is_inside_container()
            && Command::new("distrobox-host-exec")
                .args(["ssh", "-V"])
                .output()
                .is_ok());
    if ssh_ok {
        println!("  OpenSSH:       \x1b[1;32mAvailable (Ready for Live Stream Sharing)\x1b[0m");
    } else {
        println!(
            "  OpenSSH:       \x1b[1;31mNot found (Install openssh to enable live sharing)\x1b[0m"
        );
    }

    println!(
        "  Shortcuts:     {}",
        if crate::hotkeys::portal_available() {
            "\x1b[1;32mWayland GlobalShortcuts portal available\x1b[0m"
        } else {
            "X11 key grabs only (on Wayland they work while an XWayland window is focused)"
        }
    );

    println!("\x1b[1;34m[6/6] Autostart & Service Status:\x1b[0m");
    if let Ok(cfg) = Config::load() {
        println!(
            "  Autostart:     {}",
            if cfg.autostart {
                "\x1b[1;32mEnabled\x1b[0m"
            } else {
                "Disabled"
            }
        );
    }

    println!("\x1b[1;32mAll diagnostic checks completed successfully.\x1b[0m\n");
    Ok(true)
}

fn handle_uninstall() -> anyhow::Result<bool> {
    println!("\x1b[1;33mRemoving Lapse autostart...\x1b[0m");
    sync_autostart(false);
    let _ = std::fs::remove_file(ipc::socket_path());
    println!("\x1b[1;32m[OK]\x1b[0m Removed the autostart entry.");
    println!("Note: Saved clips and configuration were preserved in ~/.config/lapse.");
    Ok(true)
}

fn print_log_tail(path: &std::path::Path, name: &str, count: usize) -> u64 {
    if !path.exists() {
        println!("{}: not found", name);
        return 0;
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            println!("{}: failed to read: {}", name, e);
            return 0;
        }
    };
    let offset = bytes.len() as u64;
    let content = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(count);
    for line in &lines[start..] {
        println!("{}", line);
    }
    offset
}

struct LogFollower {
    path: PathBuf,
    prefix: &'static str,
    offset: u64,
    leftover: Vec<u8>,
}

impl LogFollower {
    fn poll(&mut self) {
        let len = match std::fs::metadata(&self.path) {
            Ok(m) => m.len(),
            Err(_) => return,
        };
        if len < self.offset {
            self.offset = 0;
            self.leftover.clear();
        }
        if len > self.offset {
            use std::io::{Read, Seek, SeekFrom};
            let mut file = match std::fs::File::open(&self.path) {
                Ok(f) => f,
                Err(_) => return,
            };
            if file.seek(SeekFrom::Start(self.offset)).is_err() {
                return;
            }
            let mut buf = Vec::new();
            if file.read_to_end(&mut buf).is_err() {
                return;
            }
            self.offset += buf.len() as u64;
            self.leftover.extend_from_slice(&buf);

            while let Some(pos) = self.leftover.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = self.leftover.drain(..=pos).collect();
                let mut line = &line_bytes[..line_bytes.len().saturating_sub(1)];
                if line.ends_with(b"\r") {
                    line = &line[..line.len().saturating_sub(1)];
                }
                println!("{}{}", self.prefix, String::from_utf8_lossy(line));
            }
        }
    }
}

fn handle_logs(follow: bool) -> anyhow::Result<bool> {
    let daemon_log = crate::daemon::log_path();
    let recorder_log = crate::recorder::log_path();

    let daemon_offset = print_log_tail(&daemon_log, "daemon.log", 40);
    println!("---");
    let recorder_offset = print_log_tail(&recorder_log, "recorder.log", 40);

    if follow {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let mut daemon_follower = LogFollower {
            path: daemon_log,
            prefix: "[daemon] ",
            offset: daemon_offset,
            leftover: Vec::new(),
        };
        let mut recorder_follower = LogFollower {
            path: recorder_log,
            prefix: "[recorder] ",
            offset: recorder_offset,
            leftover: Vec::new(),
        };
        loop {
            daemon_follower.poll();
            recorder_follower.poll();
            let _ = std::io::stdout().flush();
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    Ok(true)
}

fn handle_voice_test(seconds: u64) -> anyhow::Result<bool> {
    let cfg = Config::load().unwrap_or_default();
    println!("\x1b[1;35m--- Lapse Voice Test ({}s) ---\x1b[0m", seconds);
    println!("Configured input device: '{}'", cfg.audio_input);

    let templates = Config::get_voice_templates_path()
        .ok()
        .and_then(|p| crate::voice::VoiceTemplates::load(&p).ok());

    if let Some(ref t) = templates {
        println!(
            "Loaded voice templates: {} takes, baseline threshold: {:.3}",
            t.templates.len(),
            t.threshold
        );
    } else {
        println!("No voice templates found. Running level and VAD test only.");
    }

    let capture = match crate::voice::WizardCapture::start(&cfg.audio_input) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("\x1b[1;31m[ERROR]\x1b[0m Failed to start voice capture: {}", e);
            return Ok(true);
        }
    };

    println!("Capture running. Speak your trigger word to test detection...\n");
    let start = std::time::Instant::now();
    let total_duration = Duration::from_secs(seconds);
    let interval = Duration::from_millis(200);
    let mut next_tick = start + interval;

    while start.elapsed() < total_duration {
        while let Some(utt) = capture.try_recv_utterance() {
            let ms = (utt.len() as f32 / 16.0).round() as usize;
            if let Some(ref t) = templates {
                let mfcc = crate::voice::extract_mfcc(&utt);
                let res = crate::voice::match_utterance(t, &mfcc, cfg.voice_sensitivity);
                let tag = if res.matched {
                    "\x1b[1;32mMATCH\x1b[0m"
                } else if res.is_near_miss {
                    "\x1b[1;33mNEAR MISS\x1b[0m"
                } else {
                    "\x1b[1;31mNO MATCH\x1b[0m"
                };
                println!(
                    "\n\x1b[1;36m>>> Utterance: {} ms | Match: distance {:.3} / threshold {:.3} [{}]\x1b[0m\n",
                    ms, res.distance, res.threshold, tag
                );
            } else {
                println!(
                    "\n\x1b[1;36m>>> Utterance: {} ms (no templates)\x1b[0m\n",
                    ms
                );
            }
        }

        let now = std::time::Instant::now();
        if now >= next_tick {
            let rms_db = capture.current_level_dbfs();
            let floor_db = capture.noise_floor_dbfs();
            let vad_state = if capture.in_utterance() {
                "\x1b[1;32mspeech\x1b[0m"
            } else {
                "silence"
            };
            println!(
                "Device: {:<20} | {:>5} Hz ({} ch) | RMS: {:>6.1} dBFS | Floor: {:>6.1} dBFS | VAD: {}",
                capture.device_name(),
                capture.sample_rate(),
                capture.channels(),
                rms_db,
                floor_db,
                vad_state,
            );
            while next_tick <= now {
                next_tick += interval;
            }
        }

        std::thread::sleep(Duration::from_millis(20));
    }

    println!("\x1b[1;32m[DONE]\x1b[0m Voice test completed.");
    Ok(true)
}

fn print_help() {
    #[cfg(windows)]
    println!(
        "\x1b[1;35mLapse\x1b[0m - Replay Buffer Studio (v{})",
        env!("CARGO_PKG_VERSION")
    );
    #[cfg(not(windows))]
    println!(
        "\x1b[1;35mLapse\x1b[0m - Lightweight Wayland & X11 Replay Buffer Studio (v{})",
        env!("CARGO_PKG_VERSION")
    );
    println!("Usage: lapse <command> [options]\n");
    println!("Commands:");
    println!("  \x1b[1;36mstart\x1b[0m [--no-gui]      Start the background recording daemon");
    println!("  \x1b[1;36mstop\x1b[0m                  Stop the background daemon");
    println!("  \x1b[1;36mclip\x1b[0m, \x1b[1;36msave\x1b[0m            Save rolling gameplay replay buffer immediately");
    println!("  \x1b[1;36msave-last\x1b[0m [seconds]   Save only the last seconds (default from settings), keep the buffer");
    println!("  \x1b[1;36mscreenshot\x1b[0m            Save a PNG of the recorded monitor");
    println!("  \x1b[1;36mmark\x1b[0m                  Mark this moment (shows on the trimmer timeline)");
    println!("  \x1b[1;36mstatus\x1b[0m                Show daemon status, buffer info, and live share link");
    println!("  \x1b[1;36mlogs\x1b[0m [-f]             Show daemon and recorder logs");
    println!("  \x1b[1;36mgui\x1b[0m, \x1b[1;36mui\x1b[0m                Open the Material 3 desktop application");
    println!(
        "  \x1b[1;36mclips\x1b[0m, \x1b[1;36mlist\x1b[0m           List saved gameplay MP4 clips"
    );
    #[cfg(not(windows))]
    println!("  \x1b[1;36mshare\x1b[0m [clip]          Start live P2P stream proxy and get public HTTPS link");
    println!("  \x1b[1;36mreload\x1b[0m                Make the daemon re-read the config file");
    println!("  \x1b[1;36mconfig\x1b[0m                Print current JSON configuration");
    println!("  \x1b[1;36mopen-config\x1b[0m           Open config in $EDITOR");
    println!("  \x1b[1;36mlist-keys\x1b[0m             List valid hotkey names for configuration");
    #[cfg(windows)]
    println!("  \x1b[1;36mdoctor\x1b[0m                Run startup diagnostics");
    #[cfg(not(windows))]
    println!(
        "  \x1b[1;36mdoctor\x1b[0m                Run startup diagnostics (GPU, Audio, Init, SSH)"
    );
    #[cfg(not(windows))]
    println!("  \x1b[1;36mgpus\x1b[0m                  Show GPUs and the dual GPU (game / encoder) setup");
    #[cfg(not(windows))]
    println!("  \x1b[1;36mrun\x1b[0m -- <cmd>          Launch a game on the game GPU (Steam: Lapse run -- %command%)");
    println!("  \x1b[1;36muninstall\x1b[0m             Remove autostart services and background unit files");
    println!("  \x1b[1;36mvoice-test\x1b[0m [sec]      Test microphone capture, audio levels, and voice trigger");
    println!("  \x1b[1;36mhelp\x1b[0m                  Print this command help menu");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_log_tail_missing() {
        let path = std::env::temp_dir().join("lapse_test_missing_log.log");
        let _ = std::fs::remove_file(&path);
        let offset = print_log_tail(&path, "test.log", 40);
        assert_eq!(offset, 0);
    }

    #[test]
    fn test_print_log_tail_existing() {
        let path = std::env::temp_dir().join(format!("lapse_test_log_{}.log", std::process::id()));
        let mut content = String::new();
        for i in 0..60 {
            content.push_str(&format!("Line {}\n", i));
        }
        std::fs::write(&path, &content).unwrap();

        let offset = print_log_tail(&path, "test.log", 40);
        assert_eq!(offset, content.len() as u64);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_log_follower_poll_and_truncate() {
        let path = std::env::temp_dir().join(format!("lapse_test_follower_{}.log", std::process::id()));
        std::fs::write(&path, "initial line 1\n").unwrap();

        let mut follower = LogFollower {
            path: path.clone(),
            prefix: "[test] ",
            offset: 0,
            leftover: Vec::new(),
        };

        follower.poll();
        assert_eq!(follower.offset, 15);
        assert!(follower.leftover.is_empty());

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"partial").unwrap();
        drop(file);

        follower.poll();
        assert_eq!(follower.offset, 15 + 7);
        assert_eq!(follower.leftover, b"partial");

        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b" completed\n").unwrap();
        drop(file);

        follower.poll();
        assert_eq!(follower.offset, 15 + 7 + 11);
        assert!(follower.leftover.is_empty());

        std::fs::write(&path, "fresh\n").unwrap();
        follower.poll();
        assert_eq!(follower.offset, 6);
        assert!(follower.leftover.is_empty());

        let _ = std::fs::remove_file(&path);
    }
}
