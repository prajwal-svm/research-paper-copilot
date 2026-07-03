//! Integration tests for Anthropic-compatible custom endpoints
//! (change: add-zai-glm-provider, task 3.3): custom base URL streaming,
//! request timeout, cancellation, and bad-protocol failure — all against a
//! local mock server, no network.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use copilot_core::ai::{AiError, ChatMessage, ModelClass};
use copilot_core::provider_config::{preset, ProviderConfig};

/// One-shot mock server. Captures the request body (sent to `on_request`),
/// then answers with `head`+`body`; optional artificial stall before the body.
fn mock_server(
    status_line: &'static str,
    content_type: &'static str,
    body: &'static str,
    stall: Option<std::time::Duration>,
) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        // Drain full request (headers + content-length body).
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
        let _ = tx.send(String::from_utf8_lossy(&request).to_string());
        if let Some(stall) = stall {
            std::thread::sleep(stall);
        }
        let head = format!(
            "{status_line}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body.as_bytes());
    });
    (format!("http://{addr}"), rx)
}

const GLM_STREAM: &str = concat!(
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"GLM \"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"says hi\"}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n",
);

fn zai_config_at(base_url: &str) -> ProviderConfig {
    let mut config = ProviderConfig::from_preset(&preset("zai-glm").unwrap());
    config.base_url = base_url.to_string();
    config
}

fn ask() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".into(),
        content: "hello".into(),
    }]
}

#[test]
fn streams_through_custom_base_url_with_preset_model() {
    let (url, rx) = mock_server("HTTP/1.1 200 OK", "text/event-stream", GLM_STREAM, None);
    let config = zai_config_at(&url);

    let provider = copilot_core::ai::Provider::with_base_url(
        config.protocol,
        &config.model_for(ModelClass::Strong),
        &config.base_url,
        Some("zai-key".into()),
    );
    let full = provider.stream_chat(&ask(), &mut |_| {}).unwrap();
    assert_eq!(full, "GLM says hi");

    // The request went to the custom endpoint with the GLM model id and the
    // Anthropic protocol headers.
    let request = rx.recv().unwrap();
    assert!(request.contains("POST /v1/messages"));
    assert!(request.contains("\"model\":\"glm-5.2\""), "{request}");
    assert!(request.to_lowercase().contains("x-api-key: zai-key"));
}

#[test]
fn one_m_flag_never_produces_invalid_model_id() {
    // Z.ai's raw API rejects "[1m]"-suffixed ids (verified 2026-07-02); the
    // flag must be inert for this preset.
    let (url, rx) = mock_server("HTTP/1.1 200 OK", "text/event-stream", GLM_STREAM, None);
    let mut config = zai_config_at(&url);
    config.one_m_context = true;

    let provider = copilot_core::ai::Provider::with_base_url(
        config.protocol,
        &config.model_for(ModelClass::Strong),
        &config.base_url,
        Some("zai-key".into()),
    );
    provider.stream_chat(&ask(), &mut |_| {}).unwrap();
    let request = rx.recv().unwrap();
    assert!(request.contains("\"model\":\"glm-5.2\""), "{request}");
    assert!(!request.contains("[1m]"), "{request}");
}

#[test]
fn in_stream_error_events_surface_as_provider_errors() {
    // Z.ai answers HTTP 200 and delivers failures as `event: error` (e.g.
    // unknown model). This must never look like an empty answer.
    const ERROR_STREAM: &str = concat!(
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"[1211][Unknown Model, please check the model code.]\"}}\n\n",
    );
    let (url, _rx) = mock_server("HTTP/1.1 200 OK", "text/event-stream", ERROR_STREAM, None);
    let config = zai_config_at(&url);
    let provider = copilot_core::ai::Provider::with_base_url(
        config.protocol,
        &config.model_for(ModelClass::Strong),
        &config.base_url,
        Some("zai-key".into()),
    );
    match provider.stream_chat(&ask(), &mut |_| {}) {
        Err(AiError::Provider {
            status: 200,
            message,
        }) => {
            assert!(message.contains("Unknown Model"), "{message}");
        }
        other => panic!("expected in-stream Provider error, got {other:?}"),
    }
}

#[test]
fn short_timeout_fails_plainly_instead_of_hanging() {
    let (url, _rx) = mock_server(
        "HTTP/1.1 200 OK",
        "text/event-stream",
        GLM_STREAM,
        Some(std::time::Duration::from_secs(10)),
    );
    let config = zai_config_at(&url);
    let provider = copilot_core::ai::Provider::with_base_url(
        config.protocol,
        &config.model_for(ModelClass::Strong),
        &config.base_url,
        Some("zai-key".into()),
    )
    .with_timeout(std::time::Duration::from_millis(300));

    let start = std::time::Instant::now();
    let result = provider.stream_chat(&ask(), &mut |_| {});
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    match result {
        Err(AiError::Network(_)) | Err(AiError::Interrupted) => {}
        other => panic!("expected timeout error, got {other:?}"),
    }
}

#[test]
fn cancellation_preserves_partial_and_stops() {
    let (url, _rx) = mock_server("HTTP/1.1 200 OK", "text/event-stream", GLM_STREAM, None);
    let config = zai_config_at(&url);
    let provider = copilot_core::ai::Provider::with_base_url(
        config.protocol,
        &config.model_for(ModelClass::Strong),
        &config.base_url,
        Some("zai-key".into()),
    );

    // Cancel after the first token arrives.
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let mut partial = String::new();
    let result = provider.stream_chat_cancellable(
        &ask(),
        &mut |token| {
            partial.push_str(token);
            flag.store(true, Ordering::SeqCst);
        },
        &{
            let cancelled = cancelled.clone();
            move || cancelled.load(Ordering::SeqCst)
        },
    );
    assert!(matches!(result, Err(AiError::Cancelled)), "{result:?}");
    assert!(!partial.is_empty(), "partial output preserved");
    assert!(partial.len() < "GLM says hi".len());
}

#[test]
fn bad_protocol_endpoint_fails_with_plain_error() {
    // An endpoint that answers HTML is not Anthropic-compatible.
    let (url, _rx) = mock_server(
        "HTTP/1.1 200 OK",
        "text/html",
        "<html><body>login page</body></html>",
        None,
    );
    let config = zai_config_at(&url);
    let provider = copilot_core::ai::Provider::with_base_url(
        config.protocol,
        &config.model_for(ModelClass::Light),
        &config.base_url,
        Some("zai-key".into()),
    );

    // Validation (used before any configuration is saved) must fail —
    // the /v1/models test call gets HTML, not JSON.
    match provider.validate() {
        Err(AiError::Network(_)) | Err(AiError::Provider { .. }) => {}
        other => panic!("expected protocol failure, got {other:?}"),
    }
}
