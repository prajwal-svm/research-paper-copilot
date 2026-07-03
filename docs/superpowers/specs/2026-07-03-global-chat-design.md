# Global Chat & Workspace Store — Design

Date: 2026-07-03
Status: draft — pending user review

## Purpose

A workspace-level chat — spawnable from anywhere, ChatGPT/Claude-grade in
rendering quality — where the user can reference research artifacts (papers,
sections, equations, figures, tables), web URLs/blogs, images, files, and raw
PDFs. Distinct from the existing per-paper chat, which stays tied to its
research space and paper context.

This design also establishes the **workspace store**: the app is a workspace
platform (AFFiNE/HackMD-like), not paper-centric. Independent notes,
Excalidraw-style canvases, and workflows are planned future citizens. Global
chat is the first tenant of that store, so the storage layer is designed for
them from day one.

## Decisions (user-confirmed)

- **Surfaces:** both an overlay (spawn from anywhere) and a full chat screen,
  sharing one chat store and one view component.
- **Referencing:** explicit @-mentions only in v1 — no automatic RAG.
- **Reference types (all v1):** library papers & objects, images & files,
  web URLs/blogs, raw PDFs not in the library.
- **Reasoning/thinking display:** deferred to v2 (requires a separate
  reasoning delta channel through the Rust stream pipeline).
- **Storage:** SQLite workspace store — local-first, easily synced, easily
  exported. Not tied to any `.research` bundle.
- **Design system:** keep shadcn + AI Elements architecture; Geist is applied
  as a theme (fonts + tokens), tracked as a separate task.

## Architecture

### Storage — two layers

1. **`.research` bundles** — unchanged. Per-paper artifacts including the
   existing per-object chat journals.
2. **`workspace.db`** — one SQLite file at the workspace root (rusqlite,
   `bundled` feature; no system dependency) for everything paper-independent.
   Single file ⇒ trivially synced/backed up by any file sync; export
   commands make content portable.

### Schema v1

Migrations via `PRAGMA user_version`; later features (notes, canvases,
workflows) add tables without redesign.

```sql
chats(
  id TEXT PRIMARY KEY,          -- uuid
  title TEXT NOT NULL,          -- auto-titled from first exchange, renameable
  created_at TEXT NOT NULL,     -- rfc3339
  updated_at TEXT NOT NULL,
  deleted_at TEXT               -- tombstone (sync-friendly soft delete)
);

chat_messages(
  id TEXT PRIMARY KEY,
  chat_id TEXT NOT NULL REFERENCES chats(id),
  role TEXT NOT NULL,           -- user | assistant
  content TEXT NOT NULL,        -- markdown; mention tokens as ref:// URIs
  action TEXT,                  -- ask | ... (mirrors journal semantics)
  incomplete INTEGER NOT NULL DEFAULT 0,
  edited INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

refs(                            -- generic backlink table: knowledge-graph seed
  id TEXT PRIMARY KEY,
  source_kind TEXT NOT NULL,    -- chat_message | (future: note, canvas, ...)
  source_id TEXT NOT NULL,
  target_kind TEXT NOT NULL,    -- paper | object | url | file | pdf
  paper_id TEXT,
  object_id TEXT,
  url TEXT,
  path TEXT,
  label TEXT,
  created_at TEXT NOT NULL
);
```

Edit/delete are soft (tombstones + `edited` flag), matching the append-only
honesty of the journal chats. Rows carry `updated_at` so a future sync layer
can merge; CRDTs (Yjs/AFFiNE-style) are explicitly out of scope until
real-time collaboration matters — nothing in this schema blocks them.

### Core (Rust)

New module `workspace` in `copilot-core` (or sibling crate if it grows):

- `WorkspaceStore::open(path)` — open/create + migrate.
- Chat CRUD: `chats()`, `create_chat`, `rename_chat`, `delete_chat`,
  `messages(chat_id)`, `append_message`, `edit_message`, `delete_message`,
  plus ref rows written alongside user messages.
- Export: `export_chat_markdown(chat_id)`; a full-workspace JSON export.
- Backlink query: `refs_to(paper_id)` — "which chats/notes reference this
  paper" (feeds the knowledge graph later).

### Reference resolution (send time)

`ChatRef` = `Paper | Object | Url | File | Pdf`. A context assembler resolves
refs into model context per message:

- **Paper/Object** — semantic-tree text from the bundle (same path the
  per-object chat context uses today).
- **Images/files** — the existing attachment pipeline (images as multimodal
  content; text files as fenced context blocks, 60k-char clamp).
- **URL/blog** — new Tauri command `fetch_url_context(url)`: reqwest +
  readability extraction → markdown, cached in the workspace so re-asking
  doesn't refetch.
- **Raw PDF** — `extract_pdf_text(path)` via the existing pdfium integration;
  offer "ingest into library" as a follow-up affordance, not a blocker.

### Streaming

`global_chat_stream` Tauri command reusing the `ai.rs` streaming machinery
and provider selection (Strong class). Persistence semantics identical to
per-object chats: user turn recorded before streaming, assistant turn after,
`incomplete: true` on mid-stream failure. Events reuse the existing
`ai-stream` event shape with `request_id`.

## UI

One `GlobalChatView` component, two hosts:

- **Overlay** — floating panel summonable from any screen (hotkey +
  Omnibar), with an expand-to-full-screen button.
- **Chat screen** — sidebar (searchable chat list, new chat, rename/delete)
  + conversation pane.

Built from AI Elements: `Conversation` (auto-scroll), `Message`/`Response`
(streamed markdown), `Sources` (which refs fed the answer), `Suggestion`
(follow-up chips), `Actions` (copy/retry/edit/delete), `CodeBlock`. Composer
is the `PromptInput` implementation shipped 2026-07-03 in ObjectPanel
(multiline, Enter/Shift+Enter, paste/drop attachments, dictation, model
indicator) plus an **@-mention popover** built on the existing `Command`
component: `@` → search papers → drill into objects → inserts a chip; the
message stores `ref://paper/<id>` / `ref://object/<paper>/<id>` URIs so
rendering is clickable navigation (ObjectLinkedText pattern).

**Omnibar:** "New global chat", "Continue: <chat title>" (fuzzy over titles).

## Error handling

- No provider configured → existing `NoProviderNotice` flow.
- URL fetch failure → inline error chip on the mention with retry; message
  can still send without that context.
- Binary/unreadable file → same refusal message as `load_attachment`.
- Mid-stream failure → incomplete turn persisted, Retry affordance
  (matches ObjectPanel).

## Testing

- Rust: `WorkspaceStore` unit tests (CRUD, migrations from empty and v1,
  tombstone semantics, ref queries); context assembler tests per ref type;
  URL extraction against fixture HTML.
- TS: typecheck + existing component test setup; manual pass on both
  surfaces (overlay from Library, Reader; full screen).

## Out of scope (v2+)

- Reasoning/thinking stream + `Reasoning` component rendering.
- Automatic retrieval (RAG) over the library; cross-paper embedding index.
- Notes (BlockNote) and canvas (Excalidraw) surfaces — they join the
  workspace store using the same `refs` table.
- Real-time sync / CRDTs; branch/regenerate UI.
