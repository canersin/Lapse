//! Sends a clip to a Discord channel through a webhook (curl, in the background).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Discord's upload limit without Nitro.
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum SendState {
    /// Over 10 MB: being exported with the Discord preset first.
    Shrinking,
    Sending,
    Sent,
    Failed(String),
}

static SENDS: Mutex<Vec<(PathBuf, SendState)>> = Mutex::new(Vec::new());

fn set(path: &Path, state: SendState) {
    let mut sends = SENDS.lock().unwrap_or_else(|e| e.into_inner());
    sends.retain(|(p, _)| p != path);
    sends.push((path.to_path_buf(), state));
}

pub fn state_for(path: &Path) -> Option<SendState> {
    SENDS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, _)| p == path).map(|(_, s)| s.clone())
}

pub fn is_webhook(url: &str) -> bool {
    let url = url.trim();
    ["https://discord.com/api/webhooks/", "https://discordapp.com/api/webhooks/", "https://ptb.discord.com/api/webhooks/", "https://canary.discord.com/api/webhooks/"]
        .iter()
        .any(|p| url.starts_with(p))
}

pub fn send(path: PathBuf, webhook: String) {
    send_as(path.clone(), path, webhook);
}

/// Shown as "shrinking" for `key` while another part of the app exports it.
pub fn mark_shrinking(key: &Path) {
    set(key, SendState::Shrinking);
}

pub fn mark_failed(key: &Path, why: &str) {
    set(key, SendState::Failed(why.to_string()));
}

/// Uploads `file`, reporting the progress under `key` (the clip the user picked).
pub fn send_as(key: PathBuf, file: PathBuf, webhook: String) {
    if !is_webhook(&webhook) {
        set(&key, SendState::Failed("the webhook link doesn't look like a Discord webhook".into()));
        return;
    }
    set(&key, SendState::Sending);
    let path = file;
    std::thread::spawn(move || {
        let result = Command::new("curl")
            .args(["-sS", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "300", "-F"])
            .arg(format!("file=@\"{}\"", path.display()))
            .arg(webhook.trim())
            .output();
        let state = match result {
            Ok(out) => {
                let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if code.starts_with('2') {
                    SendState::Sent
                } else if code == "413" {
                    SendState::Failed("the file is too big for Discord".into())
                } else if code == "000" {
                    SendState::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string())
                } else {
                    SendState::Failed(format!("Discord answered {}", code))
                }
            }
            Err(e) => SendState::Failed(format!("curl: {}", e)),
        };
        set(&key, state);
    });
}

/// Sends the whole clip; one over 10 MB is first exported with the Discord preset.
pub fn send_or_shrink(path: PathBuf, webhook: String, duration_secs: f64) {
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if bytes <= MAX_BYTES {
        send(path, webhook);
        return;
    }
    use crate::export::{AudioMix, ExportPreset, ExportRequest, ExportState};
    set(&path, SendState::Shrinking);
    let output = crate::export::default_output(&path, ExportPreset::Discord);
    let job = crate::export::start(ExportRequest {
        input: path.clone(),
        start: 0.0,
        end: duration_secs.max(0.1),
        preset: ExportPreset::Discord,
        audio: AudioMix::default(),
        output,
        crop_x: 0.5,
    });
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        match job.state() {
            ExportState::Running { .. } => continue,
            ExportState::Done { path: small, .. } => return send_as(path, small, webhook),
            ExportState::Failed(e) => return set(&path, SendState::Failed(format!("shrinking failed: {}", e))),
            ExportState::Cancelled => return set(&path, SendState::Failed("cancelled".into())),
        }
    });
}

/// Posts a text message (e.g. a share link; Discord shows the video player from it).
pub fn post_message(webhook: &str, content: &str) -> Result<(), String> {
    if !is_webhook(webhook) {
        return Err("the webhook link doesn't look like a Discord webhook".into());
    }
    let body = serde_json::json!({ "content": content }).to_string();
    let out = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "30", "-H", "Content-Type: application/json", "-d"])
        .arg(body)
        .arg(webhook.trim())
        .output()
        .map_err(|e| format!("curl: {}", e))?;
    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if code.starts_with('2') {
        Ok(())
    } else if code == "000" {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    } else {
        Err(format!("Discord answered {}", code))
    }
}

