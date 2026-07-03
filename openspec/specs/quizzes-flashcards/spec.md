# quizzes-flashcards

## Purpose

Object-anchored quiz/flashcard generation, spaced-repetition scheduling, and the single quiz→mastery→dashboard data path.

## Requirements

### Requirement: Generation anchored to objects and concepts
Quizzes and flashcards SHALL be generated per concept/object (equations, figures, definitions), stored in the bundle's `quizzes/` and `flashcards/` directories keyed by anchor UUID + content hash, cached after first generation, and pre-generated for the bundled sample paper. Items SHALL cite their source objects as clickable reader links; generation SHALL stream and degrade kindly with no provider (cached items usable, generation shows the designed no-key state).

#### Scenario: Equation quiz generated once
- **WHEN** the user requests a quiz on Equation 1 twice
- **THEN** the second open serves the cached items in <100 ms without a provider call

#### Scenario: Anchor changed by re-parsing
- **WHEN** a paper is re-ingested and an anchored object's content hash changes
- **THEN** affected items are flagged for regeneration rather than silently serving stale questions

### Requirement: Spaced-repetition scheduling
Flashcard review SHALL be scheduled by a spaced-repetition curve over mastery memory (interval/ease per item), with a due-review queue accessible per paper and library-wide. Review outcomes SHALL append mastery events; nothing external schedules work (due-ness computed from timestamps at read).

#### Scenario: Failed card returns sooner
- **WHEN** the user fails a card and passes a different one in the same session
- **THEN** the failed card's next due time is sooner than the passed card's, per the curve

### Requirement: Results feed mastery and the dashboard
Quiz grading SHALL be immediate and explained (why an answer is wrong, citing the paper), and every graded outcome SHALL append mastery events that the dashboard and lesson filtering consume — the quiz→mastery→dashboard loop is one data path with no duplicated state.

#### Scenario: Quiz updates everything downstream
- **WHEN** the user completes a 5-item quiz on attention concepts
- **THEN** mastery snapshots update, the dashboard counts change on next view, and reading mode collapses newly mastered lessons — all from the same recorded events
