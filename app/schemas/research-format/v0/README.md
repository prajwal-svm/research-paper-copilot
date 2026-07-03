# `.research` format — v0 (experimental)

The `.research` bundle is the public, versioned contract at the heart of Research Paper Copilot:
a paper is not a PDF, it is a structured knowledge object. The PDF is one view of it.

**Status: v0 / experimental.** The format is semver-versioned (`format_version` in
`metadata.json`) and stays experimental until v2 of the product ships. Within v0, breaking
changes may occur between minor versions; any breaking change ships with migration tooling.

## Bundle layout

A bundle is a **directory** in the library (zip only on export):

```
paper-title.research/
├── metadata.json         # manifest: versions, paper metadata, content hashes, pipeline provenance
├── original.pdf          # immutable, content-addressed — never edited
├── layout.json           # page geometry, blocks, reading order        (derived, stage 1)
├── semantic_tree.json    # extracted objects + document hierarchy      (derived, stage 2)
├── citations.json        # bibliography + in-text mentions             (derived, stage 3)
├── embeddings.bin        # local embedding vectors, mmap-friendly      (derived, stage 4)
├── knowledge_graph.json  # per-paper concept graph                     (derived, stage 5)
├── equations/            # per-equation artifacts (LaTeX, regions)     (derived, stage 3)
├── figures/              # extracted figure images + captions          (derived, stage 3)
├── tables/               # structured table data                       (derived, stage 3)
├── glossary/             # term definitions / cached lessons           (derived, lazy)
├── implementations/      # generated+edited code per object/language   (user-editable, v3)
├── experiments/          # parameterized runs + results + discussion   (user data, v3)
├── reproduction/         # repo ref, env plan, code map, run log, report (v3)
├── consents.jsonl        # sandbox consent journal (append-only)       (user data, v3)
├── research/             # weaknesses, hypothesis cards, outline, draft (v4)
├── notes/                # user data — append-only, object-anchored
│                         #   incl. graph_overrides.jsonl (concept-graph corrections)
│                         #   and code_map_overrides.jsonl (code↔paper corrections)
├── bookmarks/            # user data — append-only, object-anchored
└── chats/                # user data — append-only JSONL per object UUID
```

Reserved for later product versions: (all previously reserved directories are
now active as of format 0.3.0). Library-level (beside bundles, not inside):
`learning_state/`, `concepts.jsonl`, `graph.db` (cache), `repos/` (clone cache),
`reviews/`, `gaps/`, `workspaces/` (v4).

## Invariants

1. **The PDF is immutable.** `metadata.json` records its content hash; nothing ever rewrites it.
2. **Derived data is regenerable.** Everything produced by the pipeline can be rebuilt from
   `original.pdf`; each derived file records the `pipeline_version` that produced it.
3. **User data anchors to object UUIDs + content hashes, never page offsets.** Re-parsing
   re-attaches user data by UUID/hash; unmatched anchors are surfaced, never dropped silently.
4. **User data is append-only** (JSONL journals), so a crash mid-write never corrupts committed
   entries and cloud sync can merge without destructive conflicts.
5. **Unknown files are preserved.** An app that doesn't understand a directory leaves it intact.
6. **Compatibility:** readers MUST open any bundle of the same major `format_version` and MUST
   refuse newer majors with a non-destructive "update required" message.

## Schemas

| File | Validates |
|---|---|
| [`common.schema.json`](common.schema.json) | shared definitions (uuid, semver, hash, bbox, confidence, stage record) |
| [`metadata.schema.json`](metadata.schema.json) | `metadata.json` |
| [`layout.schema.json`](layout.schema.json) | `layout.json` |
| [`objects.schema.json`](objects.schema.json) | the object model (used by `semantic_tree.json` and per-type artifacts) |
| [`semantic_tree.schema.json`](semantic_tree.schema.json) | `semantic_tree.json` |
| [`citations.schema.json`](citations.schema.json) | `citations.json` |
| [`knowledge_graph.schema.json`](knowledge_graph.schema.json) | `knowledge_graph.json` |

Example documents live in [`examples/`](examples/) and are validated against the schemas in CI
(`npm run validate:schemas`).
