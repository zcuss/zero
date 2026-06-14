use anyhow::{anyhow, Result};
use base64::Engine;
use hound::{SampleFormat, WavSpec, WavWriter};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttResult {
    pub text: String,
    pub language: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    pub provider: String, // "hermes" | "openai"
    pub hermes_url: String,
    pub hermes_api_key: String,
    pub openai_api_key: String,
}

/// Transcribe i16 PCM mono samples (any sample rate, will be re-written at 16kHz WAV) to text.
pub async fn transcribe(samples: &[i16], cfg: &SttConfig) -> Result<SttResult> {
    let wav = pcm_to_wav(samples, 16_000)?;
    match cfg.provider.as_str() {
        "openai" => transcribe_openai(&wav, &cfg.openai_api_key).await,
        _ => transcribe_hermes(&wav, cfg).await,
    }
}

fn pcm_to_wav(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut cursor = Cursor::new(&mut buf);
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::new(&mut cursor, spec)
            .map_err(|e| anyhow!("wav writer init failed: {e}"))?;
        for s in samples {
            writer
                .write_sample(*s)
                .map_err(|e| anyhow!("wav write failed: {e}"))?;
        }
        writer
            .finalize()
            .map_err(|e| anyhow!("wav finalize failed: {e}"))?;
    }
    Ok(buf)
}

async fn transcribe_openai(wav: &[u8], api_key: &str) -> Result<SttResult> {
    if api_key.is_empty() {
        return Err(anyhow!("OpenAI API key not set"));
    }
    let client = Client::new();
    let part = reqwest::multipart::Part::bytes(wav.to_vec())
        .file_name("audio.wav")
        .mime_str("audio/wav")?;
    let form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .text("language", "id")
        .part("file", part);
    let res = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .bearer_auth(api_key)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| anyhow!("openai stt http: {e}"))?;
    if !res.status().is_success() {
        let s = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("openai stt {s}: {body}"));
    }
    #[derive(Deserialize)]
    struct R {
        text: String,
    }
    let r: R = res
        .json()
        .await
        .map_err(|e| anyhow!("openai stt json: {e}"))?;
    Ok(SttResult {
        text: r.text,
        language: Some("id".to_string()),
        duration_ms: None,
    })
}

async fn transcribe_hermes(wav: &[u8], cfg: &SttConfig) -> Result<SttResult> {
    let url = cfg.hermes_url.trim_end_matches('/').to_string() + "/api/audio/transcribe";
    let b64 = base64::engine::general_purpose::STANDARD.encode(wav);
    let payload = serde_json::json!({
        "audio": b64,
        "mime_type": "audio/wav",
        "filename": "zero-capture.wav",
    });
    let mut req = Client::new()
        .post(&url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(60));
    if !cfg.hermes_api_key.is_empty() {
        req = req.bearer_auth(&cfg.hermes_api_key);
    }
    let res = req
        .send()
        .await
        .map_err(|e| anyhow!("hermes stt http: {e}"))?;
    if !res.status().is_success() {
        let s = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("hermes stt {s}: {body}"));
    }
    #[derive(Deserialize)]
    struct R {
        text: String,
        #[serde(default)]
        language: Option<String>,
    }
    let r: R = res
        .json()
        .await
        .map_err(|e| anyhow!("hermes stt json: {e}"))?;
    Ok(SttResult {
        text: r.text,
        language: r.language,
        duration_ms: None,
    })
}
