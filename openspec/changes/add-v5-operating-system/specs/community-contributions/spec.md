# Delta Spec: community-contributions

## ADDED Requirements

### Requirement: Contribution proposals over knowledge objects
Any enrichment artifact in a paper's knowledge object (explanations, lessons, quizzes, implementations, visualizations, graph nodes/edges, notes marked shareable) SHALL be packageable as a **contribution proposal**: a self-contained change set diffed against an identified base revision of the target bundle, carrying author identity, timestamp, and a human-readable summary. Proposals SHALL be creatable offline and queued for submission.

#### Scenario: Propose an improved explanation
- **WHEN** a user edits a concept explanation on a registry-linked paper and chooses "Propose to community"
- **THEN** a proposal is created containing only the changed artifacts, the base revision id, the author identity, and a summary — and it is queued locally until the registry is reachable

#### Scenario: Proposal against a stale base
- **WHEN** a proposal's base revision is older than the current head of the shared object
- **THEN** the system rebases journal-backed artifacts by entry-set union and flags non-mergeable file conflicts for the reviewer instead of silently overwriting either side

### Requirement: PR-style review before merge
Contribution proposals SHALL pass through review before merging into the shared knowledge object. Reviewers SHALL be able to view the full diff, accept, or reject with a recorded reason. Merges without review SHALL only be possible for contributors whose trust level explicitly grants direct-merge on that paper.

#### Scenario: Accept a contribution
- **WHEN** a reviewer accepts a proposal
- **THEN** the change set merges into the shared object as a new revision, the acceptance (reviewer, time, proposal id) is recorded in the provenance log, and the contributor is credited

#### Scenario: Reject with reason
- **WHEN** a reviewer rejects a proposal
- **THEN** the proposal is closed with the recorded reason visible to the contributor, and nothing is merged

### Requirement: Provenance and versioning on every artifact
Every merged contribution SHALL be recorded append-only, so that for any artifact in a shared knowledge object the system can answer: who contributed each revision, when, and via which proposal. Reverting a merged contribution SHALL produce a new revision (no history rewriting), and no accepted content SHALL be silently lost.

#### Scenario: Trace an artifact's history
- **WHEN** a user inspects provenance for an enrichment artifact
- **THEN** the system lists its full revision chain — original author, each contributing proposal, reviewer, and timestamps — from the local provenance log without contacting the registry

#### Scenario: Revert a bad merge
- **WHEN** a maintainer reverts a merged proposal
- **THEN** a new revision restoring the prior content is appended, the revert is attributed in provenance, and the reverted revision remains inspectable in history

### Requirement: Contributor reputation derived from provenance
Contributor reputation SHALL be computed deterministically from the public provenance record (accepted contributions, reverts, reviews performed) — never from an opaque or manually-assigned score. Recomputing reputation from the provenance log SHALL reproduce the displayed value.

#### Scenario: Reputation is recomputable
- **WHEN** reputation for a contributor is recomputed from the raw provenance log
- **THEN** the result equals the displayed reputation exactly

### Requirement: Moderation gates and trust levels
Shared knowledge objects SHALL enforce moderation gates: new contributors' proposals always require review; trust levels (new → trusted → maintainer) are earned through accepted contributions and SHALL be recorded with the same provenance guarantees. Content violating the enrichment-only policy (see knowledge-registry) SHALL be rejected at proposal validation, before review.

#### Scenario: New contributor is gated
- **WHEN** a contributor with no accepted history submits a proposal
- **THEN** the proposal enters the review queue and cannot merge until a reviewer with sufficient trust accepts it

#### Scenario: Policy violation blocked before review
- **WHEN** a proposal contains disallowed content (e.g. embedded publisher PDF pages)
- **THEN** validation rejects it at submission with the specific policy violation, and it never reaches reviewers
