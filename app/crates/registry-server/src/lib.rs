//! Reference knowledge-registry server (v5).
//!
//! A thin HTTP index over pluggable blob storage (local folder for tests
//! and small deployments, any S3-compatible bucket via copilot-core's
//! SigV4 client for real ones). Self-hostable by anyone: our instance is
//! one deployment, not the only one.
//!
//! Guarantees enforced HERE (defense in depth vs the client):
//! - enrichment-only: banned paths, `%PDF` magic bytes, text-dump caps
//! - token identity + per-identity storage quotas
//! - monotonic layer versions per canonical paper id
//! - blob digests must match the accepted manifest exactly

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path as AxPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use copilot_core::registry::{verify_layer, LayerManifest};

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Blob storage the index sits on. Key space is flat strings, like S3.
pub trait Store: Send + Sync + 'static {
    fn get(&self, key: &str) -> std::io::Result<Option<Vec<u8>>>;
    fn put(&self, key: &str, bytes: &[u8]) -> std::io::Result<()>;
    fn list(&self, prefix: &str) -> std::io::Result<Vec<String>>;
}

/// Folder-backed store (tests, small self-hosted deployments).
pub struct FolderStore {
    root: std::path::PathBuf,
}

impl FolderStore {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        FolderStore { root: root.into() }
    }
    fn path(&self, key: &str) -> std::path::PathBuf {
        // Keys are '/'-separated; encode nothing else (keys are ours).
        self.root.join(key)
    }
}

impl Store for FolderStore {
    fn get(&self, key: &str) -> std::io::Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(key)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn put(&self, key: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = self.path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, path)
    }
    fn list(&self, prefix: &str) -> std::io::Result<Vec<String>> {
        let mut out = Vec::new();
        let base = self.root.clone();
        fn walk(
            dir: &std::path::Path,
            base: &std::path::Path,
            prefix: &str,
            out: &mut Vec<String>,
        ) -> std::io::Result<()> {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return Ok(());
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, base, prefix, out)?;
                } else if let Ok(rel) = path.strip_prefix(base) {
                    let key = rel.to_string_lossy().replace('\\', "/");
                    if key.starts_with(prefix) && !key.ends_with(".tmp") {
                        out.push(key);
                    }
                }
            }
            Ok(())
        }
        walk(&base, &base, prefix, &mut out)?;
        out.sort();
        Ok(out)
    }
}

/// S3-backed store: reuses copilot-core's hand-signed SigV4 client, so any
/// endpoint that works for cloud-sync (R2, MinIO) works here.
pub struct S3Store {
    client: copilot_core::sync::s3::S3Client,
}

impl S3Store {
    pub fn new(config: copilot_core::sync::s3::S3Config) -> Self {
        S3Store {
            client: copilot_core::sync::s3::S3Client::new(config),
        }
    }

    /// Create the bucket if missing (deploy convenience; idempotent).
    pub fn ensure_bucket(&self) -> std::io::Result<()> {
        self.client.ensure_bucket().map_err(std::io::Error::other)
    }
}

impl Store for S3Store {
    fn get(&self, key: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.client.get(key).map_err(std::io::Error::other)
    }
    fn put(&self, key: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.client.put(key, bytes).map_err(std::io::Error::other)
    }
    fn list(&self, prefix: &str) -> std::io::Result<Vec<String>> {
        self.client.list(prefix).map_err(std::io::Error::other)
    }
}

// ---------------------------------------------------------------------------
// Config & app state
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Deserialize)]
pub struct Identity {
    pub id: String,
    /// Max total stored bytes for this identity.
    #[serde(default = "default_quota")]
    pub quota_bytes: u64,
}

fn default_quota() -> u64 {
    100 * 1024 * 1024
}

