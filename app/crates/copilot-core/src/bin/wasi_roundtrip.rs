//! WASI round-trip probe (v5 platform-parity 5.3): the core compiled to
//! wasm32-wasip1 doing real bundle work against a preopened filesystem —
//! exactly the surface an OPFS-backed WASI shim provides in browsers.
//! Run 1 creates the bundle and appends; run 2 must see run 1's bytes
//! (persistence) and leave metadata byte-identical (consistency).

fn main() {
    let root = std::path::Path::new("/data/p.research");
    let bundle = if root.join("metadata.json").exists() {
        copilot_core::bundle::Bundle::open(root).expect("reopen")
    } else {
        copilot_core::bundle::Bundle::create(
            root,
            b"%PDF-1.5 wasi",
            copilot_core::bundle::Paper::new("WASI Paper"),
            "file",
        )
        .expect("create")
    };

    let notes = bundle.journal("notes/notes.jsonl");
    let before: Vec<serde_json::Value> = notes.read_all().expect("read notes");
    notes
        .append(&serde_json::json!({
            "at": copilot_core::bundle::now_rfc3339(),
            "text": format!("note #{}", before.len() + 1),
        }))
        .expect("append");
    let after: Vec<serde_json::Value> = notes.read_all().expect("reread");

    let violations = copilot_core::schemas::validate_bundle(root).expect("validate");
    println!("notes={}", after.len());
    println!(
        "metadata_sha={}",
        copilot_core::bundle::sha256_file(&root.join("metadata.json")).expect("hash")
    );
    println!("violations={}", violations.len());
    println!(
        "revision={}",
        copilot_core::contributions::current_revision(&bundle).expect("revision")
    );

    // Web sync bootstrap (5.4): key derivation runs client-side in the
    // browser build; the native test asserts this digest matches native
    // derivation byte-for-byte, and that an encrypt here decrypts there.
    let key = copilot_core::sync::crypto::derive_key("web passphrase", b"fixed-salt-16byte")
        .expect("kdf");
    println!("key_sha={}", copilot_core::bundle::sha256_bytes(&key.0));
    let sealed = copilot_core::sync::crypto::encrypt(&key, b"sealed on the web");
    std::fs::write("/data/sealed.bin", &sealed).expect("write sealed");
}
