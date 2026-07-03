# PRD — v3 "The Hacker Workspace"

**Status:** Roadmap (to become OpenSpec change `add-v3-hacker-workspace`). Also in this horizon: the web app (identical to desktop, on top of cloud sync).

## Goal

From understanding to *doing*. Every equation and algorithm becomes runnable; published results become reproducible.

## Features

### 1. Implementation mode

- Every equation/algorithm → generated implementations: **Python, PyTorch, TensorFlow, JAX, Rust**.
- Code is stored in the bundle (`implementations/`), editable, runnable in a sandboxed local kernel; output linked back to the source object.
- Implementation guidance: "match this code to Equation 8" annotations; common-pitfalls notes per implementation.
- Dashboard "Implementation complete" flips when the user's implementation passes generated checks.

### 2. Experiment mode

- Tweak variables (e.g., Learning Rate) → run → observe → auto-generated graph → AI discussion of the result.
- Experiments persisted in `experiments/` with parameters, results, and the discussion thread; comparable side-by-side.
- The AI proposes experiments: "try changing X, predict what happens, then run it" (predict-observe-explain pedagogy).

### 3. Reproduction mode

If the paper has a GitHub repo:

```
Clone → Build (automatic environment setup) → Explain architecture
→ Match code to paper (file/function ↔ section/equation map)
→ Run → Verify results → Compare to reported metrics
```

- Environment setup automated (uv/conda/docker detection); every step observable and interruptible.
- Output: a reproduction report — what matched, what diverged, by how much — attached to the paper.

### 4. Code understanding

- Repo browser inside the workspace; every source file linked to the paper objects it implements; "where is Equation 12 in the code?" answered with line-level links.

## Success metrics

- ≥ 25% of active users run ≥ 1 implementation or experiment/month.
- Median time from "open repo" → "verified run" on the supported-corpus ≤ 30 min.
- Reproduction reports produced for ≥ 70% of attempted papers-with-code.

## Risks

Sandboxing and safety of running arbitrary repo code (containerize, explicit consent, no network by default); environment hell (start with a curated corpus of well-behaved ML repos); GPU-dependence of real reproductions (scope: small-scale/verification runs locally, document larger-scale paths).
