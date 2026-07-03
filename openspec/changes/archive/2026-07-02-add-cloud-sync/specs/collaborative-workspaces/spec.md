# collaborative-workspaces (delta)

## MODIFIED Requirements

### Requirement: Sync-ready collaboration data models
Workspace membership, object-anchored discussion threads, assignments, and progress records SHALL be append-only journals keyed by stable UUIDs with author attribution — merged across members by the cloud-sync journal-union semantics (**the sync layer this capability was specced against now exists**: a shared workspace is a shared remote whose members hold the workspace key; member journals converge without destructive conflicts by construction). Threads SHALL anchor to paper objects (or hypothesis cards/reviews) like all user data; a thread on an object survives re-parsing via the standard UUID+hash anchoring.

#### Scenario: Thread shape is mergeable
- **WHEN** two members comment on the same equation while offline
- **THEN** both comments appear on every member's device after sync, deterministically ordered — append-only journals make the merge conflict-free by construction

#### Scenario: Workspace key is not the personal key
- **WHEN** a member syncs both a personal library and a shared workspace
- **THEN** personal data (including learner memory) is encrypted under the personal key only; workspace content under the workspace key — neither remote can read the other's payloads
