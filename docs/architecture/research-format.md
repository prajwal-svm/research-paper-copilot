# The `.research` Format

**Status:** Draft spec (v0). This format is the foundation of the product — treat it as a public, versioned contract. The data structure is critical: everything else (UX, AI, sync, community) builds on it.

## Why a new format

PDF is *presentation*. We need *knowledge*. The PDF becomes one **view** of the data, never the source of truth.

## Design goals

1. **Structure-preserving** — nothing that exists in the paper (layout, math, figures, relationships) is flattened away.
2. **Local-first** — plain files on disk; usable offline; no server required to open.
3. **Sync-ready** — cloud sync is the roadmap priority after v1, before any web app. The format must merge cleanly (CRDT-friendly user data, append-only logs, content-addressed blobs).
4. **Diffable & community-friendly** — human-readable JSON/Markdown wherever possible so contributions (better explanations, quizzes, implementations) work like pull requests.
5. **Forward-compatible** — versioned manifest; unknown directories are preserved, not deleted.
6. **Fast** — memory-mappable binary where it matters (embeddings), lazy-loadable everywhere (see performance budgets).

## Container

A `.research` file (alias `.rpc`) is a zip-based bundle (like `.docx`/`.sketch`) that can also exist as an unpacked directory for git-based collaboration.

```
paper.research
├── metadata.json         # format version, paper identity (DOI/arXiv id), title, authors, hashes
├── original.pdf          # the presentation view (immutable, content-addressed)
├── layout.json           # page geometry: bounding boxes, reading order, column flow
├── semantic_tree.json    # structural hierarchy: sections → paragraphs → sentences → objects
├── knowledge_graph.json  # concepts, dependencies, relationships (the heart, v2+)
├── embeddings.bin        # mmap-able vectors for objects/concepts (+ embeddings.idx)
├── citations.json        # resolved references: identity, summary, relationship, why-cited
├── equations/            # per-equation: MathML/LaTeX, variables, derivation, assumptions
├── figures/              # extracted images + semantic description + source-data links
├── tables/               # extracted tables as structured data (not images)
├── glossary/             # terminology and definitions, linked to first-use locations
├── notes/                # user annotations (Markdown, anchored to object UUIDs)
├── bookmarks/            # saved locations
├── flashcards/           # auto-generated + user-created study aids (SRS state)
├── quizzes/              # assessment items + attempt history
├── learning_state/       # per-object mastery, struggle counts, style preferences
├── chats/                # conversation history, anchored to object UUIDs
├── implementations/      # code per equation/algorithm (python/, pytorch/, jax/, rust/…)
├── experiments/          # parameter sweeps, observed results, discussion threads
└── community/            # imported community contributions + provenance (v5)
```

### Separation of concerns (critical for sync & community)

| Layer | Contents | Mutability | Sync strategy |
|---|---|---|---|
| **Source** | original.pdf | Immutable | Content-addressed, dedupe |
| **Derived** | layout, semantic_tree, embeddings, citations, equations, figures, tables, glossary | Regenerable (pipeline-versioned) | Re-derive or download; never hand-edited |
| **User** | notes, bookmarks, flashcards, quizzes attempts, learning_state, chats, experiments | Private, high-value | CRDT/append-only; end-to-end encryptable |
| **Community** | explanations, animations, quiz banks, implementations | Shared | Git-like, PR-reviewed (v5) |

This split means: derived data can be regenerated when parsers improve, user data never conflicts destructively, and community data has provenance.

## The object model

The document is not pages — it is a graph of **objects**:

Paragraph, Sentence, Equation, Figure, Table, Citation, Definition, Algorithm, Experiment, Dataset, Metric, Claim, Limitation, Future Work.

Every object has:

```json
{
  "uuid": "…",
  "type": "equation",
  "bbox": { "page": 4, "x": …, "y": …, "w": …, "h": … },
  "content": { "latex": "…", "mathml": "…" },
  "semantic": "scaled dot-product attention",
  "relationships": [
    { "type": "depends_on", "target": "eq-11-uuid" },
    { "type": "referenced_by", "target": "fig-5-uuid" }
  ],
  "embedding_ref": { "offset": 123456, "dim": 1024 },
  "references": ["sec-3.2-uuid"],
  "chat_refs": ["chat-2026-07-01-uuid"],
  "learning": { "mastery": 0.7, "attempts": 3, "last_seen": "…" }
}
```

Anchoring rule: user data (notes, chats, learning state) anchors to UUIDs + content hashes, never to page/offset positions — so re-parsing the PDF with a better pipeline never orphans user data.

## Versioning & evolution

- `metadata.json` carries `format_version` (semver) and `pipeline_version` per derived artifact.
- Readers MUST open any bundle with the same major version; unknown files are preserved.
- A published JSON Schema per file enables third-party tooling — the format is the ecosystem play.

## Open questions (design phase)

1. Zip bundle vs directory as default distribution (leaning: directory in library, zip for export/share).
2. Embedding model choice & dimension pinning vs re-embedding on model change.
3. SQLite sidecar index (`index.db`) for cross-paper queries in the library — derived, rebuildable.
4. ~~CRDT library choice for user-data files (Automerge/Yjs/Loro)~~ — **resolved in the add-cloud-sync design: no CRDT library.** Append-only journals merge by entry-set union (deterministic, commutative, tested through every reader); the few non-journal user documents use last-writer-wins with preserved conflict copies. See docs/guides/sync.md and openspec/changes/add-cloud-sync/design.md.
