# Proposal: add-v5-operating-system

## Why

v1–v4 made a single desktop app that turns PDFs into rich `.research` knowledge bundles — but every user still re-derives the same enrichment for the same papers, alone. v5 (per `docs/prd/v5-operating-system.md`) turns the paper into a **shared, living knowledge object**: community contributions with review and provenance, a public registry keyed by canonical paper identity, a plugin surface so third parties can extend the format, and full web parity so none of it requires installing a desktop app. This is the "infrastructure, not app" milestone — one paper gets better forever.

## What Changes

- **Community contributions**: any enrichment in a paper's knowledge object (explanations, quizzes, implementations, visualizations, graph edits) can be proposed, reviewed PR-style, and merged with full provenance and versioning; contributor identity and reputation accrue from accepted contributions.
- **Knowledge registry**: a public registry of `.research` enrichment layers (npm/crates model). Import first pulls community enrichment for the paper's canonical identity (DOI/arXiv) instead of re-deriving; users can publish improvements back. The registry stores **enrichment only — never publisher PDFs** (licensing hard requirement).
- **Plugin API**: the `.research` format gets published JSON Schemas and a stable plugin surface — third-party panels (visualizers, domain tools), exporters (Anki, Obsidian, LaTeX), and importers (LaTeX source, HTML papers, lab notebooks) run against a versioned contract, not our internals.
- **Platform parity**: the web app becomes identical to desktop — shared React frontend, `copilot-core` compiled to WASM (with a server fallback for capabilities WASM can't host, e.g. sandboxed execution), and cloud sync as the backbone joining desktop and web sessions.
- Format version bumps to 0.5.0: contribution journals, registry metadata (canonical identity, layer manifests), and schema self-description land in the bundle.

## Capabilities

### New Capabilities

- `community-contributions`: propose/review/merge changes to a paper's knowledge object with provenance, versioning, and contributor reputation; moderation gates and trust levels.
- `knowledge-registry`: publish and pull `.research` enrichment layers against canonical paper identity (DOI/arXiv); enrichment-only content policy; convergence of the ecosystem on shared objects.
- `plugin-api`: public JSON Schemas for the `.research` format plus a versioned plugin surface for third-party panels, exporters, and importers.
- `platform-parity`: web app functionally identical to desktop via shared frontend + Rust core in WASM, with sync as the backbone and documented capability deltas (e.g. sandbox requires a runner).

### Modified Capabilities

<!-- No existing main-spec requirements change: registry pull-on-import, sync-backed
     web sessions, and contribution journals are additive layers. If implementation
     reveals a requirement change (e.g. cloud-sync manifest extensions), a delta spec
     will be added at that point. -->

## Impact

- **Rust core (`app/crates/copilot-core`)**: new `contributions/`, `registry/`, `plugin/` modules; bundle format 0.4.0 → 0.5.0 with same-major compat; WASM target (`wasm32-unknown-unknown`) build gates around fs/process/network code (sandbox, docker, keychain excluded from the WASM surface).
- **Shell (`app/src-tauri`)**: new commands for contribution workflow, registry publish/pull, plugin discovery/loading.
- **Frontend (`app/src`)**: contribution review UI, registry browse/publish panels, plugin panel host, and a web entry (same components, platform adapter instead of Tauri `invoke`).
- **Registry service**: a minimal open-source registry server (HTTP + object storage, same S3 surface as cloud-sync) — deployable by anyone; ours is one instance, not the only one.
- **Dependencies**: wasm-bindgen toolchain; JSON Schema generation (schemars); no new proprietary services.
- **Non-goals for this change**: mobile companion app (PRD lists it; deferred), foundation governance processes (documented, not implemented).
