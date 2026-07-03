# reproduction-mode

## Purpose

The resumable clone→env→map→run→verify→report pipeline and its honesty rules.

## Requirements

### Requirement: Resumable, observable reproduction pipeline
For a paper with a linked GitHub repository, the system SHALL run a staged pipeline — clone → environment detection/setup (uv/conda/docker heuristics) → architecture explanation → code↔paper mapping → run → verify → report — where every step emits observable progress, is interruptible without corrupting prior steps, and resumes from the last completed step. Clones SHALL live in a library-level cache keyed by remote+commit (bundles store references and derived artifacts, staying portable). All build/run steps execute via the sandboxed-execution substrate, network only by per-run consent.

#### Scenario: Interrupt and resume
- **WHEN** the user kills the pipeline during environment setup and restarts it later
- **THEN** the completed clone step is reused, setup restarts cleanly, and no prior artifacts are corrupted

#### Scenario: Environment detection order
- **WHEN** a repo contains both a uv lockfile and a Dockerfile
- **THEN** the faster deterministic option (uv) is chosen, the choice and exact commands are shown, and the user can override to the container path

### Requirement: Verification against reported metrics
A reproduction run SHALL compare produced metrics against the paper's reported numbers where they can be identified, and produce a reproduction report attached to the paper: what matched, what diverged and by how much, what was actually run (verification-scale vs full), with full provenance (commit, environment, commands, seeds where applicable). The report SHALL be honest about scope — a small verification run SHALL never be presented as a full reproduction.

#### Scenario: Metrics diverge
- **WHEN** the run produces BLEU 27.9 against a reported 28.4
- **THEN** the report lists the metric pair with the delta, labels the run's scale, and attaches the run log — no rounding away of the divergence

#### Scenario: Report persists with the paper
- **WHEN** the reproduction finishes
- **THEN** the report is stored in the bundle, opens from the paper dashboard, and survives re-ingestion

### Requirement: Curated corpus quality gate
Reproduction end-to-end SHALL be validated against a pinned corpus of well-behaved ML repositories (the golden-corpus pattern), and repos outside the corpus SHALL set expectations explicitly ("unverified repo — steps may need manual help") while remaining fully attemptable.

#### Scenario: Out-of-corpus repo
- **WHEN** the user starts reproduction on an arbitrary repo
- **THEN** the pipeline runs with a visible unverified-repo notice, and step failures present the exact command and log with retry/skip options
