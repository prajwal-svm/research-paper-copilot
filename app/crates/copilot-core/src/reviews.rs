//! Literature reviews (v4): living synthesis documents at the library level
//! (`reviews/<uuid>/`), generated over the cross-paper graph structure.
//!
//! The regeneration contract (the PRD's quality metric depends on it):
//! machine output lives in `generated.md`, the user's document in
//! `document.md`. Regeneration rewrites ONLY `generated.md` — the user's
//! edits are never touched; the UI offers a change summary for deliberate
//! merging. Synthesis cites only in-scope papers: `[[paper:ID]]` markers
//! referencing anything else are stripped at parse time.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const REVIEWS_DIR: &str = "reviews";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: Uuid,
    pub name: String,
    /// Global concept ids scoping the review.
    pub concepts: Vec<Uuid>,
    /// Library paper ids in scope (the only citable set).
    pub papers: Vec<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReviewsError {
    #[error("reviews: {0}")]
    Io(#[from] std::io::Error),
    #[error("review not found")]
    NotFound,
}

fn dir(library_root: &Path, id: Uuid) -> PathBuf {
    library_root.join(REVIEWS_DIR).join(id.to_string())
}

pub fn create(
    library_root: &Path,
    name: &str,
    concepts: Vec<Uuid>,
    papers: Vec<String>,
) -> Result<Review, ReviewsError> {
    let review = Review {
        id: Uuid::new_v4(),
        name: name.to_string(),
        concepts,
        papers,
        created_at: crate::bundle::now_rfc3339(),
        generated_at: None,
    };
    let d = dir(library_root, review.id);
    std::fs::create_dir_all(&d)?;
    std::fs::write(
        d.join("review.json"),
        serde_json::to_vec_pretty(&review).expect("serializable"),
    )?;
    Ok(review)
}

pub fn list(library_root: &Path) -> Vec<Review> {
    let mut reviews = Vec::new();
    let Ok(entries) = std::fs::read_dir(library_root.join(REVIEWS_DIR)) else {
        return reviews;
    };
    for entry in entries.flatten() {
        if let Ok(bytes) = std::fs::read(entry.path().join("review.json")) {
            if let Ok(review) = serde_json::from_slice::<Review>(&bytes) {
                reviews.push(review);
            }
        }
    }
    reviews.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    reviews
}

pub fn get(library_root: &Path, id: Uuid) -> Result<Review, ReviewsError> {
    let bytes = std::fs::read(dir(library_root, id).join("review.json"))
        .map_err(|_| ReviewsError::NotFound)?;
    serde_json::from_slice(&bytes).map_err(|_| ReviewsError::NotFound)
}

pub fn generated(library_root: &Path, id: Uuid) -> Option<String> {
    std::fs::read_to_string(dir(library_root, id).join("generated.md")).ok()
}

pub fn document(library_root: &Path, id: Uuid) -> Option<String> {
    std::fs::read_to_string(dir(library_root, id).join("document.md")).ok()
}

/// Save the user's document (their copy; regeneration never touches it).
pub fn write_document(library_root: &Path, id: Uuid, content: &str) -> Result<(), ReviewsError> {
    std::fs::create_dir_all(dir(library_root, id))?;
    std::fs::write(dir(library_root, id).join("document.md"), content)?;
    Ok(())
}

/// Structural inputs the LLM narrates over (the graph provides structure;
/// prose never introduces out-of-scope papers).
pub struct SynthesisInputs<'a> {
    /// (paper_id, title, published_at) for every in-scope paper.
    pub papers: &'a [(String, String, Option<String>)],
    /// (concept name, papers sharing it) — the thematic skeleton.
    pub shared_concepts: &'a [(String, Vec<String>)],
    /// (from paper, to paper, edge kind) for contradicts/extends lineage.
    pub relations: &'a [(String, String, String)],
}

