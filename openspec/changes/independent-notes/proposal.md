# Independent Notes

## Why

The workspace vision is AFFiNE-class: notes as first-class, paper-independent entities — not annotations trapped inside a paper bundle. The workspace store and unified library shell (change: `workspace-store`) are implemented and waiting; notes are the first content kind to land on them. The user's bar is explicit: "AFFiNE identical — blocks and all."

## What Changes

- New **note** entity kind in the workspace store: BlockNote block-document content, stored as BlockNote JSON with a derived markdown mirror (export/search), joined to the `items` registry.
- **Full-page block editor** (BlockNote, already a dependency via MarkdownEditor): headings, lists, checkboxes, code blocks, tables, images, quotes, dividers — the full default block palette with slash-menu, drag handles, and side menu (the AFFiNE-grade editing feel).
- **@-mentions of papers and objects** inside notes: custom BlockNote inline content that searches the library (papers → sections/equations/figures), renders as a chip, navigates on click, and writes rows to the workspace `refs` table (backlinks → knowledge graph).
- **AI assist in the editor**: selection/slash actions (continue writing, improve, summarize, expand) wired to the existing provider system — not BlockNote's AGPL XL AI package.
- **Quick capture**: a global "New note" entry in the Omnibar and library (and hotkey) that creates a note and opens it immediately.
- **Library + Omnibar integration**: notes list in the unified library (chips already exist), open into the editor, rename/delete; Omnibar routing stub becomes real navigation.
- **Export**: note → markdown file (the store's export-all gains real note content).

## Capabilities

### New Capabilities
- `independent-notes`: the note entity — editor, block palette, paper/object mentions with backlinks, AI assist, quick capture, and export.

### Modified Capabilities

<!-- none: unified-library (from workspace-store) already specifies creation entry points and routing as kinds land; requirements unchanged -->

## Impact

- **Rust**: workspace store migration v2 — `notes(id REFERENCES items, content TEXT /* BlockNote JSON */, markdown TEXT)`; note CRUD commands; note content export replaces the metadata-only markdown export for kind `note`.
- **Frontend**: `NoteEditor` view (BlockNote full editor — distinct from the small `MarkdownEditor` dialog editor); mention inline-content spec + suggestion menu; AI assist menu; App routing to the note surface; Library/Omnibar wiring.
- **Dependencies**: none new — `@blocknote/{core,react,shadcn}` already shipped.
- **Depends on**: `workspace-store` (items registry, refs table, unified library, filter chips).
