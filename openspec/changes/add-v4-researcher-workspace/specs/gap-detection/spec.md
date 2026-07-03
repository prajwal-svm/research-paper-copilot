# gap-detection

## ADDED Requirements

### Requirement: Gaps computed from structure, narrated after
Gap candidates SHALL be computed deterministically from the library's graph structure — under-explored concept co-occurrences (method never tried on a problem its siblings were tried on), unresolved `contradicts` edges, and stale assumptions (concepts whose newest support is old relative to the library) — and ranked by a structural score before any LLM involvement. The LLM SHALL only narrate pre-computed gaps; it cannot add, remove, or re-rank them. Every gap in a report SHALL trace to the registry/edge ids that produced it and cite the involved papers.

#### Scenario: Under-explored edge surfaced
- **WHEN** methods M1 and M2 share a concept neighborhood, M1 co-occurs with problem P across the library, and M2 never does
- **THEN** the report contains a "M2 on P appears untried" gap citing the papers establishing each side, ranked by the structural score

#### Scenario: Unresolved contradiction
- **WHEN** two papers hold a `contradicts` edge and no later library paper connects to both
- **THEN** the report lists the contradiction as an open question with both papers cited

### Requirement: Honest about library coverage
Gap reports SHALL state their evidential basis (papers and concepts analyzed) and SHALL refuse to manufacture gaps from sparse data: below a minimum library size for the scoped topic, the report says the library is too small to support gap claims. Reports are exportable, citable documents stored at the library level.

#### Scenario: Sparse library
- **WHEN** the user requests a gap report with three papers in scope
- **THEN** the report explains the coverage is insufficient for gap claims rather than inventing findings
