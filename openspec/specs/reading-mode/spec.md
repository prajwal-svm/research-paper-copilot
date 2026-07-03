# reading-mode

## Purpose

The paper as a course: lesson sequencing from the graph, escapability to the raw paper, and non-blocking generation.

## Requirements

### Requirement: Paper as a course
Reading mode SHALL present the paper as a lesson sequence derived from the knowledge graph: lessons ordered by prerequisite topology (low-confidence edges excluded from ordering), filtered — never gated — by mastery (mastered concepts collapse to a skippable recap). Each lesson SHALL follow the pattern: mini explanation → supporting diagram/figure from the paper → exercise → quiz item(s) → continue. Lesson content SHALL be generated lazily per node, cached in the bundle, streamed on first generation, and pre-generated for the bundled sample paper (zero-setup rule).

#### Scenario: Course order respects prerequisites
- **WHEN** the user starts reading mode on the sample paper
- **THEN** foundational concepts (e.g. Softmax, Scaling) are scheduled before dependents (e.g. Scaled Dot-Product Attention, Multi-head) per the graph topology

#### Scenario: Mastered lesson collapses, never locks
- **WHEN** a concept is recorded mastered
- **THEN** its lesson shows as a collapsed recap the user can still expand and take in full

#### Scenario: No provider
- **WHEN** reading mode is entered with no AI provider configured
- **THEN** cached/pre-generated lessons work fully; ungenerated lessons show the designed no-key state with the paper's own objects (never an error)

### Requirement: Always escapable to the raw paper
From any lesson the user SHALL be able to switch to the raw paper at the location backing that lesson in one action (<300 ms), and return to the same lesson position afterwards. Reading-mode position (lesson cursor, step) SHALL persist per paper and restore on reopen, extending v1 persisted reading state.

#### Scenario: Escape and return
- **WHEN** the user hits "show me in the paper" during a lesson and later returns to reading mode
- **THEN** the reader opened at the lesson's anchor object, and reading mode resumed at the same step

### Requirement: Reading is sacred in reading mode too
Lesson generation and quiz grading SHALL never block navigation: moving between lessons stays interactive while content streams (skeleton within 300 ms), and background work respects all v1 rendering budgets.

#### Scenario: Slow generation does not freeze the player
- **WHEN** a lesson's content takes long to generate
- **THEN** the player shows streamed partial content or a skeleton within 300 ms and next/previous/escape remain responsive throughout
