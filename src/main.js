const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

let cfg = null;
let listening = false;

function setState(state, message) {
  const dot = $("statusDot");
  const lbl = $("stateLabel");
  const orb = $("orb");
  const valid = ["idle", "listening", "thinking", "speaking", "error", "starting", "stopped", "transcribing"];
  const s = valid.includes(state) ? state : "idle";
  dot.className = "dot " + s;
  lbl.textContent = s;
  orb.className = "orb " + s;
  if (message) {
    orb.querySelector(".label").textContent = message.length > 18 ? message.slice(0, 18) : message;
  } else if (s === "idle") {
    orb.querySelector(".label").textContent = 'say "zero"';
  } else if (s === "listening") {
    orb.querySelector(".label").textContent = "listening";
  } else if (s === "thinking") {
    orb.querySelector(".label").textContent = "thinking";
  } else if (s === "speaking") {
    orb.querySelector(".label").textContent = "speaking";
  } else if (s === "error") {
    orb.querySelector(".label").textContent = "error";
  }
}

function addBubble(role, text) {
  const c = $("conversation");
  if (c.querySelector(".empty")) c.innerHTML = "";
  const div = document.createElement("div");
  div.className = "bubble " + role;
  div.textContent = text;
  c.appendChild(div);
  c.scrollTop = c.scrollHeight;
}

function clearConvo() {
  $("conversation").innerHTML = '<div class="empty">Start a conversation. Say "<b>zero</b>" then your command, or type below.</div>';
}

async function loadConfig() {
  cfg = await invoke("get_config");
  populateConfigUI();
}

function populateConfigUI() {
  if (!cfg) return;
  // Single base URL for both chat + audio — Hermes exposes both on :9119
  const baseUrl = (cfg.hermes_url || "http://127.0.0.1:9119").replace(/\/$/, "");
  $("cfg_hermes_url").value = baseUrl;
  $("cfg_hermes_audio_url").value = baseUrl;
  $("cfg_hermes_api_key").value = cfg.hermes_api_key || "";
  $("cfg_session_id").value = "zero-voice";
  $("cfg_model").value = "hermes";
  $("cfg_stt_provider").value = cfg.stt_provider || "hermes";
  $("cfg_tts_provider").value = cfg.tts_provider || "hermes";
  $("cfg_character").value = cfg.character || "assistant";
  $("cfg_elevenlabs_voice_id").value = cfg.elevenlabs_voice_id || "";
  $("cfg_openai_api_key").value = cfg.openai_api_key || "";
  $("cfg_elevenlabs_api_key").value = cfg.elevenlabs_api_key || "";
  $("cfg_wake_word").value = cfg.wake_word || "zero";
  $("cfg_autostart").checked = !!cfg.autostart;
  $("cfg_microphone_device").value = cfg.microphone_device || "default";
  $("cfg_output_device").value = cfg.output_device || "default";
}

async function saveConfig() {
  const audioUrl = $("cfg_hermes_audio_url").value.trim() || "http://127.0.0.1:9119";
  const next = {
    hermes_url: audioUrl,
    hermes_api_key: $("cfg_hermes_api_key").value,
    openai_api_key: $("cfg_openai_api_key").value,
    elevenlabs_api_key: $("cfg_elevenlabs_api_key").value,
    elevenlabs_voice_id: $("cfg_elevenlabs_voice_id").value,
    wake_word: $("cfg_wake_word").value || "zero",
    character: $("cfg_character").value,
    tts_provider: $("cfg_tts_provider").value,
    stt_provider: $("cfg_stt_provider").value,
    wake_word_provider: "energy",
    porcupine_access_key: "",
    autostart: $("cfg_autostart").checked,
    start_minimized: false,
    microphone_device: $("cfg_microphone_device").value,
    output_device: $("cfg_output_device").value,
  };
  await invoke("set_config", { cfg: next });
  cfg = next;
  const s = $("saveStatus");
  s.textContent = "Saved";
  s.classList.add("show");
  setTimeout(() => s.classList.remove("show"), 1500);
}

async function refreshDevices() {
  try {
    const dev = await invoke("list_audio_devices");
    const inSel = $("cfg_microphone_device");
    const outSel = $("cfg_output_device");
    inSel.innerHTML = '<option value="default">System default</option>';
    outSel.innerHTML = '<option value="default">System default</option>';
    for (const d of dev.inputs || []) {
      const o = document.createElement("option");
      o.value = d; o.textContent = d;
      inSel.appendChild(o);
    }
    for (const d of dev.outputs || []) {
      const o = document.createElement("option");
      o.value = d; o.textContent = d;
      outSel.appendChild(o);
    }
  } catch (e) {
    console.error("refreshDevices", e);
  }
}

async function toggleListen() {
  if (listening) {
    try {
      await invoke("stop_pipeline");
    } catch (e) { console.error(e); }
    listening = false;
    $("toggleListen").textContent = "▶ Listen";
    $("toggleListen").classList.remove("active");
    setState("idle");
  } else {
    try {
      // save config first so pipeline picks up latest
      await saveConfig();
      await invoke("start_pipeline");
      listening = true;
      $("toggleListen").textContent = "■ Stop";
      $("toggleListen").classList.add("active");
    } catch (e) {
      console.error(e);
      setState("error", String(e));
    }
  }
}

async function sendText(e) {
  e.preventDefault();
  const input = $("textInput");
  const text = input.value.trim();
  if (!text) return;
  addBubble("user", text);
  input.value = "";
  setState("thinking");
  try {
    const reply = await invoke("ask_text", { text });
    addBubble("assistant", reply);
    setState("idle");
  } catch (err) {
    addBubble("system", "Error: " + err);
    setState("error", String(err));
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  $("toggleListen").addEventListener("click", toggleListen);
  $("openSettings").addEventListener("click", () => {
    $("settings").classList.toggle("hidden");
    refreshDevices();
  });
  $("closeSettings").addEventListener("click", () => {
    $("settings").classList.add("hidden");
  });
  $("saveSettings").addEventListener("click", saveConfig);
  $("refreshMics").addEventListener("click", refreshDevices);
  $("refreshOutputs").addEventListener("click", refreshDevices);
  $("textForm").addEventListener("submit", sendText);

  await loadConfig();
  await refreshDevices();
  setState("idle");

  // listen to backend events
  await listen("zero://event", (e) => {
    const p = e.payload || {};
    if (p.type === "State") {
      setState(p.state, p.message);
    } else if (p.type === "Transcript") {
      addBubble(p.role, p.text);
    } else if (p.type === "Response") {
      // also added via Transcript assistant
    } else if (p.type === "Error") {
      addBubble("system", "Error: " + p.error);
    }
  });
});
