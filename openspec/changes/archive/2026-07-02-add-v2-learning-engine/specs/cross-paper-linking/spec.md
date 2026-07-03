# cross-paper-linking

## ADDED Requirements

### Requirement: Global concept identity
Concepts SHALL have library-global identity: a library-level append-only registry maps global concept ids to per-paper graph nodes, populated by embedding-similarity plus normalized-name matching. Automatic merges SHALL be conservative (high threshold) and every merge/split SHALL be a recorded, user-reversible event surfaced for confirmation in the graph view.

#### Scenario: Same concept across two papers
- **WHEN** a second paper using "multi-head attention" finishes ingestion
- **THEN** its node links to the existing global concept, and both papers appear in the concept's "appears in" list

#### Scenario: Wrong merge undone
- **WHEN** the user splits an incorrectly merged concept
- **THEN** a split event is recorded, both papers show separate concepts immediately, and future auto-matching respects the split

### Requirement: Cross-paper surfacing while reading
When the reader encounters a concept already known from another paper, the UI SHALL be able to surface it ("seen in <paper>") with one-click navigation to the other paper's introducing object; mastery for the concept SHALL be shared globally, so mastering it in one paper counts everywhere.

#### Scenario: Seen-elsewhere moment
- **WHEN** the user opens a node/lesson for a concept mastered in another paper
- **THEN** the lesson references the prior paper instead of re-teaching, with a link that opens that paper at the introducing object

### Requirement: Paper backlinks
Bundles SHALL support explicit paper-to-paper links as user data (append-only, citing resolved identifiers when available): citation-derived link suggestions plus user-created backlinks, listable from both sides ("links here" / "links out") to power the library's knowledge map.

#### Scenario: Backlink from citation import
- **WHEN** the user imports a cited paper via a citation hover card
- **THEN** a suggested link between citing and cited paper is recorded and visible from both papers' link lists

### Requirement: Library-wide concept search
Search SHALL extend across the library at the concept level: querying a concept (e.g. "residual connections") returns the global concept with the papers/objects where it was learned or appears, using local embeddings only, in <150 ms on the reference machine for a 200-paper library.

#### Scenario: "Where did I learn X?"
- **WHEN** the user searches "where did I learn about residual connections"
- **THEN** results list the concept's papers ordered by the user's interaction history, each opening at the relevant object, fully offline
