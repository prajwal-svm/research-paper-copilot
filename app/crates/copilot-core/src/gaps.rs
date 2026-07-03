//! Gap detection (v4): structural analysis first, narration second.
//!
//! The hallucination-proofing is architectural: gap candidates are computed
//! deterministically from registry/graph structure and ranked by a
//! structural score BEFORE any LLM involvement. Narration can only fill the
//! `narrative` field of gaps that already exist — it cannot add, remove, or
//! re-rank them (the gap set is provably identical with and without a
//! provider). Every gap traces to the ids that produced it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::concept_registry::RegistryState;

pub const GAPS_DIR: &str = "gaps";

/// Below these thresholds the report refuses to manufacture gaps.
pub const MIN_PAPERS: usize = 5;
pub const MIN_CONCEPTS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    /// "unexplored_combination" | "unresolved_contradiction" | "stale_assumption"
    pub kind: String,
    /// Deterministic structural score — the ranking key.
    pub score: f64,
    /// Machine-generated factual statement of the structure (always present).
    pub statement: String,
    /// Concepts involved (global ids) — the trace.
    pub concepts: Vec<Uuid>,
    /// Papers cited as the evidential basis.
    pub papers: Vec<String>,
    /// LLM narration (optional enrichment; never load-bearing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub narrative: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum GapReport {
    /// Honest refusal: the scoped library can't support gap claims.
    InsufficientCoverage {
        papers_analyzed: usize,
        concepts_analyzed: usize,
        minimum_papers: usize,
        minimum_concepts: usize,
    },
    Report {
        generated_at: String,
        papers_analyzed: usize,
        concepts_analyzed: usize,
        gaps: Vec<Gap>,
    },
}

/// Cross-paper `contradicts`/`extends` style edges resolved to global
/// concept ids: (paper_id, from_global, to_global, kind).
pub type GlobalEdge = (String, Uuid, Uuid, String);

