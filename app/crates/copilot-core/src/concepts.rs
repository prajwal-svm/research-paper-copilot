//! Knowledge graph: concept nodes and typed edges per paper
//! (change: add-v2-learning-engine, tasks 1.1–1.3).
//!
//! The graph is derived, regenerable data (`knowledge_graph.json`, schema
//! published) produced by pipeline stage 5. Extraction is LLM-assisted when
//! a provider exists and degrades to a heuristic graph (section headings +
//! containment edges, flagged low-confidence) with no key — reading never
//! blocks on it. Node ids are deterministic (UUID v5 over paper + normalized
//! name) so re-extraction keeps user data attached.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::objects::{ObjectType, SemanticTreeDocument};

pub const CONCEPTS_PIPELINE_VERSION: &str = "0.1.0";

/// Namespace for deterministic concept-node UUIDs. Never change.
const CONCEPT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6c, 0x1f, 0x2a, 0x9d, 0x41, 0x7b, 0x4e, 0x82, 0xb3, 0x60, 0x8f, 0x2e, 0x55, 0xa1, 0xd4, 0x77,
]);

/// Closed edge vocabulary (spec requirement) — anything else is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    PrerequisiteOf,
    DependsOn,
    DefinedIn,
    UsedBy,
    Extends,
    Contradicts,
    Cites,
}

