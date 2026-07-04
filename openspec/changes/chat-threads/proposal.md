# Chat Threads (Global Chat)

## Why

The committed design `docs/superpowers/specs/2026-07-03-global-chat-design.md` calls for a workspace-level chat — spawnable from anywhere, ChatGPT/Claude-grade — where the user references research artifacts, URLs, files, and raw PDFs, distinct from the paper-anchored per-object chat. It is the flagship workspace citizen and the fourth kind on the store (after notes and canvases, which proved the routing/mention/refs patterns). This change implements that design in full, plus the slash-action composer popover the user asked for.

## What Changes

- New **chat** entity kind in the workspace store: `chats` + `chat_messages` tables (migration v4), joined to the `items` registry; auto-titled from the first exchange, renameable; soft-deleted.
- **Two surfaces, one `GlobalChatView`**: an **overlay** panel summonable from anywhere (hotkey + Omnibar) with expand-to-full-screen, and a **full chat screen** (sidebar of chats + conversation pane). Both share the store and the view component.
- **@-mention references (all v1 types)**: library papers & objects, images & files, web URLs/blogs, and raw PDFs. Mentions render as chips, store `ref://…` tokens in the message, write `refs` rows (backlinks), and are navigable.
- **Reference resolution at send time**: papers/objects → semantic-tree text; images/files → the existing attachment pipeline; URLs → a new `fetch_url_context` (fetch + readability extraction, cached); raw PDFs → pdfium text extraction.
- **Streaming** via a `chat_stream` command reusing `ai.rs` machinery and the `ai-stream` event contract; persistence mirrors the per-object chat (user turn before, assistant turn after, `incomplete` on mid-stream failure).
- **Slash-action composer popover**: `/` opens actions (`/summarize`, `/explain`, `/search`…) on the `Command` component, alongside the `@` mention popover.
- **AI Elements rendering**: `Conversation` (auto-scroll), `Message`/`Response`, `Sources` (which refs fed the answer), `Suggestion` (follow-ups), `Actions` (copy/retry/edit/delete). Reuses the shipped `PromptInput` composer.
- **Omnibar**: "New chat" and "Continue: <title>" entries; library lists chats; canvases' transient Ask-AI can later persist here (out of scope to wire now).
- **Export**: chat → markdown transcript via the store's export paths.

## Capabilities

### New Capabilities
- `chat-threads`: the chat entity — store, both surfaces, mentions + slash actions, reference resolution (incl. URL/PDF), streaming, rendering, and export.

### Modified Capabilities

<!-- none: unified-library (workspace-store) already specifies creation entry points and routing as kinds land -->

## Impact

- **Rust**: workspace store migration v4 (`chats`, `chat_messages`); chat CRUD + message append/edit/delete + `export_chat_markdown`; `chat_stream` streaming command; `fetch_url_context(url)` (ureq + readability, cached in the store); `extract_pdf_text(path)` (pdfium).
- **Frontend**: `GlobalChatView` (overlay + full-screen hosts); mention popover (reusing the notes pattern) extended with URL/file/PDF ref types; slash-action popover; App routing (`chat:<id>`) + an overlay layer; Library/Omnibar wiring; new AI Elements `sources` + `actions` components.
- **Dependencies**: none new (ureq, pdfium, base64 already present); add the two AI Elements components via the registry.
- **Depends on**: `workspace-store`, and the `independent-notes` / `independent-canvases` patterns (surface routing, mention/refs reconciliation, the `PromptInput` composer).
