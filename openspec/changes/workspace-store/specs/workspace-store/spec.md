# workspace-store

## ADDED Requirements

### Requirement: SQLite workspace store at the workspace root
The system SHALL maintain a single SQLite database file `workspace.db` at the workspace root for all paper-independent entities. The store SHALL open (creating it if absent) via a `WorkspaceStore` API in `copilot-core`, using rusqlite with the bundled feature so no system SQLite is required. `.research` bundles SHALL remain the storage for per-paper artifacts; nothing paper-anchored moves into the store.

#### Scenario: First open creates the store
- **WHEN** the app opens a workspace that has no `workspace.db`
- **THEN** the store is created with the current schema and an empty item registry, and the app proceeds normally

#### Scenario: Papers stay in bundles
- **WHEN** a paper is imported or annotated
- **THEN** no paper content is written to `workspace.db` (only workspace-entity references to papers may appear in the refs table)

### Requirement: Versioned migrations
The store SHALL track its schema with `PRAGMA user_version` and apply forward-only migrations on open. Opening a store with a newer version than the app understands SHALL fail with a clear message rather than corrupting data.

#### Scenario: Older store upgrades on open
- **WHEN** the app opens a `workspace.db` created at schema version N and the app ships version N+1
- **THEN** the migration runs once, `user_version` becomes N+1, and existing rows are preserved

#### Scenario: Newer store is refused safely
- **WHEN** the app opens a `workspace.db` with `user_version` greater than the app supports
- **THEN** the store is not modified and the user sees an actionable error naming the version mismatch

### Requirement: Item registry with recency and tombstones
The store SHALL keep an `items` registry — one row per workspace entity — with at minimum: `id` (UUID), `kind` (e.g. `note`, `canvas`, `chat`), `title`, `created_at`, `updated_at` (RFC 3339), and `deleted_at` tombstone. Deletes SHALL be soft (tombstone set); tombstoned items SHALL be excluded from listings. Every content mutation of an entity SHALL bump its `updated_at`.

#### Scenario: Listing excludes tombstones
- **WHEN** an item is deleted and the items are listed
- **THEN** the deleted item does not appear, but its row (with `deleted_at`) remains in the database for future sync merge

#### Scenario: Edit bumps recency
- **WHEN** an entity's content is modified
- **THEN** its `updated_at` reflects the modification time and the item moves accordingly in recency-sorted listings

### Requirement: Generic refs backlink table
The store SHALL provide a `refs` table linking any workspace entity to research artifacts and external resources: `source_kind` + `source_id` → `target_kind` (`paper` | `object` | `url` | `file`) with the fields needed to resolve the target (`paper_id`, `object_id`, `url`, `path`) and an optional display `label`. The table SHALL be queryable in both directions: refs from an entity, and refs to a given paper or object.

#### Scenario: Backlink query for a paper
- **WHEN** a workspace entity references a paper and a caller asks for refs targeting that paper id
- **THEN** the entity appears in the result with its source kind and label

#### Scenario: Refs removed with their source
- **WHEN** a workspace entity is deleted (tombstoned)
- **THEN** its refs no longer appear in backlink queries

### Requirement: Export to portable formats
Every workspace entity SHALL be exportable without the app: per-entity export to markdown (content) and JSON (full fidelity), and a workspace-level export that writes all non-tombstoned entities to a user-chosen directory. Export SHALL NOT require network or AI providers.

#### Scenario: Single entity export
- **WHEN** the user exports one entity
- **THEN** a markdown file (human-readable content) and/or JSON file (full fidelity) is written to the chosen location

#### Scenario: Workspace export
- **WHEN** the user runs "export workspace"
- **THEN** all live entities are written under the chosen directory, grouped by kind, and the operation reports how many items were exported

### Requirement: Sync-ready row semantics
All store tables SHALL carry `updated_at` timestamps and soft-delete tombstones so a future sync layer can merge by recency without schema changes. CRDT/real-time collaboration is explicitly out of scope for this capability.

#### Scenario: Tombstone survives for merge
- **WHEN** an entity is deleted on this machine
- **THEN** the tombstoned row (id + `deleted_at`) remains queryable by a future sync engine
