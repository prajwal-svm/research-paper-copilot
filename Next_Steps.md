# Production-Readiness Review: Research Paper Copilot

**Verdict: Not production-ready.** The architecture, crypto, threat model, and engineering discipline are genuinely above average — but there are hard blockers across release pipeline, security, and governance that must be closed before a public 0.1.0.

The good news: the blockers are mostly *configuration and packaging* (signing, license file, error boundaries), not deep rewrites. The codebase itself is sound.

---

## Strengths (don't lose these)

- **Crypto layer** — XChaCha20-Poly1305 with fresh `OsRng` nonces per encryption, AEAD decrypt with no oracle, Argon2id KDF, ed25519 provenance. Real, correct E2E encryption.
- **Sandbox consent model** — Docker `--network=none`/`--cap-drop=ALL`/`--read-only`/resource-capped, with a compile-time `ConsentGrant` choke-point that makes "execute without consent" a *compile error*. The PRD's "containerized, consented, offline-by-default" claim holds up.
- **Capabilities** — Tauri capability file is minimal & window-scoped (`core:default`, `dialog:default` only). No `**` grants, no fs/shell/http plugins exposed to the webview. Tight.
- **Telemetry** — opt-in by default, local-only JSONL, content-free by construction (allowlist of 21 event kinds; unknown kinds rejected). A `paper_content_leak` test proves it.
- **Secrets** — all keys to OS keychain via `keyring`. None in repo, none in `.env`, none tracked.
- **Type discipline** — `strict` + `noUnusedLocals/Parameters`, zero `any`/`@ts-ignore` across 9.4k TS lines, zero `console.log`/`TODO` cruft.
- **CI** — runs on mac/win/linux, `clippy -D warnings`, `cargo fmt --check`, perf budgets as release-gated blocker, schema validation, WASM-parity build check. Serious CI.
- **Error taxonomy** — every Rust module has a typed `thiserror` enum; errors propagate with `?`. AI errors truncate provider messages at 300 chars to avoid log bloat.

---

## Blockers (must fix before any public release)

### B1. No `LICENSE` file
`README.md:218` claims `MIT OR Apache-2.0` and `Cargo.toml:8` confirms the metadata, but **no `LICENSE-MIT` or `LICENSE-APACHE` file exists**. Without the files, the project is legally *All Rights Reserved*. Dual-licensing requires both texts present. `crates.io` will reject publishing. **Highest-trust blocker.**

### B2. Unsigned + unnotarized release binaries
`release.yml` has **zero code-signing** on any platform.
- **macOS**: `.app`/`.dmg` will be unsigned and unnotarized → Gatekeeper blocks launch for every user ("app is damaged / cannot be opened"). **This alone makes a public macOS release unusable.**
- **Windows**: SmartScreen "Unknown publisher" warning.
- No `APPLE_*`/`WINDOWS_CERTIFICATE`/`TAURI_SIGNING_PRIVATE_KEY` secrets are wired up; `tauri.conf.json` has no `bundle.macOS.signingIdentity` or `bundle.windows.certificate*`.

### B3. No auto-update
Zero matches for `updater`/`pubkey`/`TAURI_SIGNING` anywhere. No `tauri-plugin-updater`, no `createUpdaterArtifacts`. Every user must manually notice releases and reinstall forever. Adding the updater also forces signing (closes B2).

### B4. No React Error Boundaries anywhere
Zero `ErrorBoundary`/`componentDidCatch` in the frontend. `main.tsx:6-9` mounts `<App/>` bare under `StrictMode`. Any render-time throw — malformed object index in `Reader.tsx:1338`, an Excalidraw/BlockNote internal error, a null `doc.getPage()` — **unmounts the entire app** to a blank window. User loses their place, ink session, chat draft. Wrap at root, Reader, and each lazy pane.

### B5. Zero frontend tests
No `*.test.*`, no vitest/jest config, no `test` script in `package.json`, no `@testing-library/*`/`playwright` deps. The entire React layer (AI streaming, sync, sandbox invocation, sync-merge UI) ships with **no verification path**. Rust has 186 inline + 32 integration tests; frontend has none.

### B6. No `SECURITY.md`
For a desktop app that **executes AI-generated and cloned code in containers** and ships E2E crypto, there is no vulnerability-reporting channel. This is governance malpractice, not just hygiene.

---

## High-severity issues

