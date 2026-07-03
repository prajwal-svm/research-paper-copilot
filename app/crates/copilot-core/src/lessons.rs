//! Reading mode, quizzes, and flashcards (v2): the paper as a course.
//!
//! The course outline is a deterministic topological sort of the concept
//! DAG — low-confidence edges are excluded from ordering, cycles (LLM edge
//! direction errors happen) are broken at the lowest-confidence edge, and
//! mastery *collapses* lessons but never gates them.
//!
//! Lesson/quiz/flashcard content is generated lazily per node (strong tier
//! outside, injected here as a closure), cached in the bundle
//! (`glossary/lessons/`, `quizzes/`, `flashcards/`), and anchored to object
//! UUID + content hash so re-parsed papers flag stale items instead of
//! silently serving outdated questions.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::concepts::{EdgeKind, KnowledgeGraph};
use crate::objects::SemanticTreeDocument;

/// Ordering edges below this confidence don't constrain the course order.
const ORDERING_CONFIDENCE: f32 = 0.6;

#[derive(Debug, thiserror::Error)]
pub enum LessonsError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
}

// ---------------------------------------------------------------------------
// Course sequencing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LessonEntry {
    pub node: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub object_ids: Vec<Uuid>,
    /// Mastered lessons collapse to a skippable recap — never locked.
    pub mastered: bool,
    pub low_confidence: bool,
}

