# PRD — v4 "The Researcher Workspace"

**Status:** Roadmap (to become OpenSpec change `add-v4-researcher-workspace`).

## Goal

From consuming research to *producing* it. v4 serves active researchers extending the frontier — and teams doing it together.

## Features

### 1. Extension mode

The researcher's power tool, one pipeline:

```
Find weaknesses → Generate hypotheses → Design novel experiment
→ Estimate novelty (against literature) → Find related work
→ Generate outline → Draft paper
```

- Weakness finding grounded in the paper's own objects: assumptions, limitations, "future work" objects extracted since v1.
- Hypothesis cards: claim, rationale, required experiment, expected evidence, novelty estimate with citations.
- Drafting produces standard formats (LaTeX) with correct citation graphs.

### 2. AI-assisted literature review

- Multi-paper synthesis over the library + open indexes (arXiv, Semantic Scholar): thematic maps, method-comparison tables, chronological lineages built from the cross-paper knowledge graph.
- Output: living literature review documents that update as papers are added.

### 3. Research gap detection

- Graph analysis across papers: under-explored edges (method A never tried on problem B), contradictory findings, stale assumptions — surfaced as ranked, citable gap reports.

### 4. Collaborative workspaces

- Shared libraries and shared papers (built on the sync layer): shared notes/highlights, threaded discussions on objects, presence.
- Roles: reading-group mode (instructor assigns papers/quizzes, sees cohort progress) and lab mode (shared graph, shared experiments).

## Success metrics

- ≥ 10% of MAU in a collaborative workspace.
- Literature reviews generated → edited → exported (not regenerated) ≥ 50% of the time (quality proxy).
- Documented cases of gap-reports → actual papers (community showcase).

## Risks

Hallucinated novelty claims (always cite; novelty is an *estimate* with evidence, never an assertion); academic-integrity optics (position as accelerant for the researcher's own thinking; provenance on all AI-drafted text); collaboration scope creep (build on sync primitives, not a new product).