### H1. Secrets leak via `Debug`
- `crates/copilot-core/src/ai.rs:150` — `#[derive(Debug)] pub struct Provider { api_key: Option<String> }` prints live API keys via `format!("{provider:?}")`/`tracing`/error context.
- `crates/copilot-core/src/sync/s3.rs:17` — `#[derive(Debug, Clone)] S3Config { access_key, secret_key }` same.
- Contrast with `crypto.rs:34-38`, which correctly hand-writes a redacting `Debug` for `LibraryKey`. Fix: manual `Debug` impls or a `secrecy` wrapper. The keychain discipline everywhere else is excellent — this one derive undermines it.

### H2. Plugin host has no resource limits (DoS/OOM)
`crates/copilot-core/src/plugin.rs:295` uses `wasmtime::Engine::default()` with **no fuel metering, no epoch interruption, no `StoreLimits`**. A malicious/buggy plugin can infinite-loop (no cancellation is plumbed in, unlike `sandbox`/`ai` which take `is_cancelled`) and OOM the host: `plugin.rs:366` does `vec![0u8; out_len]` where `out_len` comes straight from plugin-returned bytes (`0xFFFFFFFF` → 4 GiB allocation). Add `consume_fuel` + `epoch_interruption` + `StoreLimits` + bound `out_len` before allocating.

### H3. ed25519 verification not strict
`contributions.rs:387` uses `verify`, which accepts malleable/small-order signatures. For an append-only, tamper-evident provenance log that's exactly what you don't want — a malleated signature is a different `signature` field that still verifies, breaking the audit claim. Use `VerifyingKey::verify_strict`.

### H4. Tar path traversal on layer pull (defense-in-depth gap)
`registry.rs:730-737` `pull_layer` does `bundle.root().join(&path)` where `path` comes from the manifest. `verify_layer` (`:282`) checks digest + manifest membership but **not path shape**. A manifest entry like `artifacts[].path = "../../../.bashrc"` passes verification and writes outside the bundle. Reject `..`, absolute, and escaping paths in `verify_layer`.

### H5. 86 silent `.catch(() => {})` (80% of all catches)
108 `.catch` handlers in `src/`; **86 silently swallow**. Representative data-affecting failures users never see:
- `Reader.tsx:213/260/387` — ink strokes & notes fail to persist silently. Annotate offline, switch papers, lose work with zero toast.
- `Settings.tsx:247` — `telemetry_set_enabled` swallows → user thinks telemetry is off when it isn't (privacy-relevant).
- `Library.tsx:113/118` — starring/priority silently fail.

The author *did* surface errors well where they tried (`SyncSettings.tsx:97`, `ImplementationPanel.tsx:203`). Make that the default. A lint rule banning bare `.catch(() => {})` plus a `safeInvoke` helper (toast on desktop, silent on web per `Library.tsx:277` precedent) closes ~86 sites.

### H6. README ↔ PRD/CHANGELOG/OpenSpec contradiction
- `README.md:14` claims **v1, v2, v3, v4, v5, and cloud sync all "work today"**.
- `PRD.md:44`: *"only v1 is fully specified now, v2–v5 are documented as roadmap PRDs."*
- `CHANGELOG.md`: only `v0.1.0 — The Foundation (unreleased)`.
- `openspec/changes/`: **v1 is still pending** while v2/v3/cloud-sync are archived (backwards from sane release order).

The v2–v5 code is *broad and tested*, not vapor — but "works today" overstates shipping status. Pick one source of truth. This is a trust issue for a public review.

---

## Medium-severity issues

### M1. Stale/maintenance-only dependencies
- **`ureq` 2.12.1** — last 2.x, won't get fixes. 3.x is current. Migrate the 3 HTTP clients (`ai.rs`, `registry.rs`, `sync/s3.rs`).
- **`candle-core/nn/transformers` 0.9.2** — ~1+ year stale; HF's candle moves fast. Internally consistent with `tokenizers 0.23.1` + `hf-hub 0.5.0` but won't get model/CUDA/bug fixes. Plan an upgrade.

### M2. Keychain feature is Apple-only
`keyring = "4.1.2"` with `features = ["apple-native-keyring-store"]` (`copilot-core/Cargo.toml:24`). Will not offer keychain storage on Linux/Windows. Cross-platform parity gap for the "keys live in the OS keychain" promise.

### M3. `Reader.tsx` is a 1,364-line God component
Owns PDF lifecycle, virtualized slots, scroll/zoom, overlay, ink + undo, marquee, persistence, search, 8-pane routing, toolbar, and `PageView`. 20 `useState`/`useRef`. `PageView` (`:1218`) isn't memoized → every keystroke in `SearchPanel` re-renders every visible page slot. The `labelFor` closure (`Reader.tsx:552/626/645/669/711`) is inlined 5+ times and drilled through ~10 components (41 occurrences total). Extract a `<TreeContext>` and memoize `PageView`.

