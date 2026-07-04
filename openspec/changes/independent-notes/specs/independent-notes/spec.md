# independent-notes

## ADDED Requirements

### Requirement: Notes are first-class workspace entities
The system SHALL support notes as paper-independent entities stored in the workspace store: content as a BlockNote block document (JSON) with a derived markdown mirror, registered in the `items` registry (kind `note`) so listing, recency, tombstone-delete, and rename behavior follow the workspace-store capability. Every content edit SHALL persist automatically (no explicit save) and bump the item's recency.

#### Scenario: Create and edit persists without saving
- **WHEN** the user creates a note, types content, and closes the app without any save action
- **THEN** reopening shows the note with all content, and the note sits at the top of the recency-sorted library

#### Scenario: Deleting a note tombstones it
- **WHEN** the user deletes a note from the library
- **THEN** it disappears from listings while its row remains tombstoned in the store

### Requirement: Full block editor
The note surface SHALL be a full-page BlockNote editor with the complete default block palette — paragraphs, headings, bulleted/numbered/check lists, code blocks, tables, images, quotes, dividers — including the slash command menu, drag handles, and side menu. The editor SHALL follow the app theme (light/dark) and the app's typography.

#### Scenario: Slash menu inserts blocks
- **WHEN** the user types "/" in an empty paragraph
- **THEN** the block menu opens and selecting "Table" inserts an editable table block

#### Scenario: Blocks rearrange by drag
- **WHEN** the user drags a block by its handle above another block
- **THEN** the document order updates and persists

### Requirement: Paper and object mentions with backlinks
Typing `@` in a note SHALL open a suggestion menu searching the library — papers first, drill-in to their objects (sections, equations, figures, tables). Inserting a mention SHALL render an inline chip with the target's label, write a row to the workspace `refs` table (source: the note; target: paper or object), and clicking the chip SHALL navigate to the paper/object in the reader. Deleting a mention SHALL remove its ref row.

#### Scenario: Mention a paper
- **WHEN** the user types "@atten" and selects "Attention Is All You Need"
- **THEN** a chip is inserted, a refs row (note → paper) exists, and clicking the chip opens that paper

#### Scenario: Backlink is visible from the paper side
- **WHEN** a note mentions a paper and a caller queries refs targeting that paper
- **THEN** the note appears as a referencing source

#### Scenario: Removing a mention removes the backlink
- **WHEN** the user deletes the mention chip from the note
- **THEN** the corresponding refs row is removed

### Requirement: AI assist in the editor
The editor SHALL offer AI actions on selected text (improve, summarize, expand, continue writing) executed through the app's configured providers, streaming into the document with an accept/discard affordance. With no provider configured, the actions SHALL show the standard no-provider notice instead of failing silently. The implementation SHALL NOT depend on BlockNote's commercially-licensed XL AI package.

#### Scenario: Improve selection
- **WHEN** the user selects a paragraph and runs "Improve"
- **THEN** a streamed rewrite is offered and replaces the selection only on accept

#### Scenario: No provider configured
- **WHEN** the user invokes an AI action without any provider
- **THEN** the no-provider notice with a Settings link appears; the note is untouched

### Requirement: Quick capture
The system SHALL provide a "New note" action in the library, the Omnibar, and via a global hotkey — creating a note (default title "Untitled") and opening the editor with the cursor ready, in one step.

#### Scenario: Capture from anywhere
- **WHEN** the user invokes New note from the Omnibar while reading a paper
- **THEN** a new note opens for typing immediately, and appears in the library list afterward

### Requirement: Note export with real content
Exporting a note (per-entity or via workspace export-all) SHALL produce markdown containing the note's full content (headings, lists, tables, code) plus front matter and references — not just registry metadata.

#### Scenario: Exported markdown round-trips content
- **WHEN** the user exports a note containing a heading, a list, and a mention
- **THEN** the markdown file contains the heading and list as markdown and the mention as a readable label/link
