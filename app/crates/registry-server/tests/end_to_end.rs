//! Full-stack registry tests: the real axum server on a local port, driven
//! by copilot-core's RegistryClient — the exact production pairing.

use std::collections::BTreeMap;
use std::sync::Arc;

use copilot_core::bundle::{Bundle, Paper};
use copilot_core::registry::{build_layer, pull_layer, verify_layer, RegistryClient};
use registry_server::{AppState, FolderStore, Identity, Store};

fn spawn_server(store: Box<dyn Store>) -> String {
    let state = Arc::new(AppState {
        store,
        tokens: BTreeMap::from([(
            "alice-token".to_string(),
            Identity {
                id: "alice".into(),
                quota_bytes: 10 * 1024 * 1024,
            },
        )]),
        max_layer_bytes: 5 * 1024 * 1024,
    });
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tx.send(format!("http://{addr}")).unwrap();
            axum::serve(listener, registry_server::router(state))
                .await
                .unwrap();
        });
    });
    rx.recv().unwrap()
}

fn enriched_bundle() -> (tempfile::TempDir, Bundle) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("p.research");
    let bundle = Bundle::create(&root, b"%PDF-1.5 x", Paper::new("T"), "file").unwrap();
    std::fs::write(
        root.join("glossary/terms.json"),
        b"{\"attention\": \"weighted lookup\"}",
    )
    .unwrap();
    bundle
        .journal("notes/notes.jsonl")
        .append(&serde_json::json!({"at": "2026-01-01T00:00:00Z", "text": "community note"}))
        .unwrap();
    (tmp, bundle)
}

#[test]
fn publish_list_pull_roundtrip_over_http() {
    let store_dir = tempfile::tempdir().unwrap();
    let base_url = spawn_server(Box::new(FolderStore::new(store_dir.path())));
    let (_tmp, bundle) = enriched_bundle();

    let (manifest, blob) = build_layer(
        &bundle,
        "arxiv:1706.03762",
        0, // server assigns the real version
        "ignored — server stamps the identity",
        &["glossary/terms.json".into(), "notes/notes.jsonl".into()],
    )
    .unwrap();

    let publisher = RegistryClient {
        base_url: base_url.clone(),
        token: Some("alice-token".into()),
    };
    let v1 = publisher.publish(&manifest, &blob).unwrap();
    assert_eq!(v1, 1, "first layer gets version 1");
    // Republish → monotonic bump.
    let v2 = publisher.publish(&manifest, &blob).unwrap();
    assert_eq!(v2, 2);

    // Anonymous pull: list, fetch, verify, merge into a fresh bundle.
    let reader = RegistryClient {
        base_url,
        token: None,
    };
    let layers = reader.layers("arxiv:1706.03762").unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!(
        layers[0].publisher, "alice",
        "server stamps the token identity"
    );
    let blob = reader.blob("arxiv:1706.03762", 1).unwrap();
    verify_layer(&layers[0], &blob).unwrap();

    let (_tmp2, local) = {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mine.research");
        let b = Bundle::create(&root, b"%PDF-1.5 mine", Paper::new("T"), "file").unwrap();
        (tmp, b)
    };
    let report = pull_layer(&local, &layers[0], &blob).unwrap();
    assert!(report.added.contains(&"glossary/terms.json".to_string()));
    assert_eq!(
        report.merged_journals,
        vec!["notes/notes.jsonl".to_string()]
    );
}

#[test]
fn server_rejects_pdf_content_and_unknown_tokens() {
    let store_dir = tempfile::tempdir().unwrap();
    let base_url = spawn_server(Box::new(FolderStore::new(store_dir.path())));
    let (_tmp, bundle) = enriched_bundle();

    // Unknown token → 401 at manifest stage.
    let (manifest, blob) =
        build_layer(&bundle, "arxiv:x", 0, "p", &["glossary/terms.json".into()]).unwrap();
    let stranger = RegistryClient {
        base_url: base_url.clone(),
        token: Some("wrong-token".into()),
    };
    let error = stranger.publish(&manifest, &blob).unwrap_err().to_string();
    assert!(error.contains("401"), "{error}");

    // A crafted payload that bypassed client validation: PDF bytes under an
    // allowlisted path. Server catches it at blob stage.
    std::fs::write(
        bundle.root().join("glossary/sneaky.json"),
        b"%PDF-1.4 smuggled",
    )
    .unwrap();
    let (manifest, blob) =
        build_layer(&bundle, "arxiv:x", 0, "p", &["glossary/sneaky.json".into()]).unwrap();
    let alice = RegistryClient {
        base_url,
        token: Some("alice-token".into()),
    };
    let error = alice.publish(&manifest, &blob).unwrap_err().to_string();
    assert!(
        error.contains("422") && error.contains("PDF"),
        "server-side policy backstop: {error}"
    );

    // The rejected layer must not be listed (blob never landed).
    let layers = alice.layers("arxiv:x").unwrap();
    assert!(
        layers
            .iter()
            .all(|l| l.artifacts.iter().all(|a| a.path != "glossary/sneaky.json")),
        "{layers:?}"
    );
}

#[test]
fn banned_manifest_paths_rejected_before_upload() {
    let store_dir = tempfile::tempdir().unwrap();
    let base_url = spawn_server(Box::new(FolderStore::new(store_dir.path())));
    let (_tmp, bundle) = enriched_bundle();
    let (mut manifest, blob) =
        build_layer(&bundle, "arxiv:y", 0, "p", &["glossary/terms.json".into()]).unwrap();
    manifest.artifacts[0].path = "original.pdf".into();
    let alice = RegistryClient {
        base_url,
        token: Some("alice-token".into()),
    };
    let error = alice.publish(&manifest, &blob).unwrap_err().to_string();
    assert!(
        error.contains("422") && error.contains("publisher content"),
        "{error}"
    );
}

/// Live test against a real MinIO (same env vars as cloud-sync's):
/// RPC_S3_ENDPOINT / RPC_S3_ACCESS / RPC_S3_SECRET, bucket `registry-e2e`.
/// Run: cargo test -p registry-server --test end_to_end -- --ignored
#[test]
#[ignore]
fn live_minio_backed_registry_roundtrip() {
    let endpoint =
        std::env::var("RPC_S3_ENDPOINT").unwrap_or_else(|_| "http://localhost:19000".into());
    let store = registry_server::S3Store::new(copilot_core::sync::s3::S3Config {
        endpoint,
        bucket: "registry-e2e".into(),
        region: "us-east-1".into(),
        access_key: std::env::var("RPC_S3_ACCESS").unwrap_or_else(|_| "minioadmin".into()),
        secret_key: std::env::var("RPC_S3_SECRET").unwrap_or_else(|_| "minioadmin".into()),
    });
    store.ensure_bucket().expect("MinIO reachable");
    let base_url = spawn_server(Box::new(store));
    let (_tmp, bundle) = enriched_bundle();
    let (manifest, blob) = build_layer(
        &bundle,
        "arxiv:live",
        0,
        "p",
        &["glossary/terms.json".into()],
    )
    .unwrap();
    let client = RegistryClient {
        base_url,
        token: Some("alice-token".into()),
    };
    let version = client.publish(&manifest, &blob).unwrap();
    let layers = client.layers("arxiv:live").unwrap();
    assert!(layers.iter().any(|l| l.version == version));
    let pulled = client.blob("arxiv:live", version).unwrap();
    verify_layer(
        &layers.iter().find(|l| l.version == version).unwrap(),
        &pulled,
    )
    .unwrap();
}
