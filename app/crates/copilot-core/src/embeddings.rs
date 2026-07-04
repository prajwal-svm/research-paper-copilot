//! Ingestion stage 4: local embeddings → `embeddings.bin` + index sidecar.
//!
//! Embeddings are computed locally (candle, no network at query time) so
//! semantic search works offline and without API keys. The binary format is
//! mmap-friendly: raw little-endian f32 rows, L2-normalized, row order given
//! by `embeddings_index.json` (object UUID per row). The exact model and
//! dimensions are pinned in the index and in `metadata.json` so vectors stay
//! interpretable forever.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::objects::{EmbeddingRef, ObjectType, SemanticTreeDocument};

pub const EMBEDDINGS_PIPELINE_VERSION: &str = "0.1.0";
/// Pinned model (design open question resolved): small, fast, well-understood
/// sentence-embedding model; 384 dims, cosine via dot product on normalized
/// vectors.
pub const EMBEDDING_MODEL_NAME: &str = "sentence-transformers/all-MiniLM-L6-v2";
pub const EMBEDDING_DIM: usize = 384;

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingsError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("semantic_tree.json missing — run the objects stage first")]
    TreeMissing,
    #[error("model load failed: {0}")]
    Model(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("embeddings.bin size {found} does not match index ({expected} bytes)")]
    SizeMismatch { expected: usize, found: usize },
}

