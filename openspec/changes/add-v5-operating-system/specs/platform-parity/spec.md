# Delta Spec: platform-parity

## ADDED Requirements

### Requirement: Shared frontend across desktop and web
Desktop and web SHALL ship the same React frontend from one codebase. Platform differences SHALL be isolated behind a platform adapter (command invocation, file dialogs, window chrome) — no feature SHALL be implemented twice, and no view SHALL branch on platform outside the adapter and the capability matrix.

#### Scenario: One component tree, two platforms
- **WHEN** the reader, graph canvas, or research view renders on web
- **THEN** it is the same component as desktop, differing only through the platform adapter — verified by the web build importing the identical view modules

### Requirement: Rust core on the web via WASM
`copilot-core` SHALL compile to WebAssembly and power the web app's bundle operations (read/write, journals, graph, chat folding, contribution/registry client logic) in the browser against browser-local storage. Capabilities that cannot run in WASM (sandboxed execution, container runtimes, OS keychain) SHALL be excluded from the WASM surface at compile time — not stubbed to fail at runtime.

#### Scenario: Bundle round-trip in the browser
- **WHEN** the web app opens a synced bundle, edits notes and chat, and reloads
- **THEN** all edits persisted through the WASM core to browser storage and reload byte-consistent with the same-major format guarantees

#### Scenario: Non-WASM capability excluded cleanly
- **WHEN** the web build compiles
- **THEN** sandbox/container/keychain code is excluded by target gates, and the UI presents those features per the capability matrix instead of exposing broken entry points

### Requirement: Sync as the backbone
A web session SHALL bootstrap from the user's cloud-sync remote: entering the sync passphrase pulls their library and converges edits with desktop through the existing union-merge guarantees. End-to-end encryption SHALL hold on web — keys derived in the browser, plaintext never leaving the client. Web-originated edits SHALL be regular sync citizens (tombstones, conflict copies, generations — identical semantics to desktop).

#### Scenario: Web bootstrap from sync
- **WHEN** a user signs into the web app with their sync remote and passphrase
- **THEN** their library pulls and decrypts client-side, and a note added on web appears on desktop after its next sync — with no plaintext ever readable by the server

#### Scenario: Wrong passphrase on web
- **WHEN** an incorrect passphrase is entered
- **THEN** decryption fails cleanly with no partial library state, matching desktop behavior

### Requirement: Explicit capability parity matrix
A capability matrix SHALL declare, per feature: native, web, or web-via-runner (features like sandboxed execution that need a companion runner). The web UI SHALL derive feature availability from this matrix; anything unavailable degrades with an explicit explanation and never silently disappears or half-works.

#### Scenario: Sandbox feature on web
- **WHEN** a web user opens a paper's experiment pane with no runner configured
- **THEN** the pane states that execution requires the desktop app or a configured runner, links setup docs, and read-only views of past runs still work
