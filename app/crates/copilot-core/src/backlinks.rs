//! Paper-to-paper backlinks (v2): explicit user-data links between bundles,
//! stored append-only in each bundle's `links/links.jsonl` (outgoing side).
//! "Links here" is answered by scanning the library — the bundle stays the
//! portable unit; no hidden global state to desync.
//!
//! Links carry resolved identifiers (paper id when the target is in the
//! library, DOI/arXiv otherwise) so they survive renames and imports:
//! a link recorded by identifier attaches to the target once it exists.

use serde::{Deserialize, Serialize};

use crate::bundle::Bundle;
use crate::library::Library;

const LINKS_JOURNAL: &str = "links/links.jsonl";

/// How a link's target is identified. At least one field is set; `paper_id`
/// wins when the target is in the library.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PaperRef {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paper_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arxiv_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl PaperRef {
    pub fn by_id(paper_id: &str) -> PaperRef {
        PaperRef {
            paper_id: Some(paper_id.to_string()),
            ..Default::default()
        }
    }

    /// Does this reference resolve to the given library paper?
    fn matches(&self, summary: &crate::library::PaperSummary) -> bool {
        if let Some(id) = &self.paper_id {
            return *id == summary.id;
        }
        if let (Some(doi), Some(paper_doi)) = (&self.doi, &summary.doi) {
            return doi.eq_ignore_ascii_case(paper_doi);
        }
        if let (Some(arxiv), Some(paper_arxiv)) = (&self.arxiv_id, &summary.arxiv_id) {
            return arxiv == paper_arxiv;
        }
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperLink {
    pub to: PaperRef,
    /// "citation" (suggested from a citation import) | "manual"
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub at: String,
}

/// A resolved incoming link: which paper links here, and the link itself.
#[derive(Debug, Clone, Serialize)]
pub struct IncomingLink {
    pub from_paper_id: String,
    pub from_title: String,
    pub link: PaperLink,
}

#[derive(Debug, thiserror::Error)]
pub enum BacklinkError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error(transparent)]
    Library(#[from] crate::library::LibraryError),
}

/// Record an outgoing link (idempotent: an identical target+kind is a no-op).
pub fn add_link(bundle: &Bundle, link: PaperLink) -> Result<bool, BacklinkError> {
    let existing = links_out(bundle)?;
    if existing
        .iter()
        .any(|l| l.to == link.to && l.kind == link.kind)
    {
        return Ok(false);
    }
    bundle.journal(LINKS_JOURNAL).append(&link)?;
    Ok(true)
}

/// Outgoing links of a bundle, oldest first.
pub fn links_out(bundle: &Bundle) -> Result<Vec<PaperLink>, BacklinkError> {
    Ok(bundle.journal(LINKS_JOURNAL).read_all()?)
}

/// Incoming links: every library paper whose outgoing links resolve to
/// `paper_id` (by id, DOI, or arXiv id).
pub fn links_in(library: &Library, paper_id: &str) -> Result<Vec<IncomingLink>, BacklinkError> {
    let papers = library.list()?;
    let Some(target) = papers.iter().find(|p| p.id == paper_id) else {
        return Ok(Vec::new());
    };
    let mut incoming = Vec::new();
    for paper in &papers {
        if paper.id == paper_id {
            continue;
        }
        let Ok(bundle) = library.get(&paper.id) else {
            continue;
        };
        for link in links_out(&bundle)? {
            if link.to.matches(target) {
                incoming.push(IncomingLink {
                    from_paper_id: paper.id.clone(),
                    from_title: paper.title.clone(),
                    link,
                });
            }
        }
    }
    Ok(incoming)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    fn create(library: &Library, pdf: &[u8], paper: Paper) -> String {
        let id = library.new_bundle_id(&paper.title);
        Bundle::create(&library.bundle_path(&id), pdf, paper, "file").unwrap();
        id
    }

    fn library_with_two_papers() -> (tempfile::TempDir, Library, String, String) {
        let tmp = tempfile::tempdir().unwrap();
        let library = Library::open(tmp.path()).unwrap();
        let a = create(&library, b"%PDF-1.5 citing", Paper::new("Citing Paper"));
        let mut cited = Paper::new("Cited Paper");
        cited.extra.insert(
            "identifiers".to_string(),
            serde_json::json!({"arxiv_id": "1706.03762"}),
        );
        let b = create(&library, b"%PDF-1.5 cited", cited);
        (tmp, library, a, b)
    }

    #[test]
    fn links_listable_from_both_sides() {
        let (_tmp, library, citing, cited) = library_with_two_papers();
        let bundle = library.get(&citing).unwrap();
        add_link(
            &bundle,
            PaperLink {
                to: PaperRef::by_id(&cited),
                kind: "citation".to_string(),
                note: None,
                at: crate::bundle::now_rfc3339(),
            },
        )
        .unwrap();

        let out = links_out(&bundle).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].to.paper_id.as_deref(), Some(cited.as_str()));

        let incoming = links_in(&library, &cited).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from_paper_id, citing);
        assert_eq!(incoming[0].from_title, "Citing Paper");
    }

    #[test]
    fn identifier_links_resolve_and_duplicates_are_dropped() {
        let (_tmp, library, citing, cited) = library_with_two_papers();
        let bundle = library.get(&citing).unwrap();
        // Link recorded by arXiv id (target imported later in real flows).
        let link = PaperLink {
            to: PaperRef {
                arxiv_id: Some("1706.03762".to_string()),
                ..Default::default()
            },
            kind: "citation".to_string(),
            note: None,
            at: crate::bundle::now_rfc3339(),
        };
        assert!(add_link(&bundle, link.clone()).unwrap());
        assert!(!add_link(&bundle, link).unwrap(), "duplicate is a no-op");

        let incoming = links_in(&library, &cited).unwrap();
        assert_eq!(incoming.len(), 1, "arXiv-id link resolves to the paper");
    }
}
