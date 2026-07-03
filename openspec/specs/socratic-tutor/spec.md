# socratic-tutor

## Purpose

The in-lesson tutor: question–wait–hint–correction loop, learner-model feedback, and interruption safety.

## Requirements

### Requirement: Question–wait–hint–correction loop
Within lessons, the tutor SHALL follow the Socratic contract enforced as a client-side state machine over the streaming chat: pose one question → wait for the user's attempt (no answer-dumping while waiting) → on a wrong/partial attempt offer a graduated hint (not the answer) → only after hints are exhausted or on request give the correction with explanation → proceed. The model SHALL be prompted per state; it never free-runs the loop.

#### Scenario: Wrong answer gets a hint, not the solution
- **WHEN** the user answers a tutor question incorrectly the first time
- **THEN** the tutor responds with a hint that narrows the gap and re-asks, without revealing the answer

#### Scenario: User is one hint away
- **WHEN** the user's attempt is nearly correct
- **THEN** the hint addresses only the missing piece (never a full answer dump)

#### Scenario: User gives up
- **WHEN** the user asks "just tell me"
- **THEN** the tutor gives the correction immediately with a concise explanation and continues the lesson — the loop never traps the user

### Requirement: Tutor exchanges feed the learner model
Tutor attempts and outcomes SHALL be recorded as mastery events (correct/incorrect/hint-count) and confusion patterns as episodic memory on the anchored concept/object, so future explanations adapt (per learner-memory requirements).

#### Scenario: Struggle recorded and used
- **WHEN** the user needs three hints on a positional-encoding question
- **THEN** mastery events reflect the struggle and the next encounter of that concept uses a different explanatory approach

### Requirement: Tutor honesty and interruption safety
Tutor turns SHALL stream with cancel-anytime; interrupted or failed turns preserve partials marked incomplete (v1 chat rules apply). The tutor SHALL ground its questions in the paper's objects with clickable references, and with no provider configured the tutor step is skipped with the designed no-key state while the rest of the lesson (cached content, quiz review of cached items) still works.

#### Scenario: Provider drops mid-question
- **WHEN** the stream fails while the tutor is posing a question
- **THEN** the partial is kept and marked, retry is one click, and the lesson can proceed without the tutor step
