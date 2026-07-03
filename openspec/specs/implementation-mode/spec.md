# implementation-mode

## Purpose

Equations/algorithms as runnable, editable, checked multi-language implementations anchored to paper objects.

## Requirements

### Requirement: Multi-language implementations anchored to objects
Any equation or algorithm object SHALL offer generated implementations in Python, PyTorch, TensorFlow, JAX, and Rust, generated on demand (strong tier, streamed, cancel-anytime), stored in the bundle's `implementations/` directory keyed by object UUID + language with anchor content-hash and generation provenance. Implementations SHALL be user-editable; user edits SHALL never be overwritten by silent regeneration, and a changed anchor hash SHALL flag the implementation for review rather than discard it. With no provider configured, cached implementations remain fully usable and generation shows the designed no-key state.

#### Scenario: Generate then edit
- **WHEN** the user generates a PyTorch implementation of Equation 8 and then edits it
- **THEN** the edited file persists in `implementations/`, reopening shows the edited version, and regeneration is offered only as an explicit action that preserves the user's version until confirmed

#### Scenario: Re-parsed anchor
- **WHEN** re-ingestion changes the source equation's content hash
- **THEN** the implementation is flagged "source changed — review" and stays runnable, never silently deleted

### Requirement: Runnable in the sandbox with output linked back
Each implementation SHALL be runnable via the sandboxed-execution substrate, with stdout/stderr and produced artifacts captured and stored linked to the source object, and shown beside the code. Guidance annotations ("this line implements the QKᵀ term") and common-pitfall notes SHALL be part of generated output, anchored to code lines.

#### Scenario: Run links output to the equation
- **WHEN** the user runs the Python implementation of Equation 8
- **THEN** output appears beside the code, is persisted with the implementation, and the object panel for Equation 8 shows the latest run result

### Requirement: Generated checks drive honest completion
Each generated implementation SHALL come with generated correctness checks (assert-style, runnable in the same sandbox). A passing check run SHALL record a mastery event (source "implementation") and flip the paper dashboard's "implementation complete" signal for that concept; failing or never-run checks SHALL leave the signal honest ("not yet verified"). Checks passing SHALL never gate access to anything.

#### Scenario: Checks pass
- **WHEN** the user's (possibly edited) implementation passes its checks
- **THEN** a mastery event is recorded, the dashboard shows implementation complete for the linked concept, and the implementation is labeled verified

#### Scenario: Generated-unreviewed banner
- **WHEN** an implementation has been generated but its checks have never run
- **THEN** it carries a visible "generated, not yet verified" label
