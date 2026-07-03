# reader-workspace

## ADDED Requirements

### Requirement: Layout-faithful rendering
The reader SHALL render pages visually identical to the original PDF (canvas-based), across macOS, Windows, and Linux webviews, at 60 fps while scrolling (< 16 ms/frame) with no blank flashes, and idle memory (app + one open paper) SHALL stay under 300 MB.

#### Scenario: Fast scroll through a long paper
- **WHEN** the user scrolls rapidly through a 40-page paper
- **THEN** frames hold 60 fps with progressive sharpening allowed but no blank pages

#### Scenario: Cross-platform fidelity
- **WHEN** the same paper is rendered on macOS, Windows, and Linux
- **THEN** per-OS visual regression tests show pixel-equivalent output within the defined tolerance

### Requirement: Interactive object layer
Extracted objects SHALL be rendered as an invisible interactive layer over the page: hovering highlights the object's bounds in < 50 ms; clicking selects it and opens the interaction panel. Arbitrary text selection SHALL also work and creates an ad-hoc selection object.

#### Scenario: Hover an equation
- **WHEN** the pointer moves over an extracted equation
- **THEN** its bounding box highlights within 50 ms and a subtle affordance indicates it is clickable

#### Scenario: Select a passage spanning objects
- **WHEN** the user drag-selects one and a half paragraphs
- **THEN** the selection becomes an ad-hoc object that supports the same Explain/Ask actions

### Requirement: Raw fallback view
A raw PDF view (no object layer) SHALL always be available and SHALL be the automatic default for papers or pages where extraction failed or confidence is below threshold.

#### Scenario: Toggle to raw view
- **WHEN** the user toggles raw view on any page
- **THEN** the exact original rendering is shown at the same scroll position instantly (< 100 ms)

### Requirement: In-paper search
The reader SHALL provide exact text search and local semantic search over the paper's objects; semantic results SHALL return in < 50 ms on the reference machine and work fully offline.

#### Scenario: Semantic search offline
- **WHEN** the user searches "why do they scale the dot product" with no network
- **THEN** relevant objects (e.g., the scaling discussion and Equation 1) are ranked and returned in < 50 ms, and clicking a result navigates to the object
