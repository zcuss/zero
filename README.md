# Zero — voice AI assistant (Tauri 2.0)

Lightweight desktop (and Android) voice assistant that listens for a wake word,
transcribes speech, sends the query to your Hermes agent, and replies with TTS.
Single binary, no Electron, ~14 MB on Linux x86_64.

## What it does

```
   mic ──▶ VAD/capture ──▶ STT (Whisper) ──▶ Hermes (chat) ──▶ TTS (ElevenLabs/OpenAI) ──▶ speakers
              ▲                                                       │
              └────────── wake word "zero" triggers capture ─────────┘
```

- **Wake word**: always-on mic listener; says "zero, ..." to trigger
- **STT**: Hermes `/api/audio/transcribe` (default) or OpenAI Whisper
- **Chat**: Hermes `/v1/chat/completions` (preferred) → falls back to dashboard `/api/sessions/chat-prompt-proxy` + session polling
- **TTS**: Hermes `/api/audio/speak` (default), OpenAI TTS, or ElevenLabs
- **Config**: stored in `~/.config/zero/config.json` (Linux) / platform equivalent
- **Background**: Tauri tray icon, autostart plugin, `tauri-plugin-process`
- **Mobile**: Tauri 2.0 mobile target — same Rust core, Android (aarch64) APK

## Project layout

```
zero/
├── src/                  # frontend (vanilla HTML/CSS/JS, no framework)
│   ├── index.html
│   ├── style.css
│   └── main.js
├── src-tauri/            # Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── icons/
│   └── src/
│       ├── main.rs
│       ├── lib.rs        # tauri::Builder + commands
│       ├── config.rs     # load/save JSON config
│       ├── audio.rs      # cpal capture, rodio playback
│       ├── wakeword.rs   # transcript keyword match
│       ├── stt.rs        # Whisper + Hermes
│       ├── tts.rs        # OpenAI + ElevenLabs + Hermes
│       ├── hermes.rs     # chat-v1 + proxy fallback
│       └── pipeline.rs   # listen loop orchestration
└── package.json
```

## Build (Linux desktop)

System deps (Ubuntu 22.04+):
```bash
apt install -y build-essential pkg-config libssl-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libwebkit2gtk-4.1-dev \
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libasound2-dev libpulse-dev cmake
```

Toolchain:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
nvm install --lts  # or any node 18+
```

Build:
```bash
cd zero
npm install
npm run tauri build -- --no-bundle     # quick: just the ELF
# or
npm run tauri build                     # full installer (deb/rpm/AppImage)
```

Output: `src-tauri/target/release/zero` (~14 MB).

Run:
```bash
./src-tauri/target/release/zero
```

On a headless server, use `xvfb-run -a ./src-tauri/target/release/zero` to smoke-test the GUI
(it will fail to find audio devices, but should not crash on launch).

## Build (Android)

Prereqs:
- Java 17 (`apt install -y openjdk-17-jdk`)
- Android command-line tools + SDK + NDK (see `scripts/setup-android.sh` below)
- `ANDROID_HOME` and `ANDROID_NDK_HOME` exported

Initialize the Android project (one time):
```bash
npm run tauri android init
```

Build APK:
```bash
npm run tauri android build -- --apk
# Output: src-tauri/gen/android/app/build/outputs/apk/release/app-release.apk
```

Install on a connected device or emulator:
```bash
adb install -r src-tauri/gen/android/app/build/outputs/apk/release/app-release.apk
adb shell am start -n io.zero.app/.MainActivity
```

## Configure

Open the in-app **Settings** panel, or edit the JSON file directly:

```jsonc
{
  "hermes_url": "http://127.0.0.1:9119",     // Hermes dashboard (audio + chat-prompt-proxy)
  "hermes_api_key": "...",                   // optional bearer token
  "openai_api_key": "sk-...",                // for Whisper STT or OpenAI TTS
  "elevenlabs_api_key": "...",               // for ElevenLabs TTS
  "elevenlabs_voice_id": "21m00Tcm4TlvDq8ikWAM",
  "stt_provider": "hermes",                  // "hermes" | "openai"
  "tts_provider": "hermes",                  // "hermes" | "openai" | "elevenlabs"
  "character": "assistant",                  // voice id
  "wake_word": "zero",
  "wake_word_provider": "energy",            // v1: energy VAD + keyword
  "autostart": true,
  "microphone_device": "default",
  "output_device": "default"
}
```

Hermes endpoints used:
- `POST /api/audio/transcribe` — `{ audio: <base64 wav>, mime_type: "audio/wav" }` → `{ text }`
- `POST /api/audio/speak` — `{ text, voice, character }` → `{ audio: <base64> }`
- `POST /v1/chat/completions` — OpenAI-compatible chat (preferred, sync)
- `POST /api/sessions/{id}/chat-prompt-proxy` + session polling (fallback if api-server isn't running)
- `GET /api/audio/elevenlabs/voices` — list voices (in TTS dropdown)

If port 9120 is taken (e.g. by another service), Hermes' dashboard on 9119 will fall back to
the `chat-prompt-proxy` endpoint automatically.

## Run flow

1. Launch the app — system tray icon appears, main window visible
2. Click **▶ Listen** (or set `autostart: true`) to start background listening
3. Say **"zero, [your command]"** — the orb pulses, captures until silence, then:
   - STT → text
   - Hermes → reply text
   - TTS → audio
   - Audio plays through selected output device
4. Type commands in the bottom input field if mic isn't available

## Performance

- Tauri 2.0 + system webview → ~14 MB binary, ~30 MB RAM idle
- No JS framework — vanilla DOM, instant render
- Rust audio pipeline via cpal (CoreAudio/ALSA/PulseAudio/WASAPI)
- rodio for output
- Latency: ~1.5s end-to-end with Hermes + ElevenLabs Turbo v2

## Roadmap

- [ ] Real wake-word engine (Porcupine / openWakeWord sidecar)
- [ ] Local STT (whisper.cpp sidecar) for offline use
- [ ] Local TTS (Piper HTTP sidecar) for offline use
- [ ] SSH command execution to send commands to `~/workspace/`
- [ ] iOS target
- [ ] Settings migration on schema change
- [ ] i18n: Indonesian / English UI
