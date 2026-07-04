# Design — Workspace Store & Unified Library

## Context

The product direction (memory: research-workspace-vision; committed design doc `docs/superpowers/specs/2026-07-03-global-chat-design.md`) is an AFFiNE-like workspace: independent notes, canvases, and chat threads alongside `.research` paper bundles. Three upcoming changes (`independent-notes`, `independent-canvases`, `chat-threads`) all need one durable, local-first, syncable, exportable store and a Library that lists mixed content. Today: papers live in bundles under the library root; `rusqlite` is already used for the cache-class `graph.db` index; the Library lists papers only.

User decisions already made: SQLite over file journals for workspace entities; unified list + filter chips (All/Research/Notes/Canvases/Threads) sorted by last-updated; type icon per card; everything exportable.

## Goals / Non-Goals

**Goals:**
- One `workspace.db` (rusqlite bundled) with versioned migrations, item registry, refs backlink table, tombstones.
- Library shell that lists papers + workspace items uniformly with filter chips and recency sort.
- Export paths (per-entity + whole-workspace) to markdown/JSON.
- A stable Rust API + Tauri command surface the three feature changes can build on without schema redesign.

**Non-Goals:**
- No notes/canvas/threads content tables or UIs (their changes own those, added via migrations).
- No sync engine, no CRDTs (schema is merge-ready; engine comes later).
- No changes to `.research` bundle format or paper flows.

## Decisions

1. **Single DB file at the workspace root, opened by `WorkspaceStore`** (new `copilot-core` module `workspace.rs`). Alternative: per-entity JSONL journals (like per-object chats) — rejected by user decision: SQLite queries power the unified list, backlinks, and future sync more simply, and one file syncs/backs up trivially.
2. **Generic `items` registry + per-kind content tables.** `items(id, kind, title, created_at, updated_at, deleted_at)` is the listing/recency source of truth; feature changes add content tables (e.g. `notes(id REFERENCES items, content …)`) via migrations. Alternative: one polymorphic content blob table — rejected: kind-specific columns and indexes stay clean, and migrations localize risk.
3. **`refs` is generic from day one**: `refs(id, source_kind, source_id, target_kind, paper_id, object_id, url, path, label, created_at)` with indexes on (source_kind, source_id) and (paper_id), (object_id). Backlink queries join through `items` to exclude tombstoned sources. This is the knowledge-graph seed; notes/canvases/threads all write the same rows.
4. **Migrations via `PRAGMA user_version`**, forward-only, applied in a transaction on open; refuse-newer-than-known. Alternative (migration table) adds ceremony without benefit at this scale.
5. **Timestamps as RFC 3339 TEXT** (matches bundle metadata conventions and `now_rfc3339()`); sorting works lexicographically.
6. **Library merge strategy**: frontend fetches `list_papers` (existing) and `workspace_items_list` (new), merges client-side by `updated_at`. Alternative (backend merged endpoint) couples the store to the paper library; client merge keeps the store paper-agnostic.
7. **Filter chip persistence** in `localStorage` (`library-filter`), consistent with other per-machine UI state (dock offset, primary color).
8. **Type icons**: research `BookOpen`/existing, note `NotebookText`, canvas `Frame`/`PenTool`, thread `MessagesSquare` — one iconFor(kind) helper so feature changes only register a kind.
9. **Store handle lifecycle**: opened lazily on first workspace command, held in `AppState` behind a Mutex like `library`; rusqlite connections are cheap so a single connection with `busy_timeout` suffices (all access via Tauri commands on the store thread).
10. **`workspace.db` lives inside the library root** (`<app data dir>/library/workspace.db`, beside the `.research` bundles) — user-approved. One folder is the whole workspace: backup and sync of the library root carry workspace entities automatically. Implementation MUST verify the paper scanner and sync engine ignore foreign files (`workspace.db`, `workspace.db-wal`, `workspace.db-shm`) — and exclude the WAL sidecars from sync, since they are transient mid-write state. Alternative (beside `library/` in the app data dir) was rejected: it would exempt workspace entities from the existing sync/backup path, defeating the single-syncable-root rationale.

## Risks / Trade-offs

- [DB corruption risks the whole workspace] → SQLite WAL mode + the file lives in the synced/backed-up workspace root; export-all provides a portable escape hatch; papers are unaffected (separate storage).
- [Client-side merge of two lists could jank with large libraries] → both lists are lightweight metadata; virtualize only if profiling demands.
- [Filter chips for not-yet-shipped kinds confuse users] → chips render, but empty states say "coming soon"/creation guidance only once the kind is implemented; no dead buttons (per spec).
- [Schema decisions here constrain three future changes] → the three feature designs were already drafted against this schema (global-chat design doc); registry + migrations give them room without redesign.

## Migration Plan

Greenfield: no existing data to migrate. `workspace.db` appears on first open after upgrade (v1 schema: `items`, `refs`). Rollback = delete the file (papers untouched). Feature changes bump `user_version` with their tables.

## Open Questions

- Whether `workspace_export_all` also zips (defer; directory export first).
