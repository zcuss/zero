use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, Stream, StreamConfig};
use hound::WavWriter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const SAMPLE_RATE: u32 = 16000;
pub const CHANNELS: u16 = 1;

#[allow(dead_code)]
pub struct AudioDevices {
    pub input: Option<cpal::Device>,
    pub output: Option<cpal::Device>,
}

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|d| d.filter_map(|dev| dev.name().ok()).collect())
        .unwrap_or_default()
}

pub fn list_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|d| d.filter_map(|dev| dev.name().ok()).collect())
        .unwrap_or_default()
}

pub fn get_input_device(name: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if name == "default" || name.is_empty() {
        host.default_input_device()
            .ok_or_else(|| anyhow!("No input device"))
    } else {
        host.input_devices()?
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            .ok_or_else(|| anyhow!("Input device not found: {}", name))
    }
}

#[allow(dead_code)]
pub fn get_output_device(name: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if name == "default" || name.is_empty() {
        host.default_output_device()
            .ok_or_else(|| anyhow!("No output device"))
    } else {
        host.output_devices()?
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            .ok_or_else(|| anyhow!("Output device not found: {}", name))
    }
}

/// Captures audio in a background stream, accumulating into a buffer.
/// Returns a stop handle and a receiver for the captured samples.
pub fn start_capture(
    device: &cpal::Device,
    recording: Arc<AtomicBool>,
) -> Result<(Stream, Arc<Mutex<Vec<i16>>>)> {
    let config = StreamConfig {
        channels: CHANNELS,
        sample_rate: SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    let supported = device.supported_input_configs()?;
    let _ = supported; // sanity

    let buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_clone = buffer.clone();
    let rec_clone = recording.clone();

    let stream = device.build_input_stream(
        &config,
        move |data: &[i16], _info| {
            if rec_clone.load(Ordering::Relaxed) {
                buf_clone.lock().unwrap().extend_from_slice(data);
            }
        },
        |err| {
            tracing::error!("audio capture error: {err}");
        },
        None,
    )?;

    stream.play()?;
    Ok((stream, buffer))
}

/// Captures audio until silence is detected or recording flag is cleared.
/// Returns the captured samples as a Vec<i16> at 16kHz mono.
pub fn record_until_silence(
    device: &cpal::Device,
    recording: Arc<AtomicBool>,
    silence_threshold: i16,
    silence_ms: u64,
    max_ms: u64,
) -> Result<Vec<i16>> {
    let (stream, buffer) = start_capture(device, recording.clone())?;
    recording.store(true, Ordering::Relaxed);

    let silence_samples = (silence_ms * SAMPLE_RATE as u64) / 1000;
    let max_samples = (max_ms * SAMPLE_RATE as u64) / 1000;
    let start = std::time::Instant::now();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));

        let buf = buffer.lock().unwrap();
        if buf.len() as u64 >= max_samples {
            break;
        }
        // Check trailing silence
        let trailing = buf.iter().rev().take(8000); // ~0.5s
        let mut s = 0u64;
        let mut n = 0u64;
        for &smp in trailing {
            s += (smp.abs() as u64).max(silence_threshold as u64);
            n += 1;
            if smp.abs() > silence_threshold {
                break;
            }
        }
        let _ = s;
        let _ = n;

        // simpler approach: count from end how many consecutive samples are below threshold
        let mut count_silent = 0u64;
        for &smp in buf.iter().rev() {
            if smp.abs() < silence_threshold {
                count_silent += 1;
            } else {
                break;
            }
        }

        if buf.len() as u64 > silence_samples && count_silent >= silence_samples {
            break;
        }

        if start.elapsed().as_millis() as u64 > max_ms {
            break;
        }
    }

    recording.store(false, Ordering::Relaxed);
    drop(stream);

    let mut buf = buffer.lock().unwrap().clone();
    // Trim trailing silence
    while buf
        .last()
        .map(|s| s.abs() < silence_threshold)
        .unwrap_or(false)
    {
        buf.pop();
    }
    Ok(buf)
}

/// Write samples to a WAV file (16kHz mono PCM16).
pub fn write_wav(samples: &[i16], path: &std::path::Path) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec)?;
    for &s in samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Play audio bytes (mp3/wav/ogg) via rodio.
pub fn play_bytes(data: Vec<u8>, output_device: Option<&str>) -> Result<()> {
    use rodio::{Decoder, OutputStream, Sink};
    use std::io::Cursor;

    let (_stream, handle) = if output_device == Some("default") || output_device.is_none() {
        OutputStream::try_default()?
    } else {
        // rodio doesn't have direct device selection, default for now
        OutputStream::try_default()?
    };

    let cursor = Cursor::new(data);
    let source = Decoder::new(cursor)?;
    let sink = Sink::try_new(&handle)?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

#[allow(dead_code)]
pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64).powi(2)).sum();
    ((sum / samples.len() as f64).sqrt()) as f32
}

#[allow(dead_code)]
pub fn fmt_from_device(d: &cpal::Device) -> Option<SampleFormat> {
    d.default_input_config().ok().map(|c| c.sample_format())
}
