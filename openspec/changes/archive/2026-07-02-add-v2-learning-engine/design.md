# Design — add-v2-learning-engine

## Context

v1 shipped (see `openspec/changes/add-v1-foundation/design.md`, binding here): code under `app/`; Rust core (`copilot-core`) + Tauri shell + React with shadcn/COSS/AI-Elements; PDFium is process-global and effectively single-threaded (all PDFium work behind `pdfium_lock()`); local embeddings are MiniLM (384-d, mmap'd `embeddings.bin` + index); pipeline stages are versioned and resumable, re-run on open when stale; all user data is append-only journals anchored to object UUIDs; provider layer streams with cancel and per-tier model routing. The `.research` format reserved `learning_state/`, `quizzes/`, `flashcards/` for this change. Performance budgets are CI-enforced release blockers.

## Goals / Non-Goals

**Goals:** persistent understanding — graph + memory + learning surfaces — with every store local-first and sync-ready; graph context assembly that measurably cuts prompt tokens; honest mastery-derived progress; zero-setup demo parity (sample paper ships pre-built graph/lessons/quizzes).

**Non-Goals:** cloud sync transport; code execution (v3); web app; any format-breaking change; server-side anything.

## Decisions

1. **Concept extraction is a new pipeline stage (stage 5, LLM-assisted, degradable).** Prompted extraction from section/paragraph objects → concept candidates + edges, validated against a closed edge vocabulary (`prerequisite_of`, `depends_on`, `defined_in`, `used_by`, `extends`, `contradicts`, `cites`); every node links to its introducing/using object UUIDs. No-key fallback: heading/noun-phrase heuristics + embedding clustering produce a shallow but honest graph (flagged low-confidence, same as v1 parsing degradation). Rationale: concept quality is the top risk (PRD); an LLM pass with strict schema + confidence beats brittle pure-NLP, and the stage mechanism (versioned, resumable, re-run-on-open) already handles upgrades.
2. **Graph storage: `knowledge_graph.json` in-bundle (derived, schema-published) mirrored to a per-library SQLite index** (`library/graph.db`) for cross-paper queries and O(1) neighborhoods. JSON stays the portable contract (regenerable, schema like layout/semantic_tree); SQLite is a rebuildable cache, never source of truth. Rationale: keeps the format contract file-based per architecture doc while hitting the <5 ms neighborhood budget; rusqlite is the only new heavyweight dep.
3. **Cross-paper concept identity: a library-level registry** (`concepts.jsonl`, append-only, event-sourced like all user data) mapping global concept ids → per-paper node ids, built by embedding similarity (MiniLM, cosine > threshold) + normalized-name match, with merges/splits recorded as events and user-confirmable in the graph view. Rationale: silent auto-merge of "attention" (ML) with "attention" (cognitive science) is the classic failure; auto-suggest + cheap undo beats both fully-manual and fully-automatic.
4. **Learner memory = three event-sourced stores in `learning_state/`** (`mastery.jsonl`, `preferences.jsonl`, `episodes.jsonl`) folded into a snapshot (`snapshot.json`) on load — identical crash-safety/sync pattern to notes/chats. Mastery scoring: SM-2-family spaced-repetition curve (score, interval, ease; decay computed at read time from timestamps, no background jobs). Episodic summaries are generated lazily (on thread close or Nth message) by the light-tier model. Rationale: proven algorithm, no scheduler process, all local.
5. **Graph context assembly extends (not replaces) `context.rs`:** resolve anchor → graph node(s); expand ≤2 hops with a budget-aware priority (unmastered prerequisites > definitions > dependents), attach node episodic summaries + a compact learner-profile block (mastered list as ids only, style prefs); fall back to v1 object+relationships assembly when no graph exists (ingesting, degraded, old bundle). The token budget mechanism and trimming order stay. Success metric instrumented: prompt-token count logged (local telemetry) for the ≥60% reduction check.
6. **Lesson sequencing is deterministic topology, generation is lazy.** The course outline = topological sort of the paper's concept DAG filtered by mastery (edges below confidence threshold excluded); lesson *content* (explanation, exercise, quiz items) is generated on demand per node via strong tier, cached in the bundle (`quizzes/`, `glossary/lessons/`), pre-generated for the sample paper. Mastery never *gates* content — skipped lessons remain one click away (PRD risk: unproven scores must not lock content).
7. **Socratic tutor is a constrained chat mode, not a new engine:** reading-mode lessons drive the existing streaming chat with a tutor system contract (ask → wait for the user's attempt → hint ladder → correction), state machine enforced client-side (the model is prompted per state, never free-running). Reuses per-object chat persistence; tutor turns are episodic-memory inputs.
8. **UI:** graph view uses a lightweight force/dag renderer (custom SVG/canvas over d3-force; no heavyweight graph framework) with virtualized rendering beyond ~500 nodes; dashboard/reading mode/quiz surfaces are shadcn composites; tutor uses AI-Elements conversation components per the established stack decision. Reading-mode player and dashboard are routes within the reader shell so "escape to paper" is a pane switch preserving scroll state (v1's persisted reading state extends with mode + lesson cursor).
9. **Privacy boundary unchanged and explicit:** learner model never leaves the machine except inside future E2E-encrypted sync payloads; no learner data in telemetry (event kinds stay a closed set); LLM prompts include mastery *ids/levels*, never raw quiz transcripts beyond the anchored node.

## Risks / Trade-offs

- [Graph quality] → strict edge vocabulary + per-edge confidence + visible low-confidence styling; golden-corpus extension: concept/edge assertions on the Attention paper; user edits (merge/split/delete edges) stored as user-data overrides that survive re-extraction.
- [LLM cost/latency for extraction & lessons] → extraction batched per section with light tier; lessons lazy + cached; all long calls stream with cancel (v1 infra).
- [Mastery cold start] → dashboard labels progress "estimated" until ≥N quiz signals; never hides content.
- [SQLite sidecar drift] → rebuildable from bundle JSONs at any time; version-stamped; `graph.db` is gitignored/cache-class.
- [Sync not yet landed] → all new stores follow the append-only/UUID rules so `add-cloud-sync` picks them up unchanged.

## Migration Plan

Additive only. Format minor-version bump activates reserved dirs + new derived artifact (schema published like v0 artifacts); old bundles gain the graph on next open via stale-stage re-run. Rollback = previous app version (new dirs are ignored by v1 per unknown-file preservation). No data migration required.

## Open Questions

- ~~Concept extraction model tier default (light vs strong) — measure quality/cost on the golden corpus during the spike task.~~ **Resolved (task 1.1 spike, 2026-07-02, run against Z.ai GLM on the sample paper):** default = **light tier** (`glm-4.7`-class). Result: 15 well-named concepts with one-sentence descriptions and 2–3 object anchors each, 15 valid closed-vocabulary edges, ~25 s for a 24 kchar prompt — quality clearly sufficient for a background stage at light-tier cost. Heuristic fallback confirmed usable-but-noisy (section titles + appendix junk) → stays the degraded no-key mode, capped at 0.4 confidence. Known weakness: edge *direction* is occasionally reversed → the prompt now defines direction semantics explicitly, and lesson sequencing must tolerate direction errors (cycle-break by confidence; mastery never gates, so a wrong order is an inconvenience, not a lock).
- Graph view library final call after a render spike (d3-force default; consider WASM layout only if >2k-node cross-paper views miss frame budgets).
- Whether episodic summaries should be user-visible/editable in v2 (default: visible, read-only).
