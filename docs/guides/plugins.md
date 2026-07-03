# Plugin API (v5) — author quickstart

Third-party panels, exporters, and importers run against a **versioned
public contract**, not our internals: the `.research` JSON Schemas
(`app/schemas/generated/<format-major>/`, generated from the core types
and CI-checked against drift) plus a small WASM ABI.

## Anatomy

A plugin is a folder in the app's plugins directory:

```
my-plugin/
  plugin.json     # manifest
  plugin.wasm     # wasm32-unknown-unknown module
```

`plugin.json`:

```json
{
  "name": "my-plugin",
  "version": "1.0.0",
  "format_major": 0,
  "capabilities": ["exporter"],
  "permissions": [],
  "entry": "plugin.wasm",
  "description": "What it does."
}
```

`format_major` must match the app's format major or the plugin is listed
as incompatible with the reason — none of its code executes. Unknown
capabilities/permissions also refuse to load.

## The ABI (stable within a format major)

Export two functions:

```
alloc(size: i32) -> i32                 // bump allocator is fine
run(ptr: i32, len: i32) -> i64          // (out_ptr << 32) | out_len
```

The host writes the input JSON into your memory and reads your output
back. Input for exporters/panels:

```json
{ "format": "anki", "view": { "metadata": …, "knowledge_graph": …, "notes": […], "flashcards": …, "glossary": … } }
```

The `view` is a **scoped read** of the open bundle — you never see the
filesystem. Exporters return `{ "files": { "relative/path": "content" } }`
(the host writes them where the user chooses); panels return
`{ "html": "…" }` (rendered in a sandboxed iframe, `allow-scripts` only);
importers receive `{ "source": "…" }` and return structured import JSON.

## Permissions

Declare `"network"` or `"filesystem"` in the manifest; the user grants
each explicitly (recorded append-only, revocable in the Plugins pane). An
ungranted call returns an error code to your plugin and is surfaced to the
user — your plugin keeps running; nothing is granted silently.

## Reference plugins

The shipped plugins are built the same way you would build yours, using
only this ABI — that's deliberate (if the public API can't express them,
the API grows, not their access):

- `plugins/reference-exporters` (source: `app/plugins-src/exporters/`) —
  Anki decks with concept-anchor tags, Obsidian vaults with backlinks,
  LaTeX notes.
- `plugins/latex-importer` (source: `app/plugins-src/latex-importer/`) —
  LaTeX source → schema-valid bundle with explicit page-geometry
  degradation.

Build yours the same way:

```sh
cargo build --release --target wasm32-unknown-unknown
```

Validate any bundle your importer emits with the `bundle_validate`
command / `copilot_core::schemas::validate_bundle` — violations are
reported by file and JSON path.
