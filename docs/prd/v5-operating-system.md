# PRD — v5 "The Operating System"

**Status:** Roadmap (to become OpenSpec change `add-v5-operating-system`).

## Goal

Complete the transformation: a paper is no longer a static PDF but a **living, evolving knowledge object** that anyone can study, improve, reproduce, and extend. Research Paper Copilot becomes the open research operating system — infrastructure, not app.

## Features

### 1. Community knowledge layer

Open contribution to any paper's knowledge object, PR-style:

- Better explanations · better animations · better quizzes · better implementations · better visualizations.
- Provenance, review, and versioning on every contribution; reputation for contributors.
- **One paper gets better forever** — the compounding moat.

### 2. Knowledge registry

- A public registry of `.research` enrichments (like npm/crates for paper knowledge): pull community enrichment on import instead of re-deriving; publish improvements back.
- Canonical paper identity (DOI/arXiv) so the whole ecosystem converges on shared objects.

### 3. Interoperability & ecosystem

- `.research` JSON Schemas + a plugin API: third-party panels (new visualizers, domain-specific tools for biology/chemistry/math), exporters (Anki, Obsidian, LaTeX), importers (LaTeX source, HTML papers, lab notebooks).
- Cross-field bridges: interdisciplinary concept mapping so an expert in one field can walk into another via the global knowledge graph.

### 4. Full platform parity

- Web app identical to desktop (shared frontend, WASM/server core) with sync as the backbone; mobile companion for review/flashcards.

## Success metrics

- ≥ 1,000 papers with community enrichment; median enriched-paper quality rating > solo-AI baseline.
- ≥ 100 registry publishers; ≥ 10 third-party plugins.
- Institutional adoptions (courses, labs, reading groups) as the reference metric of "infrastructure" status.

## Risks

Moderation and quality of community content (review gates, trust levels); licensing of paper content vs enrichment (enrichment is community-owned; original PDFs remain under publisher rights — registry stores enrichment only); sustaining an open-source ecosystem (foundation-style governance, optional paid sync/hosting).