/// Compute gap candidates from structure alone. Deterministic: same inputs →
/// same gaps in the same order, LLM or no LLM.
pub fn compute_gaps(
    registry: &RegistryState,
    edges: &[GlobalEdge],
    published_at: &HashMap<String, Option<String>>,
) -> GapReport {
    let papers: HashSet<&str> = registry
        .concepts
        .iter()
        .flat_map(|c| c.members.iter().map(|(p, _)| p.as_str()))
        .collect();
    let concepts_analyzed = registry.concepts.len();
    if papers.len() < MIN_PAPERS || concepts_analyzed < MIN_CONCEPTS {
        return GapReport::InsufficientCoverage {
            papers_analyzed: papers.len(),
            concepts_analyzed,
            minimum_papers: MIN_PAPERS,
            minimum_concepts: MIN_CONCEPTS,
        };
    }

    let name_of = |id: Uuid| {
        registry
            .concepts
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let members_of = |id: Uuid| -> Vec<String> {
        registry
            .concepts
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.members.iter().map(|(p, _)| p.clone()).collect())
            .unwrap_or_default()
    };

    let mut gaps: Vec<Gap> = Vec::new();

    // --- 1. Unexplored combinations: A and B each well-supported, both
    // co-occur with a shared sibling S, but never with each other. ---
    let frequent: Vec<Uuid> = registry
        .concepts
        .iter()
        .filter(|c| c.members.len() >= 2)
        .map(|c| c.id)
        .collect();
    let matrix = registry.co_occurrence(&frequent);
    let co = |a: Uuid, b: Uuid| -> u32 {
        let key = if a < b { (a, b) } else { (b, a) };
        matrix.get(&key).copied().unwrap_or(0)
    };
    for i in 0..frequent.len() {
        for j in (i + 1)..frequent.len() {
            let (a, b) = (frequent[i], frequent[j]);
            if co(a, b) > 0 {
                continue;
            }
            // A shared sibling co-occurring with both makes the pair
            // plausible rather than random.
            let sibling = frequent
                .iter()
                .find(|&&s| s != a && s != b && co(s, a) > 0 && co(s, b) > 0);
            let Some(&sibling) = sibling else { continue };
            let support = (members_of(a).len().min(members_of(b).len())) as f64;
            let mut papers: Vec<String> = members_of(a);
            papers.extend(members_of(b));
            papers.sort();
            papers.dedup();
            gaps.push(Gap {
                kind: "unexplored_combination".to_string(),
                score: support,
                statement: format!(
                    "\"{}\" and \"{}\" never co-occur in the library, though both co-occur \
                     with \"{}\" — the combination appears untried here.",
                    name_of(a),
                    name_of(b),
                    name_of(sibling),
                ),
                concepts: vec![a, b, sibling],
                papers,
                narrative: None,
            });
        }
    }

    // --- 2. Unresolved contradictions: a contradicts edge with no later
    // paper containing both concepts. ---
    for (paper, from, to, kind) in edges {
        if kind != "contradicts" {
            continue;
        }
        let edge_date = published_at.get(paper).cloned().flatten();
        let both: Vec<String> = members_of(*from)
            .into_iter()
            .filter(|p| members_of(*to).contains(p))
            .collect();
        let resolved_later = both.iter().any(|p| {
            p != paper
                && match (&edge_date, published_at.get(p).cloned().flatten()) {
                    (Some(edge), Some(other)) => other > *edge,
                    _ => false,
                }
        });
        if !resolved_later {
            gaps.push(Gap {
                kind: "unresolved_contradiction".to_string(),
                score: (members_of(*from).len() + members_of(*to).len()) as f64,
                statement: format!(
                    "\"{}\" contradicts \"{}\" (recorded in {paper}) and no later library \
                     paper connects both — the contradiction stands unresolved here.",
                    name_of(*from),
                    name_of(*to),
                ),
                concepts: vec![*from, *to],
                papers: vec![paper.clone()],
                narrative: None,
            });
        }
    }

    // --- 3. Stale assumptions: well-supported concepts whose newest paper
    // is old relative to the library. ---
    let mut dates: Vec<&str> = published_at
        .values()
        .flatten()
        .map(|s| s.as_str())
        .collect();
    dates.sort();
    if let Some(&median) = dates.get(dates.len() / 2) {
        for concept in &registry.concepts {
            if concept.members.len() < 3 {
                continue;
            }
            let newest = concept
                .members
                .iter()
                .filter_map(|(p, _)| published_at.get(p).cloned().flatten())
                .max();
            if let Some(newest) = newest {
                if newest.as_str() < median {
                    gaps.push(Gap {
                        kind: "stale_assumption".to_string(),
                        score: concept.members.len() as f64 * 0.5,
                        statement: format!(
                            "\"{}\" is load-bearing across {} papers but its newest support \
                             ({newest}) predates the library median ({median}) — worth \
                             revisiting under current methods.",
                            concept.name,
                            concept.members.len(),
                        ),
                        concepts: vec![concept.id],
                        papers: concept.members.iter().map(|(p, _)| p.clone()).collect(),
                        narrative: None,
                    });
                }
            }
        }
    }

    // Deterministic ranking: score desc, then statement for stability.
    gaps.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.statement.cmp(&b.statement))
    });
    gaps.truncate(20);
    GapReport::Report {
        generated_at: crate::bundle::now_rfc3339(),
        papers_analyzed: papers.len(),
        concepts_analyzed,
        gaps,
    }
}

/// Narrate pre-computed gaps. Only the `narrative` fields change — count,
/// order, statements, traces are untouched by construction (the function
/// consumes and returns the same gaps, filling one optional field).
pub fn narrate(report: &mut GapReport, llm: &dyn Fn(&str) -> Option<String>) {
    let GapReport::Report { gaps, .. } = report else {
        return;
    };
    for gap in gaps.iter_mut() {
        let prompt = format!(
            "In 2–3 sentences, explain to a researcher why this structural gap in their paper \
             library may be worth investigating. Do not invent papers or claims beyond the \
             statement.\n\nGap: {}",
            gap.statement
        );
        gap.narrative = llm(&prompt);
    }
}

/// Persist a report at the library level; returns its path id.
pub fn save_report(library_root: &Path, report: &GapReport) -> std::io::Result<PathBuf> {
    let d = library_root.join(GAPS_DIR);
    std::fs::create_dir_all(&d)?;
    let path = d.join(format!(
        "gaps-{}.json",
        crate::bundle::now_rfc3339().replace(':', "-")
    ));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(report).expect("serializable"),
    )?;
    Ok(path)
}

