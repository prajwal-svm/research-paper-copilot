# Tasks — Chat Threads (Global Chat)

## 1. Store & backend

- [x] 1.1 Workspace store migration v4: `chats(id TEXT PK REFERENCES items(id))` and `chat_messages(id TEXT PK, chat_id TEXT REFERENCES chats(id), role TEXT, content TEXT, action TEXT, incomplete INTEGER, edited INTEGER, created_at TEXT, updated_at TEXT, deleted_at TEXT)` with an index on `chat_messages(chat_id, created_at)`
- [x] 1.2 Store API: `chat_create`, `chat_get`, `chat_messages`, `chat_append_message`, `chat_edit_message`, `chat_delete_message`, `chat_set_title`, `chat_sync_refs` (reuse `sync_refs`), and chat-aware `export_item_markdown` (ordered transcript + references)
- [x] 1.3 Unit tests: migration v3→v4, message append/edit(marked)/delete(tombstone), recency bump on append, refs reconciliation, transcript export contains ordered turns

## 2. Reference resolution

- [x] 2.1 `fetch_url_context(url)` Tauri command: ureq GET → strip script/style + tag-strip + whitespace-collapse → readable text; cache (workspace-assets/url-cache or a table); typed errors surfaced inline
- [x] 2.2 `extract_pdf_text(path)` Tauri command: pdfium `page.text()` across pages; clamp; refuse non-PDF clearly
- [x] 2.3 `ChatRef` resolver assembling context per ref type (paper/object semantic-tree text, image/file via attachment pipeline, url via 2.1, pdf via 2.2); unit-test the assembler per type

## 3. Streaming

- [x] 3.1 `chat_stream(chatId, requestId, content, refs, images)` command: assemble context, record user turn, stream via provider (Strong) with `ai-stream` events, record assistant turn or `incomplete` partial on failure/cancel
- [x] 3.2 Auto-title after first exchange: best-effort Light-tier naming (`chat_set_title`), fallback to truncated first message

## 4. AI Elements + composer popovers

- [x] 4.1 Install `sources` and `actions` AI Elements via the registry; typecheck-fix any shipped `@ts-expect-error`/v6 issues (as done for confirmation)
- [x] 4.2 Composer popovers on `PromptInput`: `@` mention popover (reuse notes menu, extended with URL / file / raw-PDF ref entries) and `/` slash-action popover (Command), driven by an active-token trigger detector so they coexist
- [x] 4.3 Mention rendering: extend the ObjectLinkedText-style parser for `ref://url/…` and `ref://file/…` and `ref://object/<paper>/<id>`; chips navigate where applicable

## 5. GlobalChatView + surfaces

- [x] 5.1 `GlobalChatView` (host-agnostic, takes `chatId`): `Conversation` + `Message`/`Response` + `Sources` + `Suggestion` + `Actions` (copy/retry/edit/delete) + the `PromptInput` composer; streams via `chat_stream`; `NoProviderNotice` on no provider
- [x] 5.2 Full-screen host: `chat:<id>` App route with a searchable sidebar (chat list, new/rename/delete) + conversation pane
- [x] 5.3 Overlay host: an app-root layer summonable by hotkey + Omnibar, with expand-to-full-screen; renders `GlobalChatView` for the active chat
- [x] 5.4 Slash actions v1 (`/summarize`, `/explain`, `/search`) as data-driven prompt templates applied to the conversation

## 6. Integration

- [x] 6.1 Library: chats list (kind `chat`) open the full screen; rename/delete via card
- [x] 6.2 Omnibar: "New chat" command + "Continue: <title>" entries; workspace-item routing for kind `chat`; overlay-open command; hotkey
- [x] 6.3 App wiring: `onOpenChat`, overlay state, hotkey registration

## 7. Verification

- [x] 7.1 `cargo test -p copilot-core -- workspace` green; URL/PDF/assembler tests green; `npx tsc --noEmit` and `vite build` green
- [ ] 7.2 Manual: new chat → send with a paper @-mention → answer streams, persists, Sources lists the ref; edit/retry/delete a message; `/summarize` runs; reference a URL (fetch + inline error path) and a raw PDF; overlay spawns from reader and expands; export-all writes the transcript
