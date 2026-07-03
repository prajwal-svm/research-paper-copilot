# cross-paper-linking (delta)

## ADDED Requirements

### Requirement: Lineage and co-occurrence queries
The concept registry SHALL support the analytical queries v4 consumes: chronological lineage for a global concept (papers ordered by publication date with connecting `extends`/`cites` edges) and concept co-occurrence across the library (which concepts appear together in papers, with counts). Both SHALL run offline within the established library-query budgets (<150 ms at 200 papers).

#### Scenario: Concept lineage
- **WHEN** literature review requests the lineage of "attention"
- **THEN** the registry returns the concept's papers in chronological order with their connecting edges, offline, within budget

#### Scenario: Co-occurrence for gap analysis
- **WHEN** gap detection requests the co-occurrence matrix for the scoped concepts
- **THEN** counts of papers where each concept pair co-occurs return within budget, computed from registry state alone
