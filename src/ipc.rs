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
    CaptureHotkeys(bool),
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
    #[serde(default)]
    pub lol_data_age: Option<u64>,
    #[serde(default)]
    pub captured_hotkey: Option<crate::hotkeys::HotkeyEvent>,
    #[serde(default)]
    pub captured_hotkey_seq: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Response {
    Status(StatusResponse),
    Ok,
    Error(String),
}

use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::config::Config;
use crate::hotkeys::HotkeyManager;
use crate::recorder::Recorder;

#[derive(Default, Debug)]
pub struct HotkeyCapture {
    pub until: Option<Instant>,
    pub last: Option<crate::hotkeys::HotkeyEvent>,
    pub seq: u64,
}

impl HotkeyCapture {
    pub fn is_active(&self) -> bool {
        match self.until {
            Some(deadline) => Instant::now() < deadline,
            None => false,
        }
    }
}

static LAST_STATUS: std::sync::Mutex<Option<StatusResponse>> = std::sync::Mutex::new(None);

/// Per-user socket path. Lives in $XDG_RUNTIME_DIR so multiple users on one
/// machine don't fight over a shared /tmp socket.
#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() && std::path::Path::new(&dir).is_dir() {
            return PathBuf::from(dir).join("lapse.sock");
        }
    }
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/lapse-{}.sock", uid))
}

#[cfg(windows)]
pub fn socket_path() -> PathBuf {
    if let Some(proj) = directories::ProjectDirs::from("com", "lapse", "lapse") {
        let dir = proj.config_dir();
        let _ = std::fs::create_dir_all(dir);
        return dir.join("ipc.port");
    }
    std::env::temp_dir().join("lapse-ipc.port")
}

#[cfg(windows)]
const DEFAULT_IPC_PORT: u16 = 41820;

#[cfg(windows)]
pub fn ipc_port() -> u16 {
    let path = socket_path();
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Ok(port) = s.trim().parse::<u16>() {
            return port;
        }
    }
    let _ = std::fs::write(&path, DEFAULT_IPC_PORT.to_string());
    DEFAULT_IPC_PORT
}

#[cfg(unix)]
pub fn daemon_alive() -> bool {
    UnixStream::connect(socket_path()).is_ok()
}

#[cfg(windows)]
pub fn daemon_alive() -> bool {
    let port = ipc_port();
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

#[derive(Debug)]
pub enum IpcError {
    NotRunning,
    Timeout,
    Other(String),
}

/// Sends a command to the daemon and waits for the reply.
#[cfg(unix)]
pub fn send_detailed(cmd: Command) -> Result<Response, IpcError> {
    let timeout = match cmd {
        Command::SaveReplay | Command::SaveLast(_) => Duration::from_secs(20),
        Command::Screenshot => Duration::from_secs(10),
        Command::Stop | Command::StartRecording | Command::ReloadConfig => Duration::from_secs(8),
        _ => Duration::from_millis(1500),
    };
    let mut stream = UnixStream::connect(socket_path()).map_err(|_| IpcError::NotRunning)?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(1500)));
    let data = serde_json::to_string(&cmd).map_err(|e| IpcError::Other(e.to_string()))?;
    stream.write_all(data.as_bytes()).map_err(|e| IpcError::Other(e.to_string()))?;
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut buf = Vec::new();
    if let Err(e) = stream.read_to_end(&mut buf) {
        if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
            return Err(IpcError::Timeout);
        }
        return Err(IpcError::Other(e.to_string()));
    }
    serde_json::from_slice(&buf).map_err(|e| IpcError::Other(e.to_string()))
}

#[cfg(unix)]
pub fn send(cmd: Command) -> Option<Response> {
    send_detailed(cmd).ok()
}

#[cfg(windows)]
pub fn send_detailed(cmd: Command) -> Result<Response, IpcError> {
    use std::net::TcpStream;
    let timeout = match cmd {
        Command::SaveReplay | Command::SaveLast(_) => Duration::from_secs(20),
        Command::Screenshot => Duration::from_secs(10),
        Command::Stop | Command::StartRecording | Command::ReloadConfig => Duration::from_secs(8),
        _ => Duration::from_millis(1500),
    };
    let port = ipc_port();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|_| IpcError::NotRunning)?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(1500)));
    let data = serde_json::to_string(&cmd).map_err(|e| IpcError::Other(e.to_string()))?;
    stream.write_all(data.as_bytes()).map_err(|e| IpcError::Other(e.to_string()))?;
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut buf = Vec::new();
    if let Err(e) = stream.read_to_end(&mut buf) {
        if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
            return Err(IpcError::Timeout);
        }
        return Err(IpcError::Other(e.to_string()));
    }
    serde_json::from_slice(&buf).map_err(|e| IpcError::Other(e.to_string()))
}

#[cfg(windows)]
pub fn send(cmd: Command) -> Option<Response> {
    send_detailed(cmd).ok()
}

