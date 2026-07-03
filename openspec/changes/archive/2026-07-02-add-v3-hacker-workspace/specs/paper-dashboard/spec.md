# paper-dashboard (delta)

## ADDED Requirements

### Requirement: Implementation-complete signal
The dashboard SHALL show an "implementation complete" indicator per implementable concept, flipping when the user's implementation passes its generated checks (fed by the same mastery-event path as quizzes — one data path, no duplicated state). The indicator SHALL be honest: generated-but-unverified implementations do not count, and the signal SHALL never gate any content.

#### Scenario: Checks flip the dashboard
- **WHEN** the user's implementation of the attention equation passes its checks
- **THEN** the next dashboard view shows the concept's implementation-complete indicator, derived from the recorded mastery event

#### Scenario: Unverified doesn't count
- **WHEN** an implementation exists but its checks have never passed
- **THEN** the indicator remains "not yet verified" rather than implying completion
