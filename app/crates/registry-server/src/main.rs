//! Run the reference registry server.
//!
//! Config via env:
//!   REGISTRY_STORE=folder|s3        (default folder)
//!   REGISTRY_ROOT=./registry-data   (folder store root)
//!   REGISTRY_S3_ENDPOINT/BUCKET/REGION/ACCESS/SECRET (s3 store)
//!   REGISTRY_TOKENS=./tokens.json   ({"<token>": {"id": "...", "quota_bytes": n}})
//!   REGISTRY_BIND=0.0.0.0:8791

use std::collections::BTreeMap;
use std::sync::Arc;

use registry_server::{AppState, FolderStore, Identity, S3Store, Store};

#[tokio::main]
async fn main() {
    let env = |k: &str| std::env::var(k).ok();
    let store: Box<dyn Store> = match env("REGISTRY_STORE").as_deref() {
        Some("s3") => {
            let store = S3Store::new(copilot_core::sync::s3::S3Config {
                endpoint: env("REGISTRY_S3_ENDPOINT").expect("REGISTRY_S3_ENDPOINT"),
                bucket: env("REGISTRY_S3_BUCKET").expect("REGISTRY_S3_BUCKET"),
                region: env("REGISTRY_S3_REGION").unwrap_or_else(|| "us-east-1".into()),
                access_key: env("REGISTRY_S3_ACCESS").expect("REGISTRY_S3_ACCESS"),
                secret_key: env("REGISTRY_S3_SECRET").expect("REGISTRY_S3_SECRET"),
            });
            store.ensure_bucket().expect("bucket reachable");
            Box::new(store)
        }
        _ => Box::new(FolderStore::new(
            env("REGISTRY_ROOT").unwrap_or_else(|| "./registry-data".into()),
        )),
    };
    let tokens: BTreeMap<String, Identity> = env("REGISTRY_TOKENS")
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    if tokens.is_empty() {
        eprintln!("warning: no REGISTRY_TOKENS configured — publishing is disabled, pulls work");
    }
    let state = Arc::new(AppState {
        store,
        tokens,
        max_layer_bytes: 50 * 1024 * 1024,
    });
    let bind = env("REGISTRY_BIND").unwrap_or_else(|| "0.0.0.0:8791".into());
    let listener = tokio::net::TcpListener::bind(&bind).await.expect("bind");
    println!("registry-server listening on {bind}");
    axum::serve(listener, registry_server::router(state))
        .await
        .expect("serve");
}
