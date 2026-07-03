# paper-library

## ADDED Requirements

### Requirement: Library home
The app SHALL open to a library listing all imported papers with title, authors, ingestion status, and last-opened time, supporting open, delete (with confirmation), and reveal-on-disk. Cold start to interactive library SHALL take < 1.5 s on the reference machine.

#### Scenario: Cold start
- **WHEN** the app is launched cold with 200 papers in the library
- **THEN** the library is interactive in < 1.5 s

#### Scenario: Delete is non-destructive to the original
- **WHEN** the user deletes a paper imported from a local file
- **THEN** only the `.research` bundle is removed after explicit confirmation; the user's original PDF elsewhere on disk is untouched

### Requirement: Bundled sample paper
The app SHALL ship with at least one pre-ingested, pre-enriched sample paper (e.g., "Attention Is All You Need") so a new user reaches a working object interaction with zero configuration and no API key.

#### Scenario: First run without API key
- **WHEN** a new user launches the app for the first time and opens the sample paper
- **THEN** they can click an equation and receive a pre-generated explanation within 2 minutes of install, with no key or network required

### Requirement: Open papers to their persisted state
Opening a paper SHALL restore the user's last location, open panels, and unfinished chats. Open-to-first-page for an ingested paper SHALL take < 500 ms.

#### Scenario: Reopen where you left off
- **WHEN** the user reopens a paper they read yesterday
- **THEN** the reader restores their last scroll position and any open object panel in < 500 ms
