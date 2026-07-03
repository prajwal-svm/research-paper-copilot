# Design — add-v4-researcher-workspace

## Context

Binding prior decisions: code under `app/` (Rust core + Tauri 2 + React/shadcn); `.research` bundles (immutable PDF, derived regenerable JSON, append-only UUID-anchored user data, format 0.3.0); versioned resumable pipeline; per-paper knowledge graph + library-level concept registry (`concepts.jsonl`) and SQLite graph index; learner memory; graph-first context assembly; sandboxed execution with type-enforced consent; BlockNote markdown editing; provider tiers with streaming/cancel and designed no-key states. v1 already ships arXiv + Crossref clients with politeness caps.

Not yet built: cloud sync (`add-cloud-sync` — unproposed). The v4 PRD's collaboration feature explicitly builds on it. This change specs collaboration and ships its local data models, but gates the shared/roles implementation on sync landing.

## Goals / Non-Goals

**Goals:** an extension pipeline whose every claim is cited and every AI-drafted passage provenance-marked; living literature reviews that never clobber user edits; gap reports derived from real graph structure (not free association); collaboration data models that sync picks up unchanged; standard-format export (LaTeX + BibTeX).

**Non-Goals:** the sync transport itself (separate change); autonomous experiment execution from hypothesis cards (extension designs experiments, v3's modes run them — a card links to an experiment, it doesn't spawn one unattended); web app; journal-submission tooling beyond LaTeX/BibTeX export; any server-side generation.

## Decisions

1. **Extension mode is a staged, resumable document pipeline — the reproduction pattern, not a chat.** Stages: weaknesses → hypotheses → novelty → related-work → outline → draft, each persisted under the bundle's `research/` directory, each re-runnable individually, later stages consuming earlier ones. Rationale: users edit between stages (the whole point); staged persistence is the proven shape (ingestion, reproduction) and makes "regenerate the outline but keep my hypotheses" trivial.
2. **Weakness finding is object-grounded extraction, not opinion.** The prompt sees only the paper's own objects (limitations/future-work/assumption-bearing sections identified by heading + content heuristics, plus equations' assumptions from v3 deep-dives when cached); every weakness must cite its source object `[[object:ID]]` or it is dropped in parsing — the same validation discipline as concept extraction and code mapping. Output: `research/weaknesses.json` (derived, regenerable).
3. **Hypothesis cards are append-only user data.** `research/hypotheses.jsonl`: card events (create/edit/archive) carrying claim, rationale, required experiment, expected evidence, source weakness ids, and the novelty estimate once computed. User edits are events like everything else; cards survive regeneration of upstream stages. A card can link to a v3 experiment (`experiment_id`) — designed there, run there.
4. **Novelty is a search verdict with evidence, never an assertion.** For each card: query Semantic Scholar + arXiv (title/abstract-level queries built from the claim; explicit user action; hosts shown per egress rules) → top-k similar works ranked by the local MiniLM embeddings → verdict enum `appears_novel | adjacent_work_exists | likely_known`, always paired with the evidence list (title, year, id, similarity) and the query used. No results ≠ novel: the verdict then is `insufficient_evidence`. Rationale: the PRD's top risk is hallucinated novelty; making the verdict a *closed enum + mandatory evidence array* makes an uncited novelty claim unrepresentable.
5. **Drafting produces LaTeX with a real citation graph.** Outline and draft generation cite only from a fixed bibliography assembled beforehand (the paper, its resolved citations, novelty evidence, library papers via the registry) — the model picks from keys it was given, never invents them; unknown keys are stripped at parse time and reported. Export = `main.tex` + `references.bib` (from resolved metadata) via plain string templating. AI-drafted passages carry `% ai-drafted` provenance comments in the source and a frontmatter provenance block.
6. **Literature reviews are living documents with a regeneration contract.** `reviews/<review-uuid>/` at the library level: `review.json` (scope: concept ids/query, member papers), `document.md` (BlockNote-edited), `generated.md` (the latest machine synthesis). Regeneration rewrites `generated.md` only; a three-way "refresh" surface shows what changed so the user merges into their edited `document.md` deliberately. Synthesis inputs come from the concept registry (shared concepts across papers), `contradicts`/`extends` edges, and paper metadata timelines — the graph provides structure; the LLM writes prose over it. Rationale: the PRD quality metric is *edited-not-regenerated*; that dies instantly if regeneration can eat edits.
7. **Gap detection is deterministic graph analysis + LLM narration, in that order.** Candidate gaps are computed from structure alone: (a) method concepts never co-occurring with problem concepts that their siblings co-occur with (registry co-occurrence matrix), (b) `contradicts` edges with no later resolving paper, (c) concepts whose newest supporting paper is old relative to the library (stale assumptions). Ranking = structural score (coverage × recency × degree); the LLM only narrates each pre-computed gap into a citable paragraph. Rationale: hallucination-proofing — the LLM cannot invent a gap, only describe one the graph exhibits; every gap report line traces to registry/edge ids.
8. **Collaboration ships as data models now, features after sync.** Workspace membership, object-anchored threads (`threads/<object-uuid>.jsonl` — the existing chat journal shape with author ids), assignments and cohort-progress records are all specced as append-only journals keyed by stable ids, exactly the shape `add-cloud-sync` will merge. Roles (instructor/member, lab/reading-group) are workspace metadata. No feature UI beyond local data inspection until sync lands — the tasks section gates on it explicitly. Rationale: PRD risk "collaboration scope creep — build on sync primitives, not a new product."
9. **Format bump 0.3.0 → 0.4.0 (additive):** bundle gains `research/` (weaknesses.json derived; hypotheses.jsonl, outline.md, draft/ user data); library level gains `reviews/` and `gaps/`. Same-major compatibility test extends the existing loop.
10. **UI placement:** extension mode is a reader-shell pane (wizard with stage timeline, like reproduction); hypothesis cards render in it and anchor chat; reviews and gaps live in a library-level "Research" view beside the paper grid; all long generations stream with cancel and skeletons ≤300 ms.

