use std::process::{Command, Child};
use anyhow::Result;
use notify_rust::Notification;
use crate::config::Config;

pub struct Recorder {
    process: Option<Child>,
    config: Config,
    mode: RecordingMode,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RecordingMode {
    None,
    Replay,
    Continuous,
}

impl Recorder {
    pub fn new(config: Config) -> Self {
        Self { process: None, config, mode: RecordingMode::None }
    }

    pub fn is_installed(&self) -> bool {
        Command::new("which")
            .arg(&self.config.recorder_path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn start_replay(&mut self) -> Result<()> {
        if self.process.is_some() {
            return Ok(()); // already running
        }

        let mut cmd = Command::new(&self.config.recorder_path);
        cmd.arg("-w").arg("screen") // capture whole screen
           .arg("-f").arg(self.config.fps.to_string());
           
        if self.config.resolution != "Native" {
            cmd.arg("-s").arg(&self.config.resolution);
        }
           
        cmd.arg("-r").arg(self.config.replay_seconds.to_string());
        cmd.arg("-restart-replay-on-save").arg("yes");
        cmd.arg("-c").arg("mp4"); // required container format
        cmd.arg("-o").arg(&self.config.save_path);

        let mut audio_args = String::new();
        if !self.config.audio_output.is_empty() && self.config.audio_output != "None" {
            audio_args.push_str(&self.config.audio_output);
        }
        if !self.config.audio_input.is_empty() && self.config.audio_input != "None" {
            if !audio_args.is_empty() {
                audio_args.push('|');
            }
            audio_args.push_str(&self.config.audio_input);
        }

        if !audio_args.is_empty() {
            cmd.arg("-a").arg(&audio_args);
        }

        // Ensure save path exists
        if !self.config.save_path.exists() {
            std::fs::create_dir_all(&self.config.save_path)?;
        }

        self.process = Some(cmd.spawn()?);
        self.mode = RecordingMode::Replay;
        Ok(())
    }

    pub fn start_recording(&mut self) -> Result<()> {
        if self.process.is_some() {
            self.stop()?;
        }

        let mut cmd = Command::new(&self.config.recorder_path);
        cmd.arg("-w").arg("screen")
           .arg("-f").arg(self.config.fps.to_string());
           
        if self.config.resolution != "Native" {
            cmd.arg("-s").arg(&self.config.resolution);
        }
        
        cmd.arg("-c").arg("mp4");
           
        // Generate a continuous record filename
        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let filepath = self.config.save_path.join(format!("lapse_record_{}.mp4", timestamp));
        cmd.arg("-o").arg(&filepath);

        let mut audio_args = String::new();
        if !self.config.audio_output.is_empty() && self.config.audio_output != "None" {
            audio_args.push_str(&self.config.audio_output);
        }
        if !self.config.audio_input.is_empty() && self.config.audio_input != "None" {
            if !audio_args.is_empty() {
                audio_args.push('|');
            }
            audio_args.push_str(&self.config.audio_input);
        }

        if !audio_args.is_empty() {
            cmd.arg("-a").arg(&audio_args);
        }

        if !self.config.save_path.exists() {
            std::fs::create_dir_all(&self.config.save_path)?;
        }

        self.process = Some(cmd.spawn()?);
        self.mode = RecordingMode::Continuous;
        
        let _ = Notification::new()
            .summary("Lapse")
            .body("Manual recording started.")
            .icon("media-record")
            .show();
            
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.mode = RecordingMode::None;
        Ok(())
    }
    
    pub fn current_mode(&self) -> RecordingMode {
        self.mode
    }

    pub fn save_replay(&self) -> Result<()> {
        // gpu-screen-recorder saves replay on SIGUSR1
        if let Some(child) = &self.process {
            let pid = child.id();
            Command::new("kill").arg("-SIGUSR1").arg(pid.to_string()).status()?;
            
            // Play embedded sound
            std::thread::spawn(|| {
                if let Ok(handle) = rodio::DeviceSinkBuilder::open_default_sink() {
                    let cursor = std::io::Cursor::new(include_bytes!("assets/shutter.ogg"));
                    if let Ok(decoder) = rodio::Decoder::new(cursor) {
                        let player = rodio::Player::connect_new(&handle.mixer());
                        player.append(decoder);
                        player.sleep_until_end();
                    }
                }
            });

            // Notify user
            let _ = Notification::new()
                .summary("Lapse")
                .body(&format!("Replay saved to {:?}", self.config.save_path))
                .icon("video-display")
                .show();
        }
        Ok(())
    }
}
