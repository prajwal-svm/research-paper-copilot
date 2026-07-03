# Design: add-v5-operating-system

## Context

v1–v4 built a local-first desktop app: `.research` bundles (format 0.4.0), append-only JSONL journals folded at read, cloud sync with union-merge + E2E encryption over any S3-compatible remote (R2 primary, MinIO/Coolify self-host), sandboxed execution with a compile-time consent choke point, and collaborative workspaces on sync primitives. v5 opens the ecosystem outward: shared knowledge objects, a public registry, third-party plugins, and web parity. Constraints carried forward: local-first always works offline; enrichment is community-owned but publisher PDFs are never redistributed; learner memory never leaves the device unencrypted; no CRDT library — journals merge by entry-set union; the repo remains commit-frozen until the user lifts it.

## Goals / Non-Goals

**Goals**
- Contribution workflow (propose → review → merge) with append-only provenance, reusing journal semantics.
- Registry client + minimal self-hostable registry server, enrichment-only enforced on both sides.
- JSON Schemas generated from core types; plugin surface (panels/exporters/importers) with manifest-declared, user-granted permissions.
- Web app from the same frontend, `copilot-core` on wasm32, sync as the web backbone; format 0.5.0.

**Non-Goals**
- Mobile companion (deferred to its own change).
- Foundation governance, paid hosting (documented in README, not built).
- Real-time collaborative editing (sync-cadence convergence is the model, as in v4).

## Decisions

1. **Contributions are journal diffs, not file diffs.** A proposal packages the journal entries and content-addressed files added/changed since a base revision id. Merging = entry-set union (existing merge.rs semantics) + file adds; conflicts only arise on non-journal file replacements, surfaced to the reviewer. *Alternative rejected:* git-style text diffs — fights the format's append-only design and reintroduces the CRDT problem we already solved.

2. **Provenance is one append-only `contributions/provenance.jsonl` per shared object**, holding proposal/review/merge/revert events, signed by contributor keys (ed25519; key generated per registry identity, public key in the registry profile). Reputation = pure fold over this log — deterministic, recomputable, cache-only materialization. *Alternative rejected:* server-side reputation scores — opaque, unverifiable, contradicts the local-first trust story.

3. **Registry = the S3 surface we already have + a thin HTTP index.** Layers are content-addressed encrypted-at-rest-optional tarballs in object storage; the index service maps canonical-id → layer manifests, handles identity (token auth) and server-side policy validation. Reference server is a small Rust axum binary storing to any S3-compatible bucket — deployable on the user's Coolify like MinIO. Client keeps a registry list in settings; default instance is just a URL constant. *Alternative rejected:* full database-backed service — more to run, nothing the index doesn't cover at this scale.

4. **Enrichment-only enforced structurally.** Publish assembles from an allowlist (the v4 `is_workspace_shareable` pattern, tightened: no `source.pdf`, no `pages/`, no extraction text blobs, quotes clamped with anchors). Server re-validates: rejects entries matching banned kinds/magic bytes (`%PDF`), size heuristics for text dumps. Canonical identity: normalized DOI (lowercased, prefix-stripped) or versionless arXiv id; resolution order arXiv > DOI; no identity → registry-ineligible flag.

5. **Schemas via `schemars` on the existing serde types**, emitted by a `cargo xtask schemas` into `schemas/<format-major>/…`, CI-diffed for drift. The same schemas back `research validate` (new core fn + CLI in the shell) and are the contract importers must satisfy.

6. **Plugins are WASM components run in-process with an explicit host API** (wasmtime on desktop; browser WASM on web), manifest (`plugin.json`) declaring format major, capabilities, and permissions. Panels get a message-channel UI slot (plugin renders to its own iframe/webview surface with a postMessage bridge); exporters/importers are pure functions over the read API. Network/filesystem calls route through host functions gated by recorded, revocable consents (same UX pattern as sandbox consent). *Alternative rejected:* JS plugins with dynamic import — no permission boundary; native dylibs — unsafe and platform-bound.

7. **Web = same Vite app + platform adapter + core-on-wasm32.** Introduce `src/platform/` with `invoke`-shaped interface; desktop impl wraps Tauri, web impl calls wasm-bindgen exports backed by OPFS storage. Feature-gate `copilot-core` (`native` feature: sandbox/docker/keychain/ureq; wasm uses fetch via host binding for registry/sync HTTP). Sync engine already hand-signs SigV4 — on web, R2/MinIO need CORS configured; document this. Capability matrix is a typed table in core, exported through the adapter; UI reads it, never `#ifdef`s views.

8. **Format 0.5.0**: adds `contributions/`, `registry.json` (canonical identity + pulled-layer manifests with provenance tags), `plugins/` (per-plugin consented permissions). Same-major read compat verified by the existing compat loop extended with a 0.4.0 fixture.

## Risks / Trade-offs

- [Moderation load on shared objects] → trust levels + review queues per paper; policy validation runs before human review; registry operators can delist objects (documented process).
- [WASM surface drift breaking web silently] → CI builds wasm32 target and runs the bundle round-trip test headlessly (wasm-pack test) on every change to core.
- [Plugin API too narrow to be real] → reference exporters (Anki/Obsidian/LaTeX) are built *as plugins* using only the public API; if they can't, the API grows before release.
- [CORS/browser limits against user-hosted MinIO] → setup docs include exact CORS config; sync falls back to explicit error listing the missing headers.
- [Registry hosting cost/abuse] → layers capped in size, enrichment-only validation, per-identity quotas in the reference server.
- [Ed25519 key loss] → keys are registry-identity-scoped and re-issuable; provenance verification treats old-key signatures as valid-at-time (keys have validity windows in the profile).

## Migration Plan

1. Core modules + format 0.5.0 behind same-major compat (bundles auto-upgrade on first write, as 0.2→0.3→0.4 did).
2. Schemas + validation CLI ship before the plugin host (external authors can start against the contract).
3. Registry client lands with the reference server in-repo (`registry-server/` crate); publish/pull gated on configuring a registry URL — zero behavior change for users who don't.
4. Web app ships behind a separate build target; desktop is untouched.
5. Rollback: every v5 surface is additive and feature-gated; disabling registry/plugins/web reverts to v4 behavior with bundles still readable (same-major guarantee).

**Decision made during apply (5.2/5.3):** the web target is `wasm32-wasip1`, not bare
`wasm32-unknown-unknown`. WASI provides `std::fs`, so the entire fs-based
bundle/journal/merge layer runs unchanged in browsers behind an OPFS-backed WASI shim
(e.g. browser_wasi_shim); the wasi_roundtrip test proves the contract under wasmtime with
a preopened dir. This replaces the OPFS-specific storage layer originally sketched.

## Open Questions

- Default registry instance hosting (user's Coolify vs a public instance) — deployment choice, not blocking implementation.
- Panel UI isolation on desktop: iframe-in-webview vs child webview — decide during 5.x implementation after a spike; the postMessage bridge contract is identical either way.
- Whether pulled community layers are encrypted at rest in the registry (public content — likely no; contributions to private workspaces stay in cloud-sync, not the registry).
