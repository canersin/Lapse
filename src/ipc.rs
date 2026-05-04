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
