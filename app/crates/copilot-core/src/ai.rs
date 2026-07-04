//! AI provider layer: BYO-key providers (Anthropic, OpenAI, OpenRouter) and
//! local models via Ollama, behind one streaming interface.
//!
//! Keys live in the OS keychain, never in bundles or plaintext config, and
//! leave the machine only toward the provider the user configured. Streaming
//! is mandatory: `stream_chat` delivers tokens through a callback so the UI
//! can render within the first-token budget.

use serde::{Deserialize, Serialize};

pub const KEYCHAIN_SERVICE: &str = "research-paper-copilot";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    OpenRouter,
    Ollama,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 4] = [
        ProviderKind::Anthropic,
        ProviderKind::OpenAi,
        ProviderKind::OpenRouter,
        ProviderKind::Ollama,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAi => "openai",
            ProviderKind::OpenRouter => "openrouter",
            ProviderKind::Ollama => "ollama",
        }
    }

    pub fn needs_key(self) -> bool {
        self != ProviderKind::Ollama
    }

    /// Default model per cost/latency class (user-overridable; task 5.2
    /// routes actions to classes).
    pub fn default_model(self, class: ModelClass) -> &'static str {
        match (self, class) {
            (ProviderKind::Anthropic, ModelClass::Light) => "claude-haiku-4-5-20251001",
            (ProviderKind::Anthropic, _) => "claude-sonnet-5",
            (ProviderKind::OpenAi, ModelClass::Light) => "gpt-4o-mini",
            (ProviderKind::OpenAi, _) => "gpt-4o",
            (ProviderKind::OpenRouter, ModelClass::Light) => "anthropic/claude-haiku-4.5",
            (ProviderKind::OpenRouter, _) => "anthropic/claude-sonnet-5",
            (ProviderKind::Ollama, _) => "llama3.2",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "https://api.anthropic.com",
            ProviderKind::OpenAi => "https://api.openai.com",
            ProviderKind::OpenRouter => "https://openrouter.ai/api",
            ProviderKind::Ollama => "http://localhost:11434",
        }
    }
}

/// Cost/latency class an action routes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelClass {
    /// Hover summaries, citation cards — fast and cheap.
    Light,
    /// Mid-tier (reserved for future routing; presets map it today).
    Balanced,
    /// Explanations, derivations — strongest available.
    Strong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String, // "system" | "user" | "assistant"
    pub content: String,
    /// Inline image attachments (base64) — sent to multimodal providers;
    /// empty for text-only turns, absent in older journals.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageAttachment>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ImageAttachment {
    /// e.g. "image/png", "image/jpeg"
    pub media_type: String,
    /// Raw base64 (no data: prefix).
    pub data_b64: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("no API key stored for {0}")]
    NoKey(String),
    #[error("keychain error: {0}")]
    Keychain(String),
    #[error("provider rejected the request ({status}): {message}")]
    Provider { status: u16, message: String },
    #[error("network problem talking to the provider: {0}")]
    Network(String),
    #[error("the stream ended unexpectedly; partial response preserved")]
    Interrupted,
    #[error("cancelled by the user; partial response preserved")]
    Cancelled,
}

// ---------------------------------------------------------------------------
// Keychain
// ---------------------------------------------------------------------------

/// Store a key under an arbitrary account id (one slot per configured
/// provider — a Z.ai key never mixes with the Anthropic key).
#[cfg(feature = "native")]
pub fn store_key_for(account: &str, key: &str) -> Result<(), AiError> {
    keyring::Entry::new(KEYCHAIN_SERVICE, account)
        .and_then(|e| e.set_password(key))
        .map_err(|e| AiError::Keychain(e.to_string()))
}

#[cfg(feature = "native")]
pub fn load_key_for(account: &str) -> Result<Option<String>, AiError> {
    match keyring::Entry::new(KEYCHAIN_SERVICE, account).and_then(|e| e.get_password()) {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AiError::Keychain(e.to_string())),
    }
}

