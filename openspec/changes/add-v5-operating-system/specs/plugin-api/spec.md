# Delta Spec: plugin-api

## ADDED Requirements

### Requirement: Published JSON Schemas for the .research format
Every file kind in the `.research` bundle SHALL have a JSON Schema generated from the core Rust types (single source of truth, no hand-maintained drift) and published versioned alongside the format version. A validation command SHALL check any bundle against the schemas and report violations by file and path.

#### Scenario: Schemas match the code
- **WHEN** schemas are regenerated from the current core types
- **THEN** the output is byte-identical to the published schemas for that format version (CI-enforced), and a bundle written by the core validates cleanly against them

#### Scenario: Third-party bundle validation
- **WHEN** an external tool produces a bundle and runs the validation command
- **THEN** every schema violation is reported with file, JSON path, and expected shape — or the bundle is confirmed valid

### Requirement: Versioned plugin surface
Plugins SHALL declare the format major version and API capabilities they target in a manifest. The plugin API SHALL remain stable within a format major version; loading a plugin targeting an incompatible major SHALL fail clearly at discovery, never at mid-use.

#### Scenario: Incompatible plugin rejected at load
- **WHEN** a plugin manifest declares an unsupported format major
- **THEN** the plugin is listed as incompatible with the reason, and none of its code executes

### Requirement: Third-party panels with scoped permissions
Panel plugins SHALL render inside the app against a read API scoped to the open bundle. Panels SHALL have no filesystem or network access beyond permissions declared in their manifest and granted explicitly by the user (consent recorded per-plugin, revocable), consistent with the sandbox consent model.

#### Scenario: Panel reads bundle content
- **WHEN** a domain-specific visualizer panel is opened on a paper
- **THEN** it receives bundle content through the scoped read API and renders in its host pane without any access outside the granted scope

#### Scenario: Undeclared access blocked
- **WHEN** a panel attempts network access without a granted network permission
- **THEN** the request is blocked and surfaced to the user, and the plugin keeps running (no silent grant, no crash)

### Requirement: Exporters
The plugin surface SHALL support exporters that enumerate bundle content through a stable read API and produce external formats. At least reference exporters for Anki (flashcard decks), Obsidian (markdown vault with backlinks), and LaTeX (annotated bibliography/notes) SHALL ship with the app as plugins using only the public API — proving the surface is sufficient.

#### Scenario: Anki export via public API
- **WHEN** a user runs the Anki exporter on a paper with flashcards
- **THEN** a valid `.apkg`/deck file is produced using only public plugin API calls, and cards preserve their concept anchors as tags

### Requirement: Importers
The plugin surface SHALL support importers that produce schema-valid bundles from non-PDF sources (LaTeX source, HTML papers, lab notebooks). Imported bundles SHALL pass validation and be indistinguishable to downstream features from PDF-derived bundles, with source-specific gaps (e.g. no page geometry) expressed through the format's optional fields rather than invalid data.

#### Scenario: LaTeX source import
- **WHEN** an importer plugin ingests a paper's LaTeX source
- **THEN** it emits a bundle that passes schema validation, opens in the reader (degrading page-geometry features explicitly), and supports graph, chat, and learning features
