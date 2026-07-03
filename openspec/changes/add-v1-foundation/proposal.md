# Proposal: add-v1-foundation

## Why

Research papers are written for publication, not learning; every existing tool flattens them into text chunks and forgets everything between sessions. v1 establishes the opposite foundation: a local-first desktop workspace where a paper becomes a structured `.research` knowledge bundle and every object (text, equation, figure, table, citation) is interactive, with conversations that persist. Without this foundation — especially the `.research` format and the object model — none of the later versions (learning engine, reproduction, extension, community) are possible.

## What Changes

- New Tauri desktop app (macOS/Windows/Linux) with a paper library.
- New ingestion pipeline: PDF → `.research` bundle (layout, semantic tree, extracted objects with UUIDs/bounding boxes, structured tables, LaTeX equations, citations, local embeddings, metadata) — staged, background, resumable, with per-object extraction confidence.
- New layout-faithful reader: 60 fps rendering, hoverable/clickable objects, raw-PDF fallback, in-paper search (text + semantic).
- New object interaction panel: Explain + free-form chat anchored to any object, with type-specific starters (equation variables/steps, figure interpretation, table queries, citation hover cards).
- New per-object persistent chats, notes, and bookmarks stored inside the bundle.
- New AI provider layer: BYO API key (Anthropic/OpenAI/OpenRouter) + local models (Ollama), streaming, designed no-key degraded mode, bundled pre-enriched sample paper.
- Performance budgets from `docs/architecture/platform-and-performance.md` adopted as acceptance criteria.

## Non-goals

- Knowledge graph UI, paper dashboard, quizzes/flashcards, reading/course mode, learner-model adaptivity (v2).
- Cloud sync and accounts (own change, `add-cloud-sync`, immediately after v1).
- Implementation/experiment/reproduction modes (v3); extension mode & collaboration (v4); community layer & web app (v5).
- Editing or annotating the original PDF itself — the PDF stays immutable.

## Capabilities

### New Capabilities
- `paper-library`: importing papers (file, drag-drop, arXiv URL/DOI), listing with ingestion status, opening/deleting, bundled sample paper.
- `paper-ingestion`: the staged PDF → `.research` parsing/enrichment pipeline with progress, resumability, and confidence reporting.
- `research-format`: the `.research` bundle contract — structure, object model, UUID anchoring, versioning, integrity.
- `reader-workspace`: layout-faithful, high-performance paper rendering with object highlighting, raw fallback, and in-paper search.
- `object-interaction`: click/select any object → anchored panel with Explain, type-specific actions, and citation hover cards.
- `contextual-chat`: object-anchored streaming conversations, persisted per object in the bundle.
- `ai-providers`: BYO-key and local-model configuration, routing, streaming, and degraded no-key behavior.
- `annotations`: object-anchored Markdown notes and bookmarks stored in the bundle.

### Modified Capabilities
<!-- none — greenfield -->

## Impact

- New repo code: Rust core (parsing, format, embeddings, search), Tauri shell, TypeScript frontend.
- New public contract: `.research` format v0 (JSON Schemas published).
- External dependencies: PDF parsing/rendering (pdf.js or equivalent), math extraction, local embedding model, LLM provider APIs, Ollama.
- No existing systems affected (greenfield), but the format decisions constrain every later version — `research-format` spec is the highest-stakes artifact here.
