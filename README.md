# Research Paper Copilot

> An open-source platform that transforms static scientific papers into
> interactive, explorable knowledge. Our goal is to make every research paper
> understandable, reproducible, and extendable by anyone.

Every tool today treats **Paper = PDF**: flatten, chunk, vector-search,
forget. Research Paper Copilot treats **Paper = Knowledge** — a structured,
interactive, persistent knowledge object where the PDF is just one view.
Import a paper and every equation, figure, table, and citation becomes
something you can click, question, and annotate — and the app remembers all
of it, forever, in an open format you own.

## What works today

### v5 — the operating system

- **Community contributions**: any paper's knowledge object improves
  PR-style — proposals as union-mergeable change sets, review with full
  diffs, ed25519-signed append-only provenance, reputation as a
  deterministic fold over the public record. One paper gets better forever.
- **Knowledge registry**: publish and pull `.research` enrichment layers
  keyed by canonical DOI/arXiv identity (self-hostable reference server
  over any S3-compatible bucket). Enrichment only — publisher PDFs never
  enter the registry, enforced client- and server-side. Pulls verify
  content addresses and never overwrite your own artifacts.
- **Plugin API**: published JSON Schemas (generated from the core types)
  plus a WASM plugin surface with user-granted, revocable permissions.
  Reference exporters (Anki/Obsidian/LaTeX) and a LaTeX importer ship as
  real plugins built on only the public ABI.
- **Web parity**: same frontend behind a platform adapter; the core
  compiles to wasm32-wasip1 (bundle round-trip proven under WASI), sync is
  the backbone with browser-side key derivation, and an explicit
  capability matrix drives degradation — nothing silently disappears.

Guides: [community](docs/guides/community.md) ·
[registry](docs/guides/registry.md) · [plugins](docs/guides/plugins.md) ·
[web](docs/guides/web.md) · workspaces (v4 §7) live in the Research view.

### Cloud sync

- **Your library on every device, no account, no server of ours**: bring your
  own storage — a free Cloudflare R2 bucket (recommended), your own MinIO
  (e.g. on Coolify), or a plain folder (iCloud Drive/Dropbox/Syncthing).
- **End-to-end encrypted**: everything leaves the machine as
  XChaCha20-Poly1305 ciphertext under an Argon2id passphrase-derived key with
  opaque blob names — the remote is assumed hostile and learns nothing.
