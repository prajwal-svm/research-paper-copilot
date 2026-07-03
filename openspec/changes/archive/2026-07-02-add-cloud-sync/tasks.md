# Tasks — add-cloud-sync

Ordered so the merge semantics (the correctness core) land and get hammered by tests before any byte touches a network, and crypto lands before any remote. The one unrecoverable failure class is destroyed user data — the merge/tombstone test suites are release blockers like perf budgets. Sync is opt-in per library; the app must behave byte-identically with sync off.

## 1. Manifests & merge semantics (pure core, no I/O)

- [x] 1.1 Spike: S3 access — `rust-s3` vs hand-signed SigV4 over ureq, tested against a MinIO container (incl. conditional-write support check); measure `figures/` sync-vs-rederive on the sample bundle; record decisions in design.md
- [x] 1.2 Manifest builder: walk a bundle + library stores, classify every file by layer (source/derived-small/derived-excluded/user) per the format table, content-hash each entry; exclusion rules tested (embeddings.bin, graph.db, repos/ never listed)
- [x] 1.3 Journal union-merge: dedupe by entry hash, deterministic (at, hash) ordering; commutativity + idempotence property tests (any interleaving of device appends, merged in any order, folds identically through every existing reader — notes, chats, mastery, overrides, runs)
- [x] 1.4 LWW + conflict copy for non-journal user files (reading_state, review document.md): loser preserved as `<name>.conflict-<device>-<date>`; test divergent edits both survive
- [x] 1.5 Tombstones: library deletion journal, local trash with grace period, remote GC only from explicit action; test that no sync path ever deletes remote user data implicitly

## 2. Encryption

- [x] 2.1 Crypto module: XChaCha20-Poly1305 AEAD, Argon2id passphrase→key, HMAC-derived blob names; standard test vectors; key in OS keychain; wrong-passphrase = clean typed error, no partial writes
- [x] 2.2 Encrypt-everything rule at the sync boundary: property test that every byte handed to a remote is ciphertext (including manifests' user entries); learner-memory stores asserted present-and-sealed in payloads (privacy delta test)

## 3. Remotes

- [x] 3.1 Remote trait (get/put/list/delete + atomic manifest swap) with an in-memory fake for the engine tests
- [x] 3.2 Local-folder backend (temp+rename, lock-file discipline, documented eventual consistency) — enables iCloud/Dropbox/Syncthing transports
- [x] 3.3 S3-compatible backend (per 1.1 spike): conditional-write manifest swap, custom endpoints + path-style addressing (self-hosted MinIO), credentials in keychain; ✅ verified against a local MinIO container — R2 free-tier + owner Coolify MinIO verification pending user resources (bucket creds / Tailscale), same code path
- [x] 3.4 Device pairing: join-with-credentials+passphrase flow; wrong-passphrase clean failure; generation compare-and-swap race test (two writers converge with all data)

## 4. Sync engine

- [x] 4.1 Reconcile loop: local manifest → remote manifest → diff → pull/merge → push → swap; `sync_state/` cache (never source of truth, rebuildable); resume-without-reupload test (kill mid-push, other devices see old consistent state)
- [x] 4.2 Second-device bootstrap: full pull, heavy derived rebuilt via stale-stage pipeline on open (end-to-end test with the sample bundle over the fake remote)
- [x] 4.3 Background scheduling in the shell: on app open + paper close + manual; never blocks reading (worker thread + `sync-progress` events); sync-off byte-identity assertion

## 5. UI & transparency

- [x] 5.1 Remote setup in Settings: backend picker, keychain-stored credentials, passphrase creation with the loss-is-unrecoverable statement, egress disclosure (host + "ciphertext only")
- [x] 5.2 Sync status surfaces: per-paper state (synced/pending/conflict) in the library, conflict-copy badges + open-both affordance, trash view for tombstoned papers
- [x] 5.3 Telemetry (opt-in, content-free): sync runs, conflicts encountered, papers synced — closed-set kinds

## 6. Gates & release

- [x] 6.1 Merge/tombstone suites wired as release blockers alongside perf budgets; perf entries: manifest build <500 ms for a 50-paper library (enforced), incremental sync no-change cycle <1 s (enforced, fake remote)
- [x] 6.2 Docs: sync guide ✅ (docs/guides/sync.md), CRDT question resolved in research-format.md ✅, README ✅, v4 section-7 unblock note ✅ — release itself remains user-gated (commit freeze, ships with v1–v4)