### M4. Sparse accessibility
Object targets are `<button>` but titled only (`title=` doesn't propagate to a11y name). No focus management on pane switches. No landmarks/skip-to-content. Ink/marquee tools are mouse-only. Search results aren't a `listbox`/`combobox`. For researchers (above-average AT usage) this needs a pass before public launch.

### M5. CSP minimal; web build has none
`tauri.conf.json:26` CSP has no `connect-src`, `object-src`, `frame-ancestors`, `base-uri`. Desktop is fine (IPC bypasses CSP), but `web.html` carries **no CSP at all** — AI calls to `api.anthropic.com`/`api.openai.com`/R2/MinIO are unrestricted.

### M6. Release workflow doesn't gate on CI
`release.yml` has no `needs: ci` — a broken `main` can be tagged and shipped. Add required status checks on tags.

### M7. No reproducible toolchain
Missing `.nvmrc` (Node "22+" only in README prose) and `rust-toolchain.toml` ("stable" unpinned). `fetch-pdfium.sh:6` defaults to `latest`, not pinned. Contributors on different Node/rustc/PDFium versions will diverge — clippy lints are version-sensitive and CI runs `clippy -D warnings`.

### M8. Public API surface too wide
`lib.rs` uses bare `pub mod` with no facade. Every struct/function in 42 modules is the de-facto public API the Tauri shell depends on. For a 0.1.0 crate, renaming anything is a breaking change. Add a `pub use` facade and mark the rest `#[doc(hidden)]` before external consumers arrive.

---

## Lower-severity / hygiene

- `telemetry.rs:103,143` — `.unwrap()` on `serde_json` serialize inside the telemetry write path. Convert to `let _ =` so logging can never crash the host.
- `read_artifact` traversal guard (`lib.rs:341`) checks `/` but not Windows `\` separators.
- `Cargo.toml:9` repo URL `github.com/research-paper-copilot/research-paper-copilot` vs identifier `io.github.researchpapercopilot` (no hyphen) — confirm the GitHub org exists.
- Release profile missing `strip = true`/`panic = "abort"` (binary size).
- `layout.rs` (830 LOC, the riskiest PDF parser) has **no direct unit tests** — only exercised indirectly via `pipeline`/`hostile_pdfs`. No fuzz harness for tar/JSON ingestion.
- `.gitignore` lacks `*.env` (latent footgun).
- `MarkdownEditor.tsx:34` reads `document.documentElement.classList.contains("dark")` at render time, not reactively — `GraphView.tsx:220-227` does it correctly with a `MutationObserver`; replicate.
- `App.tsx:9` statically imports `Reader` (pulls `pdfjs-dist` + the whole PDF stack into the initial bundle even though Library is the landing view). Lazy-load it.
- No `docs/README.md` index; ADR material buried in `platform-and-performance.md`.
- No `CONTRIBUTING.md`/`CODE_OF_CONDUCT.md` for a project whose stated moat is community.

---

## Suggested fix sequence

1. **Same-day (trust/legal)**: add `LICENSE-MIT` + `LICENSE-APACHE`, `SECURITY.md`, reconcile README/PRD/CHANGELOG/OpenSpec to one truth.
2. **Release pipeline**: wire `APPLE_*`/`WINDOWS_CERTIFICATE`/`TAURI_SIGNING_PRIVATE_KEY` secrets into `release.yml`, add `tauri-plugin-updater` + `bundle.createUpdaterArtifacts`, add `needs: ci`. Until this is done, macOS binaries are literally unlaunchable for normal users.
3. **Frontend safety net**: root + Reader + lazy-pane `ErrorBoundary`, stand up Vitest + smoke tests, replace silent catches with `safeInvoke`.
4. **Security hardening**: redact secrets in `Debug`, fuel/epoch/`StoreLimits` in plugin host, `verify_strict`, path-shape validation in `verify_layer`.
5. **Contributor basics**: `.nvmrc`, `rust-toolchain.toml`, pin `PDFIUM_VERSION`, `CONTRIBUTING.md`.
6. **Quality work** (defer past 0.1.0): dep upgrades (ureq 3, candle), keyring cross-platform features, `Reader.tsx` decomposition, a11y pass, CSP for web build.

Everything else — the bundle format, the sync engine, the crypto, the consent-gated sandbox, the error taxonomy, the test-perf-as-requirement CI — is release-ready.
