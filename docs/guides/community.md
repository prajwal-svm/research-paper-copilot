# Community contributions (v5)

A paper's knowledge object can be improved by anyone, PR-style, and **one
paper gets better forever**. Everything below lives in the paper's
Community pane (people icon in the reader dock, or `/publish` in the
omnibar).

## Proposing

Pick a contributor name (recorded in provenance permanently), choose what
to share (notes, concept canvas), and describe the improvement. A proposal
is a **change set against a base revision**: journal entries that
union-merge (the format's native diff — no CRDTs, no text conflicts) plus
content-addressed file adds. Proposals are created offline and queue until
the registry is reachable.

Policy validation runs at creation: publisher content (the PDF, page
images, extracted text) can never enter a proposal — violations are
rejected before any reviewer sees them.

## Reviewing

Reviewers see the full diff (every path, every entry). Accept merges the
change set — journals union in (stale bases are safe by construction),
whole-file conflicts abort the merge and are surfaced. Reject records the
reason. Both are **signed provenance events**.

New contributors always go through review. Trust levels (new → trusted →
maintainer) are earned from accepted contributions; only maintainers can
merge directly.

## Provenance & reputation

Every propose/review/merge/revert is an ed25519-signed, append-only event.
For any artifact you can answer: who contributed each revision, when, via
which proposal. Reverting produces a new revision — history is never
rewritten. Reputation is a **deterministic fold over this log**:
recomputing from the raw events reproduces the displayed numbers exactly.
There are no opaque scores.