impl EdgeKind {
    pub fn parse(s: &str) -> Option<EdgeKind> {
        match s {
            "prerequisite_of" => Some(EdgeKind::PrerequisiteOf),
            "depends_on" => Some(EdgeKind::DependsOn),
            "defined_in" => Some(EdgeKind::DefinedIn),
            "used_by" => Some(EdgeKind::UsedBy),
            "extends" => Some(EdgeKind::Extends),
            "contradicts" => Some(EdgeKind::Contradicts),
            "cites" => Some(EdgeKind::Cites),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::PrerequisiteOf => "prerequisite_of",
            EdgeKind::DependsOn => "depends_on",
            EdgeKind::DefinedIn => "defined_in",
            EdgeKind::UsedBy => "used_by",
            EdgeKind::Extends => "extends",
            EdgeKind::Contradicts => "contradicts",
            EdgeKind::Cites => "cites",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConceptNode {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Paper objects that introduce/use this concept.
    #[serde(default)]
    pub object_ids: Vec<Uuid>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConceptEdge {
    pub from: Uuid,
    pub to: Uuid,
    pub kind: EdgeKind,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct KnowledgeGraph {
    pub pipeline_version: String,
    /// "llm" or "heuristic" — heuristic graphs are flagged limited in the UI.
    pub extraction: String,
    pub nodes: Vec<ConceptNode>,
    pub edges: Vec<ConceptEdge>,
}

/// Deterministic node id: same paper + same normalized name → same id.
pub fn concept_id(paper_key: &str, name: &str) -> Uuid {
    let normalized = normalize_name(name);
    Uuid::new_v5(
        &CONCEPT_NAMESPACE,
        format!("{paper_key}\u{1f}{normalized}").as_bytes(),
    )
}

pub fn normalize_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Heuristic extraction (no-key fallback)
// ---------------------------------------------------------------------------

/// Build a shallow but honest graph from the semantic tree alone: section
/// headings become concepts (numbering stripped), nesting becomes
/// `depends_on` (child depends on parent), and each concept is `defined_in`
/// its section object. Confidence is capped low — the UI flags it limited.
pub fn heuristic_graph(paper_key: &str, tree: &SemanticTreeDocument) -> KnowledgeGraph {
    let mut nodes: Vec<ConceptNode> = Vec::new();
    let mut edges: Vec<ConceptEdge> = Vec::new();
    let mut by_object: HashMap<Uuid, Uuid> = HashMap::new(); // section object → node

    for object in tree
        .objects
        .iter()
        .filter(|o| o.object_type == ObjectType::Section)
    {
        let Some(name) = heading_concept_name(&object.content.text) else {
            continue;
        };
        let id = concept_id(paper_key, &name);
        if !nodes.iter().any(|n| n.id == id) {
            nodes.push(ConceptNode {
                id,
                name,
                description: None,
                object_ids: vec![object.id],
                confidence: 0.4,
            });
        } else if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
            node.object_ids.push(object.id);
        }
        by_object.insert(object.id, id);
    }

    // Nesting → depends_on (child concept depends on its parent section's).
    for object in tree
        .objects
        .iter()
        .filter(|o| o.object_type == ObjectType::Section)
    {
        let Some(&child) = by_object.get(&object.id) else {
            continue;
        };
        let parent_node = object.relationships.iter().find_map(|r| {
            matches!(
                r.relationship_type,
                crate::objects::RelationshipType::BelongsTo
            )
            .then(|| by_object.get(&r.target).copied())
            .flatten()
        });
        if let Some(parent) = parent_node {
            if parent != child {
                edges.push(ConceptEdge {
                    from: child,
                    to: parent,
                    kind: EdgeKind::DependsOn,
                    confidence: 0.4,
                });
            }
        }
    }

    KnowledgeGraph {
        pipeline_version: CONCEPTS_PIPELINE_VERSION.to_string(),
        extraction: "heuristic".to_string(),
        nodes,
        edges,
    }
}

/// "3.2.1 Scaled Dot-Product Attention" → "Scaled Dot-Product Attention";
/// boilerplate headings (References, Acknowledgements…) are not concepts.
fn heading_concept_name(heading: &str) -> Option<String> {
    let text = heading.trim();
    let name = match text.split_once(' ') {
        Some((prefix, rest))
            if prefix
                .trim_end_matches('.')
                .split('.')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) =>
        {
            rest.trim()
        }
        _ => text,
    };
    let lower = name.to_lowercase();
    const BOILERPLATE: [&str; 6] = [
        "references",
        "acknowledgements",
        "acknowledgments",
        "appendix",
        "abstract",
        "conclusion",
    ];
    if name.len() < 3 || BOILERPLATE.iter().any(|b| lower.starts_with(b)) {
        return None;
    }
    Some(name.to_string())
}

// ---------------------------------------------------------------------------
// LLM-assisted extraction
// ---------------------------------------------------------------------------

/// Prompt contract for LLM extraction: strict JSON out, validated against the
/// closed edge vocabulary; unknown names/kinds are dropped, never guessed.
pub fn extraction_prompt(tree: &SemanticTreeDocument, title: &str) -> String {
    let mut outline = String::new();
    for object in &tree.objects {
        match object.object_type {
            ObjectType::Section => {
                outline.push_str(&format!("\n## {}\n", object.content.text.trim()));
            }
            ObjectType::Paragraph => {
                let head: String = object.content.text.chars().take(300).collect();
                outline.push_str(&head);
                outline.push('\n');
            }
            ObjectType::Equation | ObjectType::Figure | ObjectType::Table => {
                if let Some(label) = &object.semantic_label {
                    outline.push_str(&format!("[{label}: {}]\n", clip(&object.content.text, 120)));
                }
            }
            _ => {}
        }
    }
    format!(
        "Extract the concept graph of this research paper.\n\
         Paper: {title}\n\
         ---\n{outline}\n---\n\
         Return ONLY JSON, no prose, in this exact shape:\n\
         {{\"concepts\": [{{\"name\": str, \"description\": str (one sentence), \"anchors\": [str] (verbatim section headings or object labels above that introduce/use it)}}],\n\
          \"edges\": [{{\"from\": str (concept name), \"to\": str (concept name), \"kind\": one of prerequisite_of|depends_on|defined_in|used_by|extends|contradicts|cites}}]}}\n\
         Rules: 8-25 concepts; concepts are ideas (e.g. \"Multi-Head Attention\"), not section titles like \"Introduction\"; \
         every concept needs at least one anchor; edges only between listed concepts. \
         Edge direction matters: \"A prerequisite_of B\" = A must be understood before B; \
         \"A depends_on B\" = A builds on B; \"A extends B\" = A generalizes/improves B. \
         Prefer prerequisite_of for learning order."
    )
}

/// Parse and validate the model's JSON into a graph, linking anchors back to
/// object UUIDs by matching section headings and semantic labels.
pub fn parse_llm_graph(
    paper_key: &str,
    tree: &SemanticTreeDocument,
    raw: &str,
) -> Option<KnowledgeGraph> {
    // Tolerate code fences and leading prose around the JSON body.
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    let value: serde_json::Value = serde_json::from_str(&raw[start..=end]).ok()?;

    // Anchor lookup: normalized heading/label text → object id.
    let mut anchors: HashMap<String, Uuid> = HashMap::new();
    for object in &tree.objects {
        if object.object_type == ObjectType::Section {
            anchors.insert(normalize_name(&object.content.text), object.id);
            if let Some(name) = heading_concept_name(&object.content.text) {
                anchors.insert(normalize_name(&name), object.id);
            }
        }
        if let Some(label) = &object.semantic_label {
            anchors.insert(normalize_name(label), object.id);
        }
    }

    let mut nodes: Vec<ConceptNode> = Vec::new();
    let mut name_to_id: HashMap<String, Uuid> = HashMap::new();
    for concept in value["concepts"].as_array()? {
        let Some(name) = concept["name"].as_str() else {
            continue;
        };
        let normalized = normalize_name(name);
        if normalized.is_empty() || name_to_id.contains_key(&normalized) {
            continue;
        }
        let object_ids: Vec<Uuid> = concept["anchors"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|a| a.as_str())
                    .filter_map(|a| anchors.get(&normalize_name(a)).copied())
                    .collect()
            })
            .unwrap_or_default();
        let id = concept_id(paper_key, name);
        name_to_id.insert(normalized, id);
        nodes.push(ConceptNode {
            id,
            name: name.trim().to_string(),
            description: concept["description"].as_str().map(|s| s.to_string()),
            confidence: if object_ids.is_empty() { 0.5 } else { 0.85 },
            object_ids,
        });
    }
    if nodes.len() < 3 {
        return None; // not a usable graph — caller falls back to heuristic
    }

    let mut edges: Vec<ConceptEdge> = Vec::new();
    for edge in value["edges"].as_array().unwrap_or(&Vec::new()) {
        let (Some(from), Some(to), Some(kind)) = (
            edge["from"].as_str(),
            edge["to"].as_str(),
            edge["kind"].as_str().and_then(EdgeKind::parse),
        ) else {
            continue; // unknown kind or malformed — dropped, never guessed
        };
        let (Some(&from), Some(&to)) = (
            name_to_id.get(&normalize_name(from)),
            name_to_id.get(&normalize_name(to)),
        ) else {
            continue;
        };
        if from != to && !edges.iter().any(|e| e.from == from && e.to == to) {
            edges.push(ConceptEdge {
                from,
                to,
                kind,
                confidence: 0.8,
            });
        }
    }

    Some(KnowledgeGraph {
        pipeline_version: CONCEPTS_PIPELINE_VERSION.to_string(),
        extraction: "llm".to_string(),
        nodes,
        edges,
    })
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect::<String>() + "…"
    }
}

