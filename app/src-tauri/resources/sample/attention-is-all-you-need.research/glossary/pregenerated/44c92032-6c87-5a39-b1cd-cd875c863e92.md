# Scaled Dot-Product Attention

**Attention(Q, K, V) = softmax(QKᵀ / √d_k) V**

## Variables

- **Q (queries)** — a matrix where each row asks "what am I looking for?" for one position in the sequence.
- **K (keys)** — each row advertises "what do I contain?" for one position. Comparing a query against every key scores how relevant each position is.
- **V (values)** — the actual content at each position, which gets mixed together according to those scores.
- **d_k** — the dimensionality of the keys (64 in this paper). Only its square root is used, as a scaling constant.

## Step by step

1. **QKᵀ** — every query is dot-multiplied with every key, producing a score matrix: how much should position *i* attend to position *j*?
2. **/ √d_k** — the scores are shrunk by √d_k. For large d_k, raw dot products grow large, pushing softmax into regions with tiny gradients; scaling keeps it in a healthy range (see the paper's footnote 4).
3. **softmax(·)** — each row of scores becomes a probability distribution: non-negative weights summing to 1.
4. **· V** — those weights blend the value vectors. Each output position is a weighted average of every position's content.

## Intuition

Attention is a *soft dictionary lookup*. A normal dictionary returns the single value whose key matches exactly. Attention returns a weighted mix of **all** values, where the weights measure how well each key matches the query. Because the comparison is a dot product, the whole lookup is one matrix multiplication — trivially parallel, which is the paper's core bet against recurrence.
