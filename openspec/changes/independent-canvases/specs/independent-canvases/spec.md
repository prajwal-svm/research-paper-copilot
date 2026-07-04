# independent-canvases

## ADDED Requirements

### Requirement: Canvases are first-class workspace entities
The system SHALL support canvases as paper-independent entities stored in the workspace store: an Excalidraw scene (elements, appState, files) as JSON, registered in the `items` registry (kind `canvas`) so listing, recency, tombstone-delete, and rename follow the workspace-store capability. Scene changes SHALL persist automatically (debounced), bumping the item's recency; there SHALL be no explicit save action.

#### Scenario: Draw and reopen intact
- **WHEN** the user creates a canvas, draws shapes and arrows, and closes the app without saving
- **THEN** reopening the canvas restores every element, and the canvas sits at the top of the recency-sorted library

#### Scenario: Deleting a canvas tombstones it
- **WHEN** the user deletes a canvas from the library
- **THEN** it disappears from listings while its row remains tombstoned in the store

### Requirement: Full freeform canvas editor
The canvas surface SHALL be a full-page Excalidraw editor with its standard tools (draw, shapes, arrows, text, images, frames, hand/zoom), theme-following (light/dark), and offline-first assets (self-hosted fonts, no CDN fetch). The editor SHALL support pan/zoom over a large working area — a Miro-like freeform board, not a fixed page.

#### Scenario: Standard drawing tools work
- **WHEN** the user selects the arrow tool and connects two rectangles
- **THEN** the bound arrow renders and persists, and reflows if a shape is moved

#### Scenario: Offline assets
- **WHEN** the app runs with no network
- **THEN** the canvas fonts and UI render correctly with no external requests

### Requirement: Pin paper content with backlinks
The user SHALL be able to place references to library content onto a canvas — whole papers, equations, or figures. Papers and equations SHALL render as labeled cards; figures SHALL render as their extracted PNG image. Each pinned reference SHALL write a row to the workspace `refs` table (source: the canvas; target: paper or object) and SHALL be navigable to the paper/object in the reader. Removing a pinned reference SHALL remove its ref row.

#### Scenario: Pin a figure
- **WHEN** the user inserts "Figure 4" from a library paper onto the canvas
- **THEN** the figure's PNG appears as a canvas image, a refs row (canvas → object) exists, and the backlink is visible when querying refs targeting that paper

#### Scenario: Removing a pin removes the backlink
- **WHEN** the user deletes a pinned reference element from the canvas
- **THEN** the corresponding refs row is removed on the next save

### Requirement: Library thumbnails
Each canvas SHALL show a rendered thumbnail (PNG of the scene) on its library card, regenerated when the scene changes. An empty canvas SHALL show a neutral placeholder rather than a broken image.

#### Scenario: Thumbnail reflects content
- **WHEN** the user edits a canvas and returns to the library
- **THEN** the card thumbnail reflects the updated scene

### Requirement: Ask AI about the canvas
The canvas SHALL offer an "Ask AI" action that sends the AI both a PNG export of the scene (for vision models) and a structural text summary of shapes and arrow connections (for all models), then streams an answer. With no provider configured, the standard no-provider notice SHALL appear instead of a silent failure.

#### Scenario: Ask about the diagram
- **WHEN** the user draws a pipeline and asks "what's missing from this flow?"
- **THEN** a streamed answer references the diagram's actual shapes and connections

#### Scenario: No provider configured
- **WHEN** the user invokes Ask AI without any provider
- **THEN** the no-provider notice with a Settings link appears; the canvas is untouched

### Requirement: AI-proposed canvas edits require confirmation
The user SHALL be able to instruct the AI to add to the canvas (e.g. "add a data-flow diagram of the training loop"). The AI's proposed additions SHALL be returned as Excalidraw elements and SHALL be presented for explicit approval via the Confirmation component before being merged; rejecting SHALL leave the canvas unchanged. Approved additions SHALL merge without destroying existing elements.

#### Scenario: Approve an AI addition
- **WHEN** the AI proposes a three-node flow and the user approves
- **THEN** the new nodes and arrows are added to the canvas alongside existing content

#### Scenario: Reject an AI addition
- **WHEN** the AI proposes elements and the user rejects
- **THEN** the canvas is unchanged and no elements are added

### Requirement: Canvas export
Canvases SHALL be exportable without the app: `.excalidraw` JSON (full fidelity) and PNG (rendered image), per-entity and via workspace export-all.

#### Scenario: Export a canvas
- **WHEN** the user exports a canvas
- **THEN** an `.excalidraw` JSON file and a PNG are written to the chosen location