#[cfg(feature = "native")]
pub fn delete_key_for(account: &str) -> Result<(), AiError> {
    match keyring::Entry::new(KEYCHAIN_SERVICE, account).and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AiError::Keychain(e.to_string())),
    }
}

#[cfg(feature = "native")]
pub fn store_key(kind: ProviderKind, key: &str) -> Result<(), AiError> {
    store_key_for(kind.id(), key)
}

#[cfg(feature = "native")]
pub fn load_key(kind: ProviderKind) -> Result<Option<String>, AiError> {
    load_key_for(kind.id())
}

#[cfg(feature = "native")]
pub fn delete_key(kind: ProviderKind) -> Result<(), AiError> {
    delete_key_for(kind.id())
}

// ---------------------------------------------------------------------------
// Provider client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Provider {
    pub kind: ProviderKind,
    pub model: String,
    pub base_url: String,
    api_key: Option<String>,
    /// Per-provider request timeout (reasoning models can be slow; the
    /// streaming/cancel UX keeps the app responsive regardless).
    timeout: std::time::Duration,
}

impl Provider {
    /// Build a provider from the keychain. `Err(NoKey)` when a key is
    /// required and absent — callers surface the designed no-key mode.
    #[cfg(feature = "native")]
    pub fn from_keychain(kind: ProviderKind, model: Option<String>) -> Result<Self, AiError> {
        let api_key = load_key(kind)?;
        if kind.needs_key() && api_key.is_none() {
            return Err(AiError::NoKey(kind.id().to_string()));
        }
        Ok(Provider {
            kind,
            model: model.unwrap_or_else(|| kind.default_model(ModelClass::Strong).to_string()),
            base_url: kind.default_base_url().to_string(),
            api_key,
            timeout: std::time::Duration::from_secs(300),
        })
    }

