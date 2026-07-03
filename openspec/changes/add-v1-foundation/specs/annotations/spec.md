# annotations

## ADDED Requirements

### Requirement: Object-anchored notes
Users SHALL be able to attach Markdown notes to any object or ad-hoc selection. Notes are stored in the bundle's `notes/` directory anchored by object UUID + content hash, visible as unobtrusive indicators in the reader, and fully searchable.

#### Scenario: Note survives re-ingestion
- **WHEN** a user writes a note on Equation 12 and the paper is later re-parsed
- **THEN** the note remains attached to the same equation (UUID/content-hash anchoring)

#### Scenario: Note capture speed
- **WHEN** the user presses the note shortcut on a selected passage
- **THEN** an inline editor opens in < 100 ms and saving is instant (append-only write)

### Requirement: Bookmarks
Users SHALL be able to bookmark any object or location, with a bookmarks panel listing them per paper for one-click navigation.

#### Scenario: Jump to bookmark
- **WHEN** the user clicks a bookmark
- **THEN** the reader navigates to the exact object and briefly highlights it, in < 300 ms

### Requirement: Export
Notes and bookmarks SHALL be exportable as plain Markdown (per paper) so user data is never locked in.

#### Scenario: Markdown export
- **WHEN** the user exports notes for a paper
- **THEN** a single Markdown file is produced with notes grouped by section, each linking back to its anchor context (quoted text)
