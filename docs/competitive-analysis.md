# Competitive Analysis — first principles

Every incumbent shares one flawed abstraction: **Paper = PDF = string of text**. OCR → chunk → vector DB → prompt. Layout, dependencies, and the reader's learning state are destroyed before the AI ever sees the paper.

## 1. ChatGPT / Claude (generic assistants)

**Architecture:** PDF → OCR → Chunks → Context window → LLM

**Strengths:** great reasoning, great explanations, flexible.

**Weaknesses:**
- No persistent understanding — every conversation starts over
- Doesn't understand page layout
- Doesn't understand relationships (doesn't know Figure 5 depends on Equation 12)
- Doesn't remember what you've learned (doesn't know you've mastered Section 3)

## 2. NotebookLM

A real improvement: Question → Retrieval → Answer instead of stuffing the whole document.

**But:** it is **document-centric, not knowledge-centric**. It retrieves passages; it never builds a mental model of the paper or of you.

## 3. SciSpace (current category leader)

**Strengths:** figure explanation, equation explanation, highlight-to-ask, literature review.

**Weakness:** everything centers on *Highlight → Explain*, never *Learn → Understand → Master → Create*. No memory, no progress, no mastery.

## 4. Explainpaper

Fantastic onboarding, very approachable. But essentially *Highlight → ELI5* — not much beyond that.

## Positioning summary

| Dimension | Incumbents | Research Paper Copilot |
|---|---|---|
| Core goal | Extract & summarize | Learn, understand, master, create |
| Abstraction | Paper = PDF | Paper = Knowledge |
| Context | Vector search per query; forgets | Knowledge graph + memory; almost no context bloat |
| Structure | Flattened chunks | Interactive, interdependent objects with UUIDs |
| Progression | Every session starts over | Persistent learning state, mastery tracking |
| Source of truth | The PDF | The `.research` bundle; PDF is one view |
| Business model | Proprietary SaaS | Open-source infrastructure + community |

## Moat

1. **The `.research` format** — a public, versioned knowledge container others can build on (network effects like `.psd`, `.fig`, git repos).
2. **Persistent understanding** — the longer you use it, the better it knows you; switching cost is your own learning history.
3. **Community-improved papers** — explanations, quizzes, animations, and implementations compound per paper. Incumbents cannot crowdsource because they own no open format.
4. **Local-first + open source** — trust story proprietary tools can't match (your library, notes, and learning history never leave your machine unless you opt into sync).
