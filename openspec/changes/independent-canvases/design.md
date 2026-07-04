# Design — Independent Canvases

## Context

`workspace-store` (items registry, refs, unified library, filter chips) and `independent-notes` (surface routing, autosave, mention-refs reconciliation, lazy-loaded editor) are implemented. The per-paper concept map `GraphView.tsx` already proves the hard parts of Excalidraw integration:
- Controlled `Excalidraw` with theme, self-hosted assets (`window.EXCALIDRAW_ASSET_PATH`), offline fonts.
- Scene persistence via `canvas_get`/`canvas_save` (JSON) — but bundle-scoped (`notes/graph_canvas.json`).
- `canvasSummary(elements)` → text description of shapes + arrow bindings.
- `exportToBlob` → PNG data URL for vision models.
- Ask-AI streaming via `useAiStream` (`canvasSummary` as adhoc text + PNG as image).
- `convertToExcalidrawElements(skeletons)` to build elements from skeletons — the same path an AI-edit would produce.
- The `Confirmation` AI-element is installed (from an earlier change) and unused so far.

User decisions: "similar to Miro"; pin paper content; Ask-AI on every canvas; AI proposes elements approved via Confirmation.

## Goals / Non-Goals

**Goals:**
- Standalone canvases in the workspace store (scene JSON + thumbnail), autosaved, library-listed with thumbnails.
- Full Excalidraw board, offline-first, theme-aware.
- Pin papers/equations (cards) and figures (PNG images) with refs backlinks + navigation.
- Ask-AI (PNG + summary) reusing GraphView's proven code.
- AI-proposed additions previewed via Confirmation, merged non-destructively on approve.
- Export `.excalidraw` + PNG.

**Non-Goals:**
- Real-time multi-user canvas collaboration.
- Migrating the per-paper `GraphView` into this system (it stays bundle-scoped; shared helpers are extracted, not moved).
- Freehand handwriting recognition, infinite-canvas performance tuning beyond Excalidraw defaults.

## Decisions

1. **Scene storage: Excalidraw JSON in the store.** Migration v3 adds `canvases(id TEXT PK REFERENCES items(id), scene TEXT NOT NULL DEFAULT '{}', thumbnail TEXT NOT NULL DEFAULT '')`. `scene` is `{elements, appState, files}` serialized (files can hold pasted/pinned images as data URLs). Alternative (scene as a file beside the DB) — deferred; JSON-in-row keeps one syncable artifact and matches how the bundle canvas already stores it. Revisit only if large embedded images bloat rows (then spill `files` to `<library root>/workspace-assets/`).
2. **Thumbnail on save**: `exportToBlob({maxWidthOrHeight: 400})` → PNG data URL, stored in `thumbnail`, refreshed by the same debounced autosave that writes the scene. Empty scene → empty string → library placeholder. Alternative (render thumbnails in the library) rejected: the editor already has the scene + Excalidraw runtime; the library shouldn't load Excalidraw per card.
3. **Extract shared helpers** from `GraphView` into `src/canvas/shared.ts`: `canvasSummary`, PNG export helper, and asset-path setup — imported by both `GraphView` and the new `CanvasEditor`. Avoids divergence; no behavior change to the per-paper map.
4. **Pin paper content as a command/`@` menu** (mirroring notes' mention menu): papers → equations/figures drill-in. Insertion:
   - paper/equation → a labeled rectangle skeleton with a `customData: { ref: {paperId, objectId} }` marker.
   - figure → an Excalidraw image element from the extracted PNG (`figures/<id>.png`, fetched as data URL), also carrying the `customData.ref` marker.
   Refs reconciliation runs on save: scan elements' `customData.ref`, diff against `refs_from("canvas", id)`, add/remove — the exact self-healing pattern notes use.
5. **Navigation**: clicking a pinned element opens its paper (via the same App-level open-paper path notes use). Excalidraw doesn't expose rich per-element click easily, so pinned refs get a small "open" affordance (linked element / context action) — final interaction confirmed in implementation; the ref + reader-open plumbing is the requirement.
6. **AI-proposed edits**: new `canvas_ai_edit(instruction, summary, image)` streams like `note_ai` but the model is prompted to return ONLY a JSON array of Excalidraw element skeletons (rectangles/arrows/text with labels). Frontend parses, runs `convertToExcalidrawElements`, and shows them in a **Confirmation** panel (title = instruction, a small preview count/summary). Approve → `updateScene` merging new elements (fresh ids) with existing; reject → discard. Malformed JSON → surfaced as an error, canvas untouched. This is the first real use of the installed Confirmation component.
7. **Ask-AI stays transient for now** (streamed into a dialog, not persisted) since the workspace chat thread lands with `chat-threads`; the anchor id concept from GraphView is reused so wiring later is trivial.
8. **Editor component**: new `CanvasEditor.tsx` (full page, lazy-loaded — Excalidraw is heavy, like BlockNote). `GraphView` unchanged except importing the extracted helpers.
9. **Routing + capture**: `canvas:<id>` view in `App.tsx` (mirrors `note:<id>`); "New canvas" in Omnibar, library header, and the Canvases-chip empty state; hotkey optional (notes took ⌘⇧N; canvas capture via menu/omnibar is enough for v1).
10. **Export**: store's per-kind exporter for `canvas` writes `<id>.excalidraw` (the scene JSON) and `<id>.png` (from stored thumbnail or a full-res re-export); export-all includes them.

## Risks / Trade-offs

- [Large embedded/pinned images bloat scene rows] → cap thumbnail size; if scenes grow large, spill `files` to `workspace-assets/` (decision 1 escape hatch). Profile before optimizing.
- [AI returns invalid Excalidraw skeletons] → strict parse + `convertToExcalidrawElements` in a try/catch; on failure show an error, never mutate the canvas (Confirmation gate already prevents silent apply).
- [Per-element click navigation is awkward in Excalidraw] → fall back to a selection-based "open referenced paper" action rather than direct chip clicks; requirement is satisfied by ref + navigation existing, not a specific gesture.
- [Duplicating GraphView logic] → mitigated by extracting shared helpers (decision 3) rather than copy-paste.

## Migration Plan

Store migration v2→v3 (additive `canvases` table); no data migration. Rollback: table ignored by older paths. `GraphView` helper extraction is a pure refactor guarded by existing behavior.

## Open Questions

- Exact pinned-element click/navigation gesture (linked element vs. selection action) — decide during implementation against Excalidraw's API.
- Whether AI-edit preview renders a live mini-canvas or a textual summary in the Confirmation panel (textual first; live preview if cheap).
