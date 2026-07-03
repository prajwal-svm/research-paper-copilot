//! Ingestion stage 2: object extraction → `semantic_tree.json`.
//!
//! Turns stage-1 layout blocks into the object model: section objects (from
//! headings + numbering patterns), paragraph objects (from text blocks), and
//! sentence objects (boundary splitting), each with a stable UUID, regions,
//! content hash, relationships, and confidence.
//!
//! Object UUIDs are deterministic (UUID v5 over type + content + occurrence),
//! so re-running the same pipeline over the same paper yields the same ids —
//! user data anchored to them survives re-ingestion for free. When content
//! changes, the content hash changes too and anchors are surfaced for
//! reattachment rather than silently dropped.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::{sha256_bytes, Bundle};
use crate::layout::{BBox, BlockKind, LayoutDocument};

pub const OBJECTS_PIPELINE_VERSION: &str = "0.1.0";

/// Namespace for deterministic object UUIDs (v5). Never change this.
const OBJECT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x8f, 0x1d, 0x1b, 0x0a, 0x6e, 0x3c, 0x45, 0x21, 0x9a, 0x54, 0xd2, 0x7b, 0x11, 0x8e, 0x42, 0x99,
]);

#[derive(Debug, thiserror::Error)]
pub enum ObjectsError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("layout.json missing — run the layout stage first")]
    LayoutMissing,
}

