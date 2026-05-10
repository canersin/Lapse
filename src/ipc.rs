use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Command {
    GetStatus,
    StartReplay,
    SaveReplay,
    StartRecording,
    Stop,
    ReloadConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatusResponse {
    pub recording: bool,
    pub mode: String,
    pub is_installed: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Response {
    Status(StatusResponse),
    Ok,
    Error(String),
}

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};
use crate::config::Config;
use crate::hotkeys::HotkeyManager;
use crate::recorder::{Recorder, RecordingMode};

pub fn start_server(
    socket_path: &str,
    recorder: Arc<Mutex<Recorder>>,
    hotkey_manager: Arc<HotkeyManager>,
) -> anyhow::Result<()> {
    let listener = UnixListener::bind(socket_path)?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let recorder = Arc::clone(&recorder);
                let hotkey_manager = Arc::clone(&hotkey_manager);
                std::thread::spawn(move || {
                    let mut buffer = [0; 1024];
                    if let Ok(n) = stream.read(&mut buffer) {
                        let cmd_str = String::from_utf8_lossy(&buffer[..n]);
                        if let Ok(cmd) = serde_json::from_str::<Command>(&cmd_str) {
                            let response = handle_command(cmd, &recorder, &hotkey_manager);
                            let _ = stream.write_all(serde_json::to_string(&response).unwrap().as_bytes());
                        }
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
    match cmd {
        Command::GetStatus => {
            if let Ok(rec) = recorder.lock() {
                Response::Status(StatusResponse {
                    recording: rec.current_mode() != RecordingMode::None,
                    mode: format!("{:?}", rec.current_mode()),
                    is_installed: rec.is_installed(),
                })
            } else {
                Response::Error("Lock failed".into())
            }
        }
        Command::StartReplay => {
            if let Ok(mut rec) = recorder.lock() {
                let _ = rec.start_replay();
                Response::Ok
            } else {
                Response::Error("Lock failed".into())
            }
        }
        Command::SaveReplay => {
            if let Ok(rec) = recorder.lock() {
                let _ = rec.save_replay();
                Response::Ok
            } else {
                Response::Error("Lock failed".into())
            }
        }
        Command::StartRecording => {
            if let Ok(mut rec) = recorder.lock() {
                let _ = rec.start_recording();
                Response::Ok
            } else {
                Response::Error("Lock failed".into())
            }
        }
        Command::Stop => {
            if let Ok(mut rec) = recorder.lock() {
                let _ = rec.stop();
                Response::Ok
            } else {
                Response::Error("Lock failed".into())
            }
        }
        Command::ReloadConfig => {
            if let Ok(new_config) = Config::load() {
                if let Ok(mut rec) = recorder.lock() {
                    rec.update_config(new_config.clone());
                }
                
                hotkey_manager.register_from_config(&new_config);

                Response::Ok
            } else {
                Response::Error("Failed to load config".into())
            }
        }
    }
}
