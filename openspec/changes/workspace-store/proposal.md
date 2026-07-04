# Workspace Store & Unified Library

## Why

The app is becoming a workspace platform (AFFiNE/HackMD-class), not a paper-centric tool: independent notes, canvases, and chat threads are planned as first-class citizens that must not be tied to any `.research` bundle. Today there is nowhere durable, syncable, and exportable for paper-independent entities to live, and the Library can only list papers — so every upcoming feature (notes, canvases, chat threads) is blocked on this foundation.

## What Changes

- New **SQLite workspace store** (`workspace.db` at the workspace root, rusqlite bundled, `PRAGMA user_version` migrations) for all paper-independent entities, per the committed design `docs/superpowers/specs/2026-07-03-global-chat-design.md`.
  - Rows carry `created_at`/`updated_at` and tombstones (`deleted_at`) so a future sync layer can merge; content stays portable (markdown/JSON).
  - Generic **`refs` backlink table**: any workspace entity → paper / object / URL / file — the knowledge-graph seed, queryable in both directions.
  - A generic **`items` registry** (id, kind, title, timestamps) so the Library can list every entity type uniformly; feature tables (notes, canvases, chats) join against it in later changes.
- **Library becomes a unified workspace list**: one list of research papers + workspace items (notes/canvases/threads when they land), sorted by last-updated, with filter chips (All / Research / Notes / Canvases / Threads) and a type icon per card. Papers keep their existing cards/actions.
- **Export paths**: every workspace entity exports to markdown/JSON (workspace-level "export all" plus per-entity export commands).
- `.research` bundles are untouched — papers remain file-bundle-based; only paper-independent entities use the store.

## Capabilities

### New Capabilities
- `workspace-store`: the SQLite store — schema, migrations, tombstone semantics, refs backlink table, item registry, and export guarantees.
- `unified-library`: the Library as a unified, filterable, recency-sorted list of all workspace content types.

### Modified Capabilities

<!-- none — papers' behavior is unchanged; existing specs (paper-dashboard, cross-paper-linking, …) keep their requirements -->

## Impact

- **New Rust**: `workspace` module (or crate) in `copilot-core` — store open/migrate, item registry CRUD, refs CRUD, export. rusqlite is already a workspace dependency (graph_index), so no new external dependency.
- **Tauri commands**: `workspace_items_list`, `workspace_refs_*`, `workspace_export_*` (feature-specific CRUD arrives with each feature change).
- **Frontend**: `Library.tsx` gains filter chips + mixed-type cards; type icons; sorting by `updated_at` across papers (bundle metadata) and workspace items (store).
- **Dependent changes**: `independent-notes`, `independent-canvases`, `chat-threads` all build on this change's tables and library shell.
- **Sync**: out of scope here, but schema decisions (tombstones, `updated_at`) are made for it.