pub fn synthesis_prompt(review: &Review, inputs: &SynthesisInputs) -> String {
    let papers: String = inputs
        .papers
        .iter()
        .map(|(id, title, date)| {
            format!(
                "- [[paper:{id}]] \"{title}\"{}\n",
                date.as_deref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            )
        })
        .collect();
    let themes: String = inputs
        .shared_concepts
        .iter()
        .map(|(concept, members)| {
            format!(
                "- {concept}: appears in {}\n",
                members
                    .iter()
                    .map(|p| format!("[[paper:{p}]]"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect();
    let relations: String = inputs
        .relations
        .iter()
        .map(|(from, to, kind)| format!("- [[paper:{from}]] {kind} [[paper:{to}]]\n"))
        .collect();
    format!(
        "Write a literature review titled \"{name}\" over ONLY the papers listed below.\n\
         Structure: thematic sections following the shared concepts; a method-comparison table \
         (papers as rows); a chronological lineage paragraph using the relations and dates.\n\
         Every claim MUST cite its papers inline as [[paper:ID]] using ids from the list — \
         citations of anything else will be removed. Markdown.\n\n\
         Papers in scope:\n{papers}\nShared concepts (thematic skeleton):\n{themes}\n\
         Cross-paper relations:\n{relations}",
        name = review.name,
    )
}

/// Strip `[[paper:ID]]` markers whose id is not in the review's scope.
/// Returns (cleaned, removed count).
pub fn strip_out_of_scope(text: &str, review: &Review) -> (String, usize) {
    let mut removed = 0;
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[[paper:") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 8..];
        match after.find("]]") {
            Some(end) => {
                let id = &after[..end];
                if review.papers.iter().any(|p| p == id) {
                    result.push_str(&format!("[[paper:{id}]]"));
                } else {
                    removed += 1;
                }
                rest = &after[end + 2..];
            }
            None => {
                result.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    result.push_str(rest);
    (result, removed)
}

/// What changed between the previous and new machine synthesis — shown so
/// the user merges deliberately.
#[derive(Debug, Clone, Serialize)]
pub struct RefreshSummary {
    pub previous_exists: bool,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub out_of_scope_citations_removed: usize,
}

/// Regenerate the machine synthesis. Writes `generated.md` ONLY; the user's
/// `document.md` is never touched (first generation also seeds `document.md`
/// as a starting copy — after that it belongs to the user).
pub fn regenerate(
    library_root: &Path,
    review: &Review,
    inputs: &SynthesisInputs,
    llm: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<RefreshSummary>, ReviewsError> {
    let Some(raw) = llm(&synthesis_prompt(review, inputs)) else {
        return Ok(None); // no key — existing documents stay editable/exportable
    };
    let (cleaned, removed) = strip_out_of_scope(&raw, review);
    let d = dir(library_root, review.id);
    std::fs::create_dir_all(&d)?;
    let previous = generated(library_root, review.id);
    std::fs::write(d.join("generated.md"), &cleaned)?;
    if document(library_root, review.id).is_none() {
        std::fs::write(d.join("document.md"), &cleaned)?;
    }
    let mut updated = review.clone();
    updated.generated_at = Some(crate::bundle::now_rfc3339());
    std::fs::write(
        d.join("review.json"),
        serde_json::to_vec_pretty(&updated).expect("serializable"),
    )?;

    let summary = match &previous {
        Some(old) => {
            use std::collections::HashSet;
            let old_lines: HashSet<&str> = old.lines().collect();
            let new_lines: HashSet<&str> = cleaned.lines().collect();
            RefreshSummary {
                previous_exists: true,
                added_lines: new_lines.difference(&old_lines).count(),
                removed_lines: old_lines.difference(&new_lines).count(),
                out_of_scope_citations_removed: removed,
            }
        }
        None => RefreshSummary {
            previous_exists: false,
            added_lines: cleaned.lines().count(),
            removed_lines: 0,
            out_of_scope_citations_removed: removed,
        },
    };
    Ok(Some(summary))
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::type_complexity)]
    fn inputs() -> (
        Vec<(String, String, Option<String>)>,
        Vec<(String, Vec<String>)>,
        Vec<(String, String, String)>,
    ) {
        (
            vec![
                ("p1".into(), "Attention".into(), Some("2017".into())),
                ("p2".into(), "BERT".into(), Some("2018".into())),
            ],
            vec![("attention".into(), vec!["p1".into(), "p2".into()])],
            vec![("p2".into(), "p1".into(), "extends".into())],
        )
    }

    #[test]
    fn regeneration_never_touches_the_users_document() {
        let tmp = tempfile::tempdir().unwrap();
        let review = create(
            tmp.path(),
            "Attention lineage",
            vec![],
            vec!["p1".into(), "p2".into()],
        )
        .unwrap();
        let (papers, shared, relations) = inputs();
        let synthesis_inputs = SynthesisInputs {
            papers: &papers,
            shared_concepts: &shared,
            relations: &relations,
        };

        regenerate(tmp.path(), &review, &synthesis_inputs, &|_| {
            Some("# Review v1\nAttention [[paper:p1]] started it.".into())
        })
        .unwrap();
        // First generation seeds the user's document.
        assert!(document(tmp.path(), review.id)
            .unwrap()
            .contains("Review v1"));

        // User edits their document.
        write_document(
            tmp.path(),
            review.id,
            "# My heavily edited review\ncustom text",
        )
        .unwrap();
        let edited = document(tmp.path(), review.id).unwrap();

        // Regeneration updates generated.md only; document.md byte-identical.
        let summary = regenerate(tmp.path(), &review, &synthesis_inputs, &|_| {
            Some("# Review v2\nBERT [[paper:p2]] extended it.".into())
        })
        .unwrap()
        .unwrap();
        assert!(summary.previous_exists);
        assert!(summary.added_lines > 0);
        assert_eq!(
            document(tmp.path(), review.id).unwrap(),
            edited,
            "user document untouched by regeneration"
        );
        assert!(generated(tmp.path(), review.id)
            .unwrap()
            .contains("Review v2"));

        // No key: nothing changes, documents keep serving.
        assert!(
            regenerate(tmp.path(), &review, &synthesis_inputs, &|_| None)
                .unwrap()
                .is_none()
        );
        assert_eq!(document(tmp.path(), review.id).unwrap(), edited);
    }

    #[test]
    fn out_of_scope_citations_are_stripped() {
        let tmp = tempfile::tempdir().unwrap();
        let review = create(tmp.path(), "R", vec![], vec!["p1".into()]).unwrap();
        let (cleaned, removed) = strip_out_of_scope(
            "In [[paper:p1]] and the invented [[paper:ghost]] we see...",
            &review,
        );
        assert_eq!(removed, 1);
        assert!(cleaned.contains("[[paper:p1]]"));
        assert!(!cleaned.contains("ghost"));
    }

    #[test]
    fn prompt_carries_structure_and_scope() {
        let tmp = tempfile::tempdir().unwrap();
        let review = create(tmp.path(), "R", vec![], vec!["p1".into(), "p2".into()]).unwrap();
        let (papers, shared, relations) = inputs();
        let prompt = synthesis_prompt(
            &review,
            &SynthesisInputs {
                papers: &papers,
                shared_concepts: &shared,
                relations: &relations,
            },
        );
        assert!(prompt.contains("[[paper:p1]] \"Attention\" (2017)"));
        assert!(prompt.contains("attention: appears in"));
        assert!(prompt.contains("[[paper:p2]] extends [[paper:p1]]"));
        assert!(prompt.contains("ONLY the papers listed"));
    }
}
