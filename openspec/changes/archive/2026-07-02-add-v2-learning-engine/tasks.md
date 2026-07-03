# Tasks — add-v2-learning-engine

Ordered for an early vertical slice: graph → memory → one lesson loop on the sample paper, then breadth. Budgets from `docs/architecture/platform-and-performance.md` are acceptance criteria; every AI surface needs its designed no-key state.

## 1. Knowledge graph core

- [x] 1.1 Spike: concept extraction quality — LLM-assisted (strict edge vocabulary, per-node/edge confidence, object-UUID linking) vs heuristic fallback on the golden corpus; pick tier defaults, record decision in design.md
- [x] 1.2 `knowledge_graph.json` schema (nodes, closed edge vocabulary, confidences, object links) published with the format schemas + CI-validated examples
- [x] 1.3 Pipeline stage 5: concept extraction (versioned, resumable, degradable to heuristic graph flagged low-confidence); golden-corpus assertions for the sample paper's core concepts
- [x] 1.4 Library-level graph index (SQLite sidecar, rebuildable from bundle JSON): <5 ms neighborhood queries enforced in the perf suite
- [x] 1.5 User graph corrections (delete edge, merge/split/rename node) as append-only overrides applied over extraction output; survive re-extraction (test)

## 2. Learner memory

- [x] 2.1 `learning_state/` stores: mastery / preferences / episodes journals + snapshot folding; SM-2-family decay computed at read; crash-safety tests
- [x] 2.2 Episodic summarizer: per-object confusion summaries generated lazily via light tier, cached, no-key skip
- [x] 2.3 Learner-profile block for prompts (compact ids/levels/style) + settings surface to inspect and reset learning data (per-store and wholesale)

## 3. Graph context assembly (contextual-chat delta)

- [x] 3.1 Entity-linking a query to graph nodes (<50 ms local, embeddings + name match)
- [x] 3.2 Graph-first assembly in context.rs: bounded edge expansion (unmastered prereqs > definitions > dependents) + episodic summary + profile block; v1 fallback path when no graph; budget/trimming tests
- [x] 3.3 Prompt-token instrumentation (local) and ≥60% reduction check vs v1 baseline on the sample paper's scripted question set

## 4. Cross-paper linking

- [x] 4.1 Global concept registry (append-only events; conservative auto-merge via MiniLM similarity + name match; user-confirmable merge/split)
- [x] 4.2 Paper backlinks as user data (citation-derived suggestions + manual links; listable both directions)
- [x] 4.3 "Seen in paper X" surfacing in lessons/panels with cross-paper navigation; shared global mastery
- [x] 4.4 Library-wide concept search ("where did I learn X") <150 ms @ 200 papers, offline

## 5. Learning surfaces

- [x] 5.1 Graph view: custom SVG/canvas force-DAG (d3-force), hover→reader highlight, click→lesson <300 ms, low-confidence styling, 60 fps to 500 nodes (render spike first if budget missed)
- [x] 5.2 Paper dashboard: mastery-derived figures (<500 ms on open), estimated-label cold start, continue-CTA restoring exact position, skip preference
- [x] 5.3 Reading mode player: topological lesson sequencing (mastery collapses, never gates), lazy generation with bundle caching, escape-to-paper round-trip <300 ms, persisted lesson cursor
- [x] 5.4 Socratic tutor state machine over streaming chat (question→wait→hint ladder→correction), per-state prompting, outcomes → mastery/episodic events
- [x] 5.5 Quizzes & flashcards: generation anchored to UUID+hash with stale-anchor flagging, immediate explained grading, spaced-repetition due queue (paper + library), one data path into mastery/dashboard
- [x] 5.6 Equation/figure deep-dive tabs in the v1 panel (derivation, assumptions, prerequisites, quiz, related, common mistakes; render-only sliders/visualization), learner-model default entry point
- [x] 5.7 Notes→graph-node linking

## 6. Sample paper & degraded modes

- [x] 6.1 Sample bundle rebuilt with pre-generated graph, lessons, quizzes, flashcards (zero-setup first wow for v2 surfaces)
- [x] 6.2 Designed no-key states audited across every new AI entry point (extraction, lessons, tutor, quiz generation, episodic summaries)

## 7. Quality gates & release

- [x] 7.1 New perf budgets in the CI suite: graph neighborhood <5 ms, entity-linking <50 ms, dashboard <500 ms, lesson-open skeleton <300 ms; all v1 budgets still green
- [x] 7.2 Golden-corpus extension: concept/edge quality assertions; format schema validation for knowledge_graph.json; format minor-version bump + unknown-file compatibility test against v1 app
- [x] 7.3 Metrics instrumentation (opt-in, content-free): quiz participation, repeat-explanation rate, context-token reduction, explanation-helped rating
- [ ] 7.4 Docs: format spec update ✅, learning-engine guide ✅ (docs/guides/learning-engine.md), README refresh ✅; v2 release — **blocked on user**: repo has no commits yet (v1 "don't commit yet" still in force) → commit, tag, and publish alongside the v1 release steps (8.1/8.5 of add-v1-foundation)
