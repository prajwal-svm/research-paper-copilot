# Proposal — add-v4-researcher-workspace

## Why

v1–v3 made papers understandable, learnable, and runnable — all in service of *consuming* research. Active researchers need the next step: turning what they've read, learned, and reproduced into new work. v4 builds the production side (extension mode, literature review, gap detection) on the assets the earlier versions already accumulated — the per-paper and cross-paper knowledge graphs, learner memory, implementations, and reproduction reports — per docs/prd/v4-researcher-workspace.md.

## What Changes

- **Extension mode**: one pipeline from a paper to a draft — find weaknesses (grounded in the paper's own objects: assumptions, limitations, future-work sections) → hypothesis cards (claim, rationale, required experiment, expected evidence) → novelty estimate with citations from open indexes (arXiv, Semantic Scholar) → related work → outline → LaTeX draft with a correct citation graph. Every claim cited; every AI-drafted passage carries provenance; novelty is always an *estimate with evidence*, never an assertion.
- **Literature review**: living multi-paper synthesis over the library's cross-paper knowledge graph plus open indexes — thematic maps, method-comparison tables, chronological lineages — stored as editable documents that update (never silently overwrite user edits) as papers join the library.
- **Gap detection**: graph analysis across the library — under-explored edges (method A never tried on problem B), contradictory findings (`contradicts` edges), stale assumptions — ranked, citable gap reports.
- **Collaborative workspaces** (spec now, implement on sync): shared libraries, object-anchored threaded discussions, reading-group mode (assignments, cohort progress) and lab mode (shared graph/experiments). **Dependency: the `add-cloud-sync` change (not yet proposed) must land first** — all v4 data stores follow the established append-only/UUID rules so they sync unchanged; the collaboration UI/roles tasks are explicitly gated.
- Format: activates a `research/` bundle area (hypotheses, drafts) and library-level `reviews/` + `gaps/` documents (additive minor bump 0.3.0 → 0.4.0).

## Capabilities

### New Capabilities
- `extension-mode`: weaknesses → hypotheses → experiment design → novelty estimate → outline → LaTeX draft, all claims cited, all AI text provenance-marked
- `literature-review`: living synthesis documents over the cross-paper graph + open indexes, edit-preserving updates
- `gap-detection`: ranked, citable gap reports from cross-library graph analysis
- `collaborative-workspaces`: shared libraries, object-anchored threads, reading-group/lab roles — **requirements specced against the future sync layer**

### Modified Capabilities
- `contextual-chat`: hypothesis cards and gap reports become anchorable discussion contexts (same budgeted assembly rules)
- `cross-paper-linking`: the concept registry gains lineage queries (chronological concept evolution) that literature review and gap detection consume

## Impact

- **Rust core**: new modules — weakness extraction (object-grounded prompting), hypothesis store (`research/hypotheses.jsonl`, append-only), novelty search (Semantic Scholar + arXiv clients with politeness caps, evidence-ranked), review/gap document generation over the registry + graph index, LaTeX export (tera-style templating, BibTeX from resolved citations).
- **Shell/UI**: extension-mode wizard (reader pane), hypothesis cards, review editor (BlockNote markdown, edit-preserving regeneration), gap report view, export dialogs.
- **AI usage**: strong tier for weakness/hypothesis/draft prose; light tier for batch novelty triage; all streamed/cancellable; designed no-key states (cached documents remain editable/exportable keyless).
- **Network**: Semantic Scholar joins arXiv/Crossref as an opt-in metadata source (same egress-transparency rules; no paper content sent — queries are titles/abstracts of the user's hypothesis text only with explicit action).
- **Dependency**: collaborative-workspaces implementation blocks on `add-cloud-sync`; its data models ship now (append-only, UUID-anchored) so sync picks them up unchanged.
- **Integrity posture** (PRD risk): provenance markers on all AI-drafted text in exports; novelty estimates always carry their evidence list; drafts are labeled as drafts.
