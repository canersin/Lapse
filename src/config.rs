use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::Result;
use directories::ProjectDirs;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub save_path: PathBuf,
    pub recorder_path: String,
    pub replay_seconds: u32,
    pub hotkey_replay: String,
    pub hotkey_record: String,
    pub audio_output: String,
    pub audio_input: String,
    pub quality: String,
    pub fps: u32,
    pub resolution: String,
}

impl Default for Config {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        Self {
            save_path: PathBuf::from(home).join("Videos/Lapse"),
            recorder_path: "gpu-screen-recorder".into(),
            replay_seconds: 60,
            hotkey_replay: "F10".into(),
            hotkey_record: "F9".into(),
            audio_output: "default_output".into(),
            audio_input: "default_input".into(),
            quality: "high".into(),
            fps: 60,
            resolution: "1920x1080".into(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;
        if !config_path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }
        let content = std::fs::read_to_string(config_path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::get_config_path()?;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
    }

    fn get_config_path() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "lapse", "lapse")
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
        Ok(proj_dirs.config_dir().join("config.toml"))
    }
}
