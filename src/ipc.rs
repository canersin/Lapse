use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Command {
    GetStatus,
    StartReplay,
    SaveReplay,
    StartRecording,
    Stop,
    ReloadConfig,
    /// Save only the last N seconds (keeps the buffer).
    SaveLast(u32),
    Screenshot,
    /// Mark this moment for the trimmer timeline.
    Mark,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct StatusResponse {
    pub recording: bool,
    pub mode: String,
    pub is_installed: bool,
    /// Last fatal error reported by gpu-screen-recorder, if any.
    pub last_error: Option<String>,
    /// Non-fatal configuration problems (e.g. dual GPU misconfiguration).
    pub warnings: Vec<String>,
    /// DRM card gpu-screen-recorder reported it is encoding on.
    pub encode_card: String,
    /// Monitor that is being captured.
    pub capture_target: String,
    /// How many times the watchdog had to restart the recorder.
    pub restarts: u32,
    /// Path of the most recently saved replay/recording.
    pub last_saved: Option<String>,
    /// Monotonic counter bumped on every save, so clients can detect new saves.
    pub save_counter: u64,
    /// Game Lapse currently sees (custom or detected).
    #[serde(default)]
    pub game: Option<String>,
    /// Game mode with no game open: the buffer is off on purpose.
    #[serde(default)]
    pub waiting_for_game: bool,
    /// Seconds since CS2 last sent game state (auto clips), if it ever did.
    #[serde(default)]
    pub cs2_data_age: Option<u64>,
    #[serde(default)]
    pub dota_data_age: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Response {
    Status(StatusResponse),
    Ok,
    Error(String),
}

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::config::Config;
use crate::hotkeys::HotkeyManager;
use crate::recorder::Recorder;

static LAST_STATUS: std::sync::Mutex<Option<StatusResponse>> = std::sync::Mutex::new(None);

/// Per-user socket path. Lives in $XDG_RUNTIME_DIR so multiple users on one
/// machine don't fight over a shared /tmp socket.
pub fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() && std::path::Path::new(&dir).is_dir() {
            return PathBuf::from(dir).join("lapse.sock");
        }
    }
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/lapse-{}.sock", uid))
}

pub fn daemon_alive() -> bool {
    UnixStream::connect(socket_path()).is_ok()
}

/// Sends a command to the daemon and waits for the reply.
pub fn send(cmd: Command) -> Option<Response> {
    let timeout = match cmd {
        // Stopping a continuous recording may take a few seconds while the mp4 is finalized.
        Command::Stop | Command::StartRecording | Command::ReloadConfig => Duration::from_secs(8),
        _ => Duration::from_millis(1500),
    };
    let mut stream = UnixStream::connect(socket_path()).ok()?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(1500)));
    let data = serde_json::to_string(&cmd).ok()?;
    stream.write_all(data.as_bytes()).ok()?;
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

pub fn start_server(
    recorder: Arc<Mutex<Recorder>>,
    hotkey_manager: Arc<HotkeyManager>,
) -> anyhow::Result<()> {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let recorder = Arc::clone(&recorder);
                let hotkey_manager = Arc::clone(&hotkey_manager);
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
                    // Read until the client closes its write half or we have a complete JSON value.
                    loop {
                        match stream.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(n) => {
                                buf.extend_from_slice(&chunk[..n]);
                                if serde_json::from_slice::<Command>(&buf).is_ok() || buf.len() > 64 * 1024 {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    let response = match serde_json::from_slice::<Command>(&buf) {
                        Ok(cmd) => handle_command(cmd, &recorder, &hotkey_manager),
                        Err(e) => Response::Error(format!("Invalid command: {}", e)),
                    };
                    if let Ok(json) = serde_json::to_string(&response) {
                        let _ = stream.write_all(json.as_bytes());
                    }
                });
            }
        }
    });
    Ok(())
}

fn handle_command(
    cmd: Command,
    recorder: &Arc<Mutex<Recorder>>,
    hotkey_manager: &Arc<HotkeyManager>,
) -> Response {
    let result = |r: anyhow::Result<()>| match r {
        Ok(()) => Response::Ok,
        Err(e) => Response::Error(e.to_string()),
    };
    match cmd {
        Command::GetStatus => {
            if let Ok(mut rec) = recorder.try_lock() {
                let status = rec.status();
                if let Ok(mut cache) = LAST_STATUS.lock() {
                    *cache = Some(status.clone());
                }
                Response::Status(status)
            } else if let Some(cached) = LAST_STATUS.lock().ok().and_then(|c| c.clone()) {
                Response::Status(cached)
            } else {
                Response::Error("Busy".into())
            }
        }
        Command::StartReplay => match recorder.lock() {
            Ok(mut rec) => result(rec.start_replay()),
            Err(_) => Response::Error("Lock failed".into()),
        },
        Command::SaveReplay => match recorder.lock() {
            Ok(mut rec) => result(rec.save_replay()),
            Err(_) => Response::Error("Lock failed".into()),
        },
        Command::SaveLast(secs) => match recorder.lock() {
            Ok(mut rec) => result(rec.save_last(secs.clamp(5, 1800), None)),
            Err(_) => Response::Error("Lock failed".into()),
        },
        Command::Screenshot => match recorder.lock() {
            Ok(mut rec) => result(rec.take_screenshot()),
            Err(_) => Response::Error("Lock failed".into()),
        },
        Command::Mark => match recorder.lock() {
            Ok(mut rec) => result(rec.add_marker()),
            Err(_) => Response::Error("Lock failed".into()),
        },
        Command::StartRecording => match recorder.lock() {
            Ok(mut rec) => result(rec.start_recording()),
            Err(_) => Response::Error("Lock failed".into()),
        },
        Command::Stop => match recorder.lock() {
            Ok(mut rec) => result(rec.stop()),
            Err(_) => Response::Error("Lock failed".into()),
        },
        Command::ReloadConfig => {
            if let Ok(new_config) = Config::load() {
                let res = match recorder.lock() {
                    Ok(mut rec) => rec.update_config(new_config.clone()),
                    Err(_) => Err(anyhow::anyhow!("Lock failed")),
                };
                hotkey_manager.register_from_config(&new_config);
                result(res)
            } else {
                Response::Error("Failed to load config".into())
            }
        }
    }
}
