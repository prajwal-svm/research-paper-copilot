# Tasks — Independent Canvases

## 1. Store & backend

- [x] 1.1 Workspace store migration v3: `canvases(id TEXT PK REFERENCES items(id), scene TEXT NOT NULL DEFAULT '{}', thumbnail TEXT NOT NULL DEFAULT '')`; store API `canvas_create`, `canvas_get`, `canvas_save(scene, thumbnail)` (touches item), `canvas_sync_refs` (reuse the notes diff pattern), canvas-aware export (`.excalidraw` + PNG)
- [x] 1.2 Unit tests: migration v2→v3, canvas CRUD, save bumps recency, pinned-ref reconciliation, export writes scene + png
- [x] 1.3 Tauri commands: `workspace_canvas_create`, `workspace_canvas_get`, `workspace_canvas_save`, `workspace_canvas_refs_sync`, registered in the handler
- [x] 1.4 `canvas_ai_edit(instruction, summary, image)` streaming command: prompts for a JSON array of Excalidraw element skeletons; same `ai-stream` event contract, cancellable

## 2. Shared helpers refactor

- [x] 2.1 Extract `canvasSummary`, PNG-export helper, and asset-path setup from `GraphView.tsx` into `src/canvas/shared.ts`; update `GraphView` to import them (no behavior change — verify the concept map still renders and Ask-AI still works)

## 3. Canvas editor surface

- [x] 3.1 `CanvasEditor.tsx` (lazy): full Excalidraw board (theme, offline assets, freeform pan/zoom), loads scene by id, debounced autosave of scene + regenerated thumbnail, editable title syncing `workspace_item_rename`
- [x] 3.2 App routing: `canvas:<id>` view state; Library card + Omnibar open it; back returns to library
- [x] 3.3 Library card thumbnails: render stored PNG (placeholder when empty); `WorkspaceItemCard` shows canvas thumbnails

## 4. Pin paper content & backlinks

- [x] 4.1 Pin menu (command/`@`): papers → equations/figures drill-in; paper/equation → labeled rectangle skeleton with `customData.ref`; figure → Excalidraw image from `figures/<id>.png` (data URL) with `customData.ref`
- [x] 4.2 Refs reconciliation on save: scan `customData.ref` across elements, diff vs `refs_from("canvas", id)`, add/remove; backlink visible via `workspace_refs_to_paper`
- [x] 4.3 Navigation: open the referenced paper from a pinned element (linked element or selection action)

## 5. AI on the canvas

- [x] 5.1 Ask AI: reuse `canvasSummary` + PNG export + Ask-AI stream in a panel (transient); no-provider notice reuses `NoProviderNotice`
- [x] 5.2 AI-proposed edits: `canvas_ai_edit` → parse element skeletons → `convertToExcalidrawElements` → preview in the AI Elements Confirmation component → approve merges non-destructively (`updateScene`), reject discards; malformed output errors without mutating the canvas

## 6. Quick capture & integration

- [x] 6.1 "New canvas" in Omnibar commands, library header, and Canvases-chip empty state — create + open in one step
- [x] 6.2 Omnibar workspace items: real navigation for kind `canvas` (replace the stub)

## 7. Verification

- [x] 7.1 `cargo test -p copilot-core -- workspace` green; `npx tsc --noEmit` and `vite build` green
- [ ] 7.2 Manual: create → draw shapes/arrows → quit → reopen intact; pin a figure then check `workspace_refs_to_paper`; remove pin removes ref; Ask-AI references the diagram; AI-edit proposes → approve merges / reject leaves unchanged; thumbnail updates in library; export-all writes `.excalidraw` + png
