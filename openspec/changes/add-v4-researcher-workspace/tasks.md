# Tasks — add-v4-researcher-workspace

Ordered integrity-first: the anti-hallucination machinery (fixed bibliographies, evidence-mandatory novelty, structure-first gaps) is built and tested before any prose generation ships. Section 7 (collaboration features) is **gated on `add-cloud-sync`** — its data models land in section 6, its behaviors wait. All AI surfaces need designed no-key states; cached/edited documents stay usable and exportable keyless.

## 1. Registry analytics (cross-paper-linking delta)

- [x] 1.1 Lineage query: global concept → papers in publication order with connecting `extends`/`cites` edges; offline, <150 ms @ 200 papers (perf suite)
- [x] 1.2 Co-occurrence matrix: concept-pair paper counts from registry state; same budget; unit tests over synthetic libraries

## 2. Extension pipeline core

- [x] 2.1 `research/` stage store in-bundle (weaknesses.json derived; hypotheses.jsonl, outline, draft as user data); staged, resumable, upstream-regeneration flags downstream instead of destroying it (tests)
- [x] 2.2 Weakness extraction: object-grounded prompt over limitations/future-work/assumption sections; parse-time rejection of any weakness without a valid `[[object:ID]]` citation (test); no-key state
- [x] 2.3 Hypothesis cards: append-only card events (create/edit/archive), fields per spec, "upstream changed" flagging, link-to-v3-experiment both ways (test)
- [x] 2.4 Novelty search: Semantic Scholar + arXiv clients (politeness caps, optional S2 key slot), MiniLM similarity ranking, closed verdict enum with mandatory evidence — `insufficient_evidence` on empty/failed/keyless search (tests); egress transparency (hosts shown, explicit action only)
- [x] 2.5 Fixed-bibliography assembly: paper + resolved citations + novelty evidence + registry papers → citation keys; outline/draft prompting constrained to those keys; unknown-key stripping with visible count (test)
- [x] 2.6 LaTeX/BibTeX export: templated main.tex + references.bib from resolved metadata only; `% ai-drafted` provenance comments + provenance block + draft label (tests)

## 3. Extension mode UI

- [x] 3.1 Extension wizard pane (reader shell): stage timeline, per-stage run/re-run, streamed generation with skeletons, resume-on-open
- [x] 3.2 Hypothesis card UI: editable cards, novelty verdict rendered only with its evidence list (each item importable), "design this experiment" → pre-filled v3 experiment
- [x] 3.3 Outline/draft editing (BlockNote) + export dialog; card/gap anchors wired into chat (contextual-chat delta)

## 4. Literature review

- [x] 4.1 Review store at library level (`reviews/<uuid>/`: review.json scope, generated.md, document.md); regeneration touches generated.md only (byte-identity test on document.md)
- [x] 4.2 Graph-structured synthesis: sections from shared concepts, comparison tables from registry membership, lineages from 1.1; every claim cites in-scope papers; scope-violation stripping (test)
- [x] 4.3 Review UI in a library-level Research view: BlockNote editor, refresh-with-change-summary merge surface, keyless edit/export

## 5. Gap detection

- [x] 5.1 Structural gap computation: under-explored co-occurrences (from 1.2), unresolved `contradicts`, stale assumptions; deterministic ranking; unit tests over synthetic graphs; sparse-library refusal below minimum coverage (test)
- [x] 5.2 LLM narration of pre-computed gaps only (narration cannot add/remove/re-rank — test that the gap set is identical with and without a provider); citable report document in `gaps/`
- [x] 5.3 Gap report UI: ranked list with per-gap paper citations opening in the reader; export

## 6. Collaboration data models (sync-ready, local now)

- [x] 6.1 Workspace/membership/thread/assignment journals: append-only, UUID-keyed, author-attributed; object-anchored threads reuse chat journal semantics (merge-shape tests: interleaved appends fold deterministically)
- [x] 6.2 Privacy boundary tests: learner-memory stores excluded from any workspace-shareable set by construction; opt-in progress-sharing record shape (what exactly is shared, stated in the record)

## 7. Collaboration features (unblocked by add-cloud-sync, implemented)

- [x] 7.1 Shared libraries + object-anchored threads over sync; authorship UI; presence
- [x] 7.2 Reading-group mode: assignments, opt-in cohort progress (shared quiz outcomes only)
- [x] 7.3 Lab mode: shared graph + shared experiments with attributed runs

## 8. Format, gates & release

- [x] 8.1 Format bump 0.4.0 (research/, reviews/, gaps/); same-major compat loop extended; unknown-file preservation
- [x] 8.2 No-key audit across new surfaces (weaknesses, novelty, synthesis, narration, drafting); cached documents editable/exportable keyless
- [x] 8.3 Perf budgets: lineage + co-occurrence <150 ms (enforced), gap computation <5 s @ 200 papers excluding narration (enforced), review open (pending/UI)
- [x] 8.4 Telemetry (opt-in, content-free): reviews generated/edited/exported (PRD quality proxy), gap reports generated, drafts exported
- [ ] 8.5 Docs: format spec update ✅, researcher-workspace guide ✅ (docs/guides/researcher-workspace.md), README refresh ✅; v4 release — **blocked on user** (commit freeze; ships with v1–v3)
