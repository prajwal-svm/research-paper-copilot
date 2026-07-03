# Vision — Research Paper Copilot

> An open-source IDE for understanding, exploring, reproducing, and extending scientific research.

## The problem

Research papers are written for publication, not for learning. A single paper is one of the densest artifacts of human knowledge — it contains text, figures, equations, tables, algorithms, references, assumptions, experiments, datasets, appendices, source code, terminology, and an implicit wall of prerequisite knowledge.

Yet every tool today reduces it to:

```
String of text → Chunk → Vector DB → LLM
```

That is like taking a Photoshop file, flattening every layer into a single JPEG, and asking an AI to edit it. The structure is gone. The dependencies are gone (Figure 5 depends on Equation 12; Section 4 assumes Section 3). The reader's own progress is gone — every conversation starts over.

## The missing abstraction

Every current product thinks **Paper = PDF**. This product thinks **Paper = Knowledge**.

Existing tools optimize for *extracting information from* papers. Research Paper Copilot optimizes for something fundamentally different: **transforming a paper into an interactive learning environment** — Learn → Understand → Master → Create, instead of Highlight → Explain.

## Philosophy

VS Code is not "a text editor" — it is a programming workspace. Research Paper Copilot is not "an AI PDF chat" — it is a **research workspace**.

The metaphor: **GitHub meets VS Code meets Figma.** GitHub stores code. VS Code edits code. Figma edits designs. Research Paper Copilot stores, edits, and grows **knowledge**.

## The biggest innovation: Persistent Understanding

AI is not the innovation. The innovation is that the system never forgets:

```
Today:   Chat → Forget → Chat → Forget
Ours:    Knowledge Graph → Memory → Relationships → Learning History → Forever
```

The system knows you struggled with LayerNorm five times, that you already mastered cross-entropy, and that you prefer visual explanations — and every future interaction adapts.

## What opening a paper feels like

Not a wall of pages. A dashboard:

```
Transformer Paper
─────────────────────────────
Progress                46%
Understanding           Intermediate
Estimated reading time  8 hours
Concepts learned        127   ·  remaining 42
Equations mastered      18 / 27
Figures understood      9 / 14
Implementation          Not started
Quiz score              82%
```

## Product identity

- **Name:** Research Paper Copilot
- **Repository:** `research-paper-copilot`
- **File format:** `.research` (alias `.rpc`) — the PDF becomes one *view*, not the source of truth
- **Positioning:** not marketed as an AI product, but as **the open research operating system**
- **License/ethos:** open source. Community-improved papers: better explanations, animations, quizzes, implementations, visualizations. One paper gets better forever.

## Why this matters for humanity

If this succeeds, its value is not slightly better AI answers — it is **lowering the barrier to understanding human knowledge**. Countless papers go unread because they assume years of prerequisites; students, engineers, independent researchers, and professionals spend more time deciphering notation than understanding contributions.

An open, interactive research workspace can:

- Make cutting-edge research accessible to students worldwide, regardless of institution.
- Help researchers reproduce results reliably, improving scientific rigor.
- Reduce duplicated effort by making prior work easier to understand.
- Accelerate interdisciplinary discovery by letting experts in one field learn another quickly.
- Create shared, open infrastructure for scientific knowledge instead of locking it behind proprietary AI products.

## Mission statement (top of the repository)

> "Research Paper Copilot is an open-source platform that transforms static scientific papers into interactive, explorable knowledge. Our goal is to make every research paper understandable, reproducible, and extendable by anyone."

## Long-term progression

| Version | Theme | One-liner |
|---|---|---|
| v1 | The Foundation | Upload papers, preserve layout, chat with selected text, figures, equations, tables |
| v2 | The Learning Engine | Persistent knowledge graph, learning progress, quizzes, notes, cross-paper linking |
| v3 | The Hacker Workspace | Code understanding, experiment reproduction, automatic environment setup, implementation guidance |
| v4 | The Researcher Workspace | AI-assisted literature reviews, hypothesis generation, research gap detection, collaborative workspaces |
| v5 | The Operating System | A paper is a living, evolving knowledge object anyone can study, improve, reproduce, and extend |

Detailed PRDs: [v1](prd/v1-foundation.md) · [v2](prd/v2-learning-engine.md) · [v3](prd/v3-hacker-workspace.md) · [v4](prd/v4-researcher-workspace.md) · [v5](prd/v5-operating-system.md)
