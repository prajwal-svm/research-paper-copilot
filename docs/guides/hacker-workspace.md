# Hacker workspace guide (v3)

v3 turns understanding into *doing*: every equation becomes runnable code,
parameters become experiments, and papers-with-code become checkable claims.
Everything executes inside a consented, offline-by-default sandbox — no
generated or cloned code ever runs directly on your machine.

## The sandbox (read this first)

All execution — implementations, experiments, reproduction — goes through one
containerized substrate. There is no second path, by construction:

- **Explicit consent, per scope.** The first run for a paper's
  implementations, an experiment, or a cloned repo shows exactly what will
  run, what's mounted, and that network is off. Grants are recorded in the
  bundle (`consents.jsonl`), visible, and revocable.
- **No network by default.** Runs that need downloads (dependency installs)
  ask separately, with the reason, per repo.
- **Isolated and capped.** Read-only rootfs, only the relevant bundle folder
  mounted (read-only source, write-only output), memory/CPU/process/time
  limits, no host environment. Every run is stoppable instantly; killed runs
  keep their partial output, labeled.
- **Requires Docker or Podman.** Without one, every viewing/editing feature
  still works — only Run buttons explain what to install.

## Implementation mode

Open any equation → the panel offers **Python, PyTorch, TensorFlow, JAX, and
Rust** implementations: generated on demand (strong tier), stored in the
bundle's `implementations/`, edited in place (CodeMirror). Your edits are
never overwritten by regeneration; re-parsed papers flag (never discard)
affected code. Each implementation ships with generated checks — passing them
records a mastery event and flips the dashboard's "implementation complete";
until then it's honestly labeled *generated, not yet verified*.

## Experiment mode

The flask icon in the reader dock opens the workbench: pick an implemented
equation, declare parameters (they reach the code as `EXP_<NAME>` env vars),
and run. Print `{"metric": value}` lines to record metrics — runs append to
`experiments/` (crash-safe), chart live across runs, and compare side by side.
Record a **prediction before running** (predict–observe–explain); the outcome
feeds your learner memory. The discussion thread sees the experiment
definition and the concrete recorded numbers, nothing else.

## Reproduction mode

The repo icon opens the wizard: link the paper's GitHub repo, then step
through **clone → detect environment → explain architecture → map code to
paper → verification run → verify → report**. Every step shows its exact
commands and logs, is interruptible, and resumes where it left off. Clones
live in a library-level cache (`repos/`) so bundles stay portable.

The result is a **reproduction report** stored in the bundle: which metrics
matched (within 1%), which diverged and by how much, with full provenance
(commit, environment, commands) — always labeled *verification run*, never
overstated as a full-scale reproduction. A small curated corpus (micrograd,
minGPT, nanoGPT) is gate-tested end-to-end; other repos work with an explicit
"unverified repo" notice.

## Code understanding

The Files tab is a read-only repo browser (offline once cloned, no container
needed). After the mapping step, code and paper link both ways: equations
show "In the code" buttons that open the file at the implementing lines, and
mapped files list their paper objects, one click back to the reader. Chat
knows the map too — ask "where is Equation 12 in the code?" and the answer
cites file and lines. Wrong links are correctable, and corrections survive
re-mapping.

## Format changes

`format_version` 0.2.0 → **0.3.0** (additive; same major, older readers
preserve the new files): activates `implementations/` and `experiments/`,
adds `reproduction/` and `consents.jsonl`. Library-level: `repos/` clone
cache (safe to clear from Settings — bundles keep references and reports).
