use crate::{audio, config::AppConfig, hermes, stt, tts, wakeword};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PipelineEvent {
    State { state: String, message: String },
    Transcript { text: String, role: String },
    Response { text: String },
    Error { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineState {
    pub listening: bool,
    pub recording: bool,
    pub processing: bool,
    pub speaking: bool,
}

pub struct Pipeline {
    pub cfg: AppConfig,
    pub running: Arc<AtomicBool>,
    pub history: std::sync::Mutex<Vec<hermes::ChatMessage>>,
}

impl Pipeline {
    pub fn new(cfg: AppConfig) -> Self {
        Self {
            cfg,
            running: Arc::new(AtomicBool::new(false)),
            history: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn emit_state(&self, app: &AppHandle, state: &str, message: &str) {
        let _ = app.emit(
            "zero://event",
            PipelineEvent::State {
                state: state.to_string(),
                message: message.to_string(),
            },
        );
    }

    pub fn emit_transcript(&self, app: &AppHandle, text: &str, role: &str) {
        let _ = app.emit(
            "zero://event",
            PipelineEvent::Transcript {
                text: text.to_string(),
                role: role.to_string(),
            },
        );
    }

    pub fn emit_error(&self, app: &AppHandle, err: &str) {
        let _ = app.emit(
            "zero://event",
            PipelineEvent::Error {
                error: err.to_string(),
            },
        );
    }

    /// One-shot: listen → STT → Hermes → TTS → play
    pub async fn run_once(&self, app: &AppHandle) -> Result<()> {
        // 1. acquire input device
        let device = match audio::get_input_device(&self.cfg.microphone_device) {
            Ok(d) => d,
            Err(e) => {
                let msg = format!("Mic error: {}", e);
                self.emit_state(app, "error", &msg);
                return Err(e);
            }
        };

        // 2. record until silence
        self.emit_state(app, "listening", "Speak now");
        let recording = Arc::new(AtomicBool::new(false));
        let samples = tokio::task::spawn_blocking({
            let device = device;
            let recording = recording.clone();
            move || -> Result<Vec<i16>> {
                audio::record_until_silence(
                    &device,
                    recording,
                    500,
                    800,
                    15_000,
                )
            }
        })
        .await??;

        if samples.is_empty() {
            self.emit_state(app, "idle", "no audio");
            return Ok(());
        }

        // 3. STT
        self.emit_state(app, "transcribing", "Converting speech to text");
        let stt_cfg = stt::SttConfig {
            provider: self.cfg.stt_provider.clone(),
            hermes_url: self.cfg.hermes_url.clone(),
            hermes_api_key: self.cfg.hermes_api_key.clone(),
            openai_api_key: self.cfg.openai_api_key.clone(),
        };
        let stt_result = match stt::transcribe(&samples, &stt_cfg).await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("STT error: {}", e);
                self.emit_error(app, &msg);
                self.emit_state(app, "idle", "STT failed");
                return Err(e);
            }
        };

        let text = stt_result.text.trim().to_string();
        if text.is_empty() {
            self.emit_state(app, "idle", "empty transcript");
            return Ok(());
        }

        self.emit_transcript(app, &text, "user");

        // 4. Wake word check
        let ww_cfg = wakeword::WakeWordConfig {
            wake_word: self.cfg.wake_word.clone(),
            ..Default::default()
        };
        let command = match wakeword::detect_and_extract(&samples, &text, &ww_cfg) {
            Some(c) => c,
            None => {
                self.emit_state(app, "idle", "no wake word");
                return Ok(());
            }
        };

        if command.is_empty() {
            self.emit_state(app, "idle", "no command after wake word");
            return Ok(());
        }

        // 5. Hermes chat
        self.emit_state(app, "thinking", "Asking Hermes");
        let hermes_cfg = hermes::HermesConfig {
            base_url: self.cfg.hermes_url.replace(":9119", ":9120"),
            api_key: self.cfg.hermes_api_key.clone(),
            ..Default::default()
        };
        let history = self.history.lock().unwrap().clone();
        let response = match hermes::ask(&command, &history, &hermes_cfg).await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("Hermes error: {}", e);
                self.emit_error(app, &msg);
                self.emit_state(app, "idle", "Hermes failed");
                return Err(e);
            }
        };

        self.emit_transcript(app, &response, "assistant");

        // Add to history
        {
            let mut h = self.history.lock().unwrap();
            h.push(hermes::ChatMessage {
                role: "user".to_string(),
                content: command,
            });
            h.push(hermes::ChatMessage {
                role: "assistant".to_string(),
                content: response.clone(),
            });
            // cap
            if h.len() > 20 {
                let drop = h.len() - 20;
                h.drain(0..drop);
            }
        }

        // 6. TTS
        self.emit_state(app, "speaking", "Synthesizing speech");
        let tts_cfg = tts::TtsConfig {
            provider: self.cfg.tts_provider.clone(),
            hermes_url: self.cfg.hermes_url.clone(),
            hermes_api_key: self.cfg.hermes_api_key.clone(),
            openai_api_key: self.cfg.openai_api_key.clone(),
            openai_voice: self.cfg.character.clone(),
            elevenlabs_api_key: self.cfg.elevenlabs_api_key.clone(),
            elevenlabs_voice_id: self.cfg.elevenlabs_voice_id.clone(),
            character: self.cfg.character.clone(),
        };
        let tts_result = match tts::synthesize(&response, &tts_cfg).await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("TTS error: {}", e);
                self.emit_error(app, &msg);
                self.emit_state(app, "idle", "TTS failed");
                return Err(e);
            }
        };

        // 7. Play
        let output_device = self.cfg.output_device.clone();
        let audio_bytes = tts_result.audio_bytes;
        let _ = tokio::task::spawn_blocking(move || {
            if let Err(e) = audio::play_bytes(audio_bytes, Some(&output_device)) {
                tracing::error!("playback error: {e}");
            }
        })
        .await;

        let _ = app.emit(
            "zero://event",
            PipelineEvent::Response { text: response },
        );

        self.emit_state(app, "idle", "ready");
        Ok(())
    }

    /// Background loop. Keeps listening, runs run_once repeatedly.
    pub async fn run_loop(self: Arc<Self>, app: AppHandle) {
        self.running.store(true, Ordering::Relaxed);
        self.emit_state(&app, "starting", "Zero is waking up");

        while self.running.load(Ordering::Relaxed) {
            let me = self.clone();
            let app2 = app.clone();
            let res = me.run_once(&app2).await;
            if let Err(e) = res {
                tracing::error!("pipeline iter: {e}");
            }
            // small breather
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        self.emit_state(&app, "stopped", "Zero stopped");
    }
}
