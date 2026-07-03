# Knowledge registry (v5)

A public registry of `.research` **enrichment layers** — npm/crates, but
for paper knowledge. Import a paper others have studied and pull their
enrichment instead of re-deriving it; publish your improvements back.

## Canonical identity

Papers are keyed by DOI or arXiv id (arXiv wins; ids are normalized —
versionless arXiv, lowercased DOI) so the whole ecosystem converges on one
shared object per paper. A paper with neither id works fully locally and
is simply registry-ineligible — no identity is ever fabricated.

## Pull

On a registry-linked paper, "Check for enrichment" lists available layers.
Pulling is **explicit and consent-gated**, and merges community content
*alongside* your own: journals union in, new files add, **your existing
artifacts are never overwritten** (the community copy is dropped and
reported). Every pulled layer is recorded in `registry.json` with its
manifest — community content is always provenance-tagged. Layers are
content-addressed and verified on pull; a corrupted layer is discarded
with a visible error before anything touches your bundle. Offline?
Everything behaves exactly like v4.

## Publish

Publish shows a preview of **exactly** what uploads and what's held back,
with reasons. The registry stores enrichment only — never the source PDF,
page imagery, or full-text extraction. That's enforced three times: the
allowlist that assembles the upload, client-side validation (`%PDF` magic
bytes and banned paths), and server-side validation on ingest.

Publishing needs a registry token (Settings → add a registry). Versions
are monotonic per paper; the server stamps your identity.

## Self-hosting

The reference server is a small open-source binary (`registry-server/`)
over any S3-compatible bucket — the same surface cloud-sync uses, so a
Coolify + MinIO box runs it fine:

```sh
REGISTRY_STORE=s3 \
REGISTRY_S3_ENDPOINT=http://your-minio:9000 \
REGISTRY_S3_BUCKET=registry \
REGISTRY_S3_ACCESS=... REGISTRY_S3_SECRET=... \
REGISTRY_TOKENS=./tokens.json \
registry-server
```

`tokens.json`: `{ "<token>": { "id": "alice", "quota_bytes": 104857600 } }`.
Add the URL in Settings; the client supports multiple registries with a
default — nothing hard-codes any single instance.
