# contextual-chat (delta)

## ADDED Requirements

### Requirement: Research artifacts as anchorable context
Context assembly SHALL support research-artifact anchors: a hypothesis card (claim, rationale, novelty verdict + evidence titles) and a gap report entry (the structural gap + its citing papers) SHALL be assemblable context blocks under the same budget and trimming rules as object anchors. Discussions on these anchors persist with standard journal semantics.

#### Scenario: Discussing a hypothesis card
- **WHEN** the user asks "what's the strongest objection to this hypothesis?" on a card
- **THEN** the prompt contains the card's fields and its novelty evidence titles within budget — not the whole paper — and the discussion persists with the card
