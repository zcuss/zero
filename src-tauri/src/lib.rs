use crate::config::AppConfig;
use crate::pipeline::Pipeline;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

mod audio;
mod config;
mod hermes;
mod pipeline;
mod stt;
mod tts;
mod wakeword;

pub struct AppState {
    pub pipeline: Arc<Pipeline>,
}

#[derive(Serialize)]
pub struct DeviceList {
    inputs: Vec<String>,
    outputs: Vec<String>,
}

#[tauri::command]
fn list_audio_devices() -> DeviceList {
    DeviceList {
        inputs: audio::list_input_devices(),
        outputs: audio::list_output_devices(),
    }
}

#[tauri::command]
fn get_config(app: AppHandle) -> AppConfig {
    crate::config::load_config(&app)
}

#[tauri::command]
fn set_config(app: AppHandle, cfg: AppConfig) -> Result<(), String> {
    crate::config::save_config(&app, &cfg).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn start_pipeline(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    if st.pipeline.is_running() {
        return Err("already running".to_string());
    }
    let cfg = st.pipeline.cfg.clone();
    *st.pipeline.history.lock().unwrap() = Vec::new();
    // Replace pipeline in state with a new one
    let new_p = Arc::new(Pipeline::new(cfg));
    // Spawn run loop on a fresh pipeline instance
    let app2 = app.clone();
    tokio::spawn(async move {
        new_p.run_loop(app2).await;
    });
    Ok(())
}

#[tauri::command]
fn stop_pipeline(app: AppHandle) {
    let st = app.state::<AppState>();
    st.pipeline.stop();
}

#[tauri::command]
async fn ask_text(app: AppHandle, text: String) -> Result<String, String> {
    let st = app.state::<AppState>();
    let cfg = st.pipeline.cfg.clone();
    let hermes_cfg = hermes::HermesConfig {
        base_url: cfg.hermes_url.replace(":9119", ":9120"),
        api_key: cfg.hermes_api_key.clone(),
        ..Default::default()
    };
    let history = st.pipeline.history.lock().unwrap().clone();
    let response = hermes::ask(&text, &history, &hermes_cfg)
        .await
        .map_err(|e| e.to_string())?;
    st.pipeline.history.lock().unwrap().push(hermes::ChatMessage {
        role: "user".to_string(),
        content: text,
    });
    st.pipeline.history.lock().unwrap().push(hermes::ChatMessage {
        role: "assistant".to_string(),
        content: response.clone(),
    });
    let _ = tauri::Emitter::emit(
        &app,
        "zero://event",
        crate::pipeline::PipelineEvent::Response { text: response.clone() },
    );
    Ok(response)
}

#[tauri::command]
async fn speak_text(app: AppHandle, text: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    let cfg = st.pipeline.cfg.clone();
    let tts_cfg = tts::TtsConfig {
        provider: cfg.tts_provider,
        hermes_url: cfg.hermes_url,
        hermes_api_key: cfg.hermes_api_key,
        openai_api_key: cfg.openai_api_key,
        openai_voice: cfg.character.clone(),
        elevenlabs_api_key: cfg.elevenlabs_api_key,
        elevenlabs_voice_id: cfg.elevenlabs_voice_id,
        character: cfg.character,
    };
    let result = tts::synthesize(&text, &tts_cfg)
        .await
        .map_err(|e| e.to_string())?;
    let output = cfg.output_device.clone();
    tokio::task::spawn_blocking(move || {
        let _ = audio::play_bytes(result.audio_bytes, Some(&output));
    });
    Ok(())
}

#[tauri::command]
async fn transcribe_audio(app: AppHandle, samples: Vec<i16>) -> Result<String, String> {
    let st = app.state::<AppState>();
    let cfg = st.pipeline.cfg.clone();
    let stt_cfg = stt::SttConfig {
        provider: cfg.stt_provider,
        hermes_url: cfg.hermes_url,
        hermes_api_key: cfg.hermes_api_key,
        openai_api_key: cfg.openai_api_key,
    };
    let r = stt::transcribe(&samples, &stt_cfg)
        .await
        .map_err(|e| e.to_string())?;
    Ok(r.text)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,zero=debug")),
        )
        .with_target(false)
        .compact()
        .init();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_http::init());
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder.plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ));
    }
    builder
        .setup(|app| {
            let handle = app.handle().clone();
            let cfg = crate::config::load_config(&handle);
            let pipeline = Arc::new(Pipeline::new(cfg));
            app.manage(AppState { pipeline });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_audio_devices,
            get_config,
            set_config,
            start_pipeline,
            stop_pipeline,
            ask_text,
            speak_text,
            transcribe_audio,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