#[cfg(unix)]
pub fn start_server(
    recorder: Arc<Mutex<Recorder>>,
    hotkey_manager: Arc<HotkeyManager>,
    hotkey_capture: Arc<Mutex<HotkeyCapture>>,
) -> anyhow::Result<()> {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let recorder = Arc::clone(&recorder);
                let hotkey_manager = Arc::clone(&hotkey_manager);
                let hotkey_capture = Arc::clone(&hotkey_capture);
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
                        Ok(cmd) => handle_command(cmd, &recorder, &hotkey_manager, &hotkey_capture),
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

#[cfg(windows)]
pub fn start_server(
    recorder: Arc<Mutex<Recorder>>,
    hotkey_manager: Arc<HotkeyManager>,
    hotkey_capture: Arc<Mutex<HotkeyCapture>>,
) -> anyhow::Result<()> {
    use std::net::TcpListener;
    let port = ipc_port();
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let recorder = Arc::clone(&recorder);
                let hotkey_manager = Arc::clone(&hotkey_manager);
                let hotkey_capture = Arc::clone(&hotkey_capture);
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
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
                        Ok(cmd) => handle_command(cmd, &recorder, &hotkey_manager, &hotkey_capture),
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
    hotkey_capture: &Arc<Mutex<HotkeyCapture>>,
) -> Response {
    let result = |r: anyhow::Result<()>| match r {
        Ok(()) => Response::Ok,
        Err(e) => Response::Error(e.to_string()),
    };
    match cmd {
        Command::GetStatus => {
            let (captured_hotkey, captured_hotkey_seq) = if let Ok(cap) = hotkey_capture.lock() {
                (cap.last, cap.seq)
            } else {
                (None, 0)
            };
            if let Ok(mut rec) = recorder.try_lock() {
                let mut status = rec.status();
                status.captured_hotkey = captured_hotkey;
                status.captured_hotkey_seq = captured_hotkey_seq;
                if let Ok(mut cache) = LAST_STATUS.lock() {
                    *cache = Some(status.clone());
                }
                Response::Status(status)
            } else if let Some(mut cached) = LAST_STATUS.lock().ok().and_then(|c| c.clone()) {
                cached.captured_hotkey = captured_hotkey;
                cached.captured_hotkey_seq = captured_hotkey_seq;
                Response::Status(cached)
            } else {
                Response::Error("Busy".into())
            }
        }
        Command::CaptureHotkeys(enable) => {
            if let Ok(mut cap) = hotkey_capture.lock() {
                if enable {
                    cap.until = Some(Instant::now() + Duration::from_secs(30));
                } else {
                    cap.until = None;
                }
            }
            Response::Ok
        }
        Command::StartReplay => match recorder.lock() {
            Ok(mut rec) => result(rec.start_replay()),
            Err(_) => Response::Error("Lock failed".into()),
        },
        Command::SaveReplay => {
            #[cfg(windows)]
            {
                let rx = match recorder.lock() {
                    Ok(mut rec) => match rec.queue_save_replay() {
                        Ok(rx) => rx,
                        Err(e) => return Response::Error(e.to_string()),
                    },
                    Err(_) => return Response::Error("Lock failed".into()),
                };
                match rx.recv_timeout(Duration::from_secs(120)) {
                    Ok(Ok(_)) => Response::Ok,
                    Ok(Err(e)) => Response::Error(e),
                    Err(_) => Response::Error("Timed out waiting for replay clip to save".into()),
                }
            }
            #[cfg(not(windows))]
            {
                match recorder.lock() {
                    Ok(mut rec) => result(rec.save_replay()),
                    Err(_) => Response::Error("Lock failed".into()),
                }
            }
        }
        Command::SaveLast(secs) => {
            let secs = secs.clamp(5, 1800);
            #[cfg(windows)]
            {
                let rx = match recorder.lock() {
                    Ok(mut rec) => match rec.queue_save_last(secs, None) {
                        Ok(rx) => rx,
                        Err(e) => return Response::Error(e.to_string()),
                    },
                    Err(_) => return Response::Error("Lock failed".into()),
                };
                match rx.recv_timeout(Duration::from_secs(18)) {
                    Ok(Ok(_)) => Response::Ok,
                    Ok(Err(e)) => Response::Error(e),
                    Err(_) => Response::Error("Timed out waiting for replay clip to save".into()),
                }
            }
            #[cfg(not(windows))]
            {
                match recorder.lock() {
                    Ok(mut rec) => result(rec.save_last(secs, None)),
                    Err(_) => Response::Error("Lock failed".into()),
                }
            }
        }
        Command::Screenshot => {
            #[cfg(windows)]
            {
                let rx = match recorder.lock() {
                    Ok(mut rec) => match rec.queue_screenshot() {
                        Ok(rx) => rx,
                        Err(e) => return Response::Error(e.to_string()),
                    },
                    Err(_) => return Response::Error("Lock failed".into()),
                };
                match rx.recv_timeout(Duration::from_secs(10)) {
                    Ok(Ok(_)) => Response::Ok,
                    Ok(Err(e)) => Response::Error(e),
                    Err(_) => Response::Error("Timed out waiting for screenshot to save".into()),
                }
            }
            #[cfg(not(windows))]
            {
                match recorder.lock() {
                    Ok(mut rec) => result(rec.take_screenshot()),
                    Err(_) => Response::Error("Lock failed".into()),
                }
            }
        },
        Command::Mark => match recorder.lock() {
            Ok(mut rec) => {
                let res = rec.add_marker();
                if res.is_ok() {
                    crate::utils::send_notification("Lapse", "Marker added", "bookmark-new");
                }
                result(res)
            }
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
