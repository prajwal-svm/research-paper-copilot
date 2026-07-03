# code-understanding

## ADDED Requirements

### Requirement: Repo browser inside the workspace
The workspace SHALL include a repo browser pane for a paper's linked repository: file tree, syntax-highlighted read-only viewing, and fast navigation — a reader-shell pane with the same escape-to-paper behavior as other panes. Browsing SHALL work fully offline once the repo is cloned and SHALL NOT require the container runtime.

#### Scenario: Browse without runtime
- **WHEN** the user opens the repo browser on a machine without Docker
- **THEN** the cloned tree and files render normally; only Run controls are gated on the runtime

### Requirement: Code↔paper mapping with line-level links
A derived mapping SHALL link source files/functions/line ranges to the paper objects they implement, with per-link confidence; "where is Equation 12 in the code?" SHALL be answerable with line-level navigation both ways (object → code, code → object). Low-confidence links SHALL be visually distinct; mapping SHALL degrade gracefully with no provider (no map → browser still works). User corrections to the map SHALL be append-only and survive re-mapping.

#### Scenario: Object to code
- **WHEN** the user asks "where is Equation 12 in the code?" or clicks the code link on Equation 12's panel
- **THEN** the browser opens the mapped file scrolled to the mapped lines with the range highlighted, showing the link's confidence

#### Scenario: Code to paper
- **WHEN** the user selects a mapped function in the browser
- **THEN** the linked paper objects are listed and one click opens the reader at that object

#### Scenario: Correcting a wrong link
- **WHEN** the user re-points a link to the correct function
- **THEN** the correction is recorded as user data and persists across re-mapping runs