    /// For custom Anthropic-compatible endpoints, tests, and Ollama-style
    /// local endpoints.
    pub fn with_base_url(
        kind: ProviderKind,
        model: &str,
        base_url: &str,
        api_key: Option<String>,
    ) -> Self {
        Provider {
            kind,
            model: model.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            timeout: std::time::Duration::from_secs(300),
        }
    }

    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Validate the configuration with a minimal request. Returns a
    /// human-readable success description (e.g. detected models).
    #[cfg(feature = "native")]
    pub fn validate(&self) -> Result<String, AiError> {
        match self.kind {
            ProviderKind::Anthropic
                if self.base_url == ProviderKind::Anthropic.default_base_url() =>
            {
                let response = ureq::get(&format!("{}/v1/models", self.base_url))
                    .set("x-api-key", self.api_key.as_deref().unwrap_or_default())
                    .set("anthropic-version", "2023-06-01")
                    .timeout(std::time::Duration::from_secs(15))
                    .call()
                    .map_err(map_ureq)?;
                let _: serde_json::Value = response
                    .into_json()
                    .map_err(|e| AiError::Network(e.to_string()))?;
                Ok("Anthropic key valid".to_string())
            }
            ProviderKind::Anthropic => {
                // Compatible endpoints (Z.ai etc.) may not implement
                // /v1/models — probe the real surface with a 1-token
                // /v1/messages call. Bearer + x-api-key like streaming.
                let key = self.api_key.as_deref().unwrap_or_default();
                let response = ureq::post(&format!("{}/v1/messages", self.base_url))
                    .set("x-api-key", key)
                    .set("Authorization", &format!("Bearer {key}"))
                    .set("anthropic-version", "2023-06-01")
                    .set("content-type", "application/json")
                    .timeout(std::time::Duration::from_secs(20))
                    .send_json(serde_json::json!({
                        "model": self.model,
                        "max_tokens": 1,
                        "messages": [{"role": "user", "content": "ping"}],
                    }))
                    .map_err(map_ureq)?;
                let _: serde_json::Value = response.into_json().map_err(|e| {
                    AiError::Network(format!(
                        "endpoint did not behave like an Anthropic-compatible API: {e}"
                    ))
                })?;
                Ok(format!(
                    "key valid — Anthropic-compatible endpoint at {}",
                    self.base_url
                ))
            }
            ProviderKind::OpenAi | ProviderKind::OpenRouter => {
                let response = ureq::get(&format!("{}/v1/models", self.base_url))
                    .set(
                        "Authorization",
                        &format!("Bearer {}", self.api_key.as_deref().unwrap_or_default()),
                    )
                    .timeout(std::time::Duration::from_secs(15))
                    .call()
                    .map_err(map_ureq)?;
                let _: serde_json::Value = response
                    .into_json()
                    .map_err(|e| AiError::Network(e.to_string()))?;
                Ok(format!("{} key valid", self.kind.id()))
            }
            ProviderKind::Ollama => {
                let response = ureq::get(&format!("{}/api/tags", self.base_url))
                    .timeout(std::time::Duration::from_secs(5))
                    .call()
                    .map_err(map_ureq)?;
                let tags: serde_json::Value = response
                    .into_json()
                    .map_err(|e| AiError::Network(e.to_string()))?;
                let models: Vec<String> = tags["models"]
                    .as_array()
                    .map(|m| {
                        m.iter()
                            .filter_map(|x| x["name"].as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(format!("Ollama running ({} models)", models.len()))
            }
        }
    }

    /// Stream a chat completion; each text delta goes to `on_token`. Returns
    /// the full accumulated text. On mid-stream failure the accumulated text
    /// is preserved by the caller (tokens already delivered) and
    /// `AiError::Interrupted` is returned.
    #[cfg(feature = "native")]
    pub fn stream_chat(
        &self,
        messages: &[ChatMessage],
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, AiError> {
        self.stream_chat_cancellable(messages, on_token, &|| false)
    }

    /// Like [`stream_chat`], but `is_cancelled` is polled between stream
    /// chunks — cancel-anytime UX for long-reasoning models. Returns
    /// [`AiError::Cancelled`] with the partial preserved by the caller.
    #[cfg(feature = "native")]
    pub fn stream_chat_cancellable(
        &self,
        messages: &[ChatMessage],
        on_token: &mut dyn FnMut(&str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<String, AiError> {
        match self.kind {
            ProviderKind::Anthropic => self.stream_anthropic(messages, on_token, is_cancelled),
            _ => self.stream_openai_compatible(messages, on_token, is_cancelled),
        }
    }

    #[cfg(feature = "native")]
    fn stream_anthropic(
        &self,
        messages: &[ChatMessage],
        on_token: &mut dyn FnMut(&str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<String, AiError> {
        let system: String = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let chat: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                if m.images.is_empty() {
                    serde_json::json!({"role": m.role, "content": m.content})
                } else {
                    let mut blocks: Vec<serde_json::Value> = m
                        .images
                        .iter()
                        .map(|img| {
                            serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": img.media_type,
                                    "data": img.data_b64,
                                }
                            })
                        })
                        .collect();
                    blocks.push(serde_json::json!({"type": "text", "text": m.content}));
                    serde_json::json!({"role": m.role, "content": blocks})
                }
            })
            .collect();
        // Generous output budget: reasoning models (GLM, o-series style)
        // spend tokens thinking before any text; a small cap can exhaust
        // entirely on reasoning and stream zero visible text.
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": 8192,
            "stream": true,
            "messages": chat,
        });
        if !system.is_empty() {
            body["system"] = serde_json::Value::String(system);
        }
        // Both auth headers: Anthropic reads x-api-key and ignores the
        // Bearer; Anthropic-compatible endpoints like Z.ai use Bearer only
        // (docs.z.ai/api-reference: "standard HTTP Bearer" exclusively).
        let key = self.api_key.as_deref().unwrap_or_default();
        let response = ureq::post(&format!("{}/v1/messages", self.base_url))
            .set("x-api-key", key)
            .set("Authorization", &format!("Bearer {key}"))
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .timeout(self.timeout)
            .send_json(body)
            .map_err(map_ureq)?;

        consume_sse(response, on_token, is_cancelled, |data| {
            let event: serde_json::Value = serde_json::from_str(data).ok()?;
            if event["type"] == "content_block_delta" {
                event["delta"]["text"].as_str().map(|s| s.to_string())
            } else {
                None
            }
        })
    }

    #[cfg(feature = "native")]
    fn stream_openai_compatible(
        &self,
        messages: &[ChatMessage],
        on_token: &mut dyn FnMut(&str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<String, AiError> {
        let path = match self.kind {
            ProviderKind::Ollama => "/v1/chat/completions", // Ollama's OpenAI-compat endpoint
            _ => "/v1/chat/completions",
        };
        let body = serde_json::json!({
            "model": self.model,
            "stream": true,
            "messages": messages
                .iter()
                .map(|m| {
                    if m.images.is_empty() {
                        serde_json::json!({"role": m.role, "content": m.content})
                    } else {
                        let mut parts: Vec<serde_json::Value> = m
                            .images
                            .iter()
                            .map(|img| {
                                serde_json::json!({
                                    "type": "image_url",
                                    "image_url": {"url": format!("data:{};base64,{}", img.media_type, img.data_b64)}
                                })
                            })
                            .collect();
                        parts.push(serde_json::json!({"type": "text", "text": m.content}));
                        serde_json::json!({"role": m.role, "content": parts})
                    }
                })
                .collect::<Vec<_>>(),
        });
        let mut request = ureq::post(&format!("{}{path}", self.base_url))
            .set("content-type", "application/json")
            .timeout(self.timeout);
        if let Some(key) = &self.api_key {
            request = request.set("Authorization", &format!("Bearer {key}"));
        }
        let response = request.send_json(body).map_err(map_ureq)?;

        consume_sse(response, on_token, is_cancelled, |data| {
            if data.trim() == "[DONE]" {
                return None;
            }
            let event: serde_json::Value = serde_json::from_str(data).ok()?;
            event["choices"][0]["delta"]["content"]
                .as_str()
                .map(|s| s.to_string())
        })
    }
}

#[cfg(feature = "native")]
fn map_ureq(e: ureq::Error) -> AiError {
    match e {
        ureq::Error::Status(status, response) => AiError::Provider {
            status,
            message: response
                .into_string()
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect(),
        },
        other => AiError::Network(other.to_string()),
    }
}

/// Read an SSE body line by line, extracting text deltas via `parse_data`.
/// `is_cancelled` is polled per line; cancellation returns the designed
/// [`AiError::Cancelled`] (the caller preserves the partial).
#[cfg(feature = "native")]
fn consume_sse(
    response: ureq::Response,
    on_token: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
    parse_data: impl Fn(&str) -> Option<String>,
) -> Result<String, AiError> {
    use std::io::{BufRead, BufReader};
    let reader = BufReader::new(response.into_reader());
    let mut full = String::new();
    let mut got_any = false;
    for line in reader.lines() {
        if is_cancelled() {
            return Err(AiError::Cancelled);
        }
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                return if got_any {
                    Err(AiError::Interrupted)
                } else {
                    Err(AiError::Network(
                        "stream failed before any output".to_string(),
                    ))
                };
            }
        };
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim_start();
        // In-stream errors: some providers (e.g. Z.ai) answer HTTP 200 and
        // deliver failures as `event: error` — swallowing them would look
        // like an empty answer.
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
            if event["type"] == "error" {
                let message = event["error"]["message"]
                    .as_str()
                    .unwrap_or("provider returned an in-stream error")
                    .to_string();
                return Err(AiError::Provider {
                    status: 200,
                    message,
                });
            }
        }
        if let Some(delta) = parse_data(data) {
            got_any = true;
            full.push_str(&delta);
            on_token(&delta);
        }
    }
    Ok(full)
}

