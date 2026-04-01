use std::process::{Command, Child};
use anyhow::Result;
use notify_rust::Notification;
use crate::config::Config;

pub struct Recorder {
    process: Option<Child>,
    config: Config,
}

impl Recorder {
    pub fn new(config: Config) -> Self {
        Self { process: None, config }
    }

    pub fn start_replay(&mut self) -> Result<()> {
        if self.process.is_some() {
            return Ok(());
        }

        // Basic command for gpu-screen-recorder in replay mode
        // Note: Actual arguments might need to be adjusted based on system (NVIDIA/AMD/Intel)
        let mut cmd = Command::new(&self.config.recorder_path);
        cmd.arg("-w").arg("screen") // capture whole screen
           .arg("-f").arg(self.config.fps.to_string())
           .arg("-r").arg(self.config.replay_seconds.to_string())
           .arg("-c").arg("mp4") // required container format
           .arg("-o").arg(&self.config.save_path);

        if !self.config.audio_source.is_empty() && self.config.audio_source != "None" {
            cmd.arg("-a").arg(&self.config.audio_source);
        }

        // Ensure save path exists
        if !self.config.save_path.exists() {
            std::fs::create_dir_all(&self.config.save_path)?;
        }

        self.process = Some(cmd.spawn()?);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            child.kill()?;
            child.wait()?;
        }
        Ok(())
    }

    pub fn save_replay(&self) -> Result<()> {
        // gpu-screen-recorder saves replay on SIGUSR1
        if let Some(child) = &self.process {
            let pid = child.id();
            Command::new("kill").arg("-SIGUSR1").arg(pid.to_string()).status()?;
            
            // Play a nice sound (non-blocking)
            let _ = Command::new("paplay")
                .arg("/usr/share/sounds/freedesktop/stereo/camera-shutter.oga")
                .spawn();

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
