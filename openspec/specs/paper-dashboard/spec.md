# paper-dashboard

## Purpose

The pre-reader dashboard: mastery-derived progress figures and the continue-reading entry point.

## Requirements

### Requirement: Knowledge dashboard on open
Opening a paper SHALL show a dashboard before/alongside the reader: progress %, understanding level, estimated remaining reading effort, concepts learned/remaining, equations mastered (x/y), figures understood (x/y), and quiz score — visible within 500 ms of open on the reference machine, with "continue where you left off" as the primary action (restoring the exact reading/lesson position). The dashboard SHALL be skippable via preference (open straight to reader).

#### Scenario: Returning to a partially studied paper
- **WHEN** the user opens a paper they have studied across multiple sessions
- **THEN** the dashboard appears in <500 ms showing mastery-derived figures and one primary continue button that restores their last position (reader scroll or lesson cursor)

#### Scenario: Prefer direct reader
- **WHEN** the user enables "skip dashboard"
- **THEN** papers open directly in the reader with a compact progress indicator available on demand

### Requirement: Honest, mastery-derived progress
All dashboard figures SHALL derive from demonstrated understanding (quiz outcomes, tutor exchanges, explanation feedback) and never from vanity signals (scroll depth, time on page). Estimates below the cold-start signal threshold SHALL be labeled as such (per learner-memory). Counts SHALL be reproducible from the learner-memory snapshot plus the paper's graph — no dashboard-private state.

#### Scenario: Scrolling changes nothing
- **WHEN** the user scrolls an entire paper without any interaction or quiz
- **THEN** progress and understanding figures do not increase

#### Scenario: Quiz moves the numbers
- **WHEN** the user completes a quiz mastering two more equations
- **THEN** "equations mastered" increments accordingly on next dashboard view and the change traces to recorded mastery events

### Requirement: Implementation-complete signal
The dashboard SHALL show an "implementation complete" indicator per implementable concept, flipping when the user's implementation passes its generated checks (fed by the same mastery-event path as quizzes — one data path, no duplicated state). The indicator SHALL be honest: generated-but-unverified implementations do not count, and the signal SHALL never gate any content.

#### Scenario: Checks flip the dashboard
- **WHEN** the user's implementation of the attention equation passes its checks
- **THEN** the next dashboard view shows the concept's implementation-complete indicator, derived from the recorded mastery event

#### Scenario: Unverified doesn't count
- **WHEN** an implementation exists but its checks have never passed
- **THEN** the indicator remains "not yet verified" rather than implying completion
