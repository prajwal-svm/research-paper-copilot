# Delta Spec: knowledge-registry

## ADDED Requirements

### Requirement: Canonical paper identity
Every imported paper SHALL carry a canonical identity derived from its DOI or arXiv id (normalized: lowercase DOI, versionless arXiv id with version recorded separately). Two imports of the same paper on different machines SHALL resolve to the same canonical identity, so the ecosystem converges on one shared knowledge object per paper. Papers without a resolvable DOI/arXiv id SHALL work fully locally and be marked registry-ineligible rather than being given a fabricated identity.

#### Scenario: Same paper converges
- **WHEN** two users import the same arXiv paper — one via arXiv URL, one via a local PDF whose metadata resolves to that arXiv id
- **THEN** both bundles carry the identical canonical identity and address the same registry object

#### Scenario: No fabricated identity
- **WHEN** a PDF has no resolvable DOI or arXiv id
- **THEN** the bundle is marked registry-ineligible, all local features work unchanged, and no synthetic global identity is invented

### Requirement: Pull enrichment on import
When importing a paper whose canonical identity has published enrichment in a configured registry, the system SHALL offer to pull that enrichment instead of re-deriving it locally. Pulling SHALL be explicit (user-consented), merge as provenance-tagged layers alongside — never overwriting — the user's own artifacts, and SHALL fall back cleanly to local derivation when the registry is unreachable.

#### Scenario: Import finds community enrichment
- **WHEN** a user imports a paper that has registry enrichment and consents to pull
- **THEN** the enrichment layers download, verify, and merge with community provenance tags, and local AI derivation is skipped for artifacts the pull satisfied

#### Scenario: Registry unreachable
- **WHEN** the registry cannot be reached during import
- **THEN** import proceeds with local derivation exactly as v1–v4 behave, and the pull offer reappears when connectivity returns

### Requirement: Publish enrichment layers
A user SHALL be able to publish selected enrichment from their bundle as a versioned layer against the paper's canonical identity: a manifest enumerating contained artifacts, provenance, format version, and a monotonically increasing layer version. Publishing SHALL be explicit per-paper (no auto-publish), SHALL respect the shareability allowlist (learning state and private notes are never publishable), and SHALL require an authenticated registry identity.

#### Scenario: Publish selected enrichment
- **WHEN** a user selects enrichment artifacts and publishes
- **THEN** a layer with manifest, provenance, and next version number is uploaded against the canonical identity, and the response records the assigned version

#### Scenario: Private data cannot be published
- **WHEN** a publish set would include learner memory, sync credentials, or notes not marked shareable
- **THEN** those artifacts are excluded by the allowlist before upload and the user is shown exactly what will be published

### Requirement: Enrichment only — never publisher content
The registry SHALL store and distribute enrichment only. Publisher-owned content — source PDFs, page images, or extracted full text beyond short quotes with location anchors — SHALL be rejected by both client-side validation before upload and server-side validation on ingest. Pulled layers SHALL re-attach to the user's own locally-imported PDF.

#### Scenario: PDF content rejected at publish
- **WHEN** a publish payload contains the source PDF, rendered page images, or full-text extraction
- **THEN** client validation blocks the upload identifying the offending entries; a crafted payload bypassing the client is rejected server-side with the same policy error

#### Scenario: Enrichment re-anchors to the local PDF
- **WHEN** a user pulls enrichment for a paper they imported from their own PDF
- **THEN** anchors resolve against their local copy, and enrichment referencing regions their copy lacks degrades explicitly (marked unresolved), never silently

### Requirement: Open, self-hostable registry protocol
The registry protocol SHALL be a documented HTTP API over object storage (the same S3-compatible surface cloud-sync uses), implementable and hostable by anyone. The client SHALL support multiple configured registries with an explicit default; nothing in the client SHALL hard-code a single vendor instance.

#### Scenario: Self-hosted registry
- **WHEN** an organization deploys the reference registry server and a user adds its URL
- **THEN** pull and publish work against it identically to the default registry

### Requirement: Layer integrity
Published layers SHALL be content-addressed and integrity-verified on pull: a layer whose content does not match its manifest digests SHALL be discarded with a visible error, never merged.

#### Scenario: Corrupted layer rejected
- **WHEN** a pulled layer fails digest verification
- **THEN** the merge is aborted, the user sees which layer failed verification, and the bundle remains unchanged
