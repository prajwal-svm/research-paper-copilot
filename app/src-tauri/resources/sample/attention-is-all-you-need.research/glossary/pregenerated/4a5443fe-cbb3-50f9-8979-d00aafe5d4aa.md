# The Transformer architecture

This figure shows the full model: an **encoder** (left column) and a **decoder** (right column), each a stack of N = 6 identical layers.

## What each element shows

- **Input / Output Embedding** — tokens become d_model = 512-dimensional vectors. The decoder input is the target sequence *shifted right*, so position *i* can only be predicted from positions before it.
- **Positional Encoding** (the ⊕ with sine waves) — since nothing in the model is recurrent or convolutional, order information is injected by adding sinusoidal position vectors to the embeddings.
- **Multi-Head Attention** (orange) — the encoder attends over its own input (self-attention). The decoder has two attention blocks: a *masked* one over its own previous outputs, and one attending over the encoder's output (this is where source and target sequences meet).
- **Feed Forward** (blue) — a two-layer position-wise network applied identically at every position.
- **Add & Norm** (yellow) — every sub-layer is wrapped in a residual connection followed by layer normalization; this is what makes a 6-layer stack trainable.
- **Linear + Softmax** (top) — the decoder output becomes next-token probabilities.

## What to conclude

The entire model is built from attention and feed-forward layers — no recurrence, no convolution. That is the paper's thesis: attention is *all* you need, and it buys massively better parallelism during training.
