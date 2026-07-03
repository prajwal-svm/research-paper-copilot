# Proposal — add-v3-hacker-workspace

## Why

v2 made papers *learnable*; understanding still ends at the edge of the page — readers can't verify an equation behaves as claimed, feel how a hyperparameter changes a result, or check whether the paper's public code actually reproduces its numbers. v3 turns understanding into *doing*: every equation/algorithm becomes runnable, every paper-with-code becomes checkable, per docs/prd/v3-hacker-workspace.md.

## What Changes

- **Implementation mode**: any equation/algorithm object → generated, editable implementations in Python, PyTorch, TensorFlow, JAX, and Rust, stored in the bundle's reserved `implementations/` directory, runnable in a sandboxed local kernel with output linked back to the source object. Generated correctness checks flip the dashboard's "implementation complete" signal (feeding v2 mastery).
- **Experiment mode**: parameter tweaks → sandboxed run → captured results → auto-generated graph → AI discussion of the outcome; experiments persisted in `experiments/` (parameters + results + discussion thread), comparable side-by-side. The AI proposes predict-observe-explain experiments.
- **Reproduction mode**: for papers with a GitHub repo: clone → automated environment setup (uv/conda/docker detection) → architecture explanation → code↔paper mapping → run → verify against reported metrics → a reproduction report attached to the paper. Every step observable and interruptible.
- **Code understanding**: a repo browser inside the workspace; source files linked to the paper objects they implement ("where is Equation 12 in the code?" answered with line-level links).
- **Sandboxed execution substrate** (hard requirement, shared by all modes): containerized execution, explicit per-run user consent, **no network access by default**, resource limits, kill-anytime. No generated or cloned code ever runs on the host.
- Format: activates the reserved `implementations/` and `experiments/` bundle directories (additive minor bump); new derived/user artifacts follow the established UUID-anchored, append-only rules.

## Capabilities

### New Capabilities
- `implementation-mode`: equation/algorithm → multi-language generated implementations, editable, runnable, checked, linked back to objects
- `experiment-mode`: parameterized runs with captured results, auto-graphs, persisted comparison + AI discussion
- `reproduction-mode`: repo clone → env setup → run → verification → reproduction report pipeline
- `code-understanding`: repo browser with code↔paper object mapping at file/function/line level
- `sandboxed-execution`: the shared containerized runner — consent, isolation, no-network default, resource caps, observability (the v3 PRD's top risk, specced once and reused)

### Modified Capabilities
- `paper-dashboard`: adds the "implementation complete" progress signal (flips when the user's implementation passes generated checks)
- `contextual-chat`: experiment/implementation context joins assembly (run results and code excerpts become anchorable context for AI discussion)

## Impact

- **Rust core (`copilot-core`)**: new modules for implementations/experiments stores (append-only + derived artifacts in-bundle), sandbox orchestration (container lifecycle, consent gating, resource limits), repo mapping (clone metadata, code↔object index), verification/report generation. Format minor bump (0.2.0 → 0.3.0, additive).
- **Tauri shell**: commands for generate/run/kill, experiment CRUD + runs, repo clone/setup/run pipeline with step events, consent dialogs; new capability permissions for spawning the container runtime.
- **Frontend**: implementation panel tabs (per-language), experiment workbench (params → run → chart → discussion), reproduction wizard with step timeline, repo browser pane; charts via the established shadcn/Recharts stack.
- **Dependencies**: a container runtime becomes an optional-but-required-for-v3 host dependency (Docker/Podman/Apple containers — detection + graceful "not installed" state); likely `bollard` (Docker API) or subprocess orchestration in core; syntax highlighting for the repo browser.
- **AI usage**: strong tier for code generation/mapping/report prose; all long operations stream with cancel (v1 infra); designed no-key states everywhere (cached implementations still runnable keyless).
- **Security surface**: running arbitrary repo code — mitigated by the `sandboxed-execution` capability being a blocking dependency for every run path (see PRD risks; consent + container + no-network defaults are spec-level requirements, not implementation details).
