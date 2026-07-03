# Researcher workspace guide (v4)

v4 is the production side: turning what you've read, learned, and reproduced
into new work. Its defining property is **anti-hallucination by
architecture** — the parts of research writing where AI systems bluff
(novelty claims, citations, "gaps in the literature") are structurally
prevented from bluffing here.

## Extension mode

Open the lightbulb icon in the reader dock. One staged pipeline:

1. **Weaknesses** — extracted only from the paper's own objects (limitations,
   future-work, assumption-bearing passages). A weakness that doesn't cite a
   real passage is dropped at parse time; every one links back into the reader.
2. **Hypothesis cards** — claim, rationale, required experiment, expected
   evidence. Cards are yours: append-only, editable, archivable; regenerating
   weaknesses *flags* your cards ("upstream changed") but can never destroy
   or rewrite them. "Design this experiment" pre-fills a v3 experiment.
3. **Novelty check** — an explicit action that searches arXiv + Semantic
   Scholar with the claim (only the claim text is sent; an optional S2 key
   raises rate limits). The verdict is a closed vocabulary — *appears novel /
   adjacent work exists / likely known / insufficient evidence* — and is
   unrepresentable without its evidence list. An empty or failed search is
   `insufficient evidence`, never "novel". Every evidence item is one click
   from becoming a paper in your library.
4. **Outline → draft** — generation may cite **only** from a pre-assembled
   bibliography (this paper, its resolved citations, your novelty evidence).
   Invented citation keys are stripped and counted ("2 unverifiable citations
   removed"), never silently kept.
5. **Export** — `main.tex` + `references.bib` where BibTeX comes exclusively
   from resolved metadata. AI-drafted passages carry `% ai-drafted` source
   markers and the document is labeled an AI-assisted draft until you remove
   that deliberately. What the machine wrote is always distinguishable from
   what you wrote.

## Literature reviews (living documents)

Library view → **Research** → Literature reviews. A review is scoped by a
concept query over your cross-paper knowledge graph; its synthesis is
*structured by the graph* — thematic sections from shared concepts, a
method-comparison table from registry membership, a lineage from publication
order — and every claim cites in-scope papers (`[[paper:ID]]`; out-of-scope
citations are stripped).

The regeneration contract: the machine synthesis (`generated.md`) and your
document (`document.md`) are separate files. Refreshing after adding papers
updates the machine copy only and shows you what changed (+N/−M lines) — your
edits are never touched, merging is always your deliberate act. Editing and
exporting work with no AI provider.

## Gap reports

Research → Gap reports. Gaps are **computed, then narrated** — never the
reverse. The structural pass finds, deterministically: method↔problem
combinations that never co-occur though their siblings do; `contradicts`
edges no later paper resolves; load-bearing concepts whose newest support
predates the library median. The AI can only write prose *about* gaps the
graph exhibits (the gap set is provably identical with and without a
provider). Below 5 papers / 10 concepts the report refuses to make claims —
a small library gets honesty, not invention.

## Collaboration (data models today, features with sync)

Workspace membership, object-anchored threads, assignments, and opt-in
progress records ship now as append-only, author-attributed journals whose
folds are order-independent — exactly the shape `add-cloud-sync` merges. Two
guarantees are already tested: learner memory (mastery, episodes,
preferences) is **unshareable by construction** (the shareable set is an
allowlist it isn't on), and progress sharing is per-member opt-in whose
record states exactly what is shared. Shared libraries, reading-group and
lab modes activate when the sync layer lands.

## Format changes

`format_version` 0.3.0 → **0.4.0** (additive): bundle gains `research/`
(weaknesses, hypothesis cards, outline, draft); the library gains `reviews/`,
`gaps/`, and `workspaces/`. Older readers open v4 bundles and preserve the
new files untouched.
