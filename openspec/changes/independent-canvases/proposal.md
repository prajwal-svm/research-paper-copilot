# Independent Canvases

## Why

Canvases are the third first-class workspace citizen (after notes): freeform, paper-independent Excalidraw boards for sketching architectures, mapping ideas across papers, and thinking visually — "similar to Miro." The workspace store and unified library are live; the per-paper concept map (`GraphView`) already proves the Excalidraw integration, scene persistence, PNG export, structure summary, and Ask-AI streaming. This change lifts that capability out of the paper bundle into standalone, library-listed canvases and adds the two AI tiers the user asked for: ask-about and AI-proposes-edits (approved via the Confirmation component).

## What Changes

- New **canvas** entity kind in the workspace store: Excalidraw scene JSON (elements + appState + files) stored in `workspace.db`, joined to the `items` registry; create/rename/delete; a rendered PNG thumbnail for the library card.
- **Full-page Excalidraw editor** (already a dependency): freeform drawing, self-hosted fonts/assets (offline-first, as `GraphView` already does), autosaved scene.
- **Pin paper content** onto a canvas: `@`/command insertion of papers, figures, equations — papers/equations as labeled cards, figures as their extracted PNG images — each writing a row to the workspace `refs` table (backlinks → knowledge graph) and navigable to the reader.
- **Ask AI about the canvas**: reuse the concept-map pattern — export PNG (vision models) + a structural text summary (every model) — streamed answer in a panel. Conversation anchored per-canvas in the workspace chat thread (once chat-threads lands; interim: transient).
- **AI proposes canvas edits**: an instruction ("add a data-flow diagram of X", "lay these out as a pipeline") returns Excalidraw element skeletons; the additions are previewed and **approved via the AI Elements Confirmation component** before merging onto the canvas — never silently applied.
- **Library + Omnibar integration**: canvases list in the unified library with thumbnails, open into the editor, rename/delete; "New canvas" entry points; Omnibar routing becomes real navigation.
- **Export**: canvas → `.excalidraw` (JSON) and PNG via the store's export paths.

## Capabilities

### New Capabilities
- `independent-canvases`: the canvas entity — editor, scene persistence + thumbnails, pinned paper content with backlinks, Ask-AI, AI-proposed edits with confirmation, and export.

### Modified Capabilities

<!-- none: unified-library (workspace-store) already specifies creation entry points and routing as kinds land -->

## Impact

- **Rust**: workspace store migration v3 — `canvases(id REFERENCES items, scene TEXT /* Excalidraw JSON */, thumbnail TEXT /* PNG data URL or path */)`; canvas CRUD; `canvas_ai_edit(instruction, summary, image)` streaming command returning proposed element skeletons; canvas export.
- **Frontend**: `CanvasEditor.tsx` (full-page Excalidraw — distinct from the per-paper `GraphView`), reusing `canvasSummary`, PNG export, and the Ask-AI stream; pin-content suggestion menu; AI-edit Confirmation flow; App routing to the canvas surface; Library/Omnibar wiring; thumbnail generation on save.
- **Dependencies**: none new — `@excalidraw/excalidraw` already shipped; Confirmation component already installed.
- **Depends on**: `workspace-store` (items registry, refs, unified library) and the patterns proven in `independent-notes` (surface routing, autosave, mention refs).
