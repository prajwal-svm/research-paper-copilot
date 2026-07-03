# Tasks: add-v5-operating-system

## 1. Format 0.5.0 + schemas (the contract everything builds on)

- [x] 1.1 Bump FORMAT_VERSION to 0.5.0; add `contributions/`, `registry.json`, `plugins/` to the bundle layout; extend the same-major compat loop with a 0.4.0 fixture (auto-upgrade on first write)
- [x] 1.2 Add `schemars` derives to all bundle-serialized types; `cargo xtask schemas` emits `schemas/0.x/*.schema.json`; CI-style test fails on drift between code and emitted schemas
- [x] 1.3 Core `validate_bundle()` reporting violations by file + JSON path; shell command `bundle_validate`; test: core-written bundle validates clean, corrupted fixture reports the exact path

## 2. Community contributions (core)

- [x] 2.1 `contributions.rs`: proposal type (base revision id, journal-entry change set, content-addressed file adds, author, summary); create/queue offline; serialization round-trip tests
- [x] 2.2 Provenance log `contributions/provenance.jsonl` (propose/review/merge/revert events), ed25519 signing with registry identity keys; verification treats old keys valid-at-time
- [x] 2.3 Merge: entry-set union for journal payloads (reuse merge.rs), file adds, conflict surfacing for non-journal replacements; stale-base rebase test; revert produces new revision (history intact) test
- [x] 2.4 Reputation fold over provenance (accepted/reverted/reviews); determinism test: recompute == displayed
- [x] 2.5 Trust levels + moderation gates: new contributors always reviewed; policy validation (enrichment-only) rejects before review; tests for gate + violation block

## 3. Knowledge registry

- [x] 3.1 Canonical identity: DOI/arXiv normalization (lowercase DOI, versionless arXiv), resolution order, registry-ineligible flag for unresolvable papers; convergence test (URL import vs local PDF with same arXiv id)
- [x] 3.2 Layer format: content-addressed tarball + manifest (artifacts, digests, provenance, format major, monotonic version); integrity verification discards mismatches with visible error
- [x] 3.3 Publish path: shareability allowlist (no source.pdf/pages/extraction text; quotes clamped with anchors), preview of exactly what uploads, token-authenticated publish; test: private data excluded, PDF payload blocked client-side
- [x] 3.4 Pull-on-import: registry query by canonical id, explicit consent, provenance-tagged layer merge that never overwrites user artifacts, re-anchoring with explicit unresolved degradation; offline fallback = v4 behavior
- [x] 3.5 Reference registry server (`registry-server/` axum crate over any S3-compatible bucket): index API (canonical-id → manifests), token identity, server-side enrichment-only validation (banned kinds, %PDF magic, text-dump heuristics), per-identity quotas; ignored live test against MinIO
- [x] 3.6 Multi-registry client settings (list + default, keychain-stored tokens); self-hosted instance works identically (covered by 3.5 live test)

## 4. Plugin API

- [x] 4.1 Plugin manifest (`plugin.json`: format major, capabilities, permissions); discovery + compatibility rejection at load with reason; no code executes for incompatible plugins
- [x] 4.2 Host API over wasmtime: scoped bundle read API, permission-gated host functions (network/fs) with recorded revocable consents (sandbox-consent UX pattern); undeclared access blocked-and-surfaced test
- [x] 4.3 Panel host: plugin UI surface (iframe/webview + postMessage bridge — spike then pick per design open question); panel pane in Reader
- [x] 4.4 Reference exporters as real plugins using only the public API: Anki deck (anchors as tags), Obsidian vault (backlinks), LaTeX notes; byte-level golden tests
- [x] 4.5 Importer surface + LaTeX-source reference importer producing schema-valid bundles (explicit page-geometry degradation); imported bundle passes validation and opens in reader/graph/chat

## 5. Platform parity (web)

- [x] 5.1 `src/platform/` adapter (invoke, dialogs, chrome); desktop impl wraps Tauri; all direct `invoke` imports migrated to the adapter; tsc + existing UI behavior unchanged
- [x] 5.2 Feature-gate `copilot-core` (`native` feature: sandbox/docker/keychain/ureq); wasm32 target compiles with HTTP via host-bound fetch; CI-style check builds both targets
- [x] 5.3 WASI surface (wasm32-wasip1 ↔ OPFS-backed shim) + bundle round-trip test under wasmtime: open, edit journals, reload byte-consistent, schema-valid in wasm
- [x] 5.4 Web sync bootstrap: passphrase → key derivation in browser, pull/decrypt client-side, union-merge convergence with a desktop peer (two-client test against MinIO); wrong-passphrase clean fail; document R2/MinIO CORS config
- [x] 5.5 Capability matrix in core (native/web/web-via-runner per feature), exported through the adapter; web UI derives availability; sandbox pane on web shows explicit runner explanation with read-only history
- [x] 5.6 Web build target (Vite config + entry) shipping the identical view modules; smoke: library, reader, graph canvas, research view render on web

## 6. Contribution & registry UI

- [x] 6.1 "Propose to community" flow on shareable artifacts; proposal queue with offline pending state
- [x] 6.2 Review UI: full diff view, accept/reject with reason, trust-level gates; provenance inspector (revision chain per artifact)
- [x] 6.3 Registry panels: enrichment available on import (consent pull), browse layers, publish with upload preview; omnibar commands (/publish, /pull)

## 7. Verification & docs

- [x] 7.1 Full suite green (core tests incl. new modules, tsc, production build, wasm32 build, perf budgets); extend budgets for pull-on-import and web bundle-open
- [x] 7.2 Docs: `docs/guides/{community,registry,plugins,web}.md`; registry self-host guide (Coolify/MinIO + CORS); plugin author quickstart against schemas; README v5 section
- [ ] 7.3 Release prep (GATED: requires user's explicit commit approval — repo commit freeze still in force)
