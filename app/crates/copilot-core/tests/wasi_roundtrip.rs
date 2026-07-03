//! v5 platform-parity 5.3: the core compiled to wasm32-wasip1 runs real
//! bundle operations against a preopened directory — the exact contract an
//! OPFS-backed WASI shim provides in browsers. Two runs over the same dir
//! prove persistence (run 2 sees run 1's journal entry) and byte
//! consistency (metadata digest unchanged, schema-valid throughout).

use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};

fn build_wasm() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new("cargo")
        .args([
            "build",
            "--bin",
            "wasi_roundtrip",
            "-p",
            "copilot-core",
            "--no-default-features",
            "--target",
            "wasm32-wasip1",
        ])
        .current_dir(&manifest)
        .status()
        .expect("cargo runs");
    assert!(status.success(), "wasip1 build must succeed");
    manifest.join("../../target/wasm32-wasip1/debug/wasi_roundtrip.wasm")
}

fn run_once(engine: &Engine, module: &Module, data_dir: &std::path::Path) -> String {
    let mut linker: Linker<WasiP1Ctx> = Linker::new(engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t| t).unwrap();
    let stdout = MemoryOutputPipe::new(64 * 1024);
    let wasi = WasiCtxBuilder::new()
        .stdout(stdout.clone())
        .inherit_stderr()
        .preopened_dir(data_dir, "/data", DirPerms::all(), FilePerms::all())
        .unwrap()
        .build_p1();
    let mut store = Store::new(engine, wasi);
    let instance = linker.instantiate(&mut store, module).unwrap();
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .unwrap();
    start.call(&mut store, ()).unwrap();
    drop(store);
    String::from_utf8(stdout.contents().to_vec()).unwrap()
}

fn field<'a>(output: &'a str, key: &str) -> &'a str {
    output
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("missing {key} in {output}"))
}

#[test]
fn wasm_core_bundle_roundtrip_is_persistent_and_byte_consistent() {
    let wasm = build_wasm();
    let engine = Engine::default();
    let module = Module::from_file(&engine, &wasm).unwrap();
    let data = tempfile::tempdir().unwrap();

    let first = run_once(&engine, &module, data.path());
    assert_eq!(field(&first, "notes"), "1");
    assert_eq!(field(&first, "violations"), "0", "schema-valid in wasm");
    assert_eq!(field(&first, "revision"), "genesis");

    // "Reload": a fresh instance over the same storage — run 1's bytes are
    // there, and metadata was not perturbed by reopening.
    let second = run_once(&engine, &module, data.path());
    assert_eq!(field(&second, "notes"), "2", "run 1's entry persisted");
    assert_eq!(
        field(&first, "metadata_sha"),
        field(&second, "metadata_sha"),
        "byte-consistent across reload"
    );
    assert_eq!(field(&second, "violations"), "0");

    // Web sync bootstrap (5.4): the wasm build derives the SAME key from
    // the same passphrase+salt as native — so a browser session can
    // decrypt what desktop pushed, and vice versa. And ciphertext sealed
    // inside wasm decrypts natively.
    let native_key =
        copilot_core::sync::crypto::derive_key("web passphrase", b"fixed-salt-16byte").unwrap();
    assert_eq!(
        field(&second, "key_sha"),
        copilot_core::bundle::sha256_bytes(&native_key.0),
        "KDF parity across targets"
    );
    let sealed = std::fs::read(data.path().join("sealed.bin")).unwrap();
    assert_eq!(
        copilot_core::sync::crypto::decrypt(&native_key, &sealed).unwrap(),
        b"sealed on the web",
        "wasm-encrypted, native-decrypted"
    );

    // Wrong passphrase fails cleanly — identical code path on web.
    let wrong = copilot_core::sync::crypto::derive_key("wrong", b"fixed-salt-16byte").unwrap();
    assert!(copilot_core::sync::crypto::decrypt(&wrong, &sealed).is_err());
}
