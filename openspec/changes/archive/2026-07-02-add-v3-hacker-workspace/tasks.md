# Tasks — add-v3-hacker-workspace

Ordered so the sandbox (the risk) lands first and every later feature consumes it. Early vertical slice: sandbox → one runnable Python implementation with checks → then breadth. Performance budgets stay CI release blockers; every AI surface needs its designed no-key state; every execution surface needs its designed no-runtime state.

## 1. Sandboxed execution substrate

- [x] 1.1 Spike: runtime orchestration — docker vs podman CLI invocation, flag set for no-network/memory/cpu/pids/read-only/tmpfs, per-language slim images vs one kernel image; record decisions in design.md
- [x] 1.2 `sandbox` module in copilot-core: runtime detection, container lifecycle (create/run/stream logs/kill), resource caps, mount policy (single bundle subdir), typed errors; unit tests with a fake runtime binary
- [x] 1.3 Consent store: per-scope append-only `consents.jsonl` in-bundle (grant/revoke events), network as separate per-run consent with reason; test that revocation blocks the next run
- [x] 1.4 Choke-point guard: the only path to runtime invocation goes through consent checks — test asserts a run without a grant never spawns the runtime process
- [x] 1.5 Shell commands + events: run/kill/log-stream (`sandbox-progress` events), consent prompt flow, kill-anytime; designed no-runtime state surfaced to the UI
- [x] 1.6 Perf/robustness: limit-killed runs preserve labeled partials; app responsive during runs (background thread + event streaming, no UI thread blocking)

## 2. Implementation mode

- [x] 2.1 `implementations/` store in core: per object+language files + `meta.json` (anchor hash, provenance, check status); edit-preserving regeneration rules; stale-anchor flagging (tests)
- [x] 2.2 Generation: one prompt template parameterized by language (Python/PyTorch/TensorFlow/JAX/Rust) producing code + line-anchored guidance + pitfalls + assert-style checks; strong tier, streamed, cancellable; no-key state
- [x] 2.3 Run integration: execute implementation and its checks in the sandbox, capture output linked to the source object, persist beside code; "generated, not yet verified" → "verified" labeling
- [x] 2.4 Checks→mastery: passing checks records a mastery event (source "implementation"); test the single data path
- [x] 2.5 UI: implementation tabs on equation/algorithm panels — language switcher, editor (CodeMirror 6), run/kill controls, output pane, guidance annotations; no-runtime and no-key states

## 3. Experiment mode

- [x] 3.1 `experiments/` store: experiment.json (name, anchor, parameter schema) + append-only runs.jsonl (params, metrics, duration, exit) + discussion journal; crash-safety tests; stdout `{"metric": value}` convention parser
- [x] 3.2 Run flow: parameterized sandbox runs; side-by-side run comparison; incomplete-run marking on interrupt
- [x] 3.3 Experiment context assembly: definition + selected runs as a budgeted anchor kind in context.rs (contextual-chat delta); trimming tests
- [x] 3.4 Predict–observe–explain: AI-proposed experiments, prediction captured pre-run, post-run comparison; outcomes → mastery/episodic events
- [x] 3.5 UI: experiment workbench pane — parameter form, run history table, Recharts metric chart (no stored images), side-by-side view, discussion thread
- [x] 3.6 Charts derive from runs only; chart of ≥50 runs renders within frame budget

## 4. Reproduction mode

- [x] 4.1 Library-level repo cache (`repos/` keyed by remote+commit); clone step with progress events; bundle stores references only (portability test)
- [x] 4.2 Env detection/setup: uv > conda > docker heuristics, exact commands shown, user-overridable; network consent integration for dependency downloads
- [x] 4.3 Resumable step pipeline (clone→env→explain→map→run→verify→report) persisted under `reproduction/`; interrupt/resume tests; every step observable and killable
- [x] 4.4 Verification + report: metric comparison vs reported numbers, verification-scale honesty labeling, report.md with provenance (commit/env/commands/seeds); persists in-bundle, opens from dashboard
- [x] 4.5 Curated corpus: pin 3–5 well-behaved ML repos; end-to-end reproduction gate against them (ignored-by-default CI test, like golden corpus); out-of-corpus "unverified repo" notice
- [x] 4.6 UI: reproduction wizard route from dashboard — step timeline, live logs, retry/skip on failure, report view

## 5. Code understanding

- [x] 5.1 Repo browser pane: file tree + read-only syntax-highlighted viewer (CodeMirror 6), offline once cloned, works without container runtime
- [x] 5.2 Code↔paper map: strong-tier mapping pass → `reproduction/code_map.json` (file/function/lines ↔ object UUID + confidence); heuristic/no-key degradation; append-only user corrections surviving re-mapping (test)
- [x] 5.3 Bidirectional navigation: object panel "show in code" → line-highlighted file; code selection → linked objects → reader; low-confidence styling
- [x] 5.4 "Where is Equation 12 in the code?" via chat: map-lookup answer with clickable line-level links (no repo dump — contextual-chat delta test)

## 6. Format & degraded modes

- [x] 6.1 Format bump 0.3.0: activates `implementations/`, `experiments/`, adds `reproduction/`; same-major compat + unknown-file preservation tests extended
- [x] 6.2 No-key / no-runtime audit across every new surface (generation, runs, mapping, reproduction, discussion); cached artifacts fully usable in both absences
- [x] 6.3 Disk hygiene: repo cache + image usage surfaced in settings with cleanup actions (cleanup requires explicit confirmation)

## 7. Quality gates & release

- [x] 7.1 Perf budgets: sandbox cold-start-to-first-output, implementation-panel open (cached), experiment chart render, repo-browser file open — added to perf/budgets.toml (enforced where core-measurable, pending for UI)
- [x] 7.2 Security tests: no-network default verified from inside a container run; no-consent path cannot execute; mount policy limits writes to the intended subdir
- [x] 7.3 Telemetry (opt-in, content-free, closed set): implementation runs, experiment runs, reproduction attempts/completions — feeds the PRD success metrics
- [ ] 7.4 Docs: format spec update ✅, hacker-workspace guide ✅ (docs/guides/hacker-workspace.md), README refresh ✅; v3 release — **blocked on user**: repo has no commits yet ("don't commit yet" still in force) → ships with the v1+v2 release
