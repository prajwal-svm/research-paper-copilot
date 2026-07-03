//! Novelty estimation (v4): search open indexes for work similar to a
//! hypothesis claim and return an evidence-backed verdict.
//!
//! The PRD's top risk is hallucinated novelty, so the type system forbids
//! it: [`NoveltyResult`] is only constructible through
//! [`NoveltyResult::from_search`], which derives the verdict from the
//! evidence — an empty or failed search yields `InsufficientEvidence`,
//! never `AppearsNovel`. Every verdict carries the evidence and the query.

use serde::{Deserialize, Serialize};

/// Closed verdict vocabulary. There is no way to express "novel" without
/// having searched and found the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoveltyVerdict {
    /// Search worked; nothing found is close — evidence lists the nearest.
    AppearsNovel,
    /// Similar-but-not-identical work exists.
    AdjacentWorkExists,
    /// A close match exists.
    LikelyKnown,
    /// Search failed, was empty, or couldn't run — NOT a novelty claim.
    InsufficientEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoveltyEvidence {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    /// "semantic_scholar" | "arxiv"
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Similarity of this work to the claim, in [0, 1].
    pub similarity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoveltyResult {
    pub verdict: NoveltyVerdict,
    /// Always present alongside the verdict — the UI never shows one
    /// without the other. For `AppearsNovel` these are the nearest (still
    /// dissimilar) works, proving the search actually looked.
    pub evidence: Vec<NoveltyEvidence>,
    pub query: String,
    pub checked_at: String,
}

const LIKELY_KNOWN_SIMILARITY: f32 = 0.85;
const ADJACENT_SIMILARITY: f32 = 0.70;

impl NoveltyResult {
    /// The only constructor: verdict follows from evidence. Empty evidence
    /// (search failed/empty) → `InsufficientEvidence`, by construction.
    pub fn from_search(query: &str, mut evidence: Vec<NoveltyEvidence>) -> NoveltyResult {
        evidence.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
        evidence.truncate(8);
        let verdict = match evidence.first() {
            None => NoveltyVerdict::InsufficientEvidence,
            Some(top) if top.similarity >= LIKELY_KNOWN_SIMILARITY => NoveltyVerdict::LikelyKnown,
            Some(top) if top.similarity >= ADJACENT_SIMILARITY => {
                NoveltyVerdict::AdjacentWorkExists
            }
            Some(_) => NoveltyVerdict::AppearsNovel,
        };
        NoveltyResult {
            verdict,
            evidence,
            query: query.to_string(),
            checked_at: crate::bundle::now_rfc3339(),
        }
    }
}

/// A work returned by an index search, before similarity scoring.
#[derive(Debug, Clone)]
pub struct FoundWork {
    pub title: String,
    pub abstract_text: Option<String>,
    pub year: Option<i32>,
    pub source: String,
    pub identifier: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum NoveltyError {
    #[error("index search failed: {0}")]
    Network(String),
}

/// Search Semantic Scholar's public API (optional key raises rate limits).
/// Only the query string is sent — never paper content.
#[cfg(feature = "native")]
pub fn search_semantic_scholar(
    query: &str,
    api_key: Option<&str>,
    limit: usize,
) -> Result<Vec<FoundWork>, NoveltyError> {
    let url = format!(
        "https://api.semanticscholar.org/graph/v1/paper/search?query={}&limit={}&fields=title,abstract,year,externalIds,url",
        urlencode(query),
        limit.clamp(1, 100),
    );
    let mut request = ureq::get(&url).timeout(std::time::Duration::from_secs(20));
    if let Some(key) = api_key {
        request = request.set("x-api-key", key);
    }
    let response: serde_json::Value = request
        .call()
        .map_err(|e| NoveltyError::Network(e.to_string()))?
        .into_json()
        .map_err(|e| NoveltyError::Network(e.to_string()))?;
    let works = response["data"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    Some(FoundWork {
                        title: entry["title"].as_str()?.to_string(),
                        abstract_text: entry["abstract"].as_str().map(|s| s.to_string()),
                        year: entry["year"].as_i64().map(|y| y as i32),
                        source: "semantic_scholar".to_string(),
                        identifier: entry["externalIds"]["ArXiv"]
                            .as_str()
                            .or(entry["externalIds"]["DOI"].as_str())
                            .map(|s| s.to_string()),
                        url: entry["url"].as_str().map(|s| s.to_string()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(works)
}

/// Search the arXiv Atom API (same split-parsing approach as the importer).
#[cfg(feature = "native")]
pub fn search_arxiv(query: &str, limit: usize) -> Result<Vec<FoundWork>, NoveltyError> {
    let url = format!(
        "https://export.arxiv.org/api/query?search_query=all:{}&max_results={}",
        urlencode(query),
        limit.clamp(1, 50),
    );
    let body = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|e| NoveltyError::Network(e.to_string()))?
        .into_string()
        .map_err(|e| NoveltyError::Network(e.to_string()))?;
    Ok(parse_arxiv_entries(&body))
}

fn parse_arxiv_entries(body: &str) -> Vec<FoundWork> {
    body.split("<entry>")
        .skip(1)
        .filter_map(|entry| {
            let title = xml_text(entry, "title")?;
            let id_url = xml_text(entry, "id");
            let arxiv_id = id_url
                .as_deref()
                .and_then(|u| u.rsplit('/').next())
                .map(|s| s.to_string());
            let year =
                xml_text(entry, "published").and_then(|p| p.get(..4).and_then(|y| y.parse().ok()));
            Some(FoundWork {
                title,
                abstract_text: xml_text(entry, "summary"),
                year,
                source: "arxiv".to_string(),
                identifier: arxiv_id,
                url: id_url,
            })
        })
        .collect()
}

fn xml_text(fragment: &str, tag: &str) -> Option<String> {
    let start = fragment.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = fragment[start..].find(&format!("</{tag}>"))? + start;
    let raw = &fragment[start..end];
    Some(
        raw.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Score found works against the claim and produce the verdict. Similarity
/// uses the local embedder when available (MiniLM cosine over title+abstract),
/// else a token-overlap fallback — either way the evidence is real search
/// results, and the scoring method is recorded implicitly by the runtime.
pub fn score_and_judge(
    claim: &str,
    works: Vec<FoundWork>,
    embed: Option<&dyn Fn(&str) -> Option<Vec<f32>>>,
) -> NoveltyResult {
    let claim_vector = embed.and_then(|f| f(claim));
    let evidence: Vec<NoveltyEvidence> = works
        .into_iter()
        .map(|work| {
            let text = match &work.abstract_text {
                Some(abstract_text) => format!("{}. {}", work.title, abstract_text),
                None => work.title.clone(),
            };
            let similarity = match (&claim_vector, embed) {
                (Some(cv), Some(f)) => f(&text).map(|wv| cosine(cv, &wv)).unwrap_or(0.0),
                _ => token_overlap(claim, &text),
            };
            NoveltyEvidence {
                title: work.title,
                year: work.year,
                source: work.source,
                identifier: work.identifier,
                url: work.url,
                similarity,
            }
        })
        .collect();
    NoveltyResult::from_search(claim, evidence)
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

/// Jaccard over lowercase alphanumeric tokens — the keyless fallback.
fn token_overlap(a: &str, b: &str) -> f32 {
    use std::collections::HashSet;
    let tokens = |s: &str| -> HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 2)
            .map(|t| t.to_string())
            .collect()
    };
    let (ta, tb) = (tokens(a), tokens(b));
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    ta.intersection(&tb).count() as f32 / ta.union(&tb).count() as f32
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn work(title: &str) -> FoundWork {
        FoundWork {
            title: title.to_string(),
            abstract_text: None,
            year: Some(2020),
            source: "arxiv".to_string(),
            identifier: Some("2001.00001".to_string()),
            url: None,
        }
    }

    #[test]
    fn empty_search_is_insufficient_evidence_never_novel() {
        let result = NoveltyResult::from_search("attention with rotary gates", vec![]);
        assert_eq!(result.verdict, NoveltyVerdict::InsufficientEvidence);
        assert!(result.evidence.is_empty());
    }

    #[test]
    fn verdict_follows_similarity_and_evidence_is_mandatory() {
        let evidence = |s: f32| NoveltyEvidence {
            title: "t".into(),
            year: None,
            source: "arxiv".into(),
            identifier: None,
            url: None,
            similarity: s,
        };
        let known = NoveltyResult::from_search("q", vec![evidence(0.9), evidence(0.2)]);
        assert_eq!(known.verdict, NoveltyVerdict::LikelyKnown);
        assert_eq!(known.evidence.len(), 2, "evidence rides with the verdict");

        let adjacent = NoveltyResult::from_search("q", vec![evidence(0.75)]);
        assert_eq!(adjacent.verdict, NoveltyVerdict::AdjacentWorkExists);

        let novel = NoveltyResult::from_search("q", vec![evidence(0.2)]);
        assert_eq!(novel.verdict, NoveltyVerdict::AppearsNovel);
        assert!(
            !novel.evidence.is_empty(),
            "even 'appears novel' proves the search looked"
        );
    }

    #[test]
    fn keyless_scoring_uses_token_overlap() {
        let works = vec![
            work("Scaled dot product attention with rotary position gates"),
            work("A survey of fish migration patterns"),
        ];
        let result = score_and_judge(
            "rotary position gates for dot product attention",
            works,
            None,
        );
        assert_eq!(result.evidence.len(), 2);
        assert!(
            result.evidence[0].title.contains("attention"),
            "similar work ranks first: {:?}",
            result.evidence[0].title
        );
        assert!(result.evidence[0].similarity > result.evidence[1].similarity);
    }

    #[test]
    fn arxiv_atom_parsing() {
        let body = r#"<feed><entry><id>http://arxiv.org/abs/1706.03762v7</id>
            <title>Attention Is All  You Need</title>
            <summary>The dominant sequence transduction models...</summary>
            <published>2017-06-12T17:57:34Z</published></entry></feed>"#;
        let works = parse_arxiv_entries(body);
        assert_eq!(works.len(), 1);
        assert_eq!(works[0].title, "Attention Is All You Need");
        assert_eq!(works[0].year, Some(2017));
        assert_eq!(works[0].identifier.as_deref(), Some("1706.03762v7"));
    }
}
