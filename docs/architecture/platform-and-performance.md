# Platform Choice & Performance Budgets

## ADR-001: Desktop shell — Tauri (over Electron)

**Status:** Proposed · **Decision owner:** core team · **Context:** local-first desktop app for macOS, Windows, Linux; performance and memory footprint are explicit product values.

| Criterion | Tauri | Electron |
|---|---|---|
| Binary size | ~10–20 MB | ~150+ MB |
| Memory at idle | Low (system webview) | High (bundled Chromium) |
| Native core language | Rust — ideal for PDF parsing, mmap embeddings, ANN search, graph index | Node/C++ addons |
| Rendering consistency | Per-OS webview variance (WebKit/WebView2) | Identical Chromium everywhere |
| Ecosystem maturity | Good, younger | Very mature |

**Decision: Tauri.**
- The compute-heavy core (PDF parsing, layout analysis, embeddings, graph queries) belongs in Rust regardless; Tauri makes that first-class.
- Perceived performance is a headline product value — a "fast, light IDE" cannot idle at 500 MB.
- Risk (webview variance) is mitigated by: strict CSS/feature baseline, PDF rendering via pdf.js/custom canvas renderer (identical across webviews), and a visual regression suite per OS.

**Consequences:** frontend is a web stack (TypeScript + React/Svelte — decided in design phase), so the future web app reuses the entire UI layer against a WASM/server build of the Rust core.

## ADR-002: Local-first, then cloud sync, then web

1. **v1 — local-first.** Everything works offline except LLM calls. User brings their own API key(s) (OpenAI/Anthropic/OpenRouter) or a local model (Ollama). No account required.
2. **v1.x–v2 — cloud sync (priority before web).** Sync the `.research` library across devices: content-addressed blobs (PDFs, figures), CRDT/append-only user data (notes, chats, learning state), derived data re-derivable or fetched. End-to-end encryption for user layer.
3. **v3+ — web app.** Identical to desktop (same frontend; Rust core compiled to WASM + thin server for heavy jobs). Sync is the backbone that makes desktop/web feel like one product.

## Performance budgets (product requirements, not aspirations)

User-facing budgets on the reference machine (4-core laptop, 8 GB RAM, integrated GPU):

| Interaction | Budget |
|---|---|
| Cold app start → interactive library | < 1.5 s |
| Open an already-ingested paper → first page rendered | < 500 ms |
| Page render while scrolling (60 fps) | < 16 ms/frame, no blank flashes |
| Click object → interaction panel visible (cached data) | < 100 ms |
| Click object → AI answer first token (network) | < 1.5 s + provider latency; streaming always |
| Object hover highlight / citation hover card (cached) | < 50 ms / < 150 ms |
| Graph node neighborhood query | < 5 ms |
| Local semantic search within a paper | < 50 ms |
| Ingestion, 10-page paper (full parse, no LLM enrichment) | < 30 s, progress shown, app fully usable meanwhile |
| Ingestion LLM enrichment | background, resumable, per-stage progress |
| Idle memory (app + one open paper) | < 300 MB |
| Bundle size on disk per paper (excl. original.pdf) | < 20 MB typical |

Enforcement: budgets are CI-tested (benchmark suite on the reference profile); a regression is a release blocker. Every OpenSpec requirement that is user-facing must cite its budget.

## Degraded modes (must be designed, not accidental)

- **No network / no API key:** reading, layout, notes, bookmarks, search, previously generated explanations all work. AI actions show a clear, kind explanation and a one-click setup path.
- **Hostile PDFs:** scanned/OCR-needed, two-column, math-heavy, malformed. Pipeline degrades per stage (raw page view is always available) and reports extraction confidence per object.
- **Low-resource machines:** embeddings/ANN lazy-built; enrichment throttled; never block reading.
