# Scaled Dot-Product Attention and Multi-Head Attention

Two diagrams, two zoom levels of the same mechanism.

## Left: Scaled Dot-Product Attention

The dataflow of Equation 1, bottom to top: Q and K are matrix-multiplied, scaled by 1/√d_k, optionally masked (the decoder uses the mask to hide future positions), softmaxed into weights, and applied to V. This is one attention computation.

## Right: Multi-Head Attention

Instead of one attention over the full 512 dimensions, the model runs **h = 8 heads in parallel**. Q, K, and V are each linearly projected down to 64 dimensions per head, each head runs scaled dot-product attention independently, the results are concatenated, and a final linear layer mixes them.

## Why multiple heads?

A single attention distribution has to average all the relationships it wants to track. Eight smaller heads can each specialize — one may track syntactic dependencies, another coreference, another adjacent words — at the same total cost as one full-width head. The paper's visualizations (appendix) show exactly this specialization emerging.