// ---------------------------------------------------------------------------
// Output model (mirrors schemas/research-format/v0/semantic_tree.schema.json
// and objects.schema.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SemanticTreeDocument {
    pub pipeline_version: String,
    pub objects: Vec<Object>,
    pub tree: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TreeNode {
    pub object: Uuid,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Object {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub object_type: ObjectType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<BBox>,
    pub content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<Relationship>,
    pub embedding: Option<EmbeddingRef>,
    pub content_hash: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Content {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObjectType {
    Section,
    Paragraph,
    Sentence,
    Equation,
    Figure,
    Table,
    Citation,
    Definition,
    Algorithm,
    Experiment,
    Dataset,
    Metric,
    Claim,
    Limitation,
    FutureWork,
    Selection,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Relationship {
    #[serde(rename = "type")]
    pub relationship_type: RelationshipType,
    pub target: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipType {
    BelongsTo,
    Contains,
    References,
    ReferencedBy,
    DependsOn,
    Defines,
    Cites,
    Follows,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EmbeddingRef {
    pub index: u32,
}

// ---------------------------------------------------------------------------
// Stage entry point
// ---------------------------------------------------------------------------

/// Run stage 2 on a bundle: read `layout.json`, extract objects, write
/// `semantic_tree.json`, record the stage in `metadata.json`.
#[cfg(feature = "native")]
pub fn run_objects_stage(bundle: &Bundle) -> Result<SemanticTreeDocument, ObjectsError> {
    let started_at = crate::bundle::now_rfc3339();
    let layout: LayoutDocument = bundle
        .read_derived_json("layout.json")?
        .ok_or(ObjectsError::LayoutMissing)?;

    let doc = build_semantic_tree(&layout);

    let stage = serde_json::json!({
        "pipeline_version": OBJECTS_PIPELINE_VERSION,
        "status": "complete",
        "started_at": started_at,
        "completed_at": crate::bundle::now_rfc3339(),
    });
    bundle.write_derived_json("semantic_tree.json", &doc, "objects", stage)?;
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Build the object model from a layout document (pure; no bundle IO).
#[cfg(feature = "native")]
pub fn build_semantic_tree(layout: &LayoutDocument) -> SemanticTreeDocument {
    let mut builder = Builder::default();

    for page in &layout.pages {
        for (i, block) in page.blocks.iter().enumerate() {
            let Some(text) = block.text.as_deref() else {
                continue; // graphics regions carry no text; matched via captions
            };
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            // Bare "(n)" markers are absorbed into their equation object.
            if crate::equations::equation_number_marker(text).is_some() {
                continue;
            }
            if matches!(block.kind, BlockKind::Header | BlockKind::Footer) {
                continue;
            }

            if block.kind == BlockKind::Heading
                || (heading_depth(text).is_some() && text.chars().count() < 90)
            {
                builder.push_section(text, block.bbox, block.confidence);
            } else if let Some(number) = caption_number(text, &["Figure", "Fig."]) {
                // Attach the nearest graphics region on the page (captions sit
                // directly below or above their figure).
                let region = nearest_graphics_region(page, block.bbox);
                builder.push_figure(text, region.unwrap_or(block.bbox), block.bbox, number);
            } else if let Some(number) = caption_number(text, &["Table"]) {
                builder.push_table(text, block.bbox, number);
            } else if crate::equations::is_display_equation(text) {
                // Right-margin "(n)" numbering sits in a nearby block.
                let number = page
                    .blocks
                    .iter()
                    .skip(i + 1)
                    .take(3)
                    .filter_map(|b| {
                        b.text
                            .as_deref()
                            .and_then(crate::equations::equation_number_marker)
                    })
                    .next();
                builder.push_equation(text, block.bbox, block.confidence, number);
            } else {
                builder.push_paragraph(text, block.bbox, block.confidence);
            }
        }
    }

    builder.finish()
}

/// Parse a caption like "Figure 2: ..." / "Table 1. ..." → the number, for
/// any of the given keywords. Requires a following ':' or '.' (or end) so
/// prose like "Table 3 shows..." doesn't match.
#[cfg(feature = "native")]
fn caption_number(text: &str, keywords: &[&str]) -> Option<String> {
    for keyword in keywords {
        let Some(rest) = text.strip_prefix(keyword) else {
            continue;
        };
        let rest = rest.trim_start();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        let after = rest[digits.len()..].trim_start();
        if after.is_empty() || after.starts_with(':') || after.starts_with('.') {
            return Some(digits);
        }
    }
    None
}

/// The graphics region on this page nearest (vertically) to a caption.
#[cfg(feature = "native")]
fn nearest_graphics_region(page: &crate::layout::LayoutPage, caption: BBox) -> Option<BBox> {
    page.blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Figure && b.text.is_none())
        .map(|b| b.bbox)
        .min_by(|a, b| {
            let da = vertical_distance(*a, caption);
            let db = vertical_distance(*b, caption);
            da.total_cmp(&db)
        })
        .filter(|region| vertical_distance(*region, caption) < 200.0)
}

fn vertical_distance(a: BBox, b: BBox) -> f32 {
    let (a0, a1) = (a.y, a.y + a.height);
    let (b0, b1) = (b.y, b.y + b.height);
    if a1 < b0 {
        b0 - a1
    } else if b1 < a0 {
        a0 - b1
    } else {
        0.0
    }
}

/// Depth of a numbered heading ("3 Model" → 1, "3.2 Attention" → 2,
/// "3.2.1 Scaled…" → 3); `None` when the text isn't a numbered heading.
fn heading_depth(text: &str) -> Option<usize> {
    let (prefix, rest) = text.split_once(' ')?;
    if rest.trim().is_empty() || !rest.trim_start().starts_with(|c: char| c.is_uppercase()) {
        return None;
    }
    let parts: Vec<&str> = prefix.trim_end_matches('.').split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        .then_some(parts.len())
}

/// Split paragraph text into sentences (naive boundary detection tuned to
/// scientific prose; abbreviations like "et al." and "Fig." are respected).
fn split_sentences(text: &str) -> Vec<String> {
    const ABBREVIATIONS: [&str; 8] = ["et al", "e.g", "i.e", "Fig", "Eq", "Sec", "cf", "vs"];
    let chars: Vec<char> = text.chars().collect();
    let mut sentences = Vec::new();
    let mut start = 0;

    for i in 0..chars.len() {
        if !matches!(chars[i], '.' | '!' | '?') {
            continue;
        }
        let next_is_boundary = chars.get(i + 1).is_none_or(|c| c.is_whitespace());
        let after = chars
            .get(i + 2)
            .copied()
            .or_else(|| chars.get(i + 1).copied());
        let next_starts_sentence = after.is_none_or(|c| c.is_uppercase() || c == '(');
        let preceding: String = chars[start..i].iter().collect();
        let is_abbreviation = ABBREVIATIONS
            .iter()
            .any(|abbr| preceding.trim_end().ends_with(abbr));
        let is_decimal = chars.get(i + 1).is_some_and(|c| c.is_ascii_digit());

        if next_is_boundary && next_starts_sentence && !is_abbreviation && !is_decimal {
            let sentence: String = chars[start..=i].iter().collect();
            let sentence = sentence.trim().to_string();
            if !sentence.is_empty() {
                sentences.push(sentence);
            }
            start = i + 1;
        }
    }
    let tail: String = chars[start..].iter().collect();
    let tail = tail.trim().to_string();
    if !tail.is_empty() {
        sentences.push(tail);
    }
    sentences
}

#[derive(Default)]
struct Builder {
    objects: Vec<Object>,
    tree: Vec<TreeNode>,
    /// Stack of (depth, index into a nodes arena path) for section nesting.
    section_stack: Vec<(usize, Uuid)>,
    /// UUID occurrence counter to keep deterministic ids unique on duplicates.
    seen: HashMap<String, u32>,
    current_section: Option<Uuid>,
}

impl Builder {
    fn object_id(&mut self, object_type: &str, text: &str) -> Uuid {
        let key = format!("{object_type}\u{1f}{text}");
        let occurrence = self.seen.entry(key.clone()).or_insert(0);
        let name = format!("{key}\u{1f}{occurrence}");
        *occurrence += 1;
        Uuid::new_v5(&OBJECT_NAMESPACE, name.as_bytes())
    }

    fn push_section(&mut self, text: &str, bbox: BBox, confidence: f32) {
        let depth = heading_depth(text).unwrap_or(1);
        let id = self.object_id("section", text);
        self.objects.push(Object {
            id,
            object_type: ObjectType::Section,
            regions: vec![bbox],
            content: Content {
                text: text.to_string(),
                latex: None,
                caption: None,
            },
            semantic_label: Some(format!("Section — {text}")),
            relationships: Vec::new(),
            embedding: None,
            content_hash: sha256_bytes(text.as_bytes()),
            confidence,
        });

        // Pop deeper/equal sections, then attach under the surviving parent.
        while self.section_stack.last().is_some_and(|(d, _)| *d >= depth) {
            self.section_stack.pop();
        }
        let parent = self.section_stack.last().map(|(_, id)| *id);
        self.attach_node(id, parent);
        if let Some(parent_id) = parent {
            self.link(id, RelationshipType::BelongsTo, parent_id);
            self.link(parent_id, RelationshipType::Contains, id);
        }
        self.section_stack.push((depth, id));
        self.current_section = Some(id);
    }

    fn push_equation(&mut self, text: &str, bbox: BBox, confidence: f32, number: Option<String>) {
        let id = self.object_id("equation", text);
        let section = self.current_section;
        let label = match &number {
            Some(n) => format!("Equation {n}"),
            None => "Equation".to_string(),
        };
        self.objects.push(Object {
            id,
            object_type: ObjectType::Equation,
            regions: vec![bbox],
            content: Content {
                text: text.to_string(),
                latex: None,
                caption: None,
            },
            semantic_label: Some(label),
            relationships: Vec::new(),
            embedding: None,
            content_hash: sha256_bytes(text.as_bytes()),
            // Numbered display equations are near-certain; unnumbered
            // candidates are flagged for the low-confidence UX path.
            confidence: confidence.min(if number.is_some() { 0.9 } else { 0.65 }),
        });
        self.attach_node(id, section);
        if let Some(section_id) = section {
            self.link(id, RelationshipType::BelongsTo, section_id);
            self.link(section_id, RelationshipType::Contains, id);
        }
    }

    fn push_figure(&mut self, caption: &str, region: BBox, caption_region: BBox, number: String) {
        let id = self.object_id("figure", caption);
        let section = self.current_section;
        let mut regions = vec![region];
        if region != caption_region {
            regions.push(caption_region);
        }
        self.objects.push(Object {
            id,
            object_type: ObjectType::Figure,
            regions,
            content: Content {
                text: caption.to_string(),
                latex: None,
                caption: Some(caption.to_string()),
            },
            semantic_label: Some(format!("Figure {number}")),
            relationships: Vec::new(),
            embedding: None,
            content_hash: sha256_bytes(caption.as_bytes()),
            confidence: 0.8,
        });
        self.attach_node(id, section);
        if let Some(section_id) = section {
            self.link(id, RelationshipType::BelongsTo, section_id);
            self.link(section_id, RelationshipType::Contains, id);
        }
    }

    fn push_table(&mut self, caption: &str, bbox: BBox, number: String) {
        let id = self.object_id("table", caption);
        let section = self.current_section;
        self.objects.push(Object {
            id,
            object_type: ObjectType::Table,
            regions: vec![bbox],
            content: Content {
                text: caption.to_string(),
                latex: None,
                caption: Some(caption.to_string()),
            },
            semantic_label: Some(format!("Table {number}")),
            relationships: Vec::new(),
            embedding: None,
            content_hash: sha256_bytes(caption.as_bytes()),
            confidence: 0.8,
        });
        self.attach_node(id, section);
        if let Some(section_id) = section {
            self.link(id, RelationshipType::BelongsTo, section_id);
            self.link(section_id, RelationshipType::Contains, id);
        }
    }

    fn push_paragraph(&mut self, text: &str, bbox: BBox, confidence: f32) {
        let id = self.object_id("paragraph", text);
        let section = self.current_section;
        self.objects.push(Object {
            id,
            object_type: ObjectType::Paragraph,
            regions: vec![bbox],
            content: Content {
                text: text.to_string(),
                latex: None,
                caption: None,
            },
            semantic_label: None,
            relationships: Vec::new(),
            embedding: None,
            content_hash: sha256_bytes(text.as_bytes()),
            confidence,
        });
        self.attach_node(id, section);
        if let Some(section_id) = section {
            self.link(id, RelationshipType::BelongsTo, section_id);
            self.link(section_id, RelationshipType::Contains, id);
        }

        // Sentences: only worth emitting when the paragraph splits.
        let sentences = split_sentences(text);
        if sentences.len() > 1 {
            for sentence in sentences {
                let sid = self.object_id("sentence", &sentence);
                self.objects.push(Object {
                    id: sid,
                    object_type: ObjectType::Sentence,
                    // v0.1 granularity: sentences inherit the paragraph region.
                    regions: vec![bbox],
                    content: Content {
                        text: sentence.clone(),
                        latex: None,
                        caption: None,
                    },
                    semantic_label: None,
                    relationships: Vec::new(),
                    embedding: None,
                    content_hash: sha256_bytes(sentence.as_bytes()),
                    confidence: confidence * 0.9,
                });
                self.attach_node(sid, Some(id));
                self.link(sid, RelationshipType::BelongsTo, id);
                self.link(id, RelationshipType::Contains, sid);
            }
        }
    }

    fn link(&mut self, from: Uuid, relationship_type: RelationshipType, target: Uuid) {
        if let Some(object) = self.objects.iter_mut().find(|o| o.id == from) {
            object.relationships.push(Relationship {
                relationship_type,
                target,
                confidence: None,
            });
        }
    }

    fn attach_node(&mut self, id: Uuid, parent: Option<Uuid>) {
        let node = TreeNode {
            object: id,
            children: Vec::new(),
        };
        match parent.and_then(|p| find_node(&mut self.tree, p)) {
            Some(parent_node) => parent_node.children.push(node),
            None => self.tree.push(node),
        }
    }

    fn finish(self) -> SemanticTreeDocument {
        SemanticTreeDocument {
            pipeline_version: OBJECTS_PIPELINE_VERSION.to_string(),
            objects: self.objects,
            tree: self.tree,
        }
    }
}

fn find_node(nodes: &mut [TreeNode], id: Uuid) -> Option<&mut TreeNode> {
    for node in nodes {
        if node.object == id {
            return Some(node);
        }
        if let Some(found) = find_node(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;
    use crate::layout::{LayoutBlock, LayoutPage, LAYOUT_PIPELINE_VERSION};

    fn block(kind: BlockKind, text: &str, y: f32) -> LayoutBlock {
        LayoutBlock {
            id: Uuid::new_v4(),
            kind,
            bbox: BBox {
                page: 0,
                x: 72.0,
                y,
                width: 400.0,
                height: 14.0,
            },
            column: None,
            text: Some(text.to_string()),
            lines: Vec::new(),
            confidence: 0.95,
        }
    }

    fn sample_layout() -> LayoutDocument {
        LayoutDocument {
            pipeline_version: LAYOUT_PIPELINE_VERSION.to_string(),
            pages: vec![LayoutPage {
                index: 0,
                width: 612.0,
                height: 792.0,
                rotation: 0,
                is_scanned: false,
                blocks: vec![
                    block(BlockKind::Heading, "3 Model Architecture", 100.0),
                    block(
                        BlockKind::Text,
                        "Most models have an encoder. The encoder maps symbols to representations.",
                        130.0,
                    ),
                    block(BlockKind::Text, "3.2 Attention", 200.0),
                    block(
                        BlockKind::Text,
                        "An attention function maps a query and key-value pairs to an output.",
                        230.0,
                    ),
                    block(BlockKind::Footer, "4", 780.0),
                ],
            }],
        }
    }

    #[test]
    fn builds_sections_paragraphs_sentences_with_relationships() {
        let doc = build_semantic_tree(&sample_layout());

        let sections: Vec<_> = doc
            .objects
            .iter()
            .filter(|o| o.object_type == ObjectType::Section)
            .collect();
        assert_eq!(sections.len(), 2, "{:#?}", sections);
        assert!(sections[0].content.text.contains("Model Architecture"));
        assert!(sections[1].content.text.contains("Attention"));

        // "3.2 Attention" (depth 2) nests under "3 Model Architecture" (depth 1).
        assert!(sections[1]
            .relationships
            .iter()
            .any(|r| r.relationship_type == RelationshipType::BelongsTo
                && r.target == sections[0].id));

        // Footer produced no object.
        assert!(!doc.objects.iter().any(|o| o.content.text == "4"));

        // Two-sentence paragraph produced sentence objects linked to it.
        let paragraph = doc
            .objects
            .iter()
            .find(|o| {
                o.object_type == ObjectType::Paragraph && o.content.text.starts_with("Most models")
            })
            .unwrap();
        let sentences: Vec<_> = doc
            .objects
            .iter()
            .filter(|o| {
                o.object_type == ObjectType::Sentence
                    && o.relationships.iter().any(|r| r.target == paragraph.id)
            })
            .collect();
        assert_eq!(sentences.len(), 2, "{:#?}", sentences);

        // Tree nests: section 3 → [paragraph, section 3.2 → [paragraph]].
        assert_eq!(doc.tree.len(), 1);
        assert_eq!(doc.tree[0].object, sections[0].id);
        assert!(doc.tree[0]
            .children
            .iter()
            .any(|n| n.object == sections[1].id));
    }

    #[test]
    fn object_ids_are_deterministic_across_reparses() {
        let a = build_semantic_tree(&sample_layout());
        let b = build_semantic_tree(&sample_layout());
        let ids_a: Vec<Uuid> = a.objects.iter().map(|o| o.id).collect();
        let ids_b: Vec<Uuid> = b.objects.iter().map(|o| o.id).collect();
        assert_eq!(ids_a, ids_b);

        // Duplicate content still gets unique ids.
        let mut layout = sample_layout();
        layout.pages[0]
            .blocks
            .push(block(BlockKind::Text, "3.2 Attention", 500.0));
        let c = build_semantic_tree(&layout);
        let all: std::collections::HashSet<Uuid> = c.objects.iter().map(|o| o.id).collect();
        assert_eq!(all.len(), c.objects.len(), "duplicate object ids");
    }

    #[test]
    fn sentence_splitting_respects_abbreviations() {
        let sentences = split_sentences(
            "Attention was proposed by Bahdanau et al. in 2014. It changed translation. See Fig. 2 for details.",
        );
        assert_eq!(
            sentences,
            vec![
                "Attention was proposed by Bahdanau et al. in 2014.",
                "It changed translation.",
                "See Fig. 2 for details.",
            ]
        );
    }

    #[test]
    fn stage_reads_layout_and_writes_semantic_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("Sample"), "file").unwrap();

        // Without layout.json the stage refuses.
        assert!(matches!(
            run_objects_stage(&bundle),
            Err(ObjectsError::LayoutMissing)
        ));

        bundle
            .write_derived_json(
                "layout.json",
                &sample_layout(),
                "layout",
                serde_json::json!({"pipeline_version": LAYOUT_PIPELINE_VERSION, "status": "complete"}),
            )
            .unwrap();

        let doc = run_objects_stage(&bundle).unwrap();
        assert!(!doc.objects.is_empty());

        let reread: Option<SemanticTreeDocument> =
            bundle.read_derived_json("semantic_tree.json").unwrap();
        assert_eq!(reread.unwrap().objects.len(), doc.objects.len());

        let metadata = bundle.metadata().unwrap();
        assert_eq!(metadata.pipeline.stages["objects"]["status"], "complete");
        assert!(metadata.content_hashes.contains_key("semantic_tree.json"));
    }
}
