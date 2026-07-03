# PRD — v2 "The Learning Engine"

**Status:** Roadmap (to become OpenSpec change `add-v2-learning-engine` after v1 ships). Prerequisite: cloud sync (`add-cloud-sync`) lands between v1 and v2 — sync is prioritized before any web app.

## Goal

Turn reading into learning. v1 made the paper interactive; v2 makes the system *know the learner* — persistent knowledge graph, mastery tracking, adaptive explanations, quizzes, and the course-like reading mode. This version delivers the product's biggest innovation: **persistent understanding**.

## Features

### 1. Knowledge graph (the heart)

- Auto-built per paper during enrichment: concept nodes (Transformer → Attention → Softmax/Scaling/Multi-head; Encoder; Decoder; Positional Encoding; LayerNorm; Residual; Feed Forward; Training), edges (`prerequisite_of`, `depends_on`, `defined_in`, `used_by`).
- Interactive graph view; click any node → instant lesson.
- **Cross-paper linking:** concepts are global across the library; "we've seen this in paper X" moments.
- Graph-based context assembly replaces per-query vector-search stuffing → near-zero context bloat (see [knowledge-graph-and-memory.md](../architecture/knowledge-graph-and-memory.md)).

### 2. Learner memory

- Mastery memory per concept/object (struggle counts, quiz outcomes, decay).
- "Struggled with LayerNorm 5×" → explanations change approach automatically.
- "Mastered cross-entropy" → never re-explained.
- Preference memory (visual/code/formal style) shapes every AI output.
- Episodic memory: prior confusions summarized into new answers.

### 3. Paper dashboard

Opening a paper shows: Progress %, Understanding level, Estimated remaining reading time, Concepts learned/remaining, Equations mastered (18/27), Figures understood (9/14), Implementation status, Quiz score. Primary CTA: continue where you left off.

### 4. Reading mode (paper-as-course)

Lesson → mini explanation → diagram → animation → exercise → quiz → continue. Lessons sequenced by prerequisite topology, filtered by mastery. Escapable to raw paper anytime.

### 5. Socratic tutor

Lesson → ask question → *wait* → hint → correction → next lesson. Professor, not chatbot.

### 6. Quizzes & flashcards

Auto-generated per object/concept; spaced repetition scheduling; results feed mastery memory and the dashboard.

### 7. Full equation/figure deep-dives

The complete v1-flagship panels: equation → variables, derivation, assumptions, implementations, numerical example, **interactive sliders + visualization**, historical origin, prerequisites, quiz, related equations, common mistakes. Figure → explain, **animate**, intuition, code, experiment, similar figures, where-referenced, later papers using it, criticisms.

### 8. Notes 2.0 & cross-paper search

Notes linked to graph nodes; library-wide semantic search ("where did I learn about residual connections?").

## Success metrics

- ≥ 40% of active readers take ≥ 1 quiz/week; quiz→mastery uplift measurable.
- Repeat explanations of already-mastered concepts ≈ 0 (memory working).
- Session length and same-paper return rate up ≥ 25% vs v1 cohort.
- "Explanation helped" rating ≥ 85%.
- Context tokens per query down ≥ 60% vs v1 RAG baseline (graph context working).

## Risks

Graph extraction quality (concept dedup, wrong edges → wrong lessons); mastery-model coldness (don't gate content behind unproven scores — always escapable); privacy of learner model (local + E2E-encrypted in sync).