- **Merges that can't destroy data**: append-only journals union
  deterministically (the format's no-CRDT bet, paid off); documents conflict
  into preserved copies; deletions tombstone into a local trash; remote GC is
  explicit-only; interrupted syncs are invisible to other devices and resume
  cleanly.

See the [sync guide](docs/guides/sync.md).

### v4 — the researcher workspace

- **Extension mode**: weaknesses (grounded in the paper's own passages, or
  dropped) → editable hypothesis cards → novelty checks whose verdicts are
  unrepresentable without evidence → outlines and LaTeX drafts that can only
  cite a fixed, resolved bibliography (invented citations stripped and
  counted; AI-drafted text provenance-marked in the source).
- **Living literature reviews**: graph-structured synthesis over your library
  (thematic sections, comparison tables, lineages), with machine output and
  your edits kept in separate files — a refresh can never eat your writing.
- **Gap reports**: computed deterministically from the cross-paper graph
  (untried combinations, unresolved contradictions, stale assumptions), only
  then narrated by AI — and honestly refused when the library is too small.
- **Collaboration data models**: sync-ready workspace/thread/assignment
  journals with learner memory unshareable by construction (features arrive
  with the sync layer).

See the [researcher workspace guide](docs/guides/researcher-workspace.md).

### v3 — the hacker workspace

- **Implementation mode**: every equation becomes runnable code — Python,
  PyTorch, TensorFlow, JAX, Rust — generated on demand, editable in place,
  stored in the bundle, and verified by generated checks that feed honest
  dashboard progress.
- **Experiment mode**: declare parameters, predict the outcome, run in the
  sandbox, watch metrics chart across runs, and discuss the concrete numbers
  with the AI. All runs persist append-only and compare side by side.
- **Reproduction mode**: link the paper's GitHub repo and step through
  clone → environment setup → code↔paper mapping → verification run →
  an honest report (matched/diverged, by how much, full provenance).
  Gate-tested end-to-end against a curated corpus.
- **Code understanding**: a repo browser with line-level code↔paper links,
  both directions — "where is Equation 12 in the code?" has a clickable answer.
- **The sandbox**: every run is containerized, explicitly consented,
  offline by default, resource-capped, and stoppable instantly. No generated
  or cloned code ever executes on the host. (Requires Docker or Podman;
  everything else works without one.)

See the [hacker workspace guide](docs/guides/hacker-workspace.md).

### v2 — the learning engine

- **Knowledge graph per paper**: a fifth pipeline stage extracts concepts
  and prerequisite/dependency edges (LLM-assisted with an honest heuristic
  fallback when no provider is configured). Explore it in the reader's graph
  view; correct it (rename/merge/delete) with edits that survive
  re-extraction.
- **Learner memory**: quiz, tutor, and flashcard outcomes feed a local
  spaced-repetition mastery model; your conversations distill into episodic
  memory. Mastered concepts are referenced, not re-explained; repeated
  struggles change the explanation approach. All local, inspectable, and
  deletable in Settings.
- **Graph-first chat context**: prompts assemble from the concept
  neighborhood + your learner profile instead of relationship dumps — ~61%
  fewer prompt tokens on the sample paper's question set, measured locally.
- **Reading mode**: the paper as a course — lessons in prerequisite order,
  a Socratic tutor (hints before answers, never traps you), quizzes and
  flashcards anchored to the paper's objects, honest mastery-based progress
  that never gates content.
- **Cross-paper linking**: concepts share identity across your library
  ("seen in paper X", shared mastery, "where did I learn X" search), plus
  citation-derived and manual paper backlinks.

See the [learning engine guide](docs/guides/learning-engine.md).

### v1 — the foundation

- **Import** from a local PDF, drag-and-drop, an arXiv URL/id, or a DOI.
- **Ingestion pipeline** (all local): layout analysis → object extraction
  (sections, paragraphs, equations, figures, tables) → citation parsing +
  arXiv/Crossref resolution → local embeddings for offline semantic search.
  Staged, resumable, and kind to hostile PDFs — a scanned or malformed file
  degrades to a raw view with a plain-language explanation, never an error.
- **Layout-true reader**: virtualized canvas rendering (pdf.js), an
  interactive object overlay, native text selection, raw-view fallback, and
  in-paper exact + semantic search (~20 ms, fully offline).
- **Object interactions**: click anything → anchored panel with Explain and
  type-specific actions (equation variables/steps/intuition, figure
  describe/interpret, table queries answered from the extracted data).
  Citation markers show hover cards with resolved metadata and
  one-click "import as paper".
- **Persistent, object-anchored conversations** stored inside the bundle;
  reopen an equation weeks later and your thread is still there.
- **Notes & bookmarks** anchored to objects, with Markdown export.
- **Bring your own AI**: Anthropic, OpenAI, OpenRouter, or local models via
  Ollama. Keys live in the OS keychain. With no key configured, everything
  non-AI still works — including the bundled, pre-enriched sample paper
  ("Attention Is All You Need"), so the first wow needs zero setup.

## The `.research` format

The heart of the project is a public, versioned bundle format: a directory
per paper holding the immutable original PDF, regenerable derived data
(layout, semantic tree, embeddings, citations), and append-only user data
(chats, notes, bookmarks) anchored to stable object UUIDs — never page
offsets. Re-parsing never orphans your annotations, unknown files are always
preserved, and everything is designed to sync cleanly later.

Format spec and JSON Schemas: [`app/schemas/research-format/v0/`](app/schemas/research-format/v0/README.md)

## Getting started (development)

Prerequisites: Rust (stable), Node 22+, and the platform WebView (WebView2 on
Windows, WebKitGTK on Linux).

```bash
cd app
./scripts/fetch-pdfium.sh   # prebuilt PDFium for your OS
npm install
npm run tauri dev
```

Tests and quality gates:

```bash
cd app
cargo test --workspace                                    # unit + integration
cargo test --release -p copilot-core --test perf_budgets  # perf budgets (CI blocker)
cargo test -p copilot-core --test golden_corpus -- --ignored  # arXiv corpus (downloads)
npm run validate:schemas                                  # format schemas
```

## AI provider setup

Open **Settings** (gear icon in the library):

| Provider | What you need |
|---|---|
| Anthropic | API key from console.anthropic.com |
| OpenAI | API key from platform.openai.com |
| OpenRouter | API key from openrouter.ai |
| Z.ai GLM Coding Plan | Z.ai API key — built-in preset (Anthropic-compatible endpoint, GLM-5.2 strong tier, `glm-4.7` light, optional 1M context); see [docs.z.ai](https://docs.z.ai/devpack/latest-model) |
| Ollama | Just install and run [Ollama](https://ollama.com) — detected automatically, no key |

Advanced: the Anthropic provider accepts a custom base URL, so any
Anthropic-compatible endpoint works. The host your paper content is sent to
is always shown in settings and while streaming.

Keys are validated with a test call and stored in your OS keychain. Paper
content is sent only to the provider you configured, only when you invoke an
AI action. Reading, search, notes, and bookmarks never touch the network.

## Project layout

```
app/        all code: Rust core (crates/copilot-core), Tauri shell, React UI, schemas
docs/       vision, architecture decisions, UX principles, roadmap PRDs (v1–v5)
openspec/   spec-driven change management (proposals, specs, tasks)
PRD.md      the master index
```

## Roadmap

v1 (foundation) → v2 (learning engine) → v3 (hacker workspace) →
v4 (researcher workspace, incl. collaborative workspaces) → cloud sync →
**v5 (community layer, registry, plugins, web — this)**. Next: mobile
companion, cross-field bridges. See [PRD.md](PRD.md) and
[docs/vision.md](docs/vision.md).

## License

MIT OR Apache-2.0
