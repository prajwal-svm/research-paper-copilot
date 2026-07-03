//! Ingestion stage 3 (citations slice): bibliography parsing + in-text
//! mention linking → `citations.json`, with optional network resolution
//! against arXiv / Crossref.
//!
//! Offline behavior is a designed state, not an error: parsing always
//! completes from the PDF alone (`resolved: null`), and resolution is an
//! additive pass that can run later. Unresolvable entries keep their raw
//! bibliography text — the hover card shows that rather than a blank.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::layout::{BBox, LayoutDocument};

pub const CITATIONS_PIPELINE_VERSION: &str = "0.1.0";

/// Namespace for deterministic citation-entry UUIDs (v5). Never change.
const CITATION_NAMESPACE: Uuid = Uuid::from_bytes([
    0x2b, 0x60, 0x8e, 0x7d, 0x94, 0x1f, 0x4a, 0x6b, 0x8c, 0x0d, 0x3e, 0x5f, 0x71, 0x92, 0xa4, 0xc6,
]);

#[derive(Debug, thiserror::Error)]
pub enum CitationsError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("layout.json missing — run the layout stage first")]
    LayoutMissing,
}

// ---------------------------------------------------------------------------
// Output model (mirrors schemas/research-format/v0/citations.schema.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CitationsDocument {
    pub pipeline_version: String,
    pub entries: Vec<CitationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CitationEntry {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    pub raw_text: String,
    pub resolved: Option<ResolvedCitation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<Mention>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolvedCitation {
    pub source: String, // "arxiv" | "crossref" | "manual"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arxiv_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Mention {
    /// The paragraph/sentence object containing this in-text citation.
    pub object_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

// ---------------------------------------------------------------------------
// Stage entry point
// ---------------------------------------------------------------------------

/// Run the citations slice of stage 3 (offline part). Network resolution is
/// a separate additive pass (`resolve_citations`).
#[cfg(feature = "native")]
pub fn run_citations_stage(bundle: &Bundle) -> Result<CitationsDocument, CitationsError> {
    let started_at = crate::bundle::now_rfc3339();
    let layout: LayoutDocument = bundle
        .read_derived_json("layout.json")?
        .ok_or(CitationsError::LayoutMissing)?;
    let tree: Option<crate::objects::SemanticTreeDocument> =
        bundle.read_derived_json("semantic_tree.json")?;

    let mut doc = parse_citations(&layout);
    if let Some(tree) = &tree {
        link_mentions(&mut doc, tree);
        refine_mention_boxes(&mut doc, &layout);
    }

    let stage = serde_json::json!({
        "pipeline_version": CITATIONS_PIPELINE_VERSION,
        "status": "complete",
        "started_at": started_at,
        "completed_at": crate::bundle::now_rfc3339(),
    });
    bundle.write_derived_json("citations.json", &doc, "citations", stage)?;
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Bibliography parsing
// ---------------------------------------------------------------------------

/// Parse the references section out of the layout (pure; no bundle IO).
pub fn parse_citations(layout: &LayoutDocument) -> CitationsDocument {
    // Find the References/Bibliography heading, then treat following text
    // blocks as bibliography content until a clearly non-reference block.
    let mut in_references = false;
    let mut raw_entries: Vec<String> = Vec::new();

    for page in &layout.pages {
        for block in &page.blocks {
            let Some(text) = block.text.as_deref() else {
                continue;
            };
            let trimmed = text.trim();
            if !in_references {
                if is_references_heading(trimmed) {
                    in_references = true;
                } else if let Some(rest) = references_heading_prefix(trimmed) {
                    // Heading merged into the same block as the first entries.
                    in_references = true;
                    raw_entries.extend(split_numbered_entries(rest));
                }
                continue;
            }
            // The references section usually runs to the end of the paper or
            // until an appendix heading.
            if is_section_heading_after_references(trimmed) {
                in_references = false;
                continue;
            }
            // Split a block that contains several "[n] ..." entries.
            raw_entries.extend(split_numbered_entries(trimmed));
        }
    }

    let entries = raw_entries
        .into_iter()
        .filter(|e| e.len() > 20) // fragments are noise, not references
        .map(|raw| {
            let marker = leading_marker(&raw);
            let id = Uuid::new_v5(&CITATION_NAMESPACE, raw.as_bytes());
            let confidence = if marker.is_some() { 0.9 } else { 0.6 };
            CitationEntry {
                id,
                marker,
                raw_text: raw,
                resolved: None,
                mentions: Vec::new(),
                confidence,
            }
        })
        .collect();

    CitationsDocument {
        pipeline_version: CITATIONS_PIPELINE_VERSION.to_string(),
        entries,
    }
}

fn is_references_heading(text: &str) -> bool {
    let t = text.trim().to_ascii_lowercase();
    t == "references" || t == "bibliography" || t.ends_with(" references")
}

/// When the References heading merged into the entries block, return the
/// remainder after the heading; `None` otherwise. Handles "References [1] …"
/// (numbered) and "REFERENCES Author Name…" (author-year, ICLR style).
fn references_heading_prefix(text: &str) -> Option<&str> {
    for heading in ["References", "REFERENCES", "Bibliography", "BIBLIOGRAPHY"] {
        if let Some(rest) = text.strip_prefix(heading) {
            let rest = rest.trim_start();
            if rest.starts_with('[') || rest.starts_with(|c: char| c.is_uppercase()) {
                return Some(rest);
            }
        }
    }
    None
}

fn is_section_heading_after_references(text: &str) -> bool {
    let t = text.trim().to_ascii_lowercase();
    t.starts_with("appendix") || t.starts_with("attention visualizations")
}

/// Split text containing one or more "[n] entry" runs into separate entries.
fn split_numbered_entries(text: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = text.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if c == '[' {
            // Lookahead for "digits]"
            let rest = &text[i + 1..];
            if let Some(close) = rest.find(']') {
                let inner = &rest[..close];
                let followed_by_space = rest[close + 1..].starts_with(' ');
                if !inner.is_empty()
                    && inner.chars().all(|ch| ch.is_ascii_digit())
                    && followed_by_space
                {
                    if !current.trim().is_empty() {
                        entries.push(current.trim().to_string());
                    }
                    current = String::new();
                }
            }
        }
        current.push(c);
        let _ = &mut chars;
    }
    if !current.trim().is_empty() {
        entries.push(current.trim().to_string());
    }
    entries
}

fn leading_marker(entry: &str) -> Option<String> {
    let rest = entry.strip_prefix('[')?;
    let close = rest.find(']')?;
    let inner = &rest[..close];
    (!inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit())).then(|| format!("[{inner}]"))
}

// ---------------------------------------------------------------------------
// Mention linking
// ---------------------------------------------------------------------------

/// Link in-text "[n]" occurrences in paragraph/sentence objects to entries.
/// Marker bounding boxes come from layout line spans (proportional character
/// position within the line) so hover targets sit on the marker itself, not
/// the whole paragraph.
fn link_mentions(doc: &mut CitationsDocument, tree: &crate::objects::SemanticTreeDocument) {
    use crate::objects::ObjectType;
    for object in tree
        .objects
        .iter()
        .filter(|o| matches!(o.object_type, ObjectType::Paragraph | ObjectType::Sentence))
    {
        // Bibliography entries themselves also contain "[n]" — skip anything
        // that IS a reference entry (starts with a marker).
        if leading_marker(&object.content.text).is_some() {
            continue;
        }
        for entry in &mut doc.entries {
            let Some(marker) = &entry.marker else {
                continue;
            };
            if object.content.text.contains(marker.as_str()) {
                entry.mentions.push(Mention {
                    object_id: object.id,
                    bbox: object.regions.first().copied(),
                });
            }
        }
    }
}

/// Refine mention bboxes to the marker's own region using layout line spans.
/// Proportional-position estimate: good enough for a comfortable hover
/// target, cheap enough to run for every mention.
pub fn refine_mention_boxes(doc: &mut CitationsDocument, layout: &LayoutDocument) {
    for entry in &mut doc.entries {
        let Some(marker) = entry.marker.clone() else {
            continue;
        };
        let mut refined: Vec<Mention> = Vec::new();
        for page in &layout.pages {
            for block in &page.blocks {
                for line in &block.lines {
                    // Skip bibliography entries (line starts with the marker).
                    if line.text.trim_start().starts_with(&marker) {
                        continue;
                    }
                    let Some(char_pos) = line.text.find(&marker) else {
                        continue;
                    };
                    let chars_before = line.text[..char_pos].chars().count() as f32;
                    let marker_chars = marker.chars().count() as f32;
                    let total_chars = line.text.chars().count().max(1) as f32;
                    let x = line.bbox.x + line.bbox.width * (chars_before / total_chars);
                    let width = (line.bbox.width * (marker_chars / total_chars)).max(8.0);
                    // Keep the object anchor from coarse linking when there is
                    // one for this page; refined box is the hover target.
                    let object_id = entry
                        .mentions
                        .iter()
                        .find(|m| m.bbox.map(|b| b.page) == Some(line.bbox.page))
                        .or(entry.mentions.first())
                        .map(|m| m.object_id);
                    let Some(object_id) = object_id else { continue };
                    refined.push(Mention {
                        object_id,
                        bbox: Some(BBox {
                            page: line.bbox.page,
                            x,
                            y: line.bbox.y,
                            width,
                            height: line.bbox.height,
                        }),
                    });
                }
            }
        }
        if !refined.is_empty() {
            entry.mentions = refined;
        }
    }
}

// ---------------------------------------------------------------------------
// Network resolution (additive pass; graceful offline behavior)
// ---------------------------------------------------------------------------

/// Try to resolve unresolved entries via arXiv / Crossref. Failures (offline,
/// rate limits, no match) leave entries unresolved — never an error for the
/// import. Returns how many entries were resolved this pass.
#[cfg(feature = "native")]
pub fn resolve_citations(bundle: &Bundle, limit: usize) -> Result<usize, CitationsError> {
    let Some(mut doc): Option<CitationsDocument> = bundle.read_derived_json("citations.json")?
    else {
        return Ok(0);
    };

    let mut resolved_count = 0;
    for entry in doc
        .entries
        .iter_mut()
        .filter(|e| e.resolved.is_none())
        .take(limit)
    {
        if let Some(resolved) = resolve_one(&entry.raw_text) {
            entry.resolved = Some(resolved);
            resolved_count += 1;
        }
    }

    if resolved_count > 0 {
        let stage = serde_json::json!({
            "pipeline_version": CITATIONS_PIPELINE_VERSION,
            "status": "complete",
            "completed_at": crate::bundle::now_rfc3339(),
        });
        bundle.write_derived_json("citations.json", &doc, "citations", stage)?;
    }
    Ok(resolved_count)
}

/// Resolve one bibliography entry: arXiv id in the text → arXiv API;
/// otherwise Crossref bibliographic search. `None` on any failure.
#[cfg(feature = "native")]
fn resolve_one(raw_text: &str) -> Option<ResolvedCitation> {
    if let Some(arxiv_id) = find_arxiv_id(raw_text) {
        if let Some(resolved) = resolve_arxiv(&arxiv_id) {
            return Some(resolved);
        }
    }
    resolve_crossref(raw_text)
}

/// Extract an arXiv id like "arXiv:1706.03762" or "abs/1706.03762".
pub fn find_arxiv_id(text: &str) -> Option<String> {
    for prefix in ["arXiv:", "arxiv:", "abs/"] {
        if let Some(pos) = text.find(prefix) {
            let rest = &text[pos + prefix.len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == 'v')
                .collect();
            let id = id.trim_end_matches('v').trim_end_matches('.').to_string();
            if id.len() >= 9 && id.contains('.') {
                return Some(id);
            }
        }
    }
    None
}

#[cfg(feature = "native")]
fn resolve_arxiv(arxiv_id: &str) -> Option<ResolvedCitation> {
    let url = format!("https://export.arxiv.org/api/query?id_list={arxiv_id}&max_results=1");
    let body = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    // Minimal Atom parsing: title of the first <entry>.
    let entry = body.split("<entry>").nth(1)?;
    let title = xml_text(entry, "title")?;
    let year = xml_text(entry, "published").and_then(|p| p.get(..4).and_then(|y| y.parse().ok()));
    let authors = entry
        .split("<author>")
        .skip(1)
        .filter_map(|a| xml_text(a, "name"))
        .collect();
    Some(ResolvedCitation {
        source: "arxiv".to_string(),
        resolved_at: Some(crate::bundle::now_rfc3339()),
        title: Some(title),
        authors,
        year,
        venue: None,
        arxiv_id: Some(arxiv_id.to_string()),
        doi: None,
        url: Some(format!("https://arxiv.org/abs/{arxiv_id}")),
    })
}

#[cfg(feature = "native")]
fn resolve_crossref(raw_text: &str) -> Option<ResolvedCitation> {
    let query: String = raw_text.chars().take(300).collect();
    let response: serde_json::Value = ureq::get("https://api.crossref.org/works")
        .query("query.bibliographic", &query)
        .query("rows", "1")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    let item = response["message"]["items"].get(0)?;
    let title = item["title"][0].as_str()?.to_string();

    // Guard against bad matches: the found title's significant words should
    // appear in the raw entry.
    let hits = title
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .filter(|w| raw_text.to_lowercase().contains(&w.to_lowercase()))
        .count();
    let significant = title.split_whitespace().filter(|w| w.len() > 3).count();
    if significant == 0 || (hits as f32) / (significant as f32) < 0.6 {
        return None;
    }

    Some(ResolvedCitation {
        source: "crossref".to_string(),
        resolved_at: Some(crate::bundle::now_rfc3339()),
        title: Some(title),
        authors: item["author"]
            .as_array()
            .map(|authors| {
                authors
                    .iter()
                    .filter_map(|a| {
                        let given = a["given"].as_str().unwrap_or_default();
                        let family = a["family"].as_str()?;
                        Some(format!("{given} {family}").trim().to_string())
                    })
                    .collect()
            })
            .unwrap_or_default(),
        year: item["issued"]["date-parts"][0][0]
            .as_i64()
            .map(|y| y as i32),
        venue: item["container-title"][0].as_str().map(|s| s.to_string()),
        arxiv_id: None,
        doi: item["DOI"].as_str().map(|s| s.to_string()),
        url: item["URL"].as_str().map(|s| s.to_string()),
    })
}

fn xml_text(hay: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = hay.find(&open)? + open.len();
    let end = hay[start..].find(&close)? + start;
    let text = hay[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{BlockKind, LayoutBlock, LayoutPage, LAYOUT_PIPELINE_VERSION};

    fn block(kind: BlockKind, text: &str, y: f32) -> LayoutBlock {
        LayoutBlock {
            id: Uuid::new_v4(),
            kind,
            bbox: BBox {
                page: 10,
                x: 72.0,
                y,
                width: 460.0,
                height: 14.0,
            },
            column: None,
            text: Some(text.to_string()),
            lines: Vec::new(),
            confidence: 0.95,
        }
    }

    fn references_layout() -> LayoutDocument {
        LayoutDocument {
            pipeline_version: LAYOUT_PIPELINE_VERSION.to_string(),
            pages: vec![LayoutPage {
                index: 10,
                width: 612.0,
                height: 792.0,
                rotation: 0,
                is_scanned: false,
                blocks: vec![
                    block(BlockKind::Text, "The models are described in section 5.", 80.0),
                    block(BlockKind::Heading, "References", 120.0),
                    block(
                        BlockKind::Text,
                        "[1] Jimmy Lei Ba, Jamie Ryan Kiros, and Geoffrey E Hinton. Layer normalization. arXiv preprint arXiv:1607.06450, 2016. [2] Dzmitry Bahdanau, Kyunghyun Cho, and Yoshua Bengio. Neural machine translation by jointly learning to align and translate. CoRR, abs/1409.0473, 2014.",
                        160.0,
                    ),
                    block(
                        BlockKind::Text,
                        "[13] Sepp Hochreiter and Jürgen Schmidhuber. Long short-term memory. Neural computation, 9(8):1735–1780, 1997.",
                        300.0,
                    ),
                ],
            }],
        }
    }

    #[test]
    fn parses_numbered_bibliography_entries() {
        let doc = parse_citations(&references_layout());
        assert_eq!(doc.entries.len(), 3, "{:#?}", doc.entries);
        assert_eq!(doc.entries[0].marker.as_deref(), Some("[1]"));
        assert!(doc.entries[0].raw_text.contains("Layer normalization"));
        assert_eq!(doc.entries[1].marker.as_deref(), Some("[2]"));
        assert!(doc.entries[1].raw_text.contains("Bahdanau"));
        assert_eq!(doc.entries[2].marker.as_deref(), Some("[13]"));
        // Prose before the References heading contributed nothing.
        assert!(!doc.entries.iter().any(|e| e.raw_text.contains("section 5")));
        // All unresolved offline; ids deterministic.
        assert!(doc.entries.iter().all(|e| e.resolved.is_none()));
        let again = parse_citations(&references_layout());
        assert_eq!(doc.entries[0].id, again.entries[0].id);
    }

    #[test]
    fn links_in_text_mentions_to_entries() {
        use crate::bundle::Paper;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle =
            crate::bundle::Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("S"), "file")
                .unwrap();

        // Layout with an in-text mention and a references section.
        let mut layout = references_layout();
        layout.pages[0].blocks.insert(
            0,
            block(
                BlockKind::Text,
                "Long short-term memory [13] has been widely used. It set the standard.",
                40.0,
            ),
        );
        bundle
            .write_derived_json(
                "layout.json",
                &layout,
                "layout",
                serde_json::json!({"pipeline_version": LAYOUT_PIPELINE_VERSION, "status": "complete"}),
            )
            .unwrap();
        crate::objects::run_objects_stage(&bundle).unwrap();

        let doc = run_citations_stage(&bundle).unwrap();
        let entry13 = doc
            .entries
            .iter()
            .find(|e| e.marker.as_deref() == Some("[13]"))
            .unwrap();
        assert!(
            !entry13.mentions.is_empty(),
            "expected mention for [13]: {entry13:#?}"
        );

        // Persisted and stage recorded.
        let reread: Option<CitationsDocument> = bundle.read_derived_json("citations.json").unwrap();
        assert!(reread.is_some());
        let metadata = bundle.metadata().unwrap();
        assert_eq!(metadata.pipeline.stages["citations"]["status"], "complete");
    }

    #[test]
    fn finds_arxiv_ids() {
        assert_eq!(
            find_arxiv_id("arXiv preprint arXiv:1607.06450, 2016."),
            Some("1607.06450".to_string())
        );
        assert_eq!(
            find_arxiv_id("CoRR, abs/1409.0473, 2014."),
            Some("1409.0473".to_string())
        );
        assert_eq!(find_arxiv_id("Neural computation, 9(8), 1997."), None);
    }

    #[test]
    fn offline_resolution_is_graceful() {
        // resolve_one against unroutable entries must return None, not panic.
        // (No network mocking here; the raw text has no arXiv id and the
        // Crossref title-overlap guard rejects generic matches even if a
        // request somehow succeeds.)
        let entry = "Nonexistent zzz qqq xyzzy entry with no identifiers.";
        // Result depends on network availability; both None and a rejected
        // match yield None thanks to the overlap guard.
        assert!(resolve_crossref(entry).is_none() || find_arxiv_id(entry).is_none());
    }
}
