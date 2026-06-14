use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HermesConfig {
    pub base_url: String,    // chat api, e.g. http://hermes:9119
    pub api_key: String,
    pub session_id: String,
    pub model: String,
    #[serde(default)]
    pub ssh_user: String,    // optional SSH fallback
    #[serde(default)]
    pub ssh_host: String,
    #[serde(default)]
    pub ssh_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Send a chat prompt to Hermes and return the assistant reply.
/// Tries the api-server (`/v1/chat/completions`) first. If it 404s (api-server not running),
/// falls back to the dashboard `chat-prompt-proxy` + session polling.
pub async fn ask(prompt: &str, history: &[ChatMessage], cfg: &HermesConfig) -> Result<String> {
    if let Ok(r) = ask_v1(prompt, history, cfg).await {
        return Ok(r);
    }
    ask_proxy_with_poll(prompt, history, cfg).await
}

async fn ask_v1(prompt: &str, history: &[ChatMessage], cfg: &HermesConfig) -> Result<String> {
    let url = cfg.base_url.trim_end_matches('/').to_string() + "/v1/chat/completions";
    let mut messages: Vec<serde_json::Value> = history
        .iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();
    messages.push(json!({ "role": "user", "content": prompt }));
    let payload = json!({
        "model": cfg.model,
        "messages": messages,
        "stream": false,
    });
    let mut req = Client::new()
        .post(&url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(60));
    if !cfg.api_key.is_empty() {
        req = req.bearer_auth(&cfg.api_key);
    }
    let res = req.send().await.map_err(|e| anyhow!("hermes v1 http: {e}"))?;
    if !res.status().is_success() {
        let s = res.status();
        return Err(anyhow!("hermes v1 {s}"));
    }
    #[derive(Deserialize)]
    struct Choice {
        message: ChatMessage,
    }
    #[derive(Deserialize)]
    struct R {
        choices: Vec<Choice>,
    }
    let r: R = res
        .json()
        .await
        .map_err(|e| anyhow!("hermes v1 json: {e}"))?;
    r.choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| anyhow!("hermes v1: empty response"))
}

/// Fallback: hit the dashboard `chat-prompt-proxy` (9119) and poll the session for the new reply.
async fn ask_proxy_with_poll(
    prompt: &str,
    history: &[ChatMessage],
    cfg: &HermesConfig,
) -> Result<String> {
    let base = cfg.base_url.trim_end_matches('/').to_string();
    // 1) snapshot current message count
    let pre = count_session_messages(&base, cfg).await.unwrap_or(0);
    // 2) post prompt
    let mut req = Client::new()
        .post(format!("{}/api/sessions/chat-prompt-proxy", base))
        .json(&json!({ "id": cfg.session_id, "prompt": prompt, "history": history }))
        .timeout(std::time::Duration::from_secs(15));
    if !cfg.api_key.is_empty() {
        req = req.bearer_auth(&cfg.api_key);
    }
    let res = req.send().await.map_err(|e| anyhow!("hermes proxy: {e}"))?;
    if !res.status().is_success() {
        let s = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("hermes proxy {s}: {body}"));
    }
    // 3) poll session for new assistant message
    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if let Some(reply) = fetch_last_assistant(&base, cfg, pre).await? {
            return Ok(reply);
        }
    }
    Err(anyhow!("hermes proxy: timeout waiting for reply"))
}

async fn count_session_messages(base: &str, cfg: &HermesConfig) -> Result<usize> {
    let mut req = Client::new()
        .get(format!("{}/api/sessions/{}", base, cfg.session_id))
        .timeout(std::time::Duration::from_secs(10));
    if !cfg.api_key.is_empty() {
        req = req.bearer_auth(&cfg.api_key);
    }
    let res = req.send().await.map_err(|e| anyhow!("session: {e}"))?;
    if !res.status().is_success() {
        return Ok(0);
    }
    let v: serde_json::Value = res.json().await.map_err(|e| anyhow!("session json: {e}"))?;
    Ok(v.get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.len())
        .unwrap_or(0))
}

async fn fetch_last_assistant(base: &str, cfg: &HermesConfig, pre: usize) -> Result<Option<String>> {
    let mut req = Client::new()
        .get(format!("{}/api/sessions/{}", base, cfg.session_id))
        .timeout(std::time::Duration::from_secs(10));
    if !cfg.api_key.is_empty() {
        req = req.bearer_auth(&cfg.api_key);
    }
    let res = req.send().await.map_err(|e| anyhow!("session: {e}"))?;
    if !res.status().is_success() {
        return Ok(None);
    }
    let v: serde_json::Value = res.json().await.map_err(|e| anyhow!("session json: {e}"))?;
    let arr = v.get("messages").and_then(|m| m.as_array());
    if let Some(a) = arr {
        if a.len() > pre {
            for m in a.iter().rev() {
                let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
                if role == "assistant" {
                    if let Some(content) = m.get("content").and_then(|c| c.as_str()) {
                        return Ok(Some(content.to_string()));
                    }
                }
            }
        }
    }
    Ok(None)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    pub command: String,
    pub cwd: Option<String>,
}

pub async fn run_command(req: CommandRequest, cfg: &HermesConfig) -> Result<String> {
    let base = cfg.base_url.trim_end_matches('/').to_string();
    let mut r = Client::new()
        .post(format!("{}/api/workspace/run", base))
        .json(&req)
        .timeout(std::time::Duration::from_secs(120));
    if !cfg.api_key.is_empty() {
        r = r.bearer_auth(&cfg.api_key);
    }
    let res = r.send().await.map_err(|e| anyhow!("run cmd http: {e}"))?;
    if !res.status().is_success() {
        let s = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("run cmd {s}: {body}"));
    }
    let v: serde_json::Value = res.json().await.map_err(|e| anyhow!("run cmd json: {e}"))?;
    Ok(v.get("output")
        .and_then(|o| o.as_str())
        .unwrap_or("")
        .to_string())
}
