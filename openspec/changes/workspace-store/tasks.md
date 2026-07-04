# Tasks — Workspace Store & Unified Library

## 1. Core store (copilot-core)

- [x] 1.1 Create `workspace.rs` module in copilot-core: `WorkspaceStore::open(root)` — creates/opens `workspace.db` (rusqlite bundled, WAL, busy_timeout), applies `user_version` migrations in a transaction, refuses newer-than-known versions with a clear error
- [x] 1.2 Migration v1: `items(id TEXT PK, kind TEXT, title TEXT, created_at TEXT, updated_at TEXT, deleted_at TEXT)` + `refs(id TEXT PK, source_kind TEXT, source_id TEXT, target_kind TEXT, paper_id TEXT, object_id TEXT, url TEXT, path TEXT, label TEXT, created_at TEXT)` with indexes on items(kind, updated_at), refs(source_kind, source_id), refs(paper_id), refs(object_id)
- [x] 1.3 Item registry API: `create_item`, `rename_item`, `touch_item` (bump updated_at), `delete_item` (tombstone), `list_items(kind: Option)` excluding tombstones, sorted by updated_at desc
- [x] 1.4 Refs API: `add_ref`, `remove_ref`, `refs_from(source)`, `refs_to_paper(paper_id)` / `refs_to_object(object_id)` — backlink queries exclude tombstoned sources
- [x] 1.5 Export API: `export_item_markdown/json` (content export delegated per kind via a registry of exporters; v1 exports registry metadata + raw JSON) and `export_all(dir)` grouping by kind, returning counts
- [x] 1.6 Unit tests: create/migrate from empty, forward migration, refuse-newer, tombstone semantics, recency bump, refs both directions, export-all counts

## 2. Tauri surface

- [x] 2.1 Add `WorkspaceStore` to `AppState`, opening `<library root>/workspace.db` (decision 10) — and verify the paper scanner (`Library::list`) and sync engine ignore `workspace.db` + `-wal`/`-shm` sidecars; exclude the WAL sidecars from sync
- [x] 2.2 Commands: `workspace_items_list(kind?)`, `workspace_item_delete`, `workspace_item_rename`, `workspace_refs_to_paper(paper_id)`, `workspace_export_all(dir)` — registered in the invoke handler
- [x] 2.3 Frontend types in `src/types.ts`: `WorkspaceItem { id, kind, title, created_at, updated_at }`

## 3. Unified library shell

- [x] 3.1 Filter chips row (All / Research / Notes / Canvases / Threads) above the list; selection persisted to localStorage (`library-filter`); chips always visible, empty-kind state explains creation (no dead buttons for unshipped kinds)
- [x] 3.2 Merge papers + workspace items into one recency-sorted list (client-side merge of `list_papers` + `workspace_items_list` by updated_at/imported_at)
- [x] 3.3 `WorkspaceItemCard` with kind icon + title + updated date + delete (typed-title confirm reused); `iconFor(kind)` helper; paper cards unchanged
- [x] 3.4 Omnibar: workspace items searchable by title alongside papers; selecting opens the kind's surface (routing stub until each feature lands)

## 4. Verification

- [x] 4.1 `cargo test -p copilot-core -- workspace` green; `npx tsc --noEmit` green
- [ ] 4.2 Manual: fresh workspace creates workspace.db; filter chips filter and persist; mixed-recency ordering correct with a seeded item; export-all writes files and reports counts
