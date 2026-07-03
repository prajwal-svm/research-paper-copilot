# learner-memory

## ADDED Requirements

### Requirement: Three memory stores, event-sourced
The system SHALL maintain, per library, a learner model in `learning_state/` composed of mastery memory (per concept/object: score, attempts, quiz outcomes, spaced-repetition interval/ease with time decay computed at read), preference memory (learning-style signals: visual/code/formal, verbosity), and episodic memory (per-object confusion/insight summaries derived from conversation threads). All three SHALL be append-only event journals folded into snapshots — crash-safe and mergeable by the future sync change without destructive conflicts.

#### Scenario: Quiz outcome updates mastery
- **WHEN** the user answers a quiz item on "LayerNorm" incorrectly twice, then correctly
- **THEN** mastery events record each attempt and the folded snapshot shows an updated score and next-review interval per the spaced-repetition curve

#### Scenario: Crash mid-update
- **WHEN** the app crashes while appending a mastery event
- **THEN** on restart all previously committed events load and the torn write is discarded (journal semantics)

### Requirement: Memory shapes every AI output
AI explanations SHALL be filtered through the learner model: concepts the model records as mastered are referenced, not re-explained; concepts with repeated struggle signals change explanatory approach (different analogy, smaller steps, preferred style); episodic summaries of prior confusions on the anchored object are included in context. The learner-profile block added to prompts SHALL be compact (ids and levels, not transcripts).

#### Scenario: Mastered concept not re-explained
- **WHEN** the user has demonstrated mastery of "softmax" and asks about scaled dot-product attention
- **THEN** the explanation references softmax without re-teaching it

#### Scenario: Repeated struggle changes approach
- **WHEN** mastery memory records ≥3 failed attempts on a concept and the user asks about it again
- **THEN** the prompt instructs a changed approach (new analogy/style per preference memory) and the answer differs in strategy, not just wording

### Requirement: Privacy boundary of the learner model
The learner model SHALL remain on the user's machine: never included in telemetry (closed event-kind set), sent to an AI provider only as the compact profile block within an explicitly invoked action, and designated E2E-encrypted-only for the future sync change. The user SHALL be able to inspect and delete their learner data (per store and wholesale).

#### Scenario: Wholesale reset
- **WHEN** the user chooses "reset learning data" in settings and confirms
- **THEN** all `learning_state/` stores are deleted, the dashboard returns to the cold-start state, and nothing else (notes, chats, papers) is touched

### Requirement: Honest cold start
Until enough signal exists (a minimum number of quiz/interaction events per concept), mastery-derived figures SHALL be labeled as estimates and SHALL never gate content — every lesson, object, and answer remains accessible regardless of recorded mastery.

#### Scenario: Fresh install dashboard
- **WHEN** a user opens the dashboard before taking any quiz
- **THEN** progress figures are labeled estimated/unknown rather than showing fabricated precision