// ---------------------------------------------------------------------------
// Stage entry point
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ConceptsError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("semantic_tree.json missing — run the objects stage first")]
    TreeMissing,
}

/// Run stage 5: build the graph (LLM via `llm` closure when provided, else
/// heuristic), apply stored user overrides, write `knowledge_graph.json`.
#[allow(clippy::type_complexity)]
pub fn run_concepts_stage(
    bundle: &Bundle,
    llm: Option<&dyn Fn(&str) -> Option<String>>,
) -> Result<KnowledgeGraph, ConceptsError> {
    let started_at = crate::bundle::now_rfc3339();
    let tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")?
        .ok_or(ConceptsError::TreeMissing)?;
    let title = bundle.metadata()?.paper.title;
    let paper_key = title.clone();

    let mut graph = llm
        .and_then(|call| call(&extraction_prompt(&tree, &title)))
        .and_then(|raw| parse_llm_graph(&paper_key, &tree, &raw))
        .unwrap_or_else(|| heuristic_graph(&paper_key, &tree));

    apply_overrides(bundle, &mut graph)?;

    let status = if graph.extraction == "llm" {
        "complete"
    } else {
        "degraded"
    };
    let mut stage = serde_json::json!({
        "pipeline_version": CONCEPTS_PIPELINE_VERSION,
        "status": status,
        "started_at": started_at,
        "completed_at": crate::bundle::now_rfc3339(),
    });
    if status == "degraded" {
        stage["failure_reason"] = serde_json::Value::String(
            "Concept graph built heuristically (no AI provider) — connect a provider and reopen the paper for a full graph."
                .to_string(),
        );
    }
    bundle.write_derived_json("knowledge_graph.json", &graph, "concepts", stage)?;
    Ok(graph)
}

