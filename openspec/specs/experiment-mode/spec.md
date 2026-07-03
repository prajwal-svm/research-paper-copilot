# experiment-mode

## Purpose

Parameterized sandbox runs with persisted comparable results, auto-charts, predict–observe–explain, and grounded AI discussion.

## Requirements

### Requirement: Parameterized, persisted, comparable experiments
The user SHALL be able to create an experiment over an implementation: declare tweakable parameters (name, type, default), run with chosen values in the sandbox, and have every run persisted append-only in `experiments/` (parameters, captured metrics/stdout, duration, exit status). Runs SHALL be comparable side-by-side, and an auto-generated chart SHALL visualize metrics across runs without storing rendered images. A crash mid-run SHALL never corrupt previously recorded runs.

#### Scenario: Learning-rate sweep
- **WHEN** the user runs the same experiment three times with learning rates 0.1, 0.01, 0.001
- **THEN** three run records persist with their parameters and metrics, the chart plots the metric across the three runs, and any two runs can be viewed side-by-side

#### Scenario: Crash mid-run
- **WHEN** the app crashes while a run is writing results
- **THEN** on restart all previously committed runs load intact and the torn run is marked incomplete

### Requirement: AI discussion grounded in run results
Each experiment SHALL carry a persistent discussion thread (same journal semantics as object chats) whose AI context includes the experiment definition and selected runs' parameters and metrics — assembled through the standard budgeted context machinery, streamed with cancel, honest no-key state.

#### Scenario: Discussing a result
- **WHEN** the user asks "why did the loss diverge at lr=0.1?"
- **THEN** the prompt contains the experiment's parameter definitions and the relevant runs' metrics (not unrelated whole-paper dumps), and the answer can reference the concrete numbers

### Requirement: Predict–observe–explain proposals
The AI SHALL be able to propose experiments on request: a suggested parameter change, a prompt for the user's prediction recorded before the run, and a post-run explanation comparing prediction to observation. The user's prediction SHALL be recorded with the run, and the outcome SHALL feed learner memory (mastery/episodic) for the anchored concept.

#### Scenario: Propose and predict
- **WHEN** the user asks for a suggested experiment on the attention implementation
- **THEN** the AI proposes a concrete parameter change, the user's prediction is captured before running, and after the run the discussion contrasts prediction with the observed metrics, recording the outcome in learner memory
