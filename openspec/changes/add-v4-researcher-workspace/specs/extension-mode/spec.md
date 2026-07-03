# extension-mode

## ADDED Requirements

### Requirement: Staged, resumable extension pipeline
Extension mode SHALL run as a staged pipeline — weaknesses → hypotheses → novelty → related work → outline → draft — with every stage persisted in the bundle's `research/` area, individually re-runnable, and edit-tolerant: regenerating an upstream stage SHALL never destroy downstream user edits (hypothesis cards, outline edits, draft text survive and are flagged for review instead). Long generations stream with cancel and show a skeleton within 300 ms.

#### Scenario: Regenerate upstream, keep downstream
- **WHEN** the user regenerates weaknesses after having edited two hypothesis cards
- **THEN** the edited cards persist unchanged (flagged "upstream changed — review"), and only the weaknesses artifact is rewritten

#### Scenario: Resume mid-pipeline
- **WHEN** the user closes the app after the novelty stage and reopens the paper
- **THEN** extension mode resumes with weaknesses, cards, and novelty evidence intact, ready to run related work

### Requirement: Object-grounded weakness finding
Weakness candidates SHALL be extracted only from the paper's own objects (limitations/future-work/assumption-bearing sections and equations), and every weakness SHALL cite its source object; a candidate without a valid object citation SHALL be dropped at parse time, not shown. With no provider, the stage shows the designed no-key state and any cached weaknesses remain usable.

#### Scenario: Every weakness is traceable
- **WHEN** weakness finding completes
- **THEN** each listed weakness carries at least one `[[object:ID]]` link that opens the reader at the supporting passage

### Requirement: Hypothesis cards as durable user data
Hypotheses SHALL be cards — claim, rationale, required experiment, expected evidence, source weaknesses, novelty verdict+evidence once computed — stored append-only, user-editable, archivable, and linkable to a v3 experiment for actually running the design. Cards SHALL be anchorable chat contexts (discuss a card with the AI under standard budgeted assembly).

#### Scenario: Card links to a real experiment
- **WHEN** the user clicks "design this experiment" on a card
- **THEN** a v3 experiment is created pre-filled from the card's required-experiment description, and the card records the experiment id both ways

### Requirement: Novelty as evidence-backed estimate, never assertion
Novelty checking SHALL query open indexes (arXiv, Semantic Scholar) with explicit user action, rank results by local embedding similarity, and produce a closed-vocabulary verdict — `appears_novel`, `adjacent_work_exists`, `likely_known`, or `insufficient_evidence` — that SHALL always be paired with its evidence list (work, year, identifier, similarity) and the query used. An empty or failed search SHALL yield `insufficient_evidence`, never `appears_novel`. The UI SHALL never render a verdict without its evidence.

#### Scenario: No results is not novelty
- **WHEN** the index search returns nothing (offline, rate-limited, or genuinely empty)
- **THEN** the verdict is `insufficient_evidence` with a retry affordance — not a novelty claim

#### Scenario: Adjacent work found
- **WHEN** the search finds a closely similar published method
- **THEN** the verdict is `likely_known` or `adjacent_work_exists` with the works listed, each one importable as a paper

### Requirement: Drafts with honest citations and provenance
Outline and draft generation SHALL cite only from a pre-assembled bibliography (the paper, its resolved citations, novelty evidence, library papers); citation keys not in that bibliography SHALL be stripped at parse time with a visible count. Export SHALL produce LaTeX + BibTeX where BibTeX entries come only from resolved metadata, AI-drafted passages carry source-level provenance markers, and the document is labeled as an AI-assisted draft until the user removes the label deliberately.

#### Scenario: Invented citation stripped
- **WHEN** the model emits `\cite{smith2023}` and no such key exists in the assembled bibliography
- **THEN** the citation is removed before display, the draft notes "1 unverifiable citation removed", and the surrounding claim is flagged for the user

#### Scenario: Provenance in the export
- **WHEN** the user exports `main.tex`
- **THEN** AI-drafted passages are marked in source comments and a provenance block states what was generated vs user-written
