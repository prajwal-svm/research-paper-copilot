# sandboxed-execution

## ADDED Requirements

### Requirement: Single execution choke point
All code execution originating from the app — generated implementations, experiment runs, reproduction builds and runs — SHALL go through one sandbox substrate. There SHALL be no code path that executes generated or cloned code on the host directly; the substrate SHALL be the only component permitted to invoke the container runtime, and this SHALL be verifiable in tests (the no-consent path cannot reach an execution call).

#### Scenario: No second executor
- **WHEN** any feature (implementation kernel, experiment, reproduction step) needs to run code
- **THEN** the invocation goes through the shared sandbox module with its consent, isolation, and resource policies applied — bypassing it is not possible via any exposed command

### Requirement: Containerized, isolated, resource-capped runs
Every run SHALL execute in a container with: **network disabled by default**, memory/CPU/process/time limits, only the minimal bundle subdirectory bind-mounted (read-write only where output is expected), and no host credentials or environment inherited. Runs SHALL be killable at any time, and a killed run SHALL leave persisted partial output clearly marked.

#### Scenario: Default run has no network
- **WHEN** the user runs a generated implementation without granting network access
- **THEN** the container is created with networking disabled and an in-code network attempt fails visibly rather than reaching the internet

#### Scenario: Runaway process
- **WHEN** running code exceeds its time or memory limit
- **THEN** the container is terminated, the partial output is preserved and labeled as limit-killed, and the app remains responsive throughout

### Requirement: Explicit, auditable, revocable consent
Before the first run in a scope (a paper's implementations, an experiment, a cloned repo), the user SHALL be shown exactly what will run, what is mounted, and that network is off, and SHALL explicitly approve. Grants SHALL be recorded as append-only user data in the bundle and be revocable. Network access SHALL require a separate per-run opt-in stating the reason. No run SHALL ever start from a background process without a standing grant for its exact scope.

#### Scenario: First run asks, second run remembers
- **WHEN** the user runs code for a paper the first and then a second time
- **THEN** the first run requires explicit approval showing the mount and network policy; the second proceeds under the recorded grant, which is visible and revocable in the paper's settings

#### Scenario: Network opt-in
- **WHEN** a reproduction build step needs to download dependencies
- **THEN** the step pauses and asks for network access for that run with the stated reason; declining keeps the run offline and reports the step as blocked by policy, not as an error

### Requirement: Designed absence of a container runtime
When no supported container runtime (Docker or Podman) is detected, execution surfaces SHALL show a designed state with install guidance; all non-execution features (viewing code, mappings, cached implementations and past results) SHALL remain fully functional. Runtime detection SHALL never crash or block the reader.

#### Scenario: Fresh machine without Docker
- **WHEN** the user opens implementation mode with no runtime installed
- **THEN** generated code is still viewable/editable, the Run control explains what to install, and nothing errors