// ---------------------------------------------------------------------------
// Index sidecar
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsIndex {
    pub pipeline_version: String,
    pub model: String,
    pub dimensions: usize,
    /// Row i of embeddings.bin belongs to `rows[i]`.
    pub rows: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// Embedder (candle + MiniLM)
// ---------------------------------------------------------------------------

/// Local sentence embedder. Loading downloads model files on first use into
/// the shared HF cache; afterwards it is fully offline.
pub struct Embedder {
    model: candle_transformers::models::bert::BertModel,
    tokenizer: tokenizers::Tokenizer,
    device: candle_core::Device,
}

impl Embedder {
    pub fn load() -> Result<Self, EmbeddingsError> {
        use candle_core::{DType, Device};
        use candle_nn::VarBuilder;
        use candle_transformers::models::bert::{BertModel, Config};

        let err = |e: String| EmbeddingsError::Model(e);

        // Local-first model resolution. hf_hub's `Api::repo().get()` always
        // makes a metadata HEAD request to huggingface.co — even when the file
        // is already cached — and that call hangs indefinitely on a stalled
        // connection (proxy, captive portal, throttled wifi), which freezes
        // the whole ingestion pipeline on "Building search index". `Cache` is
        // a pure local lookup: it reads `refs/main` → `snapshots/<hash>/` and
        // returns the path with zero network I/O. We only fall back to the
        // network `Api` when the model genuinely isn't cached yet.
        let cache = hf_hub::Cache::from_env();
        let cached = cache.model(EMBEDDING_MODEL_NAME.to_string());
        let (config_path, tokenizer_path, weights_path) = match (
            cached.get("config.json"),
            cached.get("tokenizer.json"),
            cached.get("model.safetensors"),
        ) {
            (Some(c), Some(t), Some(w)) => (c, t, w),
            _ => {
                use hf_hub::api::sync::ApiBuilder;
                use hf_hub::{Repo, RepoType};
                // First run: download once. Single retry so a flaky
                // connection fails fast (and degrades to exact-only search)
                // instead of looping or hanging.
                let api = ApiBuilder::new()
                    .with_retries(1)
                    .build()
                    .map_err(|e| err(e.to_string()))?;
                let repo = api.repo(Repo::new(EMBEDDING_MODEL_NAME.to_string(), RepoType::Model));
                (
                    repo.get("config.json").map_err(|e| err(e.to_string()))?,
                    repo.get("tokenizer.json").map_err(|e| err(e.to_string()))?,
                    repo.get("model.safetensors")
                        .map_err(|e| err(e.to_string()))?,
                )
            }
        };

        let config: Config =
            serde_json::from_slice(&std::fs::read(&config_path).map_err(|e| err(e.to_string()))?)
                .map_err(|e| err(e.to_string()))?;
        let tokenizer =
            tokenizers::Tokenizer::from_file(&tokenizer_path).map_err(|e| err(e.to_string()))?;

        // Apple Silicon: the GPU cuts "Building search index" from minutes
        // to seconds. Anything else (or a Metal init failure) uses the CPU.
        #[cfg(all(target_os = "macos", feature = "native"))]
        let device = Device::new_metal(0).unwrap_or(Device::Cpu);
        #[cfg(not(all(target_os = "macos", feature = "native")))]
        let device = Device::Cpu;
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)
                .map_err(|e| err(e.to_string()))?
        };
        let model = BertModel::load(vb, &config).map_err(|e| err(e.to_string()))?;
        Ok(Embedder {
            model,
            tokenizer,
            device,
        })
    }

    /// Embed a batch of texts → L2-normalized vectors of [`EMBEDDING_DIM`].
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingsError> {
        self.embed_progress(texts, |_, _| {})
    }

    /// Embed with per-chunk progress. `on_progress(done, total)` is called
    /// after each batch of 16, so callers can report how far a long paper has
    /// gotten instead of appearing frozen.
    pub fn embed_progress(
        &self,
        texts: &[&str],
        mut on_progress: impl FnMut(usize, usize),
    ) -> Result<Vec<Vec<f32>>, EmbeddingsError> {
        use candle_core::Tensor;
        let err = |e: String| EmbeddingsError::Model(e);

        let total = texts.len();
        let mut out = Vec::with_capacity(total);
        // Batch in small chunks to bound memory on long papers.
        for chunk in texts.chunks(16) {
            let mut tokenizer = self.tokenizer.clone();
            let tokenizer = tokenizer
                .with_padding(Some(tokenizers::PaddingParams::default()))
                .with_truncation(Some(tokenizers::TruncationParams {
                    max_length: 256,
                    ..Default::default()
                }))
                .map_err(|e| err(e.to_string()))?;
            let encodings = tokenizer
                .encode_batch(chunk.to_vec(), true)
                .map_err(|e| err(e.to_string()))?;

            let ids: Vec<Vec<u32>> = encodings.iter().map(|e| e.get_ids().to_vec()).collect();
            let masks: Vec<Vec<u32>> = encodings
                .iter()
                .map(|e| e.get_attention_mask().to_vec())
                .collect();

            let input_ids = Tensor::new(ids, &self.device).map_err(|e| err(e.to_string()))?;
            let attention_mask =
                Tensor::new(masks, &self.device).map_err(|e| err(e.to_string()))?;
            let token_type_ids = input_ids.zeros_like().map_err(|e| err(e.to_string()))?;

            let hidden = self
                .model
                .forward(&input_ids, &token_type_ids, Some(&attention_mask))
                .map_err(|e| err(e.to_string()))?;

            // Mean pooling over valid tokens, then L2 normalization.
            let mask = attention_mask
                .to_dtype(candle_core::DType::F32)
                .map_err(|e| err(e.to_string()))?
                .unsqueeze(2)
                .map_err(|e| err(e.to_string()))?;
            let summed = hidden
                .broadcast_mul(&mask)
                .map_err(|e| err(e.to_string()))?
                .sum(1)
                .map_err(|e| err(e.to_string()))?;
            let counts = mask.sum(1).map_err(|e| err(e.to_string()))?;
            let mean = summed
                .broadcast_div(&counts)
                .map_err(|e| err(e.to_string()))?;

            let vectors: Vec<Vec<f32>> = mean.to_vec2().map_err(|e| err(e.to_string()))?;
            for mut v in vectors {
                let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
                v.iter_mut().for_each(|x| *x /= norm);
                out.push(v);
            }

            on_progress(out.len(), total);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Stage entry point
// ---------------------------------------------------------------------------

/// Which objects get embeddings: everything a user would search for.
/// Sentences are covered by their paragraph (halves the work).
fn embeddable(object_type: ObjectType) -> bool {
    matches!(
        object_type,
        ObjectType::Section
            | ObjectType::Paragraph
            | ObjectType::Equation
            | ObjectType::Figure
            | ObjectType::Table
    )
}

/// Run stage 4: embed objects, write `embeddings.bin` + `embeddings_index.json`,
/// backfill `embedding.index` in `semantic_tree.json`, pin the model in metadata.
///
/// `on_progress(done, total)` fires after each batch, so the pipeline can stream
/// intra-stage progress (otherwise a long paper looks frozen on "Building search
/// index").
pub fn run_embeddings_stage(
    bundle: &Bundle,
    embedder: &Embedder,
    on_progress: impl FnMut(usize, usize),
) -> Result<usize, EmbeddingsError> {
    let started_at = crate::bundle::now_rfc3339();
    let mut tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")?
        .ok_or(EmbeddingsError::TreeMissing)?;

    let targets: Vec<(usize, Uuid, String)> = tree
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| embeddable(o.object_type) && !o.content.text.trim().is_empty())
        .map(|(i, o)| (i, o.id, o.content.text.clone()))
        .collect();

    let texts: Vec<&str> = targets.iter().map(|(_, _, t)| t.as_str()).collect();
    let vectors = embedder.embed_progress(&texts, on_progress)?;

    // Write embeddings.bin (raw f32 LE rows) atomically.
    let mut bytes = Vec::with_capacity(vectors.len() * EMBEDDING_DIM * 4);
    for v in &vectors {
        for x in v {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
    }
    let bin_path = bundle.root().join("embeddings.bin");
    let tmp = bin_path.with_extension("tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| EmbeddingsError::Io {
        path: tmp.clone(),
        source: e,
    })?;
    std::fs::rename(&tmp, &bin_path).map_err(|e| EmbeddingsError::Io {
        path: bin_path.clone(),
        source: e,
    })?;

    // Index sidecar + backfilled embedding refs.
    let index = EmbeddingsIndex {
        pipeline_version: EMBEDDINGS_PIPELINE_VERSION.to_string(),
        model: EMBEDDING_MODEL_NAME.to_string(),
        dimensions: EMBEDDING_DIM,
        rows: targets.iter().map(|(_, id, _)| *id).collect(),
    };
    for (row, (object_index, _, _)) in targets.iter().enumerate() {
        tree.objects[*object_index].embedding = Some(EmbeddingRef { index: row as u32 });
    }

    let stage = serde_json::json!({
        "pipeline_version": EMBEDDINGS_PIPELINE_VERSION,
        "status": "complete",
        "started_at": started_at,
        "completed_at": crate::bundle::now_rfc3339(),
    });
    bundle.write_derived_json("embeddings_index.json", &index, "embeddings", stage.clone())?;
    bundle.write_derived_json(
        "semantic_tree.json",
        &tree,
        "objects",
        // Preserve the objects stage record; write_derived_json replaces it,
        // so restate completion (content unchanged semantically).
        serde_json::json!({
            "pipeline_version": crate::objects::OBJECTS_PIPELINE_VERSION,
            "status": "complete",
            "completed_at": crate::bundle::now_rfc3339(),
        }),
    )?;

    // Pin the model in metadata.
    let mut metadata = bundle.metadata()?;
    metadata.embedding_model = Some(serde_json::json!({
        "name": EMBEDDING_MODEL_NAME,
        "dimensions": EMBEDDING_DIM,
        "quantization": "f32",
    }));
    bundle.write_metadata(&metadata)?;

    Ok(vectors.len())
}

// ---------------------------------------------------------------------------
// Store: mmap loading + search
// ---------------------------------------------------------------------------

/// Read view over a bundle's embeddings: mmap of `embeddings.bin` plus index.
pub struct EmbeddingStore {
    mmap: memmap2::Mmap,
    pub index: EmbeddingsIndex,
}

impl EmbeddingStore {
    /// `Ok(None)` when the embeddings stage hasn't run yet.
    pub fn open(bundle: &Bundle) -> Result<Option<Self>, EmbeddingsError> {
        let Some(index): Option<EmbeddingsIndex> =
            bundle.read_derived_json("embeddings_index.json")?
        else {
            return Ok(None);
        };
        let bin_path = bundle.root().join("embeddings.bin");
        if !bin_path.is_file() {
            return Ok(None);
        }
        let file = std::fs::File::open(&bin_path).map_err(|e| EmbeddingsError::Io {
            path: bin_path.clone(),
            source: e,
        })?;
        let mmap = unsafe {
            memmap2::Mmap::map(&file).map_err(|e| EmbeddingsError::Io {
                path: bin_path.clone(),
                source: e,
            })?
        };
        let expected = index.rows.len() * index.dimensions * 4;
        if mmap.len() != expected {
            return Err(EmbeddingsError::SizeMismatch {
                expected,
                found: mmap.len(),
            });
        }
        Ok(Some(EmbeddingStore { mmap, index }))
    }

    pub fn len(&self) -> usize {
        self.index.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.rows.is_empty()
    }

    /// Vector for row i (zero-copy view into the mmap).
    pub fn row(&self, i: usize) -> Vec<f32> {
        let dims = self.index.dimensions;
        let start = i * dims * 4;
        self.mmap[start..start + dims * 4]
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    }

    /// Top-k objects by cosine similarity (vectors are normalized, so dot
    /// product). Brute force — hundreds of objects per paper, microseconds.
    pub fn search(&self, query: &[f32], k: usize) -> Vec<(Uuid, f32)> {
        let mut scored: Vec<(Uuid, f32)> = (0..self.len())
            .map(|i| {
                let row = self.row(i);
                let score: f32 = row.iter().zip(query).map(|(a, b)| a * b).sum();
                (self.index.rows[i], score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(k);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    fn synthetic_store(dir: &std::path::Path, vectors: &[(Uuid, Vec<f32>)]) -> Bundle {
        let bundle = Bundle::create(dir, b"%PDF-1.5 fake", Paper::new("S"), "file").unwrap();
        let dims = vectors[0].1.len();
        let mut bytes = Vec::new();
        for (_, v) in vectors {
            for x in v {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        std::fs::write(dir.join("embeddings.bin"), bytes).unwrap();
        let index = EmbeddingsIndex {
            pipeline_version: EMBEDDINGS_PIPELINE_VERSION.to_string(),
            model: "test-model".to_string(),
            dimensions: dims,
            rows: vectors.iter().map(|(id, _)| *id).collect(),
        };
        bundle
            .write_derived_json(
                "embeddings_index.json",
                &index,
                "embeddings",
                serde_json::json!({"pipeline_version": EMBEDDINGS_PIPELINE_VERSION, "status": "complete"}),
            )
            .unwrap();
        bundle
    }

    #[test]
    fn mmap_store_roundtrip_and_search() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let bundle = synthetic_store(
            &root,
            &[
                (a, vec![1.0, 0.0, 0.0]),
                (b, vec![0.0, 1.0, 0.0]),
                (c, vec![0.707, 0.707, 0.0]),
            ],
        );

        let store = EmbeddingStore::open(&bundle).unwrap().expect("store");
        assert_eq!(store.len(), 3);
        assert_eq!(store.row(1), vec![0.0, 1.0, 0.0]);

        let results = store.search(&[1.0, 0.0, 0.0], 2);
        assert_eq!(results[0].0, a);
        assert_eq!(results[1].0, c);
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn size_mismatch_is_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = synthetic_store(&root, &[(Uuid::new_v4(), vec![1.0, 0.0])]);
        // Truncate the bin behind the index's back.
        std::fs::write(root.join("embeddings.bin"), [0u8; 4]).unwrap();
        assert!(matches!(
            EmbeddingStore::open(&bundle),
            Err(EmbeddingsError::SizeMismatch { .. })
        ));
    }

    #[test]
    fn store_absent_before_stage() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("S"), "file").unwrap();
        assert!(EmbeddingStore::open(&bundle).unwrap().is_none());
    }

    /// Full model test — downloads MiniLM on first run; ignored in CI.
    /// Run: cargo test -p copilot-core embeddings::tests::real_model -- --ignored
    #[test]
    #[ignore = "downloads the embedding model; run explicitly"]
    fn real_model_semantic_similarity() {
        let embedder = Embedder::load().expect("model load");
        let vectors = embedder
            .embed(&[
                "The attention mechanism computes a weighted sum of values.",
                "Why do they scale the dot product before the softmax?",
                "The training used eight GPUs for three days.",
            ])
            .unwrap();
        assert_eq!(vectors.len(), 3);
        assert_eq!(vectors[0].len(), EMBEDDING_DIM);

        let dot = |a: &[f32], b: &[f32]| -> f32 { a.iter().zip(b).map(|(x, y)| x * y).sum() };
        let sim_attention = dot(&vectors[0], &vectors[1]);
        let sim_unrelated = dot(&vectors[0], &vectors[2]);
        assert!(
            sim_attention > sim_unrelated,
            "attention question should be closer to attention text: {sim_attention} vs {sim_unrelated}"
        );
    }
}
