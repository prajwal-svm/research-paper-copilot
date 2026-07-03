# Tasks — add-v1-foundation

Ordered so a usable vertical slice (import → read → click → explain) ships as early as possible. This change is in the planning stage; tasks are the execution contract for when build begins.

## 1. Project scaffold & contracts

- [x] 1.1 Scaffold Tauri app (Rust core crate + TypeScript/React frontend), CI for mac/win/linux builds
- [x] 1.2 Define `.research` v0 JSON Schemas (metadata, layout, semantic_tree, objects, citations) and publish in-repo
- [x] 1.3 Implement bundle read/write layer (directory bundles, content hashing, unknown-file preservation, append-only journals)
- [x] 1.4 Set up performance benchmark suite in CI with reference-machine profile and the budget table as assertions

## 2. Ingestion pipeline (vertical slice first)

- [x] 2.1 Stage 1: PDF layout analysis → layout.json (pages, bounding boxes, reading order)
- [x] 2.2 Stage 2: object extraction → semantic_tree.json (sections, paragraphs, sentences) with confidence scores
- [x] 2.3 Spike: equation extraction approach (LaTeX-OCR vs hybrid) → decision + implementation (equations/)
- [x] 2.4 Table extraction to structured data (tables/); figure extraction with captions (figures/)
- [x] 2.5 Citation parsing + resolution (arXiv/Crossref) → citations.json with graceful offline behavior
- [x] 2.6 Stage 4: local embedding model integration (ONNX/candle), embeddings.bin + index, mmap loading
- [x] 2.7 Job runner: staged, background, resumable, per-stage progress events; golden-corpus regression tests (≥95% usable on arXiv ML set)

## 3. Library

- [x] 3.1 Library UI: list, import (file/drag/arXiv URL/DOI), status, open/delete/reveal; cold start < 1.5 s (budget enforced in 8.1 harness)
- [x] 3.2 Bundled pre-enriched sample paper ("Attention Is All You Need") and first-run flow (time-to-first-wow < 2 min)

## 4. Reader

- [x] 4.1 Canvas PDF rendering with virtualization: 60 fps scroll, < 500 ms open, < 300 MB idle (budgets enforced in 8.1 harness)
- [x] 4.2 Interactive object overlay: hover highlight < 50 ms, click-to-select, ad-hoc text selection objects
- [x] 4.3 Raw fallback view + low-confidence flagging; per-OS visual regression tests (native path; webview screenshots ride on 8.1 harness)
- [x] 4.4 In-paper exact + semantic search (< 50 ms, offline; measured 21 ms incl. query embedding)
- [x] 4.5 Persisted reading state (position, panels) restored on reopen

## 5. AI layer

- [x] 5.1 Provider abstraction (Anthropic/OpenAI/OpenRouter/Ollama), keychain storage, validation flow
- [x] 5.2 Per-action model routing + token-budgeted structured context assembly (object + relationships + thread)
- [x] 5.3 Streaming infrastructure with paper-object citations rendered as reader links
- [x] 5.4 Designed no-key/offline degraded modes for every AI entry point (NoProviderNotice + pre-generated enrichment; panel entries compose these)

## 6. Object interaction & chat

- [x] 6.1 Anchored interaction panel framework (< 100 ms cached open, non-blocking, tab-extensible for v2)
- [x] 6.2 Type-specific actions: equation (variables, step-by-step, intuition), figure (explain/describe/interpret), table (summarize + data-grounded queries)
- [x] 6.3 Citation hover cards (< 150 ms cached) + "import as paper"
- [x] 6.4 Per-object persistent chats (append-only JSONL, resume-on-open, honest failure handling)

## 7. Annotations

- [x] 7.1 Object-anchored Markdown notes (inline editor < 100 ms) + bookmarks panel
- [x] 7.2 Markdown export of notes/bookmarks

## 8. Quality gates & release

- [ ] 8.1 All performance budgets green in CI on all three OSes — **all enforceable budgets green locally (incl. new ingestion benchmark, 157 ms/10 s); CI workflow + golden-candidate artifacts ready; blocked on git remote + first Actions run**
- [x] 8.2 Hostile-PDF corpus pass (scanned, 2-column, math-heavy, malformed) — kind degradation verified (hostile_pdfs.rs + golden corpus covers 2-column/math-heavy)
- [x] 8.3 Crash-free session target instrumentation (opt-in, content-free telemetry; local-only in v1)
- [x] 8.4 Satisfaction instrumentation: time-to-first-wow, object interactions/session, per-answer thumbs
- [ ] 8.5 Docs: README with mission statement, format spec docs, provider setup guides; v1 release — **docs done; release blocked on GitHub remote/user decision**
