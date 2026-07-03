# Changelog

## v0.1.0 — The Foundation (unreleased)

The first release: a local-first desktop workspace where a paper becomes a
structured `.research` knowledge bundle and every object is interactive.

### Highlights

- **`.research` format v0 (experimental)** — public, versioned bundle
  contract: immutable content-addressed PDF, regenerable derived data,
  append-only user data anchored to object UUIDs + content hashes. JSON
  Schemas published in-repo with CI-validated examples.
- **Ingestion pipeline** — staged, background, resumable: PDF layout
  analysis (PDFium), object extraction (sections/paragraphs/sentences/
  equations/figures/tables with deterministic UUIDs and confidence scores),
  figure rendering + table grid extraction, citation parsing with
  arXiv/Crossref resolution, and local embeddings (MiniLM via candle) for
  offline semantic search. Golden corpus: 6/6 arXiv ML papers usable.
- **Reader** — virtualized canvas rendering, interactive object overlay,
  ad-hoc text selections, raw-view fallback with plain-language degradation,
  in-paper exact + semantic search (~20 ms), persisted reading state.
- **Object interactions** — anchored panel with Explain and type-specific
  actions; equation variables/step-by-step/intuition; figure
  describe/interpret; table queries answered from extracted data; citation
  hover cards with "import as paper".
- **Persistent object chats** — append-only JSONL per object, resume on
  reopen, partial answers preserved and marked on failure.
- **Notes & bookmarks** — object-anchored, event-sourced, Markdown export
  grouped by section.
- **AI providers** — Anthropic / OpenAI / OpenRouter / Ollama behind one
  streaming interface; keys in the OS keychain; per-action model routing;
  token-budgeted relationship-aware context assembly; designed no-key mode
  with a bundled pre-enriched sample paper ("Attention Is All You Need").
- **Quality gates** — CI-enforced performance budgets, per-OS render
  regression goldens, hostile-PDF corpus (kind degradation, never a panic),
  opt-in content-free local telemetry (crash-free sessions,
  time-to-first-wow, per-answer thumbs).

### Known limitations (v0)

- Equation LaTeX conversion is deferred to LLM enrichment (regions always
  recoverable); table grid extraction is best-effort (2/4 tables on the
  sample paper) with honest confidence flags.
- Windows/Linux render goldens are generated on first CI run and need a
  one-time review + commit.
- Telemetry is local-only; nothing is transmitted anywhere.
