# Proposal: add-v2-learning-engine

## Why

v1 made papers interactive but stateless-per-object: the app answers questions well, yet knows nothing about the *learner* and reassembles context from scratch on every query. v2 delivers the product's core bet — **persistent understanding**: a knowledge graph over concepts (within and across papers), a learner model that remembers mastery/preferences/confusions forever, and the learning surfaces built on them (dashboard, course-like reading mode, Socratic tutor, quizzes with spaced repetition). This is the moat feature; every requirement here leans on v1's object model and `.research` format, whose `flashcards/`, `quizzes/`, and `learning_state/` directories were reserved for exactly this.

## What Changes

- New enrichment stage: concept extraction → per-paper **knowledge graph** (`knowledge_graph.json` + SQLite/in-memory index sidecar; <5 ms neighborhood queries), with an interactive graph view where every node is one click from a lesson.
- **Cross-paper concept linking**: concepts become global across the library (embedding + name matching with user-confirmable merges); "seen this in paper X" surfaces, and paper-to-paper backlinks are stored as user data in bundles.
- **Learner memory** in `learning_state/`: mastery per concept/object (attempts, quiz outcomes, spaced-repetition decay), preference memory (visual/code/formal style), episodic memory (per-object confusion summaries) — all append-only events folded into snapshots, local-first, sync-ready.
- **Graph-based context assembly** replaces per-query retrieval in contextual chat: question → graph nodes → edge expansion (unmastered prerequisites, dependents, definitions) → node memory + learner profile → small precise prompt. Target: ≥60% context-token reduction vs the v1 baseline.
- **Paper dashboard** on open: honest progress (mastery-derived, never scroll depth), equations mastered x/y, concepts remaining, quiz score, continue-where-you-left-off CTA.
- **Reading mode**: prerequisite-topology-sequenced lessons (mini explanation → diagram → exercise → quiz → continue), mastery-filtered, escapable to the raw paper at the same location at any time.
- **Socratic tutor** loop (lesson → question → wait → hint → correction → next) as the reading-mode teacher — a professor, not a chatbot.
- **Quizzes & flashcards**: auto-generated per object/concept, spaced-repetition scheduling, results feed mastery memory and the dashboard.
- Notes link to graph nodes; **library-wide semantic search** ("where did I learn about residual connections?").

## Non-goals

- Cloud sync itself (`add-cloud-sync` is its own change; every v2 store is designed sync-ready but this change works fully local). The roadmap sequences sync before v2 — if sync hasn't landed, v2 proceeds local-only without blocking.
- Full equation deep-dive extras that need code execution (implementations, run-a-numerical-example) — those belong to v3's hacker workspace; v2 ships the remaining deep-dive tabs (derivation, assumptions, prerequisites, quiz, related, common mistakes, sliders/visualization where render-only).
- Web app, collaboration, community layer (v3–v5). No format-breaking changes: v2 only adds reserved directories and new derived artifacts.

## Capabilities

### New Capabilities
- `knowledge-graph`: concept extraction into a per-paper graph, storage/index contract, interactive graph view, node→lesson entry.
- `cross-paper-linking`: global concept identity across the library, paper backlinks, "seen elsewhere" surfacing, library-wide concept search.
- `learner-memory`: mastery/preference/episodic memory stores, update events, decay, and the privacy boundary (local, E2E-encrypted under future sync).
- `paper-dashboard`: the on-open knowledge dashboard with honest mastery-derived progress and continue CTA.
- `reading-mode`: paper-as-course sequencing, lesson player, escape hatch invariants.
- `socratic-tutor`: the question/wait/hint/correction interaction contract on top of lessons.
- `quizzes-flashcards`: generation, spaced-repetition scheduling, grading, and mastery feedback loop.

### Modified Capabilities
- `contextual-chat`: context assembly becomes graph-based (node resolution + edge expansion + learner profile) instead of object+relationships only; per-object episodic summaries enter prompts. Existing streaming/persistence/citation requirements unchanged.

## Impact

- `app/crates/copilot-core`: new modules (concepts, graph, graph index, learner memory, lesson sequencing, quiz engine, cross-paper registry); new pipeline stage (concept extraction, LLM-assisted with local fallback); context.rs rework.
- `.research` format: activates reserved `learning_state/`, `quizzes/`, `flashcards/` dirs; adds derived `knowledge_graph.json` (+ schema); adds a library-level `concepts/` registry outside bundles for cross-paper identity. All additive — format v0 minor bump.
- Frontend: graph view, dashboard, reading-mode player, tutor UI (AI-Elements chat components), quiz/flashcard UI, panel deep-dive tabs (the v1 panel was built tab-extensible for this).
- Performance budgets (binding, from platform doc): graph neighborhood <5 ms; entity-linking a query <50 ms local; dashboard visible <500 ms on open; all v1 budgets still enforced in CI.
- Prerequisite watch: LLM-dependent features (concept extraction quality, lesson/quiz generation) must degrade kindly with no key — pre-generated sample-paper content ships, mirroring v1's zero-setup first-wow.
