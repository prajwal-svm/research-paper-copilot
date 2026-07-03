# cloud-sync

## ADDED Requirements

### Requirement: Layered sync per the format contract
Sync SHALL classify every bundle and library file by the format's layers and treat each accordingly: source files content-addressed and uploaded once (verified by hash); user data always synced; derived data synced when small and excluded when re-derivable-and-heavy (local embedding binaries, index caches, repo clones never sync). A device receiving a bundle without heavy derived files SHALL regenerate them through the normal stale-stage pipeline on open — sync never introduces a new degradation mode.

#### Scenario: Second device receives a paper
- **WHEN** a bundle syncs to a fresh device
- **THEN** the PDF, derived JSONs, and all user data arrive; embeddings rebuild locally on first open via the standard pipeline; the paper is fully usable throughout

#### Scenario: Heavy caches never upload
- **WHEN** a sync push runs on a library with embeddings and a graph index
- **THEN** `embeddings.bin`, `graph.db`, and the repo cache are not part of the uploaded set

### Requirement: Journal merge without destructive conflicts
Append-only journals SHALL merge across devices by entry-set union — deduplicated by entry content, deterministically ordered — so concurrent activity on multiple devices never conflicts destructively and merge results are independent of sync order. Non-journal user files SHALL resolve last-writer-wins with the losing version preserved as a visible conflict copy, never silently discarded.

#### Scenario: Notes on two offline devices
- **WHEN** device A and device B both add notes to the same paper offline and then sync in either order
- **THEN** both devices converge to the identical union of notes, with every fold (edits, deletes) applied consistently

#### Scenario: Divergent review edits
- **WHEN** two devices edit the same review `document.md` before syncing
- **THEN** the newer version wins in place, the older is preserved as a conflict copy, and the UI surfaces it

### Requirement: End-to-end encryption of everything that leaves
All uploaded content SHALL be encrypted client-side with a key derived from a user passphrase that never leaves the machine; the remote stores ciphertext it cannot read. Learner-memory stores SHALL sync only under this encryption (per their privacy designation). Setup SHALL state plainly that a lost passphrase is unrecoverable by design. Egress transparency applies: the user always sees which host receives blobs.

#### Scenario: Hostile remote learns nothing
- **WHEN** an attacker reads the remote storage
- **THEN** they obtain ciphertext blobs and opaque names — no paper content, notes, learner data, or filenames

#### Scenario: Learner memory stays sealed
- **WHEN** `learning_state/` syncs
- **THEN** it is encrypted like all user data and is never included in any unencrypted or shared-workspace payload

### Requirement: Offline-first, resumable, never destructive
Sync SHALL be a background reconcile that never blocks or degrades local use: the app behaves identically with sync disabled, interrupted syncs resume without re-uploading finished blobs, partial pushes are invisible to other devices (manifest swaps last), and deletions propagate as tombstones with a local grace period — remote garbage collection only ever runs from an explicit user action.

#### Scenario: Kill mid-push
- **WHEN** the app quits halfway through uploading a large first sync
- **THEN** other devices see the previous consistent state; the next sync resumes from the completed blobs

#### Scenario: Delete propagates deliberately
- **WHEN** a paper is deleted on device A
- **THEN** device B moves it to a local trash on next sync rather than destroying it immediately, and remote blobs persist until the user explicitly cleans the remote

### Requirement: Concurrent writers converge
Two devices pushing concurrently SHALL converge without a coordination server: pushes that lose the manifest compare-and-swap re-pull, re-merge (safe by journal-union), and retry bounded. The folder backend, which cannot guarantee atomic swaps, SHALL be documented and surfaced as eventually-consistent single-writer-preferred.

#### Scenario: Simultaneous push race
- **WHEN** devices A and B push at the same moment against an S3 remote
- **THEN** one push lands, the other detects the advanced generation, re-merges, and lands second — both devices converge with all data from both