// ---------------------------------------------------------------------------
// User corrections (task 1.5): append-only overrides on top of extraction
// ---------------------------------------------------------------------------

const OVERRIDES_JOURNAL: &str = "notes/graph_overrides.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum GraphOverride {
    DeleteEdge {
        from: Uuid,
        to: Uuid,
        at: String,
    },
    RenameNode {
        node: Uuid,
        name: String,
        at: String,
    },
    DeleteNode {
        node: Uuid,
        at: String,
    },
    MergeNodes {
        keep: Uuid,
        absorb: Uuid,
        at: String,
    },
}

pub fn record_override(bundle: &Bundle, event: GraphOverride) -> Result<(), ConceptsError> {
    bundle.journal(OVERRIDES_JOURNAL).append(&event)?;
    Ok(())
}

/// Re-apply the full override journal to the stored graph and write it back
/// (every override op is idempotent, so replaying over an already-corrected
/// graph is safe). Used after `record_override` so a correction takes effect
/// without re-running extraction — the LLM graph is never downgraded.
pub fn reapply_overrides(bundle: &Bundle) -> Result<KnowledgeGraph, ConceptsError> {
    let mut graph: KnowledgeGraph = bundle
        .read_derived_json("knowledge_graph.json")?
        .ok_or(ConceptsError::TreeMissing)?;
    apply_overrides(bundle, &mut graph)?;
    let metadata = bundle.metadata()?;
    let stage = metadata
        .pipeline
        .stages
        .get("concepts")
        .cloned()
        .unwrap_or_else(|| {
            serde_json::json!({"pipeline_version": CONCEPTS_PIPELINE_VERSION, "status": "complete"})
        });
    bundle.write_derived_json("knowledge_graph.json", &graph, "concepts", stage)?;
    Ok(graph)
}

