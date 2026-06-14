use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub hermes_url: String,
    pub hermes_api_key: String,
    pub openai_api_key: String,
    pub elevenlabs_api_key: String,
    pub elevenlabs_voice_id: String,
    pub wake_word: String,
    pub character: String,
    pub tts_provider: String,
    pub stt_provider: String,
    pub wake_word_provider: String,
    pub porcupine_access_key: String,
    pub autostart: bool,
    pub start_minimized: bool,
    pub microphone_device: String,
    pub output_device: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hermes_url: "http://127.0.0.1:9119".to_string(),
            hermes_api_key: String::new(),
            openai_api_key: String::new(),
            elevenlabs_api_key: String::new(),
            elevenlabs_voice_id: "21m00Tcm4TlvDq8ikWAM".to_string(),
            wake_word: "zero".to_string(),
            character: "assistant".to_string(),
            tts_provider: "hermes".to_string(),
            stt_provider: "hermes".to_string(),
            wake_word_provider: "energy".to_string(),
            porcupine_access_key: String::new(),
            autostart: true,
            start_minimized: false,
            microphone_device: "default".to_string(),
            output_device: "default".to_string(),
        }
    }
}

pub fn config_path(app: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_config_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("config.json"))
}

pub fn load_config(app: &tauri::AppHandle) -> AppConfig {
    let path = match config_path(app) {
        Ok(p) => p,
        Err(_) => return AppConfig::default(),
    };
    if !path.exists() {
        return AppConfig::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

pub fn save_config(app: &tauri::AppHandle, cfg: &AppConfig) -> Result<()> {
    let path = config_path(app)?;
    let s = serde_json::to_string_pretty(cfg)?;
    std::fs::write(path, s)?;
    Ok(())
}
