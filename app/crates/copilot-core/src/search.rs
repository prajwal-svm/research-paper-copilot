//! In-paper search: exact text match + local semantic search over objects.
//! Fully offline; semantic results come from the bundle's mmap'd embeddings.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::embeddings::{Embedder, EmbeddingStore};
use crate::objects::SemanticTreeDocument;

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error(transparent)]
    Embeddings(#[from] crate::embeddings::EmbeddingsError),
    #[error("semantic_tree.json missing — paper still ingesting")]
    TreeMissing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub object_id: Uuid,
    /// Snippet with the match in context (exact) or the object text head
    /// (semantic).
    pub snippet: String,
    /// Similarity score for semantic hits; 1.0 for exact hits.
    pub score: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResults {
    pub exact: Vec<SearchHit>,
    pub semantic: Vec<SearchHit>,
    /// False when the embeddings stage hasn't run (exact-only degraded mode).
    pub semantic_available: bool,
}

/// Case-insensitive exact search over object text.
pub fn exact_search(tree: &SemanticTreeDocument, query: &str, limit: usize) -> Vec<SearchHit> {
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for object in &tree.objects {
        // Sentences duplicate their paragraph text; skip to avoid double hits.
        if object.object_type == crate::objects::ObjectType::Sentence {
            continue;
        }
        let hay = object.content.text.to_lowercase();
        if let Some(pos) = hay.find(&needle) {
            hits.push(SearchHit {
                object_id: object.id,
                snippet: snippet_around(&object.content.text, pos, needle.len()),
                score: 1.0,
            });
            if hits.len() >= limit {
                break;
            }
        }
    }
    hits
}

fn snippet_around(text: &str, byte_pos: usize, match_len: usize) -> String {
    const CONTEXT: usize = 60;
    let start = text
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|i| *i <= byte_pos.saturating_sub(CONTEXT))
        .last()
        .unwrap_or(0);
    let end_target = (byte_pos + match_len + CONTEXT).min(text.len());
    let end = text
        .char_indices()
        .map(|(i, _)| i)
        .find(|i| *i >= end_target)
        .unwrap_or(text.len());
    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.push_str(text[start..end].trim());
    if end < text.len() {
        snippet.push('…');
    }
    snippet
}

/// Semantic search via the bundle's embeddings; `None` when unavailable.
pub fn semantic_search(
    bundle: &Bundle,
    tree: &SemanticTreeDocument,
    embedder: &Embedder,
    query: &str,
    limit: usize,
) -> Result<Option<Vec<SearchHit>>, SearchError> {
    let Some(store) = EmbeddingStore::open(bundle)? else {
        return Ok(None);
    };
    let query_vec = embedder.embed(&[query])?;
    let hits = store
        .search(&query_vec[0], limit)
        .into_iter()
        .filter_map(|(object_id, score)| {
            let object = tree.objects.iter().find(|o| o.id == object_id)?;
            let head: String = object.content.text.chars().take(140).collect();
            Some(SearchHit {
                object_id,
                snippet: head,
                score,
            })
        })
        .collect();
    Ok(Some(hits))
}

/// Combined search. The embedder is optional so exact search works before
/// the model is available (degraded mode is explicit in the result).
pub fn search(
    bundle: &Bundle,
    embedder: Option<&Embedder>,
    query: &str,
    limit: usize,
) -> Result<SearchResults, SearchError> {
    let tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")?
        .ok_or(SearchError::TreeMissing)?;

    let exact = exact_search(&tree, query, limit);
    let (semantic, semantic_available) = match embedder {
        Some(embedder) => match semantic_search(bundle, &tree, embedder, query, limit)? {
            Some(hits) => (hits, true),
            None => (Vec::new(), false),
        },
        None => (Vec::new(), false),
    };
    Ok(SearchResults {
        exact,
        semantic,
        semantic_available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::BBox;
    use crate::objects::{Content, Object, ObjectType, TreeNode};

    fn object(object_type: ObjectType, text: &str) -> Object {
        Object {
            id: Uuid::new_v4(),
            object_type,
            regions: vec![BBox {
                page: 0,
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 10.0,
            }],
            content: Content {
                text: text.to_string(),
                latex: None,
                caption: None,
            },
            semantic_label: None,
            relationships: Vec::new(),
            embedding: None,
            content_hash: crate::bundle::sha256_bytes(text.as_bytes()),
            confidence: 0.9,
        }
    }

    fn tree(objects: Vec<Object>) -> SemanticTreeDocument {
        let nodes = objects
            .iter()
            .map(|o| TreeNode {
                object: o.id,
                children: Vec::new(),
            })
            .collect();
        SemanticTreeDocument {
            pipeline_version: "0.1.0".to_string(),
            objects,
            tree: nodes,
        }
    }

    #[test]
    fn exact_search_matches_case_insensitively_with_snippets() {
        let t = tree(vec![
            object(ObjectType::Paragraph, "The dominant sequence transduction models are based on complex recurrent networks."),
            object(ObjectType::Paragraph, "Attention mechanisms have become integral."),
            object(ObjectType::Sentence, "Attention mechanisms have become integral."),
        ]);
        let hits = exact_search(&t, "ATTENTION MECH", 10);
        assert_eq!(hits.len(), 1, "sentence duplicates skipped: {hits:#?}");
        assert!(hits[0].snippet.contains("Attention mechanisms"));

        assert!(exact_search(&t, "zzz-not-there", 10).is_empty());
        assert!(exact_search(&t, "", 10).is_empty());
    }

    #[test]
    fn snippet_is_windowed_with_ellipses() {
        let long = format!("{}NEEDLE{}", "x".repeat(200), "y".repeat(200));
        let t = tree(vec![object(ObjectType::Paragraph, &long)]);
        let hits = exact_search(&t, "needle", 10);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.starts_with('…'));
        assert!(hits[0].snippet.ends_with('…'));
        assert!(hits[0].snippet.contains("NEEDLE"));
        assert!(hits[0].snippet.len() < 200);
    }

    #[test]
    fn search_degrades_without_embeddings() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = crate::bundle::Bundle::create(
            &root,
            b"%PDF-1.5 fake",
            crate::bundle::Paper::new("S"),
            "file",
        )
        .unwrap();
        bundle
            .write_derived_json(
                "semantic_tree.json",
                &tree(vec![object(
                    ObjectType::Paragraph,
                    "attention is all you need",
                )]),
                "objects",
                serde_json::json!({"pipeline_version": "0.1.0", "status": "complete"}),
            )
            .unwrap();

        let results = search(&bundle, None, "attention", 10).unwrap();
        assert_eq!(results.exact.len(), 1);
        assert!(!results.semantic_available);
        assert!(results.semantic.is_empty());
    }
}
