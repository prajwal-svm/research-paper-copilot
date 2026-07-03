# Sync guide (add-cloud-sync)

Your library on N devices, with no account and no server of ours. You bring
the storage; everything that leaves your machine is encrypted first.

## Setup (Settings → Sync across devices)

**Recommended: a free Cloudflare R2 bucket.** 10 GB free, nothing to run,
reachable from anywhere — and since the remote only ever stores ciphertext,
Cloudflare learns object sizes and access times, nothing else. Create a
bucket + an S3 API token in the Cloudflare dashboard, then paste the
endpoint (`https://<account>.r2.cloudflarestorage.com`), bucket, and keys.

**Equally supported: your own MinIO** (one-click on Coolify, or plain
docker). Same fields, your endpoint. Reaching a home server from outside
your network is Tailscale's job, not ours — sync just retries with backoff
when the remote is unreachable ("pending", never an error).

**Or a plain folder**: point it at iCloud Drive/Dropbox/Syncthing/a NAS
mount/a USB stick. Zero configuration; note these transports are
eventually-consistent — prefer syncing from one device at a time.

Then choose a **library passphrase**. It encrypts everything client-side
(XChaCha20-Poly1305, key derived with Argon2id). A second device joins with
the same storage credentials + the same passphrase — that's the whole
pairing story. **A lost passphrase is unrecoverable by design**; the escape
hatch is re-uploading from any device that still has the data.

## What syncs

Per the format's layer table: the immutable PDFs (content-addressed,
uploaded once), all user data (notes, chats, ink, implementations,
experiments, research documents, learner memory — always encrypted), and
the small derived JSONs (so a new device skips re-ingestion). Heavy
re-derivable caches never sync: `embeddings.bin`, `graph.db`, the repo
clone cache — a fresh device rebuilds them on first open via the normal
pipeline.

## How conflicts can't destroy your data

- **Journals** (nearly everything you create) merge by set-union: two
  devices writing offline converge to all entries from both, in a
  deterministic order, regardless of sync order. This is why the format
  used append-only journals from day one.
- **Documents** (a review you edited on two devices) resolve
  last-writer-wins **with the loser preserved** as a visible
  `*.conflict-<device>-<date>` copy — never silently dropped.
- **Deletions** travel as tombstones: other devices move the paper to a
  local `.trash/` (grace period). The remote is only ever garbage-collected
  by the explicit "Clean remote" action.
- **Interrupted syncs** are invisible to other devices (the manifest is
  swapped last, atomically) and resume without re-uploading finished blobs.

## Threat model, briefly

The remote is assumed hostile: it sees encrypted blobs under opaque
HMAC-derived names, a public random salt, and encrypted manifests. It can
withhold or delete data (availability), but cannot read or undetectably
modify it (AEAD authentication fails closed). Your passphrase never leaves
the machine; keys and storage credentials live in the OS keychain. Learner
memory syncs only inside this encryption, honoring its privacy designation.

## Collaboration

A shared workspace (v4) is this same machinery pointed at a shared bucket
whose members hold the workspace key — the workspace journals were shaped
for union-merge from the start. Personal libraries and workspaces use
different keys; neither remote can read the other's payloads.
