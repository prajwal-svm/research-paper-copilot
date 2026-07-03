# Web app (v5 platform parity)

The web app is the **same frontend** as desktop — one React codebase behind
`src/platform/` — with `copilot-core` compiled to `wasm32-wasip1`. WASI
gives the core a real filesystem API, so the entire bundle/journal/merge
layer runs unchanged in the browser on top of an OPFS-backed WASI shim.
Nothing is reimplemented for web; what can't run there is excluded at
compile time and declared in the capability matrix
(`copilot-core/src/capabilities.rs`), which the UI reads to degrade
features explicitly.

## Building

```sh
npm run build:web        # emits dist-web/
scripts/check-wasm.sh    # CI gate: native + wasm32-wasip1 both compile
```

## Sync is the backbone

A web session bootstraps from your cloud-sync remote: enter the sync
passphrase, the key is derived **in the browser** (Argon2id — the
`wasi_roundtrip` test proves byte-identical keys across native and wasm),
the library pulls and decrypts client-side, and web edits converge with
desktop through the same union-merge engine. The server never sees
plaintext; a wrong passphrase fails cleanly with no partial state.

## CORS configuration (required for web sync)

Browsers enforce CORS on the S3 requests the sync engine makes; desktop
never needed this. Configure your bucket once:

### Cloudflare R2

Dashboard → R2 → your bucket → Settings → CORS policy:

```json
[
  {
    "AllowedOrigins": ["https://your-web-app-origin.example"],
    "AllowedMethods": ["GET", "PUT", "DELETE", "HEAD"],
    "AllowedHeaders": [
      "authorization",
      "content-type",
      "x-amz-content-sha256",
      "x-amz-date",
      "if-none-match"
    ],
    "ExposeHeaders": ["etag"],
    "MaxAgeSeconds": 3600
  }
]
```

### MinIO (self-hosted / Coolify)

```sh
mc alias set myminio http://your-minio:9000 ACCESS SECRET
mc admin config set myminio api cors_allow_origin="https://your-web-app-origin.example"
mc admin service restart myminio
```

(Or set the `MINIO_API_CORS_ALLOW_ORIGIN` environment variable on the
container.)

`if-none-match` must be allowed — the engine's manifest compare-and-swap
(create-only puts) depends on it. If sync fails on web with a network
error that works on desktop, missing CORS headers are the first suspect;
the error surface names the failing request.

## What degrades on web

From the capability matrix (each shows its explanation in the UI, never a
silent gap):

- **PDF ingestion** — import and process papers on desktop; processed
  bundles arrive on web via sync.
- **Semantic search** — the embedding model runs natively; web falls back
  to text search.
- **Sandboxed execution / experiments / reproduction** — need the desktop
  app or a configured runner; results and reports stay readable on web.
- **Panel plugins** — the wasmtime plugin host is desktop-side for now;
  exporters/importers work everywhere.
