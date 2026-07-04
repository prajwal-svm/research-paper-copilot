# Design — Independent Notes

## Context

`workspace-store` is implemented: `workspace.db` (items registry + refs), unified library with filter chips, Omnibar stubs. BlockNote (`@blocknote/core|react|shadcn` 0.51.x) is already a dependency, used by the small `MarkdownEditor` (lazy-loaded, markdown-in/out). The user's bar: "AFFiNE identical — blocks and all" — AFFiNE's page editor is itself BlockNote-class block editing, so the default BlockNote experience with the shadcn theme meets it. Decisions from brainstorm: mentions of papers/objects, AI assist via our providers, quick capture.

## Goals / Non-Goals

**Goals:**
- Full-page note editor with the complete BlockNote default suite (slash menu, drag handles, side menu, all default blocks) themed to the app.
- Notes persisted as BlockNote JSON + derived markdown, autosaved (debounced), recency-bumped.
- `@` mentions → chips + refs rows + navigation; deletion reconciles refs.
- AI assist (improve/summarize/expand/continue) through existing providers, streaming, accept/discard.
- Quick capture (library button, Omnibar, hotkey).

**Non-Goals:**
- Real-time collaboration, comments, page links between notes (later; refs table already supports note→note when needed).
- Databases/kanban blocks (AFFiNE-Pro features beyond BlockNote defaults).
- BlockNote XL packages (AGPL/commercial) — excluded for licensing.

## Decisions

1. **Content storage: BlockNote JSON as source of truth, markdown mirror derived on save.** Migration v2 adds `notes(id TEXT PK REFERENCES items(id), content TEXT NOT NULL, markdown TEXT NOT NULL DEFAULT '')`. Markdown comes from `blocksToMarkdownLossy` client-side at save time; used for export and future search. Alternative (markdown as source) loses block fidelity (tables, nested structures).
2. **Autosave**: debounce ~800 ms on `onChange`, plus flush on blur/unmount; each save calls `note_save(id, content, markdown)` which updates the row and `touch_item`. No dirty-state UI — the store is the truth.
3. **Mentions as BlockNote custom inline content** (`createReactInlineContentSpec`), props: `{ targetKind: "paper"|"object", paperId, objectId?, label }`. Suggestion menu via BlockNote's `SuggestionMenuController` with trigger `"@"`, items from `list_papers` (then drill-in objects via the paper's semantic tree, fetched lazily). Chip click → `onOpenPaper(paperId)` routing (reuses the App-level open-paper path; object-level navigation lands via existing goToObject once the reader opens).
4. **Refs reconciliation on save, not on keystroke**: after each autosave, diff the mentions present in the document against `refs_from("note", id)` and add/remove rows accordingly. Simpler and self-healing versus tracking insert/delete events; cost is O(mentions) per save.
5. **AI assist as a custom formatting-toolbar button + slash items** calling a new `note_ai(action, text, context)` Tauri command (thin wrapper over the provider `stream_chat`, same events as `ai_stream`). Streaming preview renders in a popover with Accept (replaces selection / inserts below) and Discard. Avoids `@blocknote/xl-ai` (license) and keeps provider selection identical to the rest of the app.
6. **Surface routing**: `App.tsx` gains a `note:<id>` view state alongside library/reader (same pattern as reader routing). Library card click and Omnibar select route there; Esc/back returns to library.
7. **Editor component**: new `NoteEditor.tsx` (full page, lazy-loaded like MarkdownEditor to keep BlockNote out of the base bundle). `MarkdownEditor` stays as-is for chat/note dialogs.
8. **Quick capture hotkey**: ⌘⇧N (registered in the same global key handler as ⌘K), plus "New note" in Omnibar commands and a library button on the Notes chip empty state and header split-button menu.
9. **Export**: store's `export_item_markdown` for kind `note` returns front matter + stored markdown mirror (replacing the metadata-only stub); export-all unchanged otherwise.

## Risks / Trade-offs

- [BlockNote default styling may not match the app theme] → `@blocknote/shadcn` maps onto our shadcn tokens (already proven by MarkdownEditor); verify dark mode + primary color variable.
- [Refs diff-on-save could miss rapid delete-then-close] → flush-on-unmount save runs the same reconciliation; acceptable.
- [Large notes re-serialize JSON+markdown per save] → debounce + BlockNote docs are small relative to paper artifacts; profile before optimizing.
- [Object drill-in menu could be slow on huge papers] → lazy-fetch tree only when the user descends into a paper; cap listed objects with search filtering.

## Migration Plan

Store migration v1→v2 (additive table); no data migration. Rollback: the notes table is ignored by older code paths (registry rows would show as items without surface — acceptable for a dev rollback, none shipped yet).

## Open Questions

- Hotkey final binding (⌘⇧N proposed) — confirm it doesn't collide in the webview.
- Whether images pasted into notes store as blobs in the DB or files beside it (proposal: files under `<library root>/workspace-assets/`, referenced by path — keeps the DB lean; decide during implementation).
