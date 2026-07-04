# Design — Chat Threads (Global Chat)

## Context

The committed design doc `docs/superpowers/specs/2026-07-03-global-chat-design.md` specifies this feature end to end; this design records how it lands on the now-implemented foundation. Available:
- `workspace-store`: items registry, generic `refs` table, `sync_refs` (shared by notes + canvases), unified library, `user_version` migrations, export-all.
- `independent-notes`: `@`-mention popover on the `Command`/BlockNote pattern, refs reconciliation on save, `note_ai` streaming, App surface routing (`note:<id>`), `NoProviderNotice`.
- `independent-canvases`: another surface (`canvas:<id>`), the `canvas_ai_edit` streaming pattern, shared canvas helpers.
- The shipped `PromptInput` composer (multiline, attachments, dictation, model indicator).
- AI Elements installed: `conversation`, `message` (with Response/CodeBlock), `suggestion`, `confirmation`, `prompt-input`. NOT installed: `sources`, `actions`.
- `ureq` (HTTP), pdfium (PDF text via `page.text()`), base64 — all present. No readability extractor yet.
- The per-object `ai_stream` command is the reference implementation for streaming + persistence.

## Goals / Non-Goals

**Goals:**
- Chats in the store (migration v4), auto-titled, renameable, soft-deleted; messages stream and persist with incomplete-turn honesty.
- One `GlobalChatView` driving an overlay (spawn anywhere, expand) and a full screen (sidebar + pane).
- `@` mentions across all five ref types; `/` slash actions; both on the `Command` popover.
- Reference resolution at send time (papers/objects, images/files, URL readability, raw-PDF text).
- AI Elements rendering incl. Sources + Actions; markdown transcript export.

**Non-Goals (v2, per the design doc):**
- Reasoning/thinking stream + Reasoning component.
- Automatic RAG over the library.
- Real-time sync/CRDTs; branch/regenerate UI.
- Rewiring the canvas Ask-AI to persist here (later).

## Decisions

1. **Schema (migration v4)** exactly as the design doc: `chats(id PK REFERENCES items, ...)` — but since the `items` registry already holds title/timestamps/tombstone, the `chats` table carries only chat-specific columns (none needed beyond the join today, so `chats(id PK REFERENCES items)`), and `chat_messages(id PK, chat_id REFERENCES chats, role, content, action, incomplete, edited, created_at, updated_at, deleted_at)`. Title/recency/soft-delete come from `items` (consistent with notes/canvases); the design doc's standalone `chats` columns are subsumed by the registry.
2. **Message refs** reuse the generic `refs` table + `sync_refs("chat", chatId, mentions)` on send — the exact pattern notes/canvases use. Mentions are stored inline in `content` as `ref://paper/<id>` / `ref://object/<paper>/<id>` / `ref://url/<url>` / `ref://file/<path>` tokens, and rendered clickable via an ObjectLinkedText-style parser extended for the new schemes.
3. **Streaming: new `chat_stream(chat_id, request_id, content, refs, images)`** command. It assembles context from resolved refs, records the user turn, streams via `provider.stream_chat_cancellable` (Strong tier) emitting `ai-stream` events, then records the assistant turn (or an `incomplete` partial on failure/cancel) — a near-copy of `ai_stream` but writing to `chat_messages` instead of the bundle journal. Auto-title: after the first assistant turn, a cheap Light-tier call names the chat (best-effort; falls back to a truncated first message).
4. **Reference resolution (send time), a `ChatRef` resolver:**
   - paper/object → semantic-tree text (reuse the per-object context path).
   - image/file → existing attachment pipeline (images multimodal; text files fenced, 60k clamp).
   - URL → new `fetch_url_context(url)`: `ureq` GET, strip `<script>/<style>`, tag-strip + whitespace-collapse to readable text (a lightweight readability; a dedicated crate is a later upgrade), cached in a store table or `<library root>/workspace-assets/url-cache/`. Failure → inline error chip, message still sends.
   - raw PDF → `extract_pdf_text(path)` via pdfium `page.text()`; offer "ingest into library" as a follow-up, not a blocker.
5. **UI: one `GlobalChatView`** taking a `chatId`, rendered by two hosts:
   - Overlay: a right-side sheet/panel layered over the app (rendered at the App root, toggled by state + hotkey), with an expand button that routes to the full screen.
   - Full screen: `chat:<id>` route (mirrors `note:`/`canvas:`) with a sidebar (searchable chat list, new/rename/delete) + conversation pane.
   Both compose `Conversation` + `Message`/`Response` + `Sources` + `Suggestion` + `Actions` and the shipped `PromptInput`.
6. **Composer popovers:** the `PromptInput` textarea gets two `Command`-based popovers — `@` (mentions, reusing the notes menu extended with URL/file/PDF entries) and `/` (slash actions, only at message start). A small trigger-tracking hook drives which popover is open based on the token under the cursor.
7. **Install `sources` and `actions`** AI Elements via the registry; everything else is already present.
8. **Slash actions v1**: `/summarize`, `/explain`, `/search` — each a prompt template applied to the current message/conversation (search is a placeholder that composes a web-oriented question; real web search is a later capability). Actions are data-driven so more can be added.
9. **Export**: store's per-kind exporter for `chat` renders an ordered markdown transcript (`**User:** …` / `**Assistant:** …`) with a references list; export-all includes it.

## Risks / Trade-offs

- [Naive HTML→text loses structure] → v1 readability is tag-strip + whitespace-collapse; adequate for context, upgradeable to a crate without API change. Cache so a bad extraction is cheap to redo.
- [Two popovers on one textarea conflict] → a single trigger detector picks `@` vs `/` by the active token; only one popover renders at a time (spec scenario covers coexistence).
- [Overlay + full-screen sharing one view] → `GlobalChatView` is host-agnostic (takes `chatId`, emits open/close/expand); hosts own layout only. Avoids the two-implementations drift notes/canvases were careful about.
- [`chat_stream` duplicating `ai_stream`] → extract the shared streaming/emit skeleton if the copy is large; otherwise a focused near-copy writing to a different store is acceptable and lower-risk than refactoring the reader's hot path.
- [Auto-title cost] → Light tier, best-effort, one call after the first exchange; skip silently on failure.

## Migration Plan

Store migration v3→v4 (additive `chats` + `chat_messages`); no data migration. Rollback: tables ignored by older paths. URL cache is regenerable.

## Open Questions

- Overlay presentation: right sheet vs. centered command-style panel — decide during implementation against the reader's existing panels.
- Whether URL-cache lives in a store table or a `workspace-assets/` folder (folder proposed for large blobs; small enough that a table is also fine).
- Hotkey binding for the chat overlay (notes took ⌘⇧N; ⌘⇧C or ⌘J candidates) — confirm no webview collision.
