# knowledge-graph

## ADDED Requirements

### Requirement: Concept graph extraction stage
Ingestion SHALL gain a concept-extraction stage that builds a per-paper knowledge graph: concept nodes (each linked to the object UUIDs that introduce/use it) and typed edges from the closed vocabulary `prerequisite_of`, `depends_on`, `defined_in`, `used_by`, `extends`, `contradicts`, `cites` — every node and edge carrying an extraction-confidence score. The stage SHALL be versioned and resumable like all pipeline stages, LLM-assisted when a provider is available, and SHALL degrade to a heuristic graph (headings + embedding clustering, flagged low-confidence) with no key — never blocking reading.

#### Scenario: Graph produced for an arXiv ML paper
- **WHEN** ingestion completes on the sample paper with a provider configured
- **THEN** `knowledge_graph.json` validates against its published schema, contains concept nodes for the paper's core ideas (e.g. Attention, Multi-head, Positional Encoding) each linked to ≥1 object UUID, and edges only from the closed vocabulary

#### Scenario: No provider configured
- **WHEN** a paper is ingested with no key and no local model
- **THEN** a heuristic graph is produced and visibly flagged as limited, the paper remains fully readable, and re-running the stage after configuring a provider upgrades the graph in place

### Requirement: Graph storage and query budget
The graph SHALL be stored in-bundle as derived, regenerable `knowledge_graph.json` (schema published alongside the v0 artifacts) and mirrored into a rebuildable library-level index. Node-neighborhood queries (node + edges + neighbor summaries) SHALL return in <5 ms on the reference machine; the index SHALL be reconstructible from bundle JSON at any time and never treated as source of truth.

#### Scenario: Neighborhood query within budget
- **WHEN** the UI requests the neighborhood of a concept node in an open paper
- **THEN** nodes, edges, and linked-object references return in <5 ms from the index

#### Scenario: Index deleted
- **WHEN** the library-level index file is deleted and the app restarts
- **THEN** the index is rebuilt from bundle `knowledge_graph.json` files with no data loss

### Requirement: Interactive graph view
The reader SHALL offer a graph view of the current paper's concepts: pan/zoom, hover to highlight linked objects in the reader, and click any node to open its lesson/panel within 300 ms (skeleton allowed per streaming rule). Low-confidence nodes/edges SHALL be visually distinct. The view SHALL stay responsive (60 fps interaction) up to at least 500 visible nodes via virtualization.

#### Scenario: Node click opens a lesson
- **WHEN** the user clicks the "Positional Encoding" node
- **THEN** the node's lesson panel opens within 300 ms, listing the paper objects that introduce it as clickable reader links

#### Scenario: Low-confidence edge shown honestly
- **WHEN** an edge was extracted below the confidence threshold
- **THEN** it renders in the distinct low-confidence style and its detail popover names the reason

### Requirement: User corrections survive re-extraction
Users SHALL be able to correct the graph (delete an edge, merge/split/rename nodes); corrections are stored as append-only user-data overrides applied on top of extraction output, so re-running the stage with an improved extractor never silently reverts them.

#### Scenario: Edge deleted then paper re-ingested
- **WHEN** the user deletes a wrong `prerequisite_of` edge and the concept stage later re-runs
- **THEN** the regenerated graph applies the stored override and the edge stays deleted
