# Learning engine guide (v2)

v2 turns a paper from something you *read* into something you *learn*: a
knowledge graph of the paper's concepts, a learner model that remembers what
you understand, and learning surfaces built on both. Everything is
local-first and lives in the open `.research` format (or next to it in your
library folder) — no accounts, no server.

## The knowledge graph

Ingestion gained a fifth pipeline stage: **concept extraction**. It reads the
paper's semantic tree and produces `knowledge_graph.json` — concept nodes
(each linked to the paper objects that introduce it) connected by a closed
edge vocabulary: `prerequisite_of`, `depends_on`, `defined_in`, `used_by`,
`extends`, `contradicts`, `cites`.

- **With an AI provider**: extraction runs on the light tier (fast, cheap;
  quality validated on the golden corpus). The stage is versioned and
  resumable like every other stage — reopening a paper after a parser
  upgrade re-runs only what's stale.
- **Without a provider**: a heuristic graph is built from the section
  structure, capped at 0.4 confidence and clearly flagged "limited" in the
  UI. Connect a provider and reopen the paper for the full graph.
- **Corrections are yours**: delete edges, rename/delete/merge nodes in the
  graph view. Corrections are stored as append-only user data
  (`notes/graph_overrides.jsonl`) and re-applied after every re-extraction —
  never silently reverted.

Open the graph from the reader dock (the waypoints icon). Hover to inspect,
click a node to jump to its introducing object. Low-confidence nodes render
dashed.

A library-level SQLite index (`graph.db`, a rebuildable cache — never source
of truth) keeps cross-paper neighborhood queries under 5 ms.

## The learner model

Three event-sourced stores under your library's `learning_state/` directory:

| Store | Contents |
|---|---|
| `mastery.jsonl` | per-concept quiz/tutor/flashcard outcomes on an SM-2 spaced-repetition curve; retention decays with time, computed at read |
| `preferences.jsonl` | learning-style signals (visual/code/formal, verbosity) |
| `episodes.jsonl` | one-line summaries of what you struggled with or understood, distilled from your conversations by the light tier |

All three are append-only journals (crash-safe, sync-ready) folded into a
snapshot at read. **Privacy boundary**: this data never leaves your machine —
it is excluded from telemetry by construction and reaches an AI provider only
as a compact profile block (concept names/levels, never transcripts) inside
an action you explicitly invoke. Inspect and reset everything (per store or
wholesale) in Settings → Learning data.

Concepts have **global identity** across your library (`concepts.jsonl`):
"multi-head attention" mastered in one paper counts in every paper that uses
it. Auto-merging is conservative (name match, embedding-tightened); wrong
merges are one click to split, and splits are respected forever.

## How answers change

Chat context is now assembled **graph-first**: your question resolves to
concept nodes; unmastered prerequisites are expanded with grounding excerpts,
mastered ones are referenced by name only ("you know this — building on it"),
and your episodic history with that part of the paper rides along. On the
sample paper's scripted question set this cuts prompt tokens by ~61% versus
v1 assembly — measured locally (`prompt_tokens_approx` in opt-in telemetry).
Papers without a graph fall back to the v1 path automatically.

## Learning surfaces

- **Dashboard** (on paper open, skippable): honest mastery-derived figures.
  Until enough quiz signal exists, numbers are labeled *estimated* — and
  nothing here ever gates content.
- **Reading mode** (graduation-cap icon in the dock): the paper as a course.
  Lessons follow the graph's prerequisite topology; mastered lessons
  collapse to recaps but stay one click away. Lesson content is generated
  lazily (strong tier), cached in the bundle, and pre-generated for the
  sample paper. "Show me in the paper" escapes to the exact object in
  <300 ms; your lesson cursor persists.
- **Socratic tutor** (inside lessons): one question → your attempt → a
  graduated hint ladder → correction only when hints are exhausted or you
  ask. "Just tell me" always works — the loop never traps you. Outcomes feed
  mastery.
- **Quizzes & flashcards**: generated per concept, anchored to object UUID +
  content hash (a re-parsed paper flags stale items instead of serving them
  silently), graded immediately with explanations citing the paper.
  Flashcard reviews follow the spaced-repetition curve; "review due" badges
  appear in the course outline.
- **Deep-dive tabs** on equations and figures: derivation, assumptions,
  prerequisites, common mistakes — all filtered through your learner profile.
- **Cross-paper**: "Seen in *paper X*" chips appear on objects whose concepts
  you know from elsewhere; concept search answers "where did I learn X"
  across a 200-paper library in <150 ms, offline.

## Format changes

`format_version` bumped **0.1.0 → 0.2.0** (additive; same major — v1 readers
open v2 bundles and preserve the new files):

- `knowledge_graph.json` — derived, regenerable, schema-published
  ([schema](../../app/schemas/research-format/v0/knowledge_graph.schema.json))
- `glossary/lessons/`, `quizzes/`, `flashcards/` — cached generated content,
  anchored to object UUID + hash
- `notes/graph_overrides.jsonl` — user graph corrections (append-only)
- library-level (beside your bundles, not inside them): `learning_state/`,
  `concepts.jsonl`, `graph.db` (cache)