pub struct AppState {
    pub store: Box<dyn Store>,
    /// Bearer token → identity.
    pub tokens: BTreeMap<String, Identity>,
    /// Layer blob hard cap.
    pub max_layer_bytes: u64,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/papers/{key}/layers", post(publish_manifest))
        .route("/v1/papers/{key}/layers", get(list_layers))
        .route("/v1/papers/{key}/layers/{version}/blob", put(upload_blob))
        .route("/v1/papers/{key}/layers/{version}/blob", get(get_blob))
        .with_state(state)
}

fn paper_prefix(key: &str) -> String {
    // ':' and '/' are the only separators in canonical keys; both are safe
    // to fold into a flat storage prefix.
    format!("papers/{}/", key.replace([':', '/'], "_"))
}

fn err(status: StatusCode, message: impl Into<String>) -> Response {
    (status, message.into()).into_response()
}

fn identify(state: &AppState, headers: &HeaderMap) -> Result<Identity, Box<Response>> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| Box::new(err(StatusCode::UNAUTHORIZED, "missing bearer token")))?;
    state
        .tokens
        .get(token)
        .cloned()
        .ok_or_else(|| Box::new(err(StatusCode::UNAUTHORIZED, "unknown token")))
}

// ---------------------------------------------------------------------------
// Policy (server side — the client validates too; this is the backstop)
// ---------------------------------------------------------------------------

const BANNED_FIRST: [&str; 6] = [
    "original.pdf",
    "pages",
    "figures",
    "tables",
    "layout.json",
    "semantic_tree.json",
];
/// A single text artifact larger than this is treated as a full-text dump.
const TEXT_DUMP_CAP: u64 = 2 * 1024 * 1024;

fn manifest_policy_error(manifest: &LayerManifest) -> Option<String> {
    for artifact in &manifest.artifacts {
        let first = artifact.path.split('/').next().unwrap_or("");
        if BANNED_FIRST.contains(&first) {
            return Some(format!(
                "enrichment-only policy: {} is publisher content",
                artifact.path
            ));
        }
        if artifact.path.contains("..") {
            return Some(format!("path traversal rejected: {}", artifact.path));
        }
        if artifact.size > TEXT_DUMP_CAP {
            return Some(format!(
                "enrichment-only policy: {} exceeds the {}MB artifact cap (full-text dump heuristic)",
                artifact.path,
                TEXT_DUMP_CAP / (1024 * 1024)
            ));
        }
    }
    None
}

fn blob_policy_error(blob: &[u8]) -> Option<String> {
    let mut archive = tar::Archive::new(blob);
    let Ok(entries) = archive.entries() else {
        return Some("blob is not a tar archive".into());
    };
    for entry in entries {
        let Ok(mut entry) = entry else {
            return Some("unreadable tar entry".into());
        };
        let path = entry
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut head = [0u8; 4];
        use std::io::Read;
        if entry.read(&mut head).unwrap_or(0) >= 4 && &head == b"%PDF" {
            return Some(format!("enrichment-only policy: {path} contains PDF bytes"));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn publish_manifest(
    State(state): State<Arc<AppState>>,
    AxPath(key): AxPath<String>,
    headers: HeaderMap,
    Json(mut manifest): Json<LayerManifest>,
) -> Response {
    let identity = match identify(&state, &headers) {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    if manifest.canonical_id != key {
        return err(StatusCode::BAD_REQUEST, "manifest canonical_id mismatch");
    }
    if let Some(reason) = manifest_policy_error(&manifest) {
        return err(StatusCode::UNPROCESSABLE_ENTITY, reason);
    }
    let total: u64 = manifest.artifacts.iter().map(|a| a.size).sum();
    if total > state.max_layer_bytes {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "layer exceeds size cap");
    }

    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let prefix = paper_prefix(&key);
        // Quota: sum of this identity's stored usage.
        let usage_key = format!("identities/{}/usage.json", identity.id);
        let used: u64 = state2
            .store
            .get(&usage_key)
            .ok()
            .flatten()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v["bytes"].as_u64())
            .unwrap_or(0);
        if used + total > identity.quota_bytes {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                "identity quota exceeded".to_string(),
            ));
        }
        // Monotonic version: max existing + 1.
        let existing = state2
            .store
            .list(&prefix)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let next = existing
            .iter()
            .filter_map(|k| {
                k.strip_prefix(&prefix)?
                    .strip_suffix(".manifest.json")?
                    .parse::<u64>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
            + 1;
        manifest.version = next;
        manifest.publisher = identity.id.clone();
        state2
            .store
            .put(
                &format!("{prefix}{next}.manifest.json"),
                &serde_json::to_vec(&manifest).expect("manifest serializes"),
            )
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        state2
            .store
            .put(
                &usage_key,
                serde_json::json!({ "bytes": used + total })
                    .to_string()
                    .as_bytes(),
            )
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(next)
    })
    .await;

    match result {
        Ok(Ok(version)) => Json(serde_json::json!({ "version": version })).into_response(),
        Ok(Err((status, message))) => err(status, message),
        Err(join) => err(StatusCode::INTERNAL_SERVER_ERROR, join.to_string()),
    }
}