## Risks / Trade-offs

- [Hallucinated novelty] → decision 4: closed verdict enum + mandatory evidence + `insufficient_evidence` when search is empty/keyless; UI renders verdicts only alongside their evidence list.
- [Fabricated citations in drafts] → decision 5: fixed-bibliography prompting + unknown-key stripping with a visible "N citations removed" notice; BibTeX generated from resolved metadata only.
- [Academic integrity optics] → provenance comments in LaTeX source, provenance block in exports, drafts watermarked "draft — AI-assisted" until the user removes it deliberately; positioning language in docs.
- [Regeneration eats edits] → decision 6's generated/document split + explicit merge surface; test: regenerate after edit, edited document byte-identical.
- [Semantic Scholar rate limits / downtime] → politeness caps like Crossref; cached evidence; degraded verdict `insufficient_evidence` with retry, never a blocked pipeline.
- [Gap reports over sparse libraries] → minimum-library-size honesty: below N papers/concepts the report says the library is too small to support gap claims rather than manufacturing them.
- [Sync dependency slip] → collaboration tasks isolated in their own gated section; everything else in v4 is fully local and ships regardless.

## Migration Plan

Additive only. 0.4.0 bundles open in older readers (same major; unknown dirs preserved). Library-level `reviews/`/`gaps/` are new directories older versions ignore. No data migration. Rollback = previous app version.

## Open Questions

- Semantic Scholar API key (optional, raises rate limits) — support as an optional keychain entry from day one or add when caps bite? Leaning: optional entry in provider settings from day one (same slot mechanics as providers).
- LaTeX template flavor (plain article vs NeurIPS/ICML class files) — start with `article` + a class-file dropdown; ship class files only if licenses permit, else document.
- Whether gap co-occurrence uses the SQLite index or an in-memory fold of the registry — decide at task time by measuring at 200-paper scale (budget: gap report < 5 s end-to-end excluding LLM narration).
