//! Library-global concept identity (v2): `concepts.jsonl` at the library
//! root maps global concept ids to per-paper graph nodes.
//!
//! Event-sourced like all user data: links, splits, and merges are
//! append-only events folded at read. Auto-matching is conservative —
//! normalized-name equality, optionally tightened by embedding similarity —
//! and every automatic decision is recorded and user-reversible. A split
//! blocklists the (node, global) pair so future auto-matching respects it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Journal;
use crate::concepts::KnowledgeGraph;

pub const REGISTRY_FILE: &str = "concepts.jsonl";

/// Cosine similarity two name embeddings must exceed for an auto-merge when
/// names differ (conservative: silent wrong merges are the classic failure).
pub const AUTO_MERGE_SIMILARITY: f32 = 0.92;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum RegistryEvent {
    /// A per-paper node linked into a global concept.
    Link {
        global: Uuid,
        paper_id: String,
        node: Uuid,
        name: String,
        /// "auto" | "user"
        source: String,
        at: String,
    },
    /// A node detached from a global concept (wrong merge). Auto-matching
    /// never re-links this pair.
    Split {
        global: Uuid,
        paper_id: String,
        node: Uuid,
        at: String,
    },
    /// Two global concepts merged (user-confirmed): `absorb`'s members move
    /// to `keep`.
    Merge {
        keep: Uuid,
        absorb: Uuid,
        at: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct GlobalConcept {
    pub id: Uuid,
    /// Canonical name = the first linked node's name.
    pub name: String,
    /// (paper_id, node_id) pairs, link order preserved.
    pub members: Vec<(String, Uuid)>,
}

#[derive(Debug, Default)]
pub struct RegistryState {
    pub concepts: Vec<GlobalConcept>,
    /// (paper_id, node) pairs the user split — never auto-rematched.
    split_pairs: HashSet<(String, Uuid)>,
    /// node → global for fast lookup.
    by_node: HashMap<(String, Uuid), Uuid>,
}

impl RegistryState {
    pub fn global_for(&self, paper_id: &str, node: Uuid) -> Option<&GlobalConcept> {
        let id = self.by_node.get(&(paper_id.to_string(), node))?;
        self.concepts.iter().find(|c| c.id == *id)
    }

    /// Other papers where this node's concept appears ("seen in paper X").
    pub fn occurrences_elsewhere(&self, paper_id: &str, node: Uuid) -> Vec<(String, Uuid)> {
        self.global_for(paper_id, node)
            .map(|c| {
                c.members
                    .iter()
                    .filter(|(p, _)| p != paper_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Chronological lineage of a global concept (v4): its member papers
    /// ordered by publication date (undated papers last, then by paper id
    /// for determinism), each with the concept's per-paper node. Callers
    /// enrich entries with incident `extends`/`cites` edges from the graph
    /// index. Offline; <150 ms at 200 papers (perf suite).
    pub fn lineage(
        &self,
        global: Uuid,
        published_at: &HashMap<String, Option<String>>,
    ) -> Vec<(String, Uuid, Option<String>)> {
        let Some(concept) = self.concepts.iter().find(|c| c.id == global) else {
            return Vec::new();
        };
        let mut entries: Vec<(String, Uuid, Option<String>)> = concept
            .members
            .iter()
            .map(|(paper, node)| {
                (
                    paper.clone(),
                    *node,
                    published_at.get(paper).cloned().flatten(),
                )
            })
            .collect();
        entries.sort_by(|a, b| match (&a.2, &b.2) {
            (Some(x), Some(y)) => x.cmp(y).then(a.0.cmp(&b.0)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.cmp(&b.0),
        });
        entries
    }

    /// Concept co-occurrence across the library (v4, gap analysis): for the
    /// scoped global concepts, how many papers contain each pair. Computed
    /// from registry state alone; symmetric pairs stored once with
    /// `(min, max)` key order. Offline; same <150 ms budget.
    pub fn co_occurrence(&self, scope: &[Uuid]) -> HashMap<(Uuid, Uuid), u32> {
        let paper_sets: Vec<(Uuid, HashSet<&str>)> = scope
            .iter()
            .filter_map(|id| {
                self.concepts.iter().find(|c| c.id == *id).map(|c| {
                    (
                        *id,
                        c.members
                            .iter()
                            .map(|(p, _)| p.as_str())
                            .collect::<HashSet<_>>(),
                    )
                })
            })
            .collect();
        let mut matrix = HashMap::new();
        for i in 0..paper_sets.len() {
            for j in (i + 1)..paper_sets.len() {
                let count = paper_sets[i].1.intersection(&paper_sets[j].1).count() as u32;
                if count > 0 {
                    let (a, b) = (paper_sets[i].0, paper_sets[j].0);
                    let key = if a < b { (a, b) } else { (b, a) };
                    matrix.insert(key, count);
                }
            }
        }
        matrix
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
}

pub struct ConceptRegistry {
    path: PathBuf,
}

impl ConceptRegistry {
    pub fn open(library_root: &Path) -> ConceptRegistry {
        ConceptRegistry {
            path: library_root.join(REGISTRY_FILE),
        }
    }

    fn journal(&self) -> Journal {
        Journal::at(self.path.clone())
    }

    pub fn record(&self, event: RegistryEvent) -> Result<(), RegistryError> {
        self.journal().append(&event)?;
        Ok(())
    }

    /// Fold all events into current state.
    pub fn state(&self) -> Result<RegistryState, RegistryError> {
        let events: Vec<RegistryEvent> = self.journal().read_all()?;
        let mut state = RegistryState::default();
        for event in events {
            match event {
                RegistryEvent::Link {
                    global,
                    paper_id,
                    node,
                    name,
                    ..
                } => {
                    let key = (paper_id.clone(), node);
                    state.split_pairs.remove(&key); // explicit re-link overrides a split
                    if let Some(concept) = state.concepts.iter_mut().find(|c| c.id == global) {
                        if !concept.members.contains(&key) {
                            concept.members.push(key.clone());
                        }
                    } else {
                        state.concepts.push(GlobalConcept {
                            id: global,
                            name,
                            members: vec![key.clone()],
                        });
                    }
                    state.by_node.insert(key, global);
                }
                RegistryEvent::Split {
                    global,
                    paper_id,
                    node,
                    ..
                } => {
                    let key = (paper_id, node);
                    if let Some(concept) = state.concepts.iter_mut().find(|c| c.id == global) {
                        concept.members.retain(|m| *m != key);
                    }
                    state.by_node.remove(&key);
                    state.split_pairs.insert(key);
                    state.concepts.retain(|c| !c.members.is_empty());
                }
                RegistryEvent::Merge { keep, absorb, .. } => {
                    let absorbed = state
                        .concepts
                        .iter()
                        .position(|c| c.id == absorb)
                        .map(|i| state.concepts.remove(i));
                    if let Some(absorbed) = absorbed {
                        if let Some(kept) = state.concepts.iter_mut().find(|c| c.id == keep) {
                            for member in absorbed.members {
                                state.by_node.insert(member.clone(), keep);
                                if !kept.members.contains(&member) {
                                    kept.members.push(member);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(state)
    }

    /// Link a paper's graph nodes into the registry. Name-equality matches
    /// always link; when `embed` is provided, near-name matches above
    /// [`AUTO_MERGE_SIMILARITY`] also link. Unmatched nodes create new
    /// global concepts. Idempotent per (paper, node); split pairs respected.
    /// Returns the number of nodes linked to *existing* concepts.
    #[allow(clippy::type_complexity)]
    pub fn auto_link(
        &self,
        paper_id: &str,
        graph: &KnowledgeGraph,
        embed: Option<&dyn Fn(&str) -> Option<Vec<f32>>>,
    ) -> Result<usize, RegistryError> {
        let state = self.state()?;
        let by_name: HashMap<String, Uuid> = state
            .concepts
            .iter()
            .map(|c| (normalize(&c.name), c.id))
            .collect();
        let mut existing_embeddings: Vec<(Uuid, Vec<f32>)> = Vec::new();
        if let Some(embed) = embed {
            for concept in &state.concepts {
                if let Some(vector) = embed(&concept.name) {
                    existing_embeddings.push((concept.id, vector));
                }
            }
        }

        let mut merged = 0;
        for node in &graph.nodes {
            let key = (paper_id.to_string(), node.id);
            if state.by_node.contains_key(&key) || state.split_pairs.contains(&key) {
                continue; // already linked, or user said no
            }
            let normalized = normalize(&node.name);
            let matched = by_name.get(&normalized).copied().or_else(|| {
                let embed = embed?;
                let vector = embed(&node.name)?;
                existing_embeddings
                    .iter()
                    .map(|(id, other)| (*id, cosine(&vector, other)))
                    .filter(|(_, sim)| *sim >= AUTO_MERGE_SIMILARITY)
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(id, _)| id)
            });
            let global = match matched {
                Some(existing) => {
                    merged += 1;
                    existing
                }
                None => Uuid::new_v4(),
            };
            self.record(RegistryEvent::Link {
                global,
                paper_id: paper_id.to_string(),
                node: node.id,
                name: node.name.clone(),
                source: "auto".to_string(),
                at: crate::bundle::now_rfc3339(),
            })?;
        }
        Ok(merged)
    }

    /// Library-wide concept search: normalized substring match over global
    /// concept names, exact-name matches first, then by member count.
    /// Budget: <150 ms at 200 papers (perf suite) — offline, in-memory fold.
    pub fn search(&self, query: &str) -> Result<Vec<GlobalConcept>, RegistryError> {
        let state = self.state()?;
        let normalized_query = normalize(query);
        if normalized_query.is_empty() {
            return Ok(Vec::new());
        }
        let mut hits: Vec<(u8, usize, GlobalConcept)> = state
            .concepts
            .into_iter()
            .filter_map(|concept| {
                let name = normalize(&concept.name);
                let rank = if name == normalized_query {
                    0
                } else if normalized_query.contains(&name) || name.contains(&normalized_query) {
                    1
                } else {
                    return None;
                };
                Some((rank, concept.members.len(), concept))
            })
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        Ok(hits.into_iter().map(|(_, _, c)| c).collect())
    }
}

fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::{concept_id, ConceptNode};

    fn graph_with(paper: &str, names: &[&str]) -> KnowledgeGraph {
        KnowledgeGraph {
            pipeline_version: "0.1.0".to_string(),
            extraction: "llm".to_string(),
            nodes: names
                .iter()
                .map(|n| ConceptNode {
                    id: concept_id(paper, n),
                    name: n.to_string(),
                    description: None,
                    object_ids: vec![],
                    confidence: 0.8,
                })
                .collect(),
            edges: vec![],
        }
    }

    #[test]
    fn same_name_across_papers_links_to_one_global() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        registry
            .auto_link(
                "p1",
                &graph_with("p1", &["Multi-Head Attention", "Softmax"]),
                None,
            )
            .unwrap();
        let merged = registry
            .auto_link("p2", &graph_with("p2", &["multi-head attention"]), None)
            .unwrap();
        assert_eq!(merged, 1, "name match links to the existing global");

        let state = registry.state().unwrap();
        assert_eq!(state.concepts.len(), 2);
        let mha = state
            .concepts
            .iter()
            .find(|c| normalize(&c.name) == "multi head attention")
            .unwrap();
        assert_eq!(mha.members.len(), 2, "appears in both papers");
        let node = concept_id("p1", "Multi-Head Attention");
        assert_eq!(
            state.occurrences_elsewhere("p1", node),
            vec![("p2".to_string(), concept_id("p2", "multi-head attention"),)]
        );
    }

    #[test]
    fn auto_link_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        let graph = graph_with("p1", &["Residual Connection"]);
        registry.auto_link("p1", &graph, None).unwrap();
        registry.auto_link("p1", &graph, None).unwrap();
        let state = registry.state().unwrap();
        assert_eq!(state.concepts.len(), 1);
        assert_eq!(state.concepts[0].members.len(), 1);
    }

    #[test]
    fn split_is_respected_by_future_auto_matching() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        registry
            .auto_link("p1", &graph_with("p1", &["Attention"]), None)
            .unwrap();
        registry
            .auto_link("p2", &graph_with("p2", &["Attention"]), None)
            .unwrap();
        let state = registry.state().unwrap();
        let global = state.concepts[0].id;
        let p2_node = concept_id("p2", "Attention");

        // Wrong merge: cognitive-science attention ≠ ML attention. Split.
        registry
            .record(RegistryEvent::Split {
                global,
                paper_id: "p2".to_string(),
                node: p2_node,
                at: crate::bundle::now_rfc3339(),
            })
            .unwrap();
        let state = registry.state().unwrap();
        assert_eq!(state.concepts[0].members.len(), 1, "detached immediately");
        assert!(state.global_for("p2", p2_node).is_none());

        // Re-running auto-link must NOT re-merge the split pair.
        let merged = registry
            .auto_link("p2", &graph_with("p2", &["Attention"]), None)
            .unwrap();
        assert_eq!(merged, 0);
        assert!(registry
            .state()
            .unwrap()
            .global_for("p2", p2_node)
            .is_none());
    }

    #[test]
    fn user_merge_moves_members() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        registry
            .auto_link("p1", &graph_with("p1", &["LayerNorm"]), None)
            .unwrap();
        registry
            .auto_link("p2", &graph_with("p2", &["Layer Normalization"]), None)
            .unwrap();
        let state = registry.state().unwrap();
        assert_eq!(state.concepts.len(), 2, "different names stay separate");
        let (keep, absorb) = (state.concepts[0].id, state.concepts[1].id);

        registry
            .record(RegistryEvent::Merge {
                keep,
                absorb,
                at: crate::bundle::now_rfc3339(),
            })
            .unwrap();
        let state = registry.state().unwrap();
        assert_eq!(state.concepts.len(), 1);
        assert_eq!(state.concepts[0].members.len(), 2);
        assert_eq!(
            state
                .global_for("p2", concept_id("p2", "Layer Normalization"))
                .unwrap()
                .id,
            keep
        );
    }

    #[test]
    fn embedding_similarity_merges_near_names_conservatively() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        // Toy embedder: near-identical vectors for the two attention names,
        // orthogonal for the unrelated one.
        let embed = |name: &str| -> Option<Vec<f32>> {
            let n = normalize(name);
            if n.contains("attention") {
                Some(vec![1.0, if n.contains("multi") { 0.1 } else { 0.0 }, 0.0])
            } else {
                Some(vec![0.0, 0.0, 1.0])
            }
        };
        registry
            .auto_link(
                "p1",
                &graph_with("p1", &["Multi-Head Attention"]),
                Some(&embed),
            )
            .unwrap();
        let merged = registry
            .auto_link(
                "p2",
                &graph_with("p2", &["Multihead Attention", "Beam Search"]),
                Some(&embed),
            )
            .unwrap();
        assert_eq!(merged, 1, "near-name merged, unrelated not");
        assert_eq!(registry.state().unwrap().concepts.len(), 2);
    }

    #[test]
    fn lineage_orders_by_publication_date_deterministically() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        for paper in ["p-bahdanau", "p-transformer", "p-bert"] {
            registry
                .auto_link(paper, &graph_with(paper, &["Attention"]), None)
                .unwrap();
        }
        let state = registry.state().unwrap();
        let global = state.concepts[0].id;
        let dates = HashMap::from([
            ("p-transformer".to_string(), Some("2017-06-12".to_string())),
            ("p-bahdanau".to_string(), Some("2014-09-01".to_string())),
            ("p-bert".to_string(), None), // undated → last
        ]);
        let lineage = state.lineage(global, &dates);
        let order: Vec<&str> = lineage.iter().map(|(p, _, _)| p.as_str()).collect();
        assert_eq!(order, ["p-bahdanau", "p-transformer", "p-bert"]);
        assert_eq!(lineage[0].1, concept_id("p-bahdanau", "Attention"));
        assert!(state.lineage(Uuid::new_v4(), &dates).is_empty());
    }

    #[test]
    fn co_occurrence_counts_shared_papers() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        // p1: A+B, p2: A+B, p3: A+C — so (A,B)=2, (A,C)=1, (B,C)=0.
        registry
            .auto_link("p1", &graph_with("p1", &["A", "B"]), None)
            .unwrap();
        registry
            .auto_link("p2", &graph_with("p2", &["A", "B"]), None)
            .unwrap();
        registry
            .auto_link("p3", &graph_with("p3", &["A", "C"]), None)
            .unwrap();
        let state = registry.state().unwrap();
        let id = |name: &str| {
            state
                .concepts
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.id)
                .unwrap()
        };
        let (a, b, c) = (id("A"), id("B"), id("C"));
        let matrix = state.co_occurrence(&[a, b, c]);
        let key = |x: Uuid, y: Uuid| if x < y { (x, y) } else { (y, x) };
        assert_eq!(matrix.get(&key(a, b)), Some(&2));
        assert_eq!(matrix.get(&key(a, c)), Some(&1));
        assert_eq!(matrix.get(&key(b, c)), None, "zero pairs omitted");
    }

    #[test]
    fn search_ranks_exact_matches_first() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        registry
            .auto_link(
                "p1",
                &graph_with(
                    "p1",
                    &["Residual Connections", "Residual Connections in CNNs"],
                ),
                None,
            )
            .unwrap();
        registry
            .auto_link("p2", &graph_with("p2", &["Residual Connections"]), None)
            .unwrap();
        let hits = registry.search("residual connections").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(normalize(&hits[0].name), "residual connections");
        assert_eq!(hits[0].members.len(), 2);
        assert!(registry.search("nonexistent thing").unwrap().is_empty());
    }
}
