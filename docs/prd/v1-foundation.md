# PRD — v1 "The Foundation"

**Status:** Active — fully specified in OpenSpec change [`add-v1-foundation`](../../openspec/changes/add-v1-foundation/proposal.md).

## Goal

Ship a fast, local-first desktop app (macOS/Windows/Linux, Tauri) that ingests a PDF into a `.research` bundle, renders it with layout fully preserved, and lets the user chat with *selected objects* — text, figures, equations, tables — with conversations persisted per object. Prove the core abstraction (Paper = Knowledge) and the core feel (instant, object-level interaction).

## Target users & jobs-to-be-done

- **Grad student / advanced undergrad:** "Help me actually understand this paper my group is reading, without 40 ChatGPT tabs."
- **Industry ML engineer:** "I need to understand the method well enough to implement it."
- **Independent learner / career-switcher:** "I lack the prerequisites; meet me where I am."
- **Researcher (secondary in v1):** "Give me faster comprehension of related work."

## Scope

### In

1. **Library** — import PDFs (file/drag/arXiv URL/DOI), list papers with ingestion status, open/delete/reveal-on-disk. Bundled sample paper ("Attention Is All You Need") with pre-generated enrichment for a zero-key first run.
2. **Ingestion pipeline** — PDF → `.research` bundle: layout.json (bounding boxes, reading order), semantic_tree.json, object extraction (paragraphs, sentences, equations w/ LaTeX, figures, tables as structured data, citations, section headers), local embeddings, metadata. Staged, resumable, background; per-stage progress; per-object extraction confidence.
3. **Reader** — layout-faithful rendering at 60 fps; objects hover-highlighted and clickable; raw-PDF fallback view always available; in-paper semantic + text search.
4. **Object interaction (v1 slice)** — click/select any object → anchored panel: Explain, Ask-anything (chat), plus type-specific starters (equation: variables & step-by-step; figure: describe & interpret; table: summarize & query; citation: hover card with title/summary/relationship/why-cited).
5. **Persistent per-object chats** — every conversation is stored in the bundle, anchored to object UUIDs; reopening an object resumes its thread. (Full learner-model memory is v2; v1 persistence is the foundation.)
6. **Notes & bookmarks** — Markdown notes anchored to objects; bookmarks. Stored in the bundle.
7. **AI provider layer** — bring-your-own-key (Anthropic/OpenAI/OpenRouter) + local models via Ollama; streaming; per-action model routing; graceful no-key mode. Anthropic-compatible custom endpoints (e.g., Z.ai GLM Coding Plan, GLM-5.2) follow as change [`add-zai-glm-provider`](../../openspec/changes/add-zai-glm-provider/proposal.md).

### Out (deferred)

Knowledge graph UI, dashboard, quizzes/flashcards, reading/course mode, memory-adapted explanations (v2) · implementation/experiment/reproduction modes (v3) · extension mode, collaboration (v4) · community layer, web app (v5). Cloud sync ships between v1 and v2 as its own change.

## UX requirements

Follows [ux-principles.md](../ux-principles.md) in full: reading is sacred; zero-setup first win < 2 min; streaming always; kind degraded modes; extraction confidence visible.

## Performance requirements

The budgets in [platform-and-performance.md](../architecture/platform-and-performance.md) are v1 acceptance criteria, CI-enforced.

## Success metrics

- Time-to-first-wow < 2 min (median).
- ≥ 60% of new users perform ≥ 5 object interactions in first session.
- ≥ 30% W1 return rate to the *same paper* (evidence of real study, not tourism).
- Ingestion success (usable reader + ≥90% objects extracted) on ≥ 95% of arXiv ML PDFs test corpus.
- Crash-free sessions ≥ 99.5%; all perf budgets green in CI.

## Risks

| Risk | Mitigation |
|---|---|
| PDF parsing quality (math, 2-column, scans) | Staged pipeline, confidence flags, raw view fallback, corpus-driven testing; parsing is the v1 engineering center of gravity |
| BYO-key friction | Bundled pre-enriched samples; Ollama path; 1-click key setup |
| Scope creep toward v2 | OpenSpec change boundary is the contract; dashboard/graph explicitly out |
| Webview rendering variance (Tauri) | pdf.js/canvas renderer, per-OS visual regression suite |
