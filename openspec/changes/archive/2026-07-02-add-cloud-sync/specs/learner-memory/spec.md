# learner-memory (delta)

## MODIFIED Requirements

### Requirement: Privacy boundary of the learner model
The learner model SHALL remain on the user's machine: never included in telemetry (closed event-kind set), sent to an AI provider only as the compact profile block within an explicitly invoked action, and synced across the user's devices **only inside end-to-end-encrypted sync payloads under the user's own passphrase-derived key — never readable by the storage remote, and never part of any shared-workspace payload**. The user SHALL be able to inspect and delete their learner data (per store and wholesale), and deleting it locally SHALL tombstone it across their synced devices.

#### Scenario: Wholesale reset
- **WHEN** the user chooses "reset learning data" in settings and confirms
- **THEN** all `learning_state/` stores are deleted, the dashboard returns to the cold-start state, and nothing else (notes, chats, papers) is touched

#### Scenario: Sync carries learner memory sealed
- **WHEN** the library syncs to a configured remote
- **THEN** learner stores travel only as ciphertext under the user's key, and a shared workspace export never includes them