/// Validate a key against the provider, then persist it in the keychain.
/// Returns a human-readable success summary.
#[cfg(feature = "native")]
pub fn validate_and_store_key(kind: ProviderKind, key: &str) -> Result<String, AiError> {
    let provider = Provider::with_base_url(
        kind,
        kind.default_model(ModelClass::Light),
        kind.default_base_url(),
        Some(key.to_string()),
    );
    let summary = provider.validate()?;
    store_key(kind, key)?;
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Availability summary for the UI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub kind: ProviderKind,
    pub has_key: bool,
    /// For Ollama: reachable locally.
    pub available: bool,
}

/// Snapshot of configured providers (keychain lookups + Ollama probe).
#[cfg(feature = "native")]
pub fn provider_statuses() -> Vec<ProviderStatus> {
    ProviderKind::ALL
        .into_iter()
        .map(|kind| {
            let has_key = load_key(kind).ok().flatten().is_some();
            let available = match kind {
                ProviderKind::Ollama => ureq::get("http://localhost:11434/api/tags")
                    .timeout(std::time::Duration::from_millis(800))
                    .call()
                    .is_ok(),
                _ => has_key,
            };
            ProviderStatus {
                kind,
                has_key,
                available,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    /// Minimal one-shot SSE server; returns its base URL.
    fn sse_server(body: &'static str, truncate: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Drain the full request (headers + content-length body); closing
            // with unread data would RST the connection and kill the response.
            use std::io::Read;
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                if let Some(header_end) = request
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| p + 4)
                {
                    let head = String::from_utf8_lossy(&request[..header_end]).to_lowercase();
                    let content_length: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if request.len() >= header_end + content_length {
                        break;
                    }
                }
            }
            let payload = if truncate {
                // Cut the stream mid-flight (no terminating event).
                &body[..body.len() / 2]
            } else {
                body
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n",
                payload.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(payload.as_bytes()).unwrap();
        });
        format!("http://{addr}")
    }

    const OPENAI_STREAM: &str = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
        "data: [DONE]\n\n",
    );

    #[test]
    fn streams_openai_compatible_tokens() {
        let url = sse_server(OPENAI_STREAM, false);
        let provider =
            Provider::with_base_url(ProviderKind::OpenAi, "test-model", &url, Some("k".into()));
        let mut tokens = Vec::new();
        let full = provider
            .stream_chat(
                &[ChatMessage {
                    role: "user".into(),
                    content: "hi".into(),
                    images: vec![],
                }],
                &mut |t| tokens.push(t.to_string()),
            )
            .unwrap();
        assert_eq!(full, "Hello world");
        assert_eq!(tokens, vec!["Hello", " world"]);
    }

    #[test]
    fn streams_anthropic_deltas() {
        const ANTHROPIC_STREAM: &str = concat!(
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Que\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ries\"}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let url = sse_server(ANTHROPIC_STREAM, false);
        let provider = Provider::with_base_url(
            ProviderKind::Anthropic,
            "test-model",
            &url,
            Some("k".into()),
        );
        let full = provider
            .stream_chat(
                &[
                    ChatMessage {
                        role: "system".into(),
                        content: "be brief".into(),
                        images: vec![],
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: "what is Q".into(),
                        images: vec![],
                    },
                ],
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(full, "Queries");
    }

    #[test]
    fn provider_error_statuses_are_typed() {
        // Nothing listening → network error, not a panic.
        let provider = Provider::with_base_url(
            ProviderKind::OpenAi,
            "m",
            "http://127.0.0.1:1",
            Some("k".into()),
        );
        match provider.stream_chat(
            &[ChatMessage {
                role: "user".into(),
                content: "x".into(),
                images: vec![],
            }],
            &mut |_| {},
        ) {
            Err(AiError::Network(_)) => {}
            other => panic!("expected Network error, got {other:?}"),
        }
    }

    #[test]
    fn missing_key_is_designed_state() {
        // openrouter is unlikely to have a key in the test keychain; if the
        // keychain itself is unavailable (headless CI), that surfaces as
        // Keychain, which is also acceptable here.
        match Provider::from_keychain(ProviderKind::OpenRouter, None) {
            Err(AiError::NoKey(id)) if id == "openrouter" => {}
            Err(AiError::Keychain(_)) | Ok(_) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
