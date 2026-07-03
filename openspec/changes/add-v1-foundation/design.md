# Design — add-v1-foundation

## Context

Greenfield. Product foundations are defined in `PRD.md`, `docs/vision.md`, `docs/architecture/*`, `docs/ux-principles.md`. Binding constraints: local-first desktop (mac/win/linux), performance budgets in `docs/architecture/platform-and-performance.md` are acceptance criteria, `.research` format is a public versioned contract, cloud sync comes next so every v1 storage decision must be sync-ready.

## Goals / Non-Goals

**Goals:**
- Prove Paper = Knowledge end-to-end: PDF in → structured, interactive, persistent workspace out.
- Nail perceived performance and the object-interaction feel (the product's identity).
- Freeze `.research` v0 well enough that v2–v5 build on it without breaking users.

**Non-Goals:**
- Knowledge graph UI, learner model, sync, web app, code execution (later changes).
- Perfect parsing of all PDFs — target corpus is arXiv-style ML papers; everything else degrades kindly.

## Decisions

1. **Tauri over Electron** — see ADR-001. Rust core is needed anyway for parsing/embeddings/search; footprint is a product value. Alternative (Electron) rejected on memory/size; rendering-variance risk mitigated with pdf.js-based canvas rendering + per-OS visual regression tests.
2. **Rendering: pdf.js canvas + overlay DOM layer for objects.** The PDF renders as canvas (identical across webviews); extracted objects render as positioned transparent overlays (hover/click targets) driven by `layout.json`. Alternative (rebuild the document as HTML) rejected for v1 — layout fidelity risk; it can become a later "reader view".
3. **Pipeline as staged, resumable jobs:** `layout → objects → tables/equations/citations → embeddings → (optional) LLM enrichment`. Each stage writes its artifact + `pipeline_version` + per-object confidence; UI is usable after stage 1. Rationale: hostile PDFs and interruptions are the norm; also enables re-running single stages when parsers improve.
4. **Bundle as directory in the library, zip on export.** Sync- and git-friendly; zip only for share. `metadata.json` carries format/pipeline versions and content hashes.
5. **Anchoring rule:** all user data (chats, notes, bookmarks) references object UUIDs + content hashes, never page offsets — re-parsing never orphans user data. Chats/notes are append-only JSONL (CRDT-upgradeable for sync).
6. **AI layer as provider-agnostic trait** (Anthropic/OpenAI/OpenRouter/Ollama) with per-action routing (cheap model for hover summaries, strong model for derivations), streaming mandatory, and a "no-key" mode that serves pre-generated enrichment (bundled sample) and clear setup CTAs.
7. **Local embeddings by default** (small multilingual model via ONNX/candle) so search works offline and without keys; embeddings mmap'd with an index sidecar.
8. **Frontend: TypeScript + React** (team familiarity, ecosystem), state anchored to object UUIDs; virtualized page list to hold 60 fps.
9. **Repo layout (user decision, 2026-07-01):** all code lives under `app/` (Cargo workspace, frontend, src-tauri, schemas, scripts, perf, vendor); repo root keeps only docs, PRD, and openspec. CI runs with `working-directory: app`.
10. **UI stack (user decision, 2026-07-01):** shadcn/ui (nova preset, radix base, Tailwind v4) as the base system; the COSS UI registry (`@coss` → coss.com/ui) for additional app chrome; AI SDK Elements (`@ai-elements` → elements.ai-sdk.dev) for AI chat surfaces (tasks 5.3/6.x — Conversation, Message, PromptInput, Response). All three share the same shadcn token system so the design stays uniform. Registries configured in `app/components.json`.
11. **PDF parsing engine: PDFium via `pdfium-render`** (decided during implementation of task 2.1). Chrome's PDF engine — battle-tested on hostile PDFs, Apache-2.0 (MuPDF rejected on AGPL), prebuilt binaries for all three OSes (`scripts/fetch-pdfium.sh` → `vendor/pdfium/`), positioned text now and image extraction for stage 3. Pure-Rust parsers (`lopdf`, `pdf-extract`) rejected: they can't plausibly hit the ≥95% golden-corpus bar. Constraints learned: one live binding per process (exposed as a process-global in `copilot_core::layout::pdfium()`), and PDFium is effectively single-threaded — the pipeline must funnel all PDFium work through one worker thread; the `thread_safe` cargo feature alone is not sufficient for interleaved document operations. **`pdfium_lock()` is a non-reentrant `std::sync::Mutex`: never hold it across a call into `pipeline::run`/`import_pdf` (they take it internally) — that deadlocks silently.**

## Risks / Trade-offs

- [Parsing quality on math/2-column PDFs] → staged confidence flags, raw-view fallback, golden-corpus regression suite; parsing owns the largest engineering share.
- [Webview divergence] → canvas rendering, feature baseline, per-OS visual tests in CI.
- [BYO-key onboarding friction] → pre-enriched sample paper delivers first wow with zero setup; Ollama path documented.
- [Format regret (v0 locked too early)] → format marked v0/experimental until v2 ships; migration tooling required for any breaking change; unknown-file preservation from day one.
- [Perf budgets vs feature pressure] → budgets in CI as release blockers; features lose.

## Migration Plan

Greenfield — no migration. Rollback = previous app version; bundles carry versions so older apps refuse newer majors gracefully.

## Open Questions

- ~~Equation extraction approach (Mathpix-class model vs open-source LaTeX-OCR vs hybrid) — spike task.~~ **Resolved (task 2.3): hybrid.** Deterministic display-equation *region detection* in Rust (math-symbol density + "(n)" margin markers; precision over recall) with the raw region always recoverable; LaTeX conversion deferred to the optional LLM-enrichment stage via the user's provider, cached per equation in `equations/<uuid>.json` (`latex: null` until enriched, `latex_source` recorded). Mathpix rejected (proprietary, network-bound pipeline stage); bundled LaTeX-OCR rejected for v1 (100–400 MB weights, slow CPU inference); a local model can be added later as a stage upgrade via `pipeline_version`.
- ~~Exact local embedding model + dimensions (pin in `metadata.json`).~~ **Resolved (task 2.6):** `sentence-transformers/all-MiniLM-L6-v2`, 384 dims, f32, mean pooling + L2 norm, via candle (pinned ≤0.9 — 0.10+ needs unstable `stdarch_neon_f16` on stable rustc). Pinned in `embeddings_index.json` and `metadata.json.embedding_model`. Model downloads to the shared HF cache on first ingestion (~90 MB); fully offline afterwards. 336 vectors for the 15-page sample paper in ~12 s CPU; search ~150 µs.
- Svelte vs React final call after rendering spike (React default).
