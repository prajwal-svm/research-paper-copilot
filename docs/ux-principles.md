# UX Principles

Customer satisfaction is a stated core value. These principles are binding on every PRD and spec.

## Principles

1. **Reading is sacred.** The paper view is never blocked, jittered, or reflowed by AI work. All enrichment is background and progressive. Budgets in [platform-and-performance.md](architecture/platform-and-performance.md).
2. **Everything is an object; every object is one click away from learning.** No modes to learn first — click an equation, figure, table, citation, or sentence and useful actions appear in place.
3. **Never re-explain what the user knows; never assume what they don't.** All AI output is filtered through the learner model (see [knowledge-graph-and-memory.md](architecture/knowledge-graph-and-memory.md)).
4. **Progress must be visible and honest.** Dashboards show mastery, not vanity metrics. Progress derives from demonstrated understanding (quizzes, explanations back), not scroll depth.
5. **Streaming, always.** No spinner longer than 300 ms without content or skeleton. AI responses stream token-by-token.
6. **Degrade kindly.** No key, no network, hostile PDF — every failure state has a designed screen with a next step, never a stack trace or dead end.
7. **Zero-setup first win.** From install → open bundled sample paper → first "wow" interaction in under 2 minutes, without an API key (bundled samples ship with pre-generated enrichment).
8. **The user's data is theirs.** Local files, exportable everything, no lock-in. Trust is a UX feature.

## Signature interactions

### Object interaction — Equation (the flagship demo)

Click Equation 12 → panel with progressive depth, not a wall of text:

Variables → Derivation → Assumptions → Python implementation → Numerical example → Interactive sliders → Visualization → Historical origin → Prerequisites → Quiz → Related equations → Common mistakes.

Each is a tab/step; the learner model chooses the default entry point (visual learner → visualization first).

### Object interaction — Figure

Click Figure 7 → Explain · Animate · Generate intuition · Show generating code · Show experiment · Find similar figures · Show where referenced · Show later papers using it · Show criticisms.

### Citation hover card

Hover `[13]` → instantly (cached): Title ("Attention Is All You Need"), Summary, Main contribution, Relationship to this paper, Why cited here, Key figure, Key equation, Common misconceptions. One click: open as its own `.research` paper.

### Reading mode (course mode, v2)

Instead of scrolling: Lesson → mini explanation → diagram → animation → exercise → quiz → continue. The paper becomes a course. Always escapable back to the raw paper at the same location.

### Paper dashboard (v2)

Opening a paper shows the knowledge dashboard (progress %, understanding level, estimated remaining time, concepts learned/remaining, equations mastered, figures understood, implementation status, quiz score) with "continue where you left off" as the primary action.

### Socratic tutor (v2)

Lesson → question → *wait* → hint → correction → next. The tutor never dumps answers when the user is one hint away.

## Anti-patterns (explicitly rejected)

- Chat as the primary interface. Chat is one tool anchored to objects, not the product.
- Modal onboarding tours. The first paper *is* the onboarding.
- Confidence theater — hiding extraction uncertainty. Low-confidence parses are visibly flagged per object.
- Feature-count bloat that violates performance budgets. Budgets outrank features.

## Satisfaction metrics (tracked from v1, all opt-in/local-respecting)

- Time-to-first-wow (install → first successful object interaction) — target < 2 min median.
- D7/D30 retention of readers (opened ≥1 paper in week/month).
- Papers per active user per week; return-to-same-paper rate (proxy for real study).
- NPS/CSAT in-app pulse; qualitative "did this explanation help?" per AI answer (thumbs).
- Rage signals: AI answer regenerations, panel closes <2 s after open, ingestion abandonment.
