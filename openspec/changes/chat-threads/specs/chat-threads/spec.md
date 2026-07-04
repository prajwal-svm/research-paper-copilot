# chat-threads

## ADDED Requirements

### Requirement: Chats are first-class workspace entities
The system SHALL support chat threads as paper-independent entities in the workspace store: a `chats` row (registry-joined, kind `chat`) with `chat_messages` rows. Chats SHALL auto-title from the first exchange, be renameable, and be soft-deleted (tombstone). Messages SHALL persist as they stream; a mid-stream failure SHALL persist the partial assistant turn marked `incomplete`, mirroring the per-object chat's honesty.

#### Scenario: Message persists across restart
- **WHEN** the user sends a message, receives a streamed answer, and restarts the app
- **THEN** reopening the chat shows the full exchange, and the chat sits at the top of the recency-sorted library

#### Scenario: Mid-stream failure keeps the partial
- **WHEN** a streaming answer fails partway
- **THEN** the partial assistant turn is saved marked incomplete, with a Retry affordance

#### Scenario: Auto-title from first exchange
- **WHEN** the user starts a new chat and sends the first message
- **THEN** the chat gains a concise title derived from that exchange, editable afterward

### Requirement: Two surfaces sharing one view
The chat SHALL be available as both an overlay panel summonable from any screen (hotkey and Omnibar) and a full-screen chat screen with a searchable sidebar of chats plus a conversation pane. Both surfaces SHALL render the same conversation from the same store, and the overlay SHALL offer expand-to-full-screen.

#### Scenario: Spawn overlay from anywhere
- **WHEN** the user presses the chat hotkey while reading a paper
- **THEN** a chat overlay appears over the current view without navigating away

#### Scenario: Expand to full screen
- **WHEN** the user clicks expand in the overlay
- **THEN** the same chat opens in the full-screen surface with its history intact

#### Scenario: Sidebar lists and switches chats
- **WHEN** the user opens the full chat screen and selects another chat in the sidebar
- **THEN** the conversation pane switches to that chat

### Requirement: Reference mentions of all v1 types
Typing `@` in the composer SHALL open a mention popover covering: library papers and objects (sections, equations, figures, tables), attached images and files, web URLs/blogs, and raw PDFs not in the library. Each mention SHALL render as a chip, store a `ref://…` token in the message content, write a `refs` row (source: the message/chat; target: paper/object/url/file), and be navigable where applicable. Removing a mention SHALL remove its ref row.

#### Scenario: Mention a paper section
- **WHEN** the user types "@" and selects a section of a library paper
- **THEN** a chip is inserted, a refs row exists, and the section's text is included as context when the message is sent

#### Scenario: Reference a URL
- **WHEN** the user adds a blog URL as a reference and sends
- **THEN** the URL's readable text is fetched, cached, and included as context; a fetch failure surfaces inline and the message can still send without it

#### Scenario: Reference a raw PDF
- **WHEN** the user references a PDF not in the library
- **THEN** its extracted text is included as context, with an option to ingest it into the library as a follow-up

### Requirement: Slash-action composer popover
Typing `/` at the start of the composer SHALL open an action popover (e.g. `/summarize`, `/explain`, `/search`) built on the Command component. Selecting an action SHALL apply its behavior to the message/conversation. The slash popover SHALL coexist with the `@` mention popover without conflict.

#### Scenario: Run a slash action
- **WHEN** the user types "/sum" and selects "/summarize"
- **THEN** the summarize action runs against the current conversation

#### Scenario: Slash and mention coexist
- **WHEN** the user uses "@" for a mention and "/" for an action in the same session
- **THEN** each opens its own popover without interfering

### Requirement: Streamed, richly rendered conversation
The conversation SHALL render with AI Elements: auto-scrolling conversation, streamed markdown messages, a Sources view of the references that fed each answer, follow-up Suggestion chips, and per-message Actions (copy, retry, edit, delete). Streaming SHALL use the app's provider selection and the shared `ai-stream` event contract; with no provider configured the standard no-provider notice SHALL appear.

#### Scenario: Answer shows its sources
- **WHEN** an answer is produced from two referenced papers
- **THEN** a Sources view lists those references, each navigable

#### Scenario: Edit a message
- **WHEN** the user edits one of their earlier messages
- **THEN** the edit is persisted (marked edited) without destroying the original journal semantics

#### Scenario: No provider configured
- **WHEN** the user sends a message with no provider configured
- **THEN** the no-provider notice with a Settings link appears; nothing is sent

### Requirement: Chat export
Chats SHALL be exportable without the app as a markdown transcript (roles, message content, and references), per-entity and via workspace export-all.

#### Scenario: Export a transcript
- **WHEN** the user exports a chat
- **THEN** a markdown file with the ordered exchange and its references is written to the chosen location