async fn upload_blob(
    State(state): State<Arc<AppState>>,
    AxPath((key, version)): AxPath<(String, u64)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(response) = identify(&state, &headers) {
        return *response;
    }
    if body.len() as u64 > state.max_layer_bytes {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "blob exceeds size cap");
    }
    let result = tokio::task::spawn_blocking(move || {
        let prefix = paper_prefix(&key);
        let manifest_bytes = state
            .store
            .get(&format!("{prefix}{version}.manifest.json"))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .ok_or((StatusCode::NOT_FOUND, "manifest not found".to_string()))?;
        let manifest: LayerManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        // Digest verification: the client's authoritative check, rerun here.
        verify_layer(&manifest, &body)
            .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
        if let Some(reason) = blob_policy_error(&body) {
            return Err((StatusCode::UNPROCESSABLE_ENTITY, reason));
        }
        state
            .store
            .put(&format!("{prefix}{version}.blob"), &body)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err((status, message))) => err(status, message),
        Err(join) => err(StatusCode::INTERNAL_SERVER_ERROR, join.to_string()),
    }
}

async fn list_layers(State(state): State<Arc<AppState>>, AxPath(key): AxPath<String>) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        let prefix = paper_prefix(&key);
        let keys = state.store.list(&prefix)?;
        let mut manifests = Vec::new();
        for k in keys.iter().filter(|k| k.ends_with(".manifest.json")) {
            // Only layers whose blob landed are served (publish is two-step).
            let version = k
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(".manifest.json"))
                .unwrap_or_default();
            if state
                .store
                .get(&format!("{prefix}{version}.blob"))?
                .is_none()
            {
                continue;
            }
            if let Some(bytes) = state.store.get(k)? {
                if let Ok(manifest) = serde_json::from_slice::<LayerManifest>(&bytes) {
                    manifests.push(manifest);
                }
            }
        }
        manifests.sort_by_key(|m| m.version);
        Ok::<_, std::io::Error>(manifests)
    })
    .await;
    match result {
        Ok(Ok(manifests)) => Json(manifests).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        Err(join) => err(StatusCode::INTERNAL_SERVER_ERROR, join.to_string()),
    }
}

async fn get_blob(
    State(state): State<Arc<AppState>>,
    AxPath((key, version)): AxPath<(String, u64)>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        state
            .store
            .get(&format!("{}{version}.blob", paper_prefix(&key)))
    })
    .await;
    match result {
        Ok(Ok(Some(bytes))) => bytes.into_response(),
        Ok(Ok(None)) => err(StatusCode::NOT_FOUND, "no such layer blob"),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        Err(join) => err(StatusCode::INTERNAL_SERVER_ERROR, join.to_string()),
    }
}
