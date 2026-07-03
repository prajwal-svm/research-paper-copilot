# contextual-chat (delta)

## ADDED Requirements

### Requirement: Execution artifacts as anchorable context
Context assembly SHALL support execution-derived anchors: an implementation (code + latest run output) and an experiment (definition + selected runs' parameters and metrics) SHALL be assemblable context blocks, budgeted and trimmed under the same rules as object anchors — code and metrics excerpts, never whole-repo dumps. Discussion threads on these anchors persist with the same journal semantics as object chats.

#### Scenario: Asking about a run
- **WHEN** the user asks a question anchored to an implementation that has run output
- **THEN** the prompt includes the relevant code excerpt and the captured output within budget, and the answer can cite both

#### Scenario: Repo never dumped
- **WHEN** a question is asked in reproduction context
- **THEN** only the mapped, relevant files/excerpts enter the prompt — the repository is never bulk-included