/// Topological course order over prerequisite edges.
/// `prerequisite_of: A→B` schedules A before B; `depends_on: A→B` schedules
/// B before A. Ties keep graph node order (deterministic).
pub fn lesson_sequence(
    graph: &KnowledgeGraph,
    mastered: &dyn Fn(Uuid) -> bool,
) -> Vec<LessonEntry> {
    let ids: Vec<Uuid> = graph.nodes.iter().map(|n| n.id).collect();
    let index_of = |id: Uuid| ids.iter().position(|&i| i == id);

    // (before, after, confidence)
    let mut constraints: Vec<(usize, usize, f32)> = Vec::new();
    for edge in &graph.edges {
        if edge.confidence < ORDERING_CONFIDENCE {
            continue;
        }
        let pair = match edge.kind {
            EdgeKind::PrerequisiteOf => Some((edge.from, edge.to)),
            EdgeKind::DependsOn => Some((edge.to, edge.from)),
            _ => None,
        };
        let Some((before, after)) = pair else {
            continue;
        };
        if let (Some(b), Some(a)) = (index_of(before), index_of(after)) {
            if b != a {
                constraints.push((b, a, edge.confidence));
            }
        }
    }

    // Kahn's algorithm; on a cycle, drop the lowest-confidence remaining
    // constraint (edge direction errors are an inconvenience, not a lock).
    let n = ids.len();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut placed = vec![false; n];
    while order.len() < n {
        let next = (0..n)
            .find(|&i| !placed[i] && !constraints.iter().any(|&(b, a, _)| a == i && !placed[b]));
        match next {
            Some(i) => {
                placed[i] = true;
                order.push(i);
            }
            None => {
                // Cycle: remove the weakest constraint among unplaced nodes.
                if let Some(weakest) = constraints
                    .iter()
                    .enumerate()
                    .filter(|(_, &(b, a, _))| !placed[b] && !placed[a])
                    .min_by(|a, b| a.1 .2.total_cmp(&b.1 .2))
                    .map(|(i, _)| i)
                {
                    constraints.remove(weakest);
                } else {
                    break; // defensive: shouldn't happen
                }
            }
        }
    }

    order
        .into_iter()
        .map(|i| {
            let node = &graph.nodes[i];
            LessonEntry {
                node: node.id,
                name: node.name.clone(),
                description: node.description.clone(),
                object_ids: node.object_ids.clone(),
                mastered: mastered(node.id),
                low_confidence: node.confidence < ORDERING_CONFIDENCE,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Lesson content (lazy, cached in glossary/lessons/)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub node: Uuid,
    /// Markdown: mini explanation, worked example/exercise prompt.
    pub content: String,
    pub exercise: Option<String>,
    pub generated_at: String,
}

fn lesson_path(node: Uuid) -> String {
    format!("glossary/lessons/{node}.json")
}

pub fn lesson_get(bundle: &Bundle, node: Uuid) -> Result<Option<Lesson>, LessonsError> {
    Ok(bundle.read_derived_json(&lesson_path(node))?)
}

/// Generate and cache a lesson for one concept. `llm` gets a prompt built
/// from the node, its introducing objects, and prerequisite names; `None`
/// from the closure (no key, failure) yields `Ok(None)` — the UI shows the
/// designed no-key state with the paper's own objects.
pub fn lesson_generate(
    bundle: &Bundle,
    graph: &KnowledgeGraph,
    tree: &SemanticTreeDocument,
    node_id: Uuid,
    llm: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<Lesson>, LessonsError> {
    if let Some(cached) = lesson_get(bundle, node_id)? {
        return Ok(Some(cached));
    }
    let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) else {
        return Ok(None);
    };
    let excerpts: String = node
        .object_ids
        .iter()
        .filter_map(|oid| tree.objects.iter().find(|o| o.id == *oid))
        .map(|o| format!("[[object:{}]] {}\n", o.id, clip(&o.content.text, 1500)))
        .collect();
    let prerequisites: Vec<&str> = graph
        .edges
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::PrerequisiteOf if e.to == node_id => Some(e.from),
            EdgeKind::DependsOn if e.from == node_id => Some(e.to),
            _ => None,
        })
        .filter_map(|id| graph.nodes.iter().find(|n| n.id == id))
        .map(|n| n.name.as_str())
        .collect();

    let prompt = format!(
        "Write a short lesson (markdown) teaching the concept \"{name}\" from a research paper.\n\
         Structure: ## {name}, a 2-3 paragraph explanation grounded in the excerpts below \
         (cite them inline as [[object:ID]]), then exactly one practice exercise under \"### Try it\".\n\
         Assume the reader knows: {prereqs}. Do not re-teach those.\n\n\
         {description}Paper excerpts:\n{excerpts}",
        name = node.name,
        prereqs = if prerequisites.is_empty() {
            "general ML basics".to_string()
        } else {
            prerequisites.join(", ")
        },
        description = node
            .description
            .as_deref()
            .map(|d| format!("Concept summary: {d}\n"))
            .unwrap_or_default(),
    );
    let Some(content) = llm(&prompt) else {
        return Ok(None);
    };
    let exercise = content
        .split("### Try it")
        .nth(1)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let lesson = Lesson {
        node: node_id,
        content,
        exercise,
        generated_at: crate::bundle::now_rfc3339(),
    };
    bundle.write_user_json(&lesson_path(node_id), &lesson)?;
    Ok(Some(lesson))
}

// ---------------------------------------------------------------------------
// Quizzes & flashcards (anchored to object UUID + content hash)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizItem {
    pub id: Uuid,
    pub question: String,
    pub options: Vec<String>,
    pub correct: usize,
    /// Shown immediately after grading — why, citing the paper.
    pub explanation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_object: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_hash: Option<String>,
    /// Computed at read: anchor content changed since generation.
    #[serde(default)]
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quiz {
    pub node: Uuid,
    pub items: Vec<QuizItem>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flashcard {
    pub id: Uuid,
    pub front: String,
    pub back: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_object: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_hash: Option<String>,
    #[serde(default)]
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashcardDeck {
    pub node: Uuid,
    pub cards: Vec<Flashcard>,
    pub generated_at: String,
}

fn quiz_path(node: Uuid) -> String {
    format!("quizzes/{node}.json")
}

fn deck_path(node: Uuid) -> String {
    format!("flashcards/{node}.json")
}

/// Mark items whose anchored object's content hash changed (re-parsed
/// paper): flagged for regeneration, never silently served as current.
fn hash_of(tree: &SemanticTreeDocument, object: Uuid) -> Option<String> {
    tree.objects
        .iter()
        .find(|o| o.id == object)
        .map(|o| o.content_hash.clone())
}

pub fn quiz_get(
    bundle: &Bundle,
    tree: &SemanticTreeDocument,
    node: Uuid,
) -> Result<Option<Quiz>, LessonsError> {
    let Some(mut quiz) = bundle.read_derived_json::<Quiz>(&quiz_path(node))? else {
        return Ok(None);
    };
    for item in &mut quiz.items {
        item.stale = match (&item.anchor_object, &item.anchor_hash) {
            (Some(object), Some(hash)) => hash_of(tree, *object).as_deref() != Some(hash),
            _ => false,
        };
    }
    Ok(Some(quiz))
}

pub fn quiz_generate(
    bundle: &Bundle,
    graph: &KnowledgeGraph,
    tree: &SemanticTreeDocument,
    node_id: Uuid,
    llm: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<Quiz>, LessonsError> {
    if let Some(cached) = quiz_get(bundle, tree, node_id)? {
        if cached.items.iter().all(|i| !i.stale) {
            return Ok(Some(cached));
        }
    }
    let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) else {
        return Ok(None);
    };
    let anchor = node.object_ids.first().copied();
    let excerpts: String = node
        .object_ids
        .iter()
        .filter_map(|oid| tree.objects.iter().find(|o| o.id == *oid))
        .map(|o| format!("{}\n", clip(&o.content.text, 1500)))
        .collect();
    let prompt = format!(
        "Write 3 multiple-choice quiz questions testing understanding of \"{name}\" \
         based ONLY on these paper excerpts. Respond with JSON only:\n\
         [{{\"question\": \"...\", \"options\": [\"...\", \"...\", \"...\", \"...\"], \
         \"correct\": 0, \"explanation\": \"why, citing the paper\"}}]\n\n{excerpts}",
        name = node.name,
    );
    let Some(raw) = llm(&prompt) else {
        return Ok(None);
    };
    let json = raw
        .find('[')
        .and_then(|s| raw.rfind(']').map(|e| &raw[s..=e]));
    let Some(parsed) = json.and_then(|j| serde_json::from_str::<Vec<serde_json::Value>>(j).ok())
    else {
        return Ok(None);
    };
    let items: Vec<QuizItem> = parsed
        .into_iter()
        .filter_map(|v| {
            let options: Vec<String> = v["options"]
                .as_array()?
                .iter()
                .filter_map(|o| o.as_str().map(|s| s.to_string()))
                .collect();
            let correct = v["correct"].as_u64()? as usize;
            if options.len() < 2 || correct >= options.len() {
                return None;
            }
            Some(QuizItem {
                id: Uuid::new_v4(),
                question: v["question"].as_str()?.to_string(),
                options,
                correct,
                explanation: v["explanation"].as_str().unwrap_or_default().to_string(),
                anchor_object: anchor,
                anchor_hash: anchor.and_then(|a| hash_of(tree, a)),
                stale: false,
            })
        })
        .collect();
    if items.is_empty() {
        return Ok(None);
    }
    let quiz = Quiz {
        node: node_id,
        items,
        generated_at: crate::bundle::now_rfc3339(),
    };
    bundle.write_user_json(&quiz_path(node_id), &quiz)?;
    Ok(Some(quiz))
}

pub fn deck_get(
    bundle: &Bundle,
    tree: &SemanticTreeDocument,
    node: Uuid,
) -> Result<Option<FlashcardDeck>, LessonsError> {
    let Some(mut deck) = bundle.read_derived_json::<FlashcardDeck>(&deck_path(node))? else {
        return Ok(None);
    };
    for card in &mut deck.cards {
        card.stale = match (&card.anchor_object, &card.anchor_hash) {
            (Some(object), Some(hash)) => hash_of(tree, *object).as_deref() != Some(hash),
            _ => false,
        };
    }
    Ok(Some(deck))
}

pub fn deck_generate(
    bundle: &Bundle,
    graph: &KnowledgeGraph,
    tree: &SemanticTreeDocument,
    node_id: Uuid,
    llm: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<FlashcardDeck>, LessonsError> {
    if let Some(cached) = deck_get(bundle, tree, node_id)? {
        if cached.cards.iter().all(|c| !c.stale) {
            return Ok(Some(cached));
        }
    }
    let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) else {
        return Ok(None);
    };
    let anchor = node.object_ids.first().copied();
    let excerpts: String = node
        .object_ids
        .iter()
        .filter_map(|oid| tree.objects.iter().find(|o| o.id == *oid))
        .map(|o| format!("{}\n", clip(&o.content.text, 1200)))
        .collect();
    let prompt = format!(
        "Write 3 flashcards for the concept \"{name}\" from these paper excerpts. \
         Respond with JSON only:\n[{{\"front\": \"prompt/question\", \"back\": \"concise answer\"}}]\n\n{excerpts}",
        name = node.name,
    );
    let Some(raw) = llm(&prompt) else {
        return Ok(None);
    };
    let json = raw
        .find('[')
        .and_then(|s| raw.rfind(']').map(|e| &raw[s..=e]));
    let Some(parsed) = json.and_then(|j| serde_json::from_str::<Vec<serde_json::Value>>(j).ok())
    else {
        return Ok(None);
    };
    let cards: Vec<Flashcard> = parsed
        .into_iter()
        .filter_map(|v| {
            Some(Flashcard {
                id: Uuid::new_v4(),
                front: v["front"].as_str()?.to_string(),
                back: v["back"].as_str()?.to_string(),
                anchor_object: anchor,
                anchor_hash: anchor.and_then(|a| hash_of(tree, a)),
                stale: false,
            })
        })
        .collect();
    if cards.is_empty() {
        return Ok(None);
    }
    let deck = FlashcardDeck {
        node: node_id,
        cards,
        generated_at: crate::bundle::now_rfc3339(),
    };
    bundle.write_user_json(&deck_path(node_id), &deck)?;
    Ok(Some(deck))
}

fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let clipped: String = text.chars().take(max_chars).collect();
        format!("{clipped}…")
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;
    use crate::concepts::{concept_id, ConceptEdge, ConceptNode};

    fn node(paper: &str, name: &str, confidence: f32) -> ConceptNode {
        ConceptNode {
            id: concept_id(paper, name),
            name: name.to_string(),
            description: None,
            object_ids: vec![],
            confidence,
        }
    }

    fn edge(from: Uuid, to: Uuid, kind: EdgeKind, confidence: f32) -> ConceptEdge {
        ConceptEdge {
            from,
            to,
            kind,
            confidence,
        }
    }

    #[test]
    fn course_order_respects_prerequisites_and_mastery_collapses() {
        let softmax = node("p", "Softmax", 0.9);
        let scaling = node("p", "Scaling", 0.9);
        let attention = node("p", "Scaled Dot-Product Attention", 0.9);
        let multihead = node("p", "Multi-Head Attention", 0.9);
        let graph = KnowledgeGraph {
            pipeline_version: "0.1.0".to_string(),
            extraction: "llm".to_string(),
            edges: vec![
                edge(softmax.id, attention.id, EdgeKind::PrerequisiteOf, 0.9),
                edge(scaling.id, attention.id, EdgeKind::PrerequisiteOf, 0.9),
                edge(multihead.id, attention.id, EdgeKind::DependsOn, 0.9),
            ],
            nodes: vec![
                multihead.clone(),
                attention.clone(),
                softmax.clone(),
                scaling.clone(),
            ],
        };
        let mastered_id = softmax.id;
        let seq = lesson_sequence(&graph, &|id| id == mastered_id);
        let pos = |id: Uuid| seq.iter().position(|e| e.node == id).unwrap();
        assert!(pos(softmax.id) < pos(attention.id));
        assert!(pos(scaling.id) < pos(attention.id));
        assert!(pos(attention.id) < pos(multihead.id), "depends_on inverted");
        // Mastered collapses but is present — never gated out.
        assert!(seq[pos(softmax.id)].mastered);
        assert_eq!(seq.len(), 4);
    }

    #[test]
    fn cycles_break_at_lowest_confidence_edge() {
        let a = node("p", "A", 0.9);
        let b = node("p", "B", 0.9);
        let graph = KnowledgeGraph {
            pipeline_version: "0.1.0".to_string(),
            extraction: "llm".to_string(),
            edges: vec![
                edge(a.id, b.id, EdgeKind::PrerequisiteOf, 0.9),
                edge(b.id, a.id, EdgeKind::PrerequisiteOf, 0.65), // weaker, wrong
            ],
            nodes: vec![a.clone(), b.clone()],
        };
        let seq = lesson_sequence(&graph, &|_| false);
        assert_eq!(seq.len(), 2, "cycle never drops a lesson");
        assert_eq!(seq[0].node, a.id, "stronger edge wins the direction");
    }

    #[test]
    fn low_confidence_edges_do_not_constrain_order() {
        let a = node("p", "A", 0.9);
        let b = node("p", "B", 0.9);
        let graph = KnowledgeGraph {
            pipeline_version: "0.1.0".to_string(),
            extraction: "llm".to_string(),
            // Would order B before A, but it's below the ordering threshold.
            edges: vec![edge(b.id, a.id, EdgeKind::PrerequisiteOf, 0.3)],
            nodes: vec![a.clone(), b.clone()],
        };
        let seq = lesson_sequence(&graph, &|_| false);
        assert_eq!(seq[0].node, a.id, "graph node order kept for weak edges");
    }

    fn bundle_with_object() -> (tempfile::TempDir, Bundle, SemanticTreeDocument, Uuid) {
        use crate::layout::BBox;
        use crate::objects::{Content, Object, ObjectType};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("T"), "file").unwrap();
        let object_id = Uuid::new_v4();
        let object = Object {
            id: object_id,
            object_type: ObjectType::Section,
            regions: vec![BBox {
                page: 0,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }],
            content: Content {
                text: "Softmax normalizes attention scores into probabilities.".to_string(),
                latex: None,
                caption: None,
            },
            semantic_label: Some("3.1".to_string()),
            relationships: vec![],
            embedding: None,
            content_hash: crate::bundle::sha256_bytes(b"softmax v1"),
            confidence: 0.9,
        };
        let tree = SemanticTreeDocument {
            pipeline_version: "0.1.0".to_string(),
            objects: vec![object],
            tree: vec![],
        };
        (tmp, bundle, tree, object_id)
    }

    #[test]
    fn quiz_caches_and_flags_stale_anchors() {
        let (_tmp, bundle, mut tree, object_id) = bundle_with_object();
        let mut softmax = node("p", "Softmax", 0.9);
        softmax.object_ids = vec![object_id];
        let graph = KnowledgeGraph {
            pipeline_version: "0.1.0".to_string(),
            extraction: "llm".to_string(),
            nodes: vec![softmax.clone()],
            edges: vec![],
        };
        let calls = std::cell::Cell::new(0u32);
        let llm = |_: &str| {
            calls.set(calls.get() + 1);
            Some(
                r#"[{"question": "What does softmax output?", "options": ["Probabilities", "Logits", "Gradients", "Weights"], "correct": 0, "explanation": "It normalizes scores into a distribution."}]"#
                    .to_string(),
            )
        };

        let quiz = quiz_generate(&bundle, &graph, &tree, softmax.id, &llm)
            .unwrap()
            .expect("generated");
        assert_eq!(quiz.items.len(), 1);
        assert_eq!(calls.get(), 1);

        // Second request: cached, no provider call, not stale.
        let again = quiz_generate(&bundle, &graph, &tree, softmax.id, &llm)
            .unwrap()
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(!again.items[0].stale);

        // Re-parse changes the anchor's content hash → flagged stale.
        tree.objects[0].content_hash = crate::bundle::sha256_bytes(b"softmax v2");
        let reread = quiz_get(&bundle, &tree, softmax.id).unwrap().unwrap();
        assert!(reread.items[0].stale, "changed anchor must be flagged");
    }

    #[test]
    fn lesson_caches_and_no_key_returns_none() {
        let (_tmp, bundle, tree, object_id) = bundle_with_object();
        let mut softmax = node("p", "Softmax", 0.9);
        softmax.object_ids = vec![object_id];
        let graph = KnowledgeGraph {
            pipeline_version: "0.1.0".to_string(),
            extraction: "llm".to_string(),
            nodes: vec![softmax.clone()],
            edges: vec![],
        };
        // No key → None, nothing cached, no error.
        assert!(
            lesson_generate(&bundle, &graph, &tree, softmax.id, &|_| None)
                .unwrap()
                .is_none()
        );
        assert!(lesson_get(&bundle, softmax.id).unwrap().is_none());

        let lesson = lesson_generate(&bundle, &graph, &tree, softmax.id, &|_| {
            Some("## Softmax\nExplanation.\n### Try it\nCompute softmax of [1, 2].".to_string())
        })
        .unwrap()
        .expect("generated");
        assert!(lesson
            .exercise
            .as_deref()
            .unwrap_or("")
            .contains("Compute softmax"));
        // Cached for keyless reopen.
        assert!(lesson_get(&bundle, softmax.id).unwrap().is_some());
    }
}
