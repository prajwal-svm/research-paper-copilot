# Tasks — Independent Notes

## 1. Store & backend

- [x] 1.1 Workspace store migration v2: `notes(id TEXT PK REFERENCES items(id), content TEXT NOT NULL, markdown TEXT NOT NULL DEFAULT '')`; store API `note_create`, `note_get`, `note_save(content, markdown)` (touches item), note-aware `export_item_markdown` (front matter + markdown mirror)
- [x] 1.2 Unit tests: migration v1→v2, note CRUD, autosave touch bumps recency, export contains content
- [x] 1.3 Tauri commands: `note_create`, `note_get`, `note_save`, registered in the handler
- [x] 1.4 `note_ai(action, text, context)` command streaming via the provider system (same event shape as ai_stream, cancellable)

## 2. Editor surface

- [x] 2.1 `NoteEditor.tsx` (lazy): full BlockNote editor (shadcn theme, default blocks, slash menu, drag handles), loads note by id, debounced autosave (~800 ms) + flush on unmount, editable title syncing `workspace_item_rename`
- [x] 2.2 App routing: `note:<id>` view state; Library card click and Omnibar select open it; back/Esc returns to library
- [x] 2.3 Theme pass: dark mode + primary color across editor UI (menus, selection, handles)

## 3. Mentions & backlinks

- [x] 3.1 Mention inline-content spec (chip: label + kind icon) with `@` SuggestionMenuController — papers list, drill-in to objects (lazy tree fetch, filterable)
- [x] 3.2 Refs reconciliation on save: diff document mentions vs `refs_from("note", id)`, add/remove rows
- [x] 3.3 Chip click navigation: open paper (and object once reader is open); backlink visible via `workspace_refs_to_paper`

## 4. AI assist

- [x] 4.1 Formatting-toolbar AI button + slash items (Improve, Summarize, Expand, Continue writing) → `note_ai` streaming popover with Accept/Discard; no-provider notice reuses `NoProviderNotice`

## 5. Quick capture & integration

- [x] 5.1 "New note" in Omnibar commands, library header (split button menu) and Notes-chip empty state; global hotkey (⌘⇧N) — all create + open in one step
- [x] 5.2 Omnibar workspace items: replace the routing stub for kind `note` with real navigation
- [x] 5.3 Library: note cards open the editor; rename inline via card menu (uses `workspace_item_rename`)

## 6. Verification

- [x] 6.1 `cargo test -p copilot-core -- workspace` and `npx tsc --noEmit` green
- [ ] 6.2 Manual: create → type blocks (table, list, code) → quit → reopen intact; @-mention a paper then check `workspace_refs_to_paper`; delete mention removes ref; AI improve round-trip; quick capture from reader; export-all contains the note's markdown
