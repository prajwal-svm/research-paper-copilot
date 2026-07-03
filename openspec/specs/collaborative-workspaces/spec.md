# collaborative-workspaces

## Purpose

Shared libraries, object-anchored threads, and reading-group/lab roles over
the cloud-sync primitives. (Specced by `add-v4-researcher-workspace`; the
data-models requirement carries `add-cloud-sync`'s revision — when v4
archives, keep this version. The role/sharing feature requirements below
are implemented by v4's gated section 7.)

## Requirements

### Requirement: Sync-ready collaboration data models
Workspace membership, object-anchored discussion threads, assignments, and progress records SHALL be append-only journals keyed by stable UUIDs with author attribution — merged across members by the cloud-sync journal-union semantics (**the sync layer this capability was specced against now exists**: a shared workspace is a shared remote whose members hold the workspace key; member journals converge without destructive conflicts by construction). Threads SHALL anchor to paper objects (or hypothesis cards/reviews) like all user data; a thread on an object survives re-parsing via the standard UUID+hash anchoring.

#### Scenario: Thread shape is mergeable
- **WHEN** two members comment on the same equation while offline
- **THEN** both comments appear on every member's device after sync, deterministically ordered — append-only journals make the merge conflict-free by construction

#### Scenario: Workspace key is not the personal key
- **WHEN** a member syncs both a personal library and a shared workspace
- **THEN** personal data (including learner memory) is encrypted under the personal key only; workspace content under the workspace key — neither remote can read the other's payloads

### Requirement: Shared libraries and object-anchored discussion
Once sync is available, a workspace SHALL offer shared libraries (papers, notes, highlights visible to members per role) and threaded discussions anchored to objects, with authorship always visible. Local-only data (learner memory) SHALL never be shared: the privacy boundary from learner-memory holds — mastery, preferences, and episodic memory stay on the member's machine.

#### Scenario: Learner privacy in a shared workspace
- **WHEN** a member joins a shared library and syncs
- **THEN** papers, shared notes, and threads sync; the member's mastery/episodic data does not leave their machine

### Requirement: Reading-group and lab modes
Workspaces SHALL support two role configurations: reading-group mode (an instructor assigns papers and quizzes and sees cohort progress derived from members' *opt-in shared* quiz outcomes — never raw learner memory) and lab mode (shared concept graph and shared v3 experiments with attributed runs). Progress sharing SHALL be explicit opt-in per member with a clear statement of exactly what is shared.

#### Scenario: Cohort progress is opt-in
- **WHEN** an instructor views cohort progress
- **THEN** only members who opted in appear, showing assignment completion and shared quiz outcomes — not mastery scores, episodes, or chat content

#### Scenario: Lab-mode shared experiment
- **WHEN** a lab member runs a shared experiment
- **THEN** the run record syncs with the member's attribution and joins the shared side-by-side comparison