/// Apply stored overrides to a freshly extracted graph — corrections survive
/// re-extraction (spec: never silently reverted).
pub fn apply_overrides(bundle: &Bundle, graph: &mut KnowledgeGraph) -> Result<(), ConceptsError> {
    let events: Vec<GraphOverride> = bundle.journal(OVERRIDES_JOURNAL).read_all()?;
    for event in events {
        match event {
            GraphOverride::DeleteEdge { from, to, .. } => {
                graph.edges.retain(|e| !(e.from == from && e.to == to));
            }
            GraphOverride::RenameNode { node, name, .. } => {
                if let Some(n) = graph.nodes.iter_mut().find(|n| n.id == node) {
                    n.name = name;
                }
            }
            GraphOverride::DeleteNode { node, .. } => {
                graph.nodes.retain(|n| n.id != node);
                graph.edges.retain(|e| e.from != node && e.to != node);
            }
            GraphOverride::MergeNodes { keep, absorb, .. } => {
                let absorbed = graph
                    .nodes
                    .iter()
                    .position(|n| n.id == absorb)
                    .map(|i| graph.nodes.remove(i));
                if let (Some(absorbed), Some(kept)) =
                    (absorbed, graph.nodes.iter_mut().find(|n| n.id == keep))
                {
                    kept.object_ids.extend(absorbed.object_ids);
                    kept.object_ids.dedup();
                }
                for edge in graph.edges.iter_mut() {
                    if edge.from == absorb {
                        edge.from = keep;
                    }
                    if edge.to == absorb {
                        edge.to = keep;
                    }
                }
                graph.edges.retain(|e| e.from != e.to);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;
    use crate::layout::BBox;
    use crate::objects::{Content, Object, Relationship, RelationshipType, TreeNode};

    fn section(id: Uuid, text: &str, parent: Option<Uuid>) -> Object {
        Object {
            id,
            object_type: ObjectType::Section,
            regions: vec![BBox {
                page: 0,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }],
            content: Content {
                text: text.to_string(),
                latex: None,
                caption: None,
            },
            semantic_label: Some(format!("Section — {text}")),
            relationships: parent
                .map(|p| {
                    vec![Relationship {
                        relationship_type: RelationshipType::BelongsTo,
                        target: p,
                        confidence: None,
                    }]
                })
                .unwrap_or_default(),
            embedding: None,
            content_hash: crate::bundle::sha256_bytes(text.as_bytes()),
            confidence: 0.95,
        }
    }

    fn sample_tree() -> (SemanticTreeDocument, Uuid, Uuid) {
        let model = Uuid::new_v4();
        let attention = Uuid::new_v4();
        let refs = Uuid::new_v4();
        let objects = vec![
            section(model, "3 Model Architecture", None),
            section(attention, "3.2 Attention", Some(model)),
            section(refs, "References", None),
        ];
        let tree = objects
            .iter()
            .map(|o| TreeNode {
                object: o.id,
                children: Vec::new(),
            })
            .collect();
        (
            SemanticTreeDocument {
                pipeline_version: "0.1.0".into(),
                objects,
                tree,
            },
            model,
            attention,
        )
    }

    #[test]
    fn heuristic_graph_from_sections() {
        let (tree, _, _) = sample_tree();
        let graph = heuristic_graph("paper", &tree);
        let names: Vec<&str> = graph.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"Model Architecture"), "{names:?}");
        assert!(names.contains(&"Attention"));
        assert!(!names.iter().any(|n| n.contains("References")));
        // Child depends on parent.
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].kind, EdgeKind::DependsOn);
        assert_eq!(graph.extraction, "heuristic");
        assert!(graph.nodes.iter().all(|n| n.confidence <= 0.5));
    }

    #[test]
    fn concept_ids_are_deterministic() {
        assert_eq!(
            concept_id("p", "Multi-Head Attention"),
            concept_id("p", "  multi-head   attention ")
        );
        assert_ne!(concept_id("p1", "Attention"), concept_id("p2", "Attention"));
    }

    #[test]
    fn llm_graph_parses_and_validates_closed_vocabulary() {
        let (tree, _, attention_obj) = sample_tree();
        let raw = r#"Here is the graph:
        {"concepts": [
            {"name": "Attention", "description": "Weighted lookup.", "anchors": ["3.2 Attention"]},
            {"name": "Softmax", "description": "Normalizes scores.", "anchors": []},
            {"name": "Transformer", "description": "The architecture.", "anchors": ["3 Model Architecture"]}
        ],
        "edges": [
            {"from": "Softmax", "to": "Attention", "kind": "prerequisite_of"},
            {"from": "Attention", "to": "Transformer", "kind": "made_up_kind"},
            {"from": "Attention", "to": "Nonexistent", "kind": "depends_on"}
        ]}"#;
        let graph = parse_llm_graph("paper", &tree, raw).expect("parses");
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.extraction, "llm");
        // Anchored node links to the section object; unanchored is lower confidence.
        let attention = graph.nodes.iter().find(|n| n.name == "Attention").unwrap();
        assert_eq!(attention.object_ids, vec![attention_obj]);
        let softmax = graph.nodes.iter().find(|n| n.name == "Softmax").unwrap();
        assert!(softmax.confidence < attention.confidence);
        // Only the valid edge survives (closed vocabulary + known endpoints).
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].kind, EdgeKind::PrerequisiteOf);
    }

    #[test]
    fn overrides_survive_reextraction() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("T"), "file").unwrap();
        let (tree, _, _) = sample_tree();
        bundle
            .write_derived_json(
                "semantic_tree.json",
                &tree,
                "objects",
                serde_json::json!({"pipeline_version": "0.1.0", "status": "complete"}),
            )
            .unwrap();

        // First extraction (heuristic — no llm closure).
        let graph = run_concepts_stage(&bundle, None).unwrap();
        assert_eq!(graph.edges.len(), 1);
        let (from, to) = (graph.edges[0].from, graph.edges[0].to);

        // User deletes the edge; re-extraction must keep it deleted.
        record_override(
            &bundle,
            GraphOverride::DeleteEdge {
                from,
                to,
                at: crate::bundle::now_rfc3339(),
            },
        )
        .unwrap();
        let regenerated = run_concepts_stage(&bundle, None).unwrap();
        assert!(regenerated.edges.is_empty(), "override was reverted");

        // Stage recorded as degraded (heuristic) with a plain reason.
        let metadata = bundle.metadata().unwrap();
        assert_eq!(metadata.pipeline.stages["concepts"]["status"], "degraded");
    }
}
