# contextual-chat (delta)

## MODIFIED Requirements

### Requirement: Structured context assembly
Prompts SHALL be assembled graph-first: resolve the anchored object (or entity-linked query terms, <50 ms locally) to knowledge-graph nodes; expand along edges within a bounded neighborhood prioritizing unmastered prerequisites, then definitions, then dependents; attach the anchored object's episodic-memory summary and a compact learner-profile block (mastery ids/levels, style preferences). Context SHALL stay within the fixed token budget with the same trimming order guarantees as v1, and prompt-token counts SHALL be measurable locally to verify the ≥60% reduction target vs the v1 object+relationships baseline. When no graph exists for the paper (still ingesting, degraded extraction, pre-v2 bundle), assembly SHALL fall back to the v1 behavior (object content + relationships + thread) — never a worse experience than v1.

#### Scenario: Relationship-aware answer
- **WHEN** the user asks about Figure 5 which depends on Equation 12
- **THEN** the prompt includes Equation 12's content via the graph/relationship link, and the answer can reference it correctly

#### Scenario: Graph context replaces bulk retrieval
- **WHEN** the user asks a question anchored to a concept node in a fully ingested paper
- **THEN** the prompt contains the node, its unmastered prerequisites, linked definitions, episodic summary, and learner profile — and no whole-document chunk dump

#### Scenario: Mastery filters the expansion
- **WHEN** a prerequisite concept is recorded mastered
- **THEN** it enters the context as a reference id, not as full re-explained content

#### Scenario: Fallback without a graph
- **WHEN** the user chats on a paper whose concept stage hasn't run
- **THEN** context assembly uses the v1 path and the answer quality/latency budgets still hold
