# object-interaction

## ADDED Requirements

### Requirement: Anchored interaction panel
Clicking any object SHALL open a panel anchored to it (never obscuring the object) within 100 ms for cached content, offering Explain, Ask-anything, Add note, and Bookmark, plus type-specific actions. Reading flow SHALL never be blocked: the page never reflows and scrolling stays live while the panel is open.

#### Scenario: Panel opens instantly
- **WHEN** the user clicks a paragraph whose enrichment is cached
- **THEN** the panel is visible with content in < 100 ms

#### Scenario: Panel on uncached object without network
- **WHEN** the user clicks an object with no cached enrichment and no API key configured
- **THEN** the panel shows non-AI data (extracted content, relationships, note/bookmark actions) plus a kind explanation of how to enable AI, never an error

### Requirement: Equation actions (v1 slice)
For equation objects the panel SHALL offer: variable breakdown (each symbol named and explained), step-by-step explanation, plain-language intuition, and "show original region". The full deep-dive set (derivation, assumptions, implementations, numerical example, interactive sliders, visualization, historical origin, prerequisites, quiz, related equations, common mistakes) is defined in the v2 change; the v1 panel layout SHALL be structured so these can be added as tabs without redesign.

#### Scenario: Variable breakdown
- **WHEN** the user clicks Equation 1 (scaled dot-product attention) and selects Variables
- **THEN** each symbol (Q, K, V, d_k) is listed with its meaning in this paper's context, streamed within the AI latency budget

### Requirement: Figure and table actions (v1 slice)
For figures the panel SHALL offer: explain, describe visually (what each axis/element shows), and interpret ("what should I conclude?"). For tables, extracted as structured data, the panel SHALL offer: summarize, and natural-language queries answered from the table's actual data ("which model has the best BLEU?").

#### Scenario: Query a results table
- **WHEN** the user asks "which row is best on BLEU EN-DE?" on a results table
- **THEN** the answer is computed from the extracted table data (not from an image) and cites the specific cells

### Requirement: Citation hover cards
Hovering a citation marker SHALL show, within 150 ms when cached, a card with: title, summary, main contribution, relationship to this paper, and why it is cited at this location. The card SHALL offer "import as paper" to bring the cited work into the library.

#### Scenario: Hover a reference
- **WHEN** the user hovers `[13]`
- **THEN** the card shows the cited paper's title and why-cited context for this passage in < 150 ms (cached), or a loading state that streams in metadata when resolution is still pending

#### Scenario: Unresolvable citation
- **WHEN** a citation cannot be resolved to external metadata
- **THEN** the card shows the raw bibliography entry with a manual search link, never a blank card
