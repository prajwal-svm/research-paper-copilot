# unified-library

## ADDED Requirements

### Requirement: One list for all workspace content
The Library SHALL present research papers and workspace items (notes, canvases, chat threads — as those features land) in a single unified list sorted by last-updated, newest first. Recency SHALL come from bundle metadata for papers and from the item registry's `updated_at` for workspace items. Each card SHALL show a type icon identifying its kind.

#### Scenario: Mixed recency ordering
- **WHEN** the library contains a paper updated yesterday and a note updated today
- **THEN** the note appears above the paper, each with its kind's icon

#### Scenario: Papers keep their affordances
- **WHEN** a research paper card renders in the unified list
- **THEN** it retains the existing paper actions (open, star, priority, import status, delete, markdown view)

### Requirement: Filter chips
The Library SHALL offer filter chips — All / Research / Notes / Canvases / Threads — above the list. "All" is the default. Selecting a chip SHALL filter the list to that kind without a reload; the selection SHALL persist per machine across sessions. Chips for kinds with no items SHALL still be shown (empty state explains how to create the first one).

#### Scenario: Filtering to one kind
- **WHEN** the user selects the "Notes" chip
- **THEN** only note cards remain, still sorted by last-updated

#### Scenario: Filter persistence
- **WHEN** the user selects "Threads", closes the app, and reopens it
- **THEN** the library opens with "Threads" still selected

#### Scenario: Empty kind
- **WHEN** the user selects a kind that has no items yet
- **THEN** an empty state describes how to create one (e.g. "New note" action) rather than a blank screen

### Requirement: Search and Omnibar cover all kinds
Library search and the command palette SHALL match workspace items (by title) alongside papers, opening each kind in its own surface.

#### Scenario: Omnibar finds a note
- **WHEN** the user types part of a note's title in the command palette
- **THEN** the note appears as a result and selecting it opens the note editor

### Requirement: Creation entry points
The Library SHALL provide creation actions for workspace kinds (e.g. a "New" split button or per-chip empty-state actions): new note, new canvas, new thread — wired up as each feature change lands; kinds not yet implemented SHALL NOT show dead buttons.

#### Scenario: Creating from the library
- **WHEN** the user clicks "New note" (once the notes feature is present)
- **THEN** a note is created in the workspace store and opens for editing immediately
