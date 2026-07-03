# Research Paper Copilot — Master PRD

> "Research Paper Copilot is an open-source platform that transforms static scientific papers into interactive, explorable knowledge. Our goal is to make every research paper understandable, reproducible, and extendable by anyone."

**This file is the index.** Like a `CLAUDE.md` referencing `agents.md` and `memory.md`, every actionable detail lives in the referenced documents; this page tells you where and why.

## What we're building

An open-source **IDE for understanding, exploring, reproducing, and extending scientific research** — VS Code meets Figma meets GitHub, for knowledge. Not an AI PDF chat.

The core bet: every tool today treats **Paper = PDF** (flatten → chunk → vector DB → LLM, forgetting everything between sessions). We treat **Paper = Knowledge** — a structured, interactive, persistent knowledge object with the PDF as just one view. The headline innovation is **persistent understanding**: a knowledge graph plus a learner model that remembers what you've mastered, where you struggle, and how you like to learn. Forever.

## Product foundations — read these first

| Document | What it defines |
|---|---|
| [docs/vision.md](docs/vision.md) | Problem, missing abstraction, philosophy, mission, why it matters for humanity, v1→v5 arc |
| [docs/competitive-analysis.md](docs/competitive-analysis.md) | First-principles teardown of ChatGPT/Claude, NotebookLM, SciSpace, Explainpaper; our moat |
| [docs/ux-principles.md](docs/ux-principles.md) | Binding UX rules, signature interactions (equation/figure/citation/reading-mode), satisfaction metrics |
| [docs/architecture/research-format.md](docs/architecture/research-format.md) | The `.research` bundle — the critical data structure everything builds on |
| [docs/architecture/knowledge-graph-and-memory.md](docs/architecture/knowledge-graph-and-memory.md) | Knowledge graph, learner memory, context assembly without bloat, learning engine |
| [docs/architecture/platform-and-performance.md](docs/architecture/platform-and-performance.md) | ADR: Tauri desktop (mac/win/linux); local-first → **cloud sync → web app** sequence; CI-enforced performance budgets |

## Hard constraints (apply to every version)

1. **Local-first desktop first** (Tauri, Rust core). Cloud sync is the priority after v1, **before** any web app; the eventual web app must be identical to desktop.
2. **Performance budgets are requirements**, CI-enforced release blockers — see platform doc.
3. **The `.research` format is a public, versioned contract** — user data anchored to object UUIDs, derived data regenerable, community data provenance-tracked.
4. **UX & customer satisfaction over feature count** — every spec cites its budget and its degraded modes.
5. **Open source** — the community layer is the long-term moat.

## Version roadmap — actionable PRDs

| Version | Theme | PRD | Execution |
|---|---|---|---|
| **v1** | The Foundation — ingest, layout-true reader, object-level chat (text/figures/equations/tables), per-object persistence | [docs/prd/v1-foundation.md](docs/prd/v1-foundation.md) | **OpenSpec change: [`add-v1-foundation`](openspec/changes/add-v1-foundation/proposal.md)** (proposal · design · specs · tasks) |
| v1.x | Z.ai GLM provider (in-app only) — Anthropic-compatible custom endpoints + GLM-5.2 preset ([overview](docs/dev/glm-devpack.md)) | covered in ai-providers capability | OpenSpec change: [`add-zai-glm-provider`](openspec/changes/add-zai-glm-provider/proposal.md) |
| v1.5 | Cloud sync (before web) | covered in platform doc §ADR-002 | future change: `add-cloud-sync` |
| v2 | The Learning Engine — knowledge graph, dashboard, memory, quizzes, reading mode, Socratic tutor, cross-paper linking | [docs/prd/v2-learning-engine.md](docs/prd/v2-learning-engine.md) | future change: `add-v2-learning-engine` |
| v3 | The Hacker Workspace — implementation mode (Python/PyTorch/TF/JAX/Rust), experiment mode, reproduction mode (clone→build→verify), web app | [docs/prd/v3-hacker-workspace.md](docs/prd/v3-hacker-workspace.md) | future change: `add-v3-hacker-workspace` |
| v4 | The Researcher Workspace — extension mode (weaknesses→hypotheses→draft), literature reviews, gap detection, collaboration | [docs/prd/v4-researcher-workspace.md](docs/prd/v4-researcher-workspace.md) | future change: `add-v4-researcher-workspace` |
| v5 | The Operating System — community knowledge layer, registry, plugins, full platform parity | [docs/prd/v5-operating-system.md](docs/prd/v5-operating-system.md) | future change: `add-v5-operating-system` |

Per the process decision: **one OpenSpec change per version** from v1 onward; only v1 is fully specified now, v2–v5 are documented as roadmap PRDs and get their OpenSpec changes when their turn comes.

## OpenSpec working agreement

- `openspec/config.yaml` carries project context + artifact rules (non-goals mandatory, budgets cited, degraded modes covered).
- Capabilities are kebab-case and user-outcome-named; specs use SHALL + WHEN/THEN scenarios and are the test contract.
- Nothing in `docs/prd/v2+` is buildable until it has an approved change under `openspec/changes/`.

## North-star metrics

- **Time-to-first-wow** < 2 minutes from install.
- **Return-to-same-paper rate** — people *studying*, not sampling.
- **Mastery created** (v2+): concepts/equations mastered per user per month.
- **Reproductions verified** (v3+), **papers extended** (v4+), **community-enriched papers** (v5).
