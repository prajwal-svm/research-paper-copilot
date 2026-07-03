# literature-review

## ADDED Requirements

### Requirement: Graph-structured multi-paper synthesis
Literature reviews SHALL be generated over the library's cross-paper structure — shared concepts from the registry, `contradicts`/`extends`/`cites` edges, and paper timelines — producing thematic sections, method-comparison tables, and chronological lineages where every synthesized claim cites the library papers (and imported evidence) it derives from. The graph provides the structure; prose generation never introduces papers that aren't in scope.

#### Scenario: Comparison table from the graph
- **WHEN** a review is generated over five papers sharing the "attention" concept
- **THEN** the method-comparison table's rows are those papers, its groupings follow registry concepts, and each cell's claim cites its paper

#### Scenario: Lineage from timelines
- **WHEN** the scoped papers span 2014–2024 with `extends` edges between them
- **THEN** the review includes a chronological lineage consistent with those edges and publication dates

### Requirement: Living documents that never eat edits
A review SHALL keep the machine synthesis (`generated.md`) separate from the user's document (`document.md`). Adding papers to the library (or explicit refresh) SHALL regenerate only the machine synthesis and present a change summary for deliberate merging; the user's edited document SHALL never be modified by regeneration. Reviews SHALL be editable with the standard block markdown editor and exportable keyless.

#### Scenario: New paper joins the library
- **WHEN** a paper matching a review's scope is added and the user refreshes the review
- **THEN** `generated.md` updates, the UI shows what changed, and `document.md` is byte-identical until the user merges

#### Scenario: Keyless editing
- **WHEN** no AI provider is configured
- **THEN** existing reviews open, edit, and export normally; only regeneration shows the no-key state