pub fn latest_report(library_root: &Path) -> Option<GapReport> {
    let d = library_root.join(GAPS_DIR);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    let bytes = std::fs::read(files.last()?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concept_registry::ConceptRegistry;
    use crate::concepts::{concept_id, ConceptNode, KnowledgeGraph};

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

    /// A library where "method M2 on problem P" is structurally untried:
    /// M1+P co-occur, M1+M2 co-occur (sibling S = M1), M2+P never do.
    fn build_registry() -> RegistryState {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        // 6 papers, 12+ concepts for coverage thresholds.
        registry
            .auto_link("p1", &graph_with("p1", &["M1", "P", "f1", "f2"]), None)
            .unwrap();
        registry
            .auto_link("p2", &graph_with("p2", &["M1", "P", "f3", "f4"]), None)
            .unwrap();
        registry
            .auto_link("p3", &graph_with("p3", &["M1", "M2", "f5"]), None)
            .unwrap();
        registry
            .auto_link("p4", &graph_with("p4", &["M2", "f6", "f7"]), None)
            .unwrap();
        registry
            .auto_link("p5", &graph_with("p5", &["M2", "f8"]), None)
            .unwrap();
        registry
            .auto_link("p6", &graph_with("p6", &["P", "f9"]), None)
            .unwrap();
        registry.state().unwrap()
    }

    fn dates() -> HashMap<String, Option<String>> {
        (1..=6)
            .map(|i| (format!("p{i}"), Some(format!("202{}-01-01", i % 5))))
            .collect()
    }

    #[test]
    fn unexplored_combination_surfaces_with_trace() {
        let state = build_registry();
        let report = compute_gaps(&state, &[], &dates());
        let GapReport::Report {
            gaps,
            papers_analyzed,
            ..
        } = &report
        else {
            panic!("expected a report, got {report:?}");
        };
        assert_eq!(*papers_analyzed, 6);
        let combination = gaps
            .iter()
            .find(|g| g.kind == "unexplored_combination")
            .expect("M2×P gap found");
        assert!(
            combination.statement.contains("M2") && combination.statement.contains("P"),
            "{}",
            combination.statement
        );
        assert_eq!(combination.concepts.len(), 3, "a, b, sibling traced");
        assert!(!combination.papers.is_empty(), "citable papers attached");
    }

    #[test]
    fn narration_cannot_change_the_gap_set() {
        let state = build_registry();
        let dates = dates();
        let without_llm = compute_gaps(&state, &[], &dates);
        let mut with_llm = compute_gaps(&state, &[], &dates);
        narrate(&mut with_llm, &|_| Some("Compelling story!".into()));

        let strip = |r: &GapReport| -> Vec<(String, String, Vec<Uuid>)> {
            match r {
                GapReport::Report { gaps, .. } => gaps
                    .iter()
                    .map(|g| (g.kind.clone(), g.statement.clone(), g.concepts.clone()))
                    .collect(),
                _ => vec![],
            }
        };
        assert_eq!(
            strip(&without_llm),
            strip(&with_llm),
            "same gaps, same order, same traces — narration only adds prose"
        );
        if let GapReport::Report { gaps, .. } = &with_llm {
            assert!(gaps.iter().all(|g| g.narrative.is_some()));
        }
    }

    #[test]
    fn sparse_library_refuses_honestly() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = ConceptRegistry::open(tmp.path());
        registry
            .auto_link("p1", &graph_with("p1", &["A", "B"]), None)
            .unwrap();
        registry
            .auto_link("p2", &graph_with("p2", &["A", "C"]), None)
            .unwrap();
        let state = registry.state().unwrap();
        let report = compute_gaps(&state, &[], &HashMap::new());
        assert!(
            matches!(
                report,
                GapReport::InsufficientCoverage {
                    papers_analyzed: 2,
                    ..
                }
            ),
            "refuses instead of inventing gaps"
        );
    }

    #[test]
    fn unresolved_contradiction_detected() {
        let state = build_registry();
        let m1 = state.concepts.iter().find(|c| c.name == "M1").unwrap().id;
        let m2 = state.concepts.iter().find(|c| c.name == "M2").unwrap().id;
        let edges = vec![("p3".to_string(), m1, m2, "contradicts".to_string())];
        let report = compute_gaps(&state, &edges, &dates());
        let GapReport::Report { gaps, .. } = report else {
            panic!()
        };
        assert!(
            gaps.iter().any(|g| g.kind == "unresolved_contradiction"),
            "contradiction with no later resolving paper surfaces"
        );
    }
}
