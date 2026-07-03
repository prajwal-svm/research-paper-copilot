# research-format

## ADDED Requirements

### Requirement: Bundle structure
The system SHALL store each imported paper as a `.research` bundle (directory in the library; zip on export) containing at minimum `metadata.json`, `original.pdf`, `layout.json`, `semantic_tree.json`, `embeddings.bin`, `citations.json`, and the `equations/`, `figures/`, `tables/`, `glossary/`, `notes/`, `bookmarks/`, `chats/` directories, with `flashcards/`, `quizzes/`, `learning_state/`, `implementations/`, `experiments/` reserved for later versions. The original PDF SHALL be immutable and content-addressed; it is one view, never the source of truth.

#### Scenario: Successful ingestion produces a valid bundle
- **WHEN** a PDF completes ingestion
- **THEN** a bundle exists whose files validate against the published JSON Schemas, and `metadata.json` records `format_version`, per-stage `pipeline_version`, and content hashes

#### Scenario: Unknown files are preserved
- **WHEN** a bundle containing directories unknown to this app version is opened and modified
- **THEN** the unknown files SHALL remain intact after save

### Requirement: Object model with UUID anchoring
Every extracted element (paragraph, sentence, equation, figure, table, citation, definition, algorithm, experiment, dataset, metric, claim, limitation, future work) SHALL be an object with a stable UUID, type, bounding box (page + coordinates), extracted content, semantic label, relationships, embedding reference, and extraction-confidence score. All user data SHALL anchor to object UUIDs plus content hashes, never to page offsets.

#### Scenario: Re-parsing preserves user data
- **WHEN** a paper is re-ingested with an improved pipeline version
- **THEN** existing notes, chats, and bookmarks SHALL re-attach to matching objects via UUID/content-hash and none are orphaned silently; unmatched anchors are surfaced to the user for reattachment

#### Scenario: Object relationships are queryable
- **WHEN** an equation object that is referenced by a figure is loaded
- **THEN** its `relationships` list SHALL contain the reference so the UI can navigate between them in < 100 ms

### Requirement: Versioning and compatibility
The format SHALL be semver-versioned in `metadata.json`. Readers MUST open any bundle of the same major version; on encountering a newer major version the app SHALL refuse with a clear upgrade message rather than corrupt data.

#### Scenario: Newer-major bundle opened by older app
- **WHEN** the app opens a bundle with a higher major `format_version`
- **THEN** it SHALL show a non-destructive "update required" message and leave the bundle untouched

### Requirement: Sync-ready user-data layout
User-generated files (notes, chats, bookmarks) SHALL be stored as append-only or CRDT-upgradeable structures, separated from regenerable derived data, so the forthcoming cloud-sync change can merge them without destructive conflicts.

#### Scenario: Crash during write
- **WHEN** the app crashes mid-write of a chat message
- **THEN** on restart the bundle SHALL load with all previously committed messages intact (append-only journal, no partial-state corruption)
