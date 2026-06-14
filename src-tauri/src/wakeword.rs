use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Match a wake word in a transcript. Returns the cleaned command (wake word stripped) if found.
pub fn match_wake_word(transcript: &str, wake_word: &str) -> Option<String> {
    let t = transcript.trim().to_lowercase();
    let w = wake_word.trim().to_lowercase();
    if w.is_empty() {
        return Some(t);
    }
    // Find wake word anywhere in transcript
    if let Some(idx) = t.find(&w) {
        let after = &t[idx + w.len()..];
        let cleaned = after.trim_start_matches(|c: char| c == ',' || c == '.' || c == '!' || c == '?' || c == ' ' || c == '\t');
        return Some(cleaned.to_string());
    }
    None
}

/// Energy-based VAD: returns true if audio has speech above threshold.
pub fn has_speech(samples: &[i16], threshold: i16) -> bool {
    if samples.is_empty() {
        return false;
    }
    let max = samples.iter().map(|s| s.abs()).max().unwrap_or(0);
    max > threshold
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeWordConfig {
    pub wake_word: String,
    pub energy_threshold: i16,
    pub silence_threshold: i16,
    pub silence_duration_ms: u64,
    pub max_record_ms: u64,
    pub provider: String, // "energy" | "porcupine"
    pub porcupine_access_key: String,
}

impl Default for WakeWordConfig {
    fn default() -> Self {
        Self {
            wake_word: "zero".to_string(),
            energy_threshold: 800,
            silence_threshold: 500,
            silence_duration_ms: 800,
            max_record_ms: 15_000,
            provider: "energy".to_string(),
            porcupine_access_key: String::new(),
        }
    }
}

/// Process a captured audio buffer through the wake word pipeline.
/// Returns Some(command) if wake word detected, None otherwise.
pub fn detect_and_extract(samples: &[i16], transcript: &str, cfg: &WakeWordConfig) -> Option<String> {
    if !has_speech(samples, cfg.energy_threshold) {
        return None;
    }
    match_wake_word(transcript, &cfg.wake_word)
}

#[allow(dead_code)]
pub fn audio_energy_db(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return -100.0;
    }
    let rms = (samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
    if rms <= 0.0 {
        -100.0
    } else {
        (20.0 * rms.log10()) as f32
    }
}

#[allow(dead_code)]
pub fn _suppress_unused() -> Result<()> {
    Ok(())
}
