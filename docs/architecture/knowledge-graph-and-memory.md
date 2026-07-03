# Knowledge Graph & Memory

The heart of the project. This is what makes Research Paper Copilot *knowledge-centric* where every competitor is *document-centric*.

## Knowledge graph

Concept dependencies within and across papers:

```
Transformer
├── Attention
│   ├── Softmax
│   ├── Scaling
│   └── Multi-head
├── Encoder
├── Decoder
├── Positional Encoding
├── LayerNorm
├── Residual
├── Feed Forward
└── Training
```

- **Nodes:** concepts, plus links to the paper objects that introduce/use them (equations, figures, sections).
- **Edges:** `prerequisite_of`, `depends_on`, `defined_in`, `used_by`, `contradicts`, `extends`, `cites`.
- **Cross-paper (v2+):** concepts are global; "Attention" in paper A links to the same node used in paper B. This powers cross-paper linking, literature maps, and gap detection (v4).
- **Interaction:** click any node → instant lesson scoped to that node.

## Context assembly — killing context bloat

Today's tools do: every question → vector search → chunks → giant prompt → answer. Again. Again. Again.

Ours:

```
Question
  → resolve to graph nodes (the object you clicked, or entity-linked from the query)
  → expand along edges (prerequisites the user hasn't mastered, dependents, definitions)
  → attach conversation memory for those nodes
  → attach learner profile (mastery, style)
  → small, precise prompt → answer
```

Result: almost no context bloat, dramatically cheaper and faster queries, and answers grounded in *relationships*, not lexical similarity.

## Memory — the learner model

Three memory layers, all persisted in `learning_state/` and the graph:

1. **Mastery memory** — per concept/object: mastery score, attempts, quiz results, decay over time (spaced-repetition curve).
   - *"User struggled with LayerNorm 5 times"* → next explanation automatically changes approach (different analogy, more visuals, smaller steps).
   - *"Already understands cross-entropy"* → never re-explained; referenced instead.
2. **Preference memory** — learning style signals: prefers visual explanations → use diagrams first; prefers code → lead with implementations; verbosity tolerance.
3. **Episodic memory** — conversation history anchored to objects: reopening Equation 12 resumes its thread, with prior confusions summarized into the context.

## The learning engine (v2)

The AI is a **professor, not a chatbot** — Socratic loop:

```
Lesson → Ask question → Wait → Hint → Correction → Next lesson
```

- Lessons are generated per graph node, sequenced by topological order over prerequisites, filtered by mastery (skip what's known).
- Reading mode turns the paper into a course: mini explanation → diagram → animation → exercise → quiz → continue.
- Quiz outcomes update mastery memory, which updates the dashboard (progress %, equations mastered 18/27, concepts remaining…).

## Performance notes

- Graph lives in `knowledge_graph.json`, mirrored into an in-memory index + SQLite sidecar for O(1) neighborhood queries; target <5 ms node-neighborhood retrieval.
- Embeddings mmap'd (`embeddings.bin`), ANN index built lazily per paper; entity-linking a query must complete <50 ms locally.
- Memory updates are append-only events, folded into snapshots — cheap writes, crash-safe, sync-friendly.
