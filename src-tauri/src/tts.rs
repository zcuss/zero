use anyhow::{anyhow, Result};
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsResult {
    pub audio_bytes: Vec<u8>,
    pub mime_type: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    pub provider: String, // "hermes" | "openai" | "elevenlabs" | "edge"
    pub hermes_url: String,
    pub hermes_api_key: String,
    pub openai_api_key: String,
    pub openai_voice: String,
    pub elevenlabs_api_key: String,
    pub elevenlabs_voice_id: String,
    pub character: String,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            provider: "hermes".to_string(),
            hermes_url: "http://127.0.0.1:9119".to_string(),
            hermes_api_key: String::new(),
            openai_api_key: String::new(),
            openai_voice: "alloy".to_string(),
            elevenlabs_api_key: String::new(),
            elevenlabs_voice_id: "21m00Tcm4TlvDq8ikWAM".to_string(),
            character: "assistant".to_string(),
        }
    }
}

pub async fn synthesize(text: &str, cfg: &TtsConfig) -> Result<TtsResult> {
    let text = text.trim();
    if text.is_empty() {
        return Err(anyhow!("Empty text for TTS"));
    }
    match cfg.provider.as_str() {
        "openai" => synthesize_openai(text, cfg).await,
        "elevenlabs" => synthesize_elevenlabs(text, cfg).await,
        "hermes" | _ => synthesize_hermes(text, cfg).await,
    }
}

#[derive(Serialize)]
struct HermesSpeakBody<'a> {
    text: &'a str,
}

#[derive(Deserialize)]
struct HermesSpeakResp {
    ok: bool,
    data_url: String,
    mime_type: String,
    provider: Option<String>,
}

async fn synthesize_hermes(text: &str, cfg: &TtsConfig) -> Result<TtsResult> {
    let client = Client::new();
    let body = HermesSpeakBody { text };
    let mut req = client
        .post(format!("{}/api/audio/speak", cfg.hermes_url.trim_end_matches('/')))
        .json(&body);
    if !cfg.hermes_api_key.is_empty() {
        req = req.bearer_auth(&cfg.hermes_api_key);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let s = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Hermes TTS failed ({}): {}", s, body));
    }
    let r: HermesSpeakResp = resp.json().await?;
    if !r.ok {
        return Err(anyhow!("Hermes TTS not-ok"));
    }
    // Parse data URL
    let data_url = r.data_url;
    let comma = data_url
        .find(',')
        .ok_or_else(|| anyhow!("Bad data URL"))?;
    let b64 = &data_url[comma + 1..];
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
    Ok(TtsResult {
        audio_bytes: bytes,
        mime_type: r.mime_type,
        provider: r.provider.unwrap_or_else(|| "hermes".to_string()),
    })
}

#[derive(Serialize)]
struct OpenAiTtsBody<'a> {
    model: &'a str,
    input: &'a str,
    voice: &'a str,
    response_format: &'a str,
}

async fn synthesize_openai(text: &str, cfg: &TtsConfig) -> Result<TtsResult> {
    if cfg.openai_api_key.is_empty() {
        return Err(anyhow!("OpenAI API key not set"));
    }
    let client = Client::new();
    let body = OpenAiTtsBody {
        model: "tts-1",
        input: text,
        voice: &cfg.openai_voice,
        response_format: "mp3",
    };
    let resp = client
        .post("https://api.openai.com/v1/audio/speech")
        .bearer_auth(&cfg.openai_api_key)
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        let s = resp.status();
        let b = resp.text().await.unwrap_or_default();
        return Err(anyhow!("OpenAI TTS failed ({}): {}", s, b));
    }
    let bytes = resp.bytes().await?.to_vec();
    Ok(TtsResult {
        audio_bytes: bytes,
        mime_type: "audio/mpeg".to_string(),
        provider: "openai".to_string(),
    })
}

async fn synthesize_elevenlabs(text: &str, cfg: &TtsConfig) -> Result<TtsResult> {
    if cfg.elevenlabs_api_key.is_empty() {
        return Err(anyhow!("ElevenLabs API key not set"));
    }
    let client = Client::new();
    let url = format!(
        "https://api.elevenlabs.io/v1/text-to-speech/{}",
        cfg.elevenlabs_voice_id
    );
    let resp = client
        .post(&url)
        .header("xi-api-key", &cfg.elevenlabs_api_key)
        .header("Accept", "audio/mpeg")
        .json(&serde_json::json!({
            "text": text,
            "model_id": "eleven_turbo_v2_5",
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75
            }
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        let s = resp.status();
        let b = resp.text().await.unwrap_or_default();
        return Err(anyhow!("ElevenLabs TTS failed ({}): {}", s, b));
    }
    let bytes = resp.bytes().await?.to_vec();
    Ok(TtsResult {
        audio_bytes: bytes,
        mime_type: "audio/mpeg".to_string(),
        provider: "elevenlabs".to_string(),
    })
}
