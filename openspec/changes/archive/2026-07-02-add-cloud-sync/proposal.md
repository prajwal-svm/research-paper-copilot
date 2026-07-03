# Proposal — add-cloud-sync

## Why

Everything since v1 was built "sync-ready" — append-only journals, UUID anchors, content-addressed PDFs, regenerable derived data — but the sync itself doesn't exist: a user's library still lives and dies on one machine. ADR-002 makes sync the roadmap priority before any web app, and v4's collaborative workspaces are explicitly blocked on it. This change cashes in the design debt the whole format was structured around.

## What Changes

- **Sync engine** in the Rust core: per-bundle manifests classify every file by the format's existing layers — source (immutable, content-addressed), derived (regenerable; small JSONs synced for speed, heavy caches like `embeddings.bin`/`graph.db` re-derived), user (the valuable part). Push/pull cycles reconcile against a remote; interrupted syncs resume; nothing is ever destructively overwritten.
- **Merge semantics that resolve the format's open CRDT question**: append-only journals (notes, chats, ink, mastery, consents, runs, threads — i.e. nearly all user data) merge by **entry-set union with deterministic ordering** — no CRDT library needed; the journals were designed for exactly this. The few non-journal user files (reading_state.json, review `document.md`) use last-writer-wins **plus a conflict copy** — divergent edits are preserved, never silently dropped.
- **End-to-end encryption for the user layer** (and source PDFs): client-side encryption with a passphrase-derived library key; the remote stores ciphertext blobs it cannot read. Learner memory syncs **only** E2E-encrypted, honoring the learner-memory privacy designation.
- **Bring-your-own-storage remotes, free by default, no accounts with us**: the remote is a dumb blob store — an S3-compatible endpoint (**primary recommended path: a Cloudflare R2 free-tier bucket** — 10 GB free, zero infrastructure to run, stores only ciphertext; **self-hosted MinIO** (e.g. one-click on Coolify) is the equal-citizen option for your own hardware — no paid cloud required anywhere) or a plain folder (iCloud Drive/Dropbox/Syncthing/USB). No WebDAV. No first-party server, no sign-up with us; a second device joins with the remote credentials + the library passphrase. This is the BYO-key ethos applied to storage, and it's the substrate v4 workspaces build on (a shared bucket = a shared library).
- **Sync status UI**: per-library remote configuration, per-paper sync state, explicit egress transparency (what goes where, encrypted how), conflict-copy surfacing, offline-first always (sync is a background enhancement, never a gate).
- Library-level stores (`learning_state/`, `concepts.jsonl`, `reviews/`, `gaps/`) sync under the same rules as bundle contents.

## Capabilities

### New Capabilities
- `cloud-sync`: the engine — manifests, layer rules, journal union-merge, LWW+conflict-copy, E2E encryption, resumable push/pull, offline-first guarantees
- `sync-remotes`: pluggable dumb-blob backends (S3-compatible — free-tier R2 recommended, self-hosted MinIO supported — plus local folder), credential storage in the OS keychain, device pairing without accounts

### Modified Capabilities
- `learner-memory`: privacy requirement gains its sync clause — learner stores leave the machine only inside E2E-encrypted sync payloads (as designated since v2)
- `collaborative-workspaces`: the dependency note resolves — workspace journals sync as shared-library content under these primitives (unblocks v4 section 7)

## Impact

- **Rust core**: new `sync/` modules — manifest builder (layer classification per format table), journal merge (union + dedupe by entry hash + stable ordering), encryption (XChaCha20-Poly1305 with an Argon2id passphrase-derived key), remote trait + two backends (S3-compatible incl. self-hosted MinIO/R2, local folder), sync-state db (what was pushed/pulled when). New deps: crypto crates (`chacha20poly1305`, `argon2`), S3 via `rust-s3` or hand-signed ureq (spike decides).
- **Shell/UI**: remote setup in Settings (keychain-stored credentials), sync status indicators in library/reader, background sync scheduling (on open/close/interval), conflict-copy notices.
- **Format**: no version bump needed — sync adds a local `sync_state/` (library-level, cache-class, gitignored equivalent) and remote-side manifests; bundles themselves are unchanged.
- **Security surface**: passphrase loss = data loss on remote (documented, deliberate — no recovery backdoor); credentials in OS keychain; every byte leaving the machine is client-side encrypted; egress hosts shown per the established transparency rules.
- **Unblocks**: v4 tasks 7.1–7.3 (collaborative workspaces) and the future web app horizon.
