//! Ingestion stage 3 (equations slice): equation detection → `equations/`.
//!
//! Spike outcome (see design.md): display equations are detected
//! deterministically from layout (math-symbol density, isolation, and
//! "(n)" equation-number markers) — no bundled OCR model. Each equation gets
//! an artifact in `equations/<uuid>.json` holding its region, raw unicode
//! text, number, and a `latex: null` slot that the optional LLM-enrichment
//! stage (or a future local model) fills in. The original region is always
//! recoverable, which is what the low-confidence UX contract requires.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::layout::BBox;
use crate::objects::{ObjectType, SemanticTreeDocument};

pub const EQUATIONS_PIPELINE_VERSION: &str = "0.1.0";

#[derive(Debug, thiserror::Error)]
pub enum EquationsError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("semantic_tree.json missing — run the objects stage first")]
    TreeMissing,
}

/// Per-equation artifact stored at `equations/<object_id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquationArtifact {
    pub object_id: Uuid,
    pub region: BBox,
    /// Raw unicode text as extracted from the PDF text layer (often mangled
    /// for heavy math — that's expected; the region is the source of truth).
    pub raw_text: String,
    /// Paper-assigned equation number, e.g. "1" for "(1)", when detected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    /// LaTeX form; `null` until enrichment (LLM stage or future local model).
    pub latex: Option<String>,
    /// Where `latex` came from: "llm_enrichment", "local_model", "manual".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latex_source: Option<String>,
    pub confidence: f32,
}

/// Is this block a display equation? Deterministic heuristics tuned to
/// arXiv-style papers; precision is favoured over recall (a missed equation
/// still reads fine as a paragraph; a false positive gets equation actions
/// on prose).
pub fn is_display_equation(text: &str) -> bool {
    let trimmed = text.trim();
    let char_count = trimmed.chars().count();
    if !(3..=300).contains(&char_count) {
        return false;
    }
    // Prose filter: long sentence-like text with few operators is not a
    // display equation even if it contains '='.
    let words = trimmed.split_whitespace().count();
    let math_chars = trimmed
        .chars()
        .filter(|c| {
            matches!(
                c,
                '=' | '+'
                    | '−'
                    | '-'
                    | '×'
                    | '·'
                    | '∑'
                    | '∏'
                    | '∫'
                    | '√'
                    | '∂'
                    | '∇'
                    | '≈'
                    | '≤'
                    | '≥'
                    | '∈'
                    | '∞'
                    | '^'
                    | '/'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '|'
            ) || ('α'..='ω').contains(c)
                || ('Α'..='Ω').contains(c)
                || c.is_ascii_digit()
        })
        .count();
    let density = math_chars as f32 / char_count as f32;

    trimmed.contains('=') && (density > 0.14 || words <= 6)
}

/// Does this block look like a bare equation-number marker, e.g. "(1)"?
pub fn equation_number_marker(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    (!inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit())).then(|| inner.to_string())
}

/// Run the equations stage: read `semantic_tree.json`, write one artifact per
/// equation object into `equations/`, record the stage.
pub fn run_equations_stage(bundle: &Bundle) -> Result<Vec<EquationArtifact>, EquationsError> {
    let started_at = crate::bundle::now_rfc3339();
    let tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")?
        .ok_or(EquationsError::TreeMissing)?;

    let mut artifacts = Vec::new();
    for object in tree
        .objects
        .iter()
        .filter(|o| o.object_type == ObjectType::Equation)
    {
        let artifact = EquationArtifact {
            object_id: object.id,
            region: object.regions.first().copied().unwrap_or(BBox {
                page: 0,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }),
            raw_text: object.content.text.clone(),
            number: object
                .semantic_label
                .as_deref()
                .and_then(|l| l.strip_prefix("Equation "))
                .map(|s| s.to_string()),
            latex: object.content.latex.clone(),
            latex_source: None,
            confidence: object.confidence,
        };
        let relative = format!("equations/{}.json", object.id);
        bundle.write_derived_json(
            &relative,
            &artifact,
            "enrichment_parsing",
            serde_json::json!({
                "pipeline_version": EQUATIONS_PIPELINE_VERSION,
                "status": "running",
                "started_at": started_at,
            }),
        )?;
        artifacts.push(artifact);
    }

    // Finalize the stage record.
    let mut metadata = bundle.metadata()?;
    metadata.pipeline.stages.insert(
        "enrichment_parsing".to_string(),
        serde_json::json!({
            "pipeline_version": EQUATIONS_PIPELINE_VERSION,
            "status": "complete",
            "started_at": started_at,
            "completed_at": crate::bundle::now_rfc3339(),
        }),
    );
    bundle.write_metadata(&metadata)?;
    Ok(artifacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_display_equations_not_prose() {
        assert!(is_display_equation(
            "Attention(Q, K, V) = softmax( QKT / √dk )V"
        ));
        assert!(is_display_equation("y = mx + b"));
        assert!(!is_display_equation(
            "The dominant sequence transduction models are based on complex recurrent \
             or convolutional neural networks that include an encoder and a decoder."
        ));
        // Prose containing '=' but reading as a sentence.
        assert!(!is_display_equation(
            "We set the number of layers to N = 6 because deeper stacks did not \
             improve validation performance in our preliminary experiments at all."
        ));
    }

    #[test]
    fn recognizes_equation_number_markers() {
        assert_eq!(equation_number_marker("(1)"), Some("1".to_string()));
        assert_eq!(equation_number_marker(" (12) "), Some("12".to_string()));
        assert_eq!(equation_number_marker("(a)"), None);
        assert_eq!(equation_number_marker("1"), None);
    }

    #[test]
    fn stage_writes_per_equation_artifacts() {
        use crate::bundle::Paper;
        use crate::layout::{LayoutBlock, LayoutDocument, LayoutPage, LAYOUT_PIPELINE_VERSION};

        let layout = LayoutDocument {
            pipeline_version: LAYOUT_PIPELINE_VERSION.to_string(),
            pages: vec![LayoutPage {
                index: 0,
                width: 612.0,
                height: 792.0,
                rotation: 0,
                is_scanned: false,
                blocks: vec![
                    LayoutBlock {
                        id: Uuid::new_v4(),
                        kind: crate::layout::BlockKind::Text,
                        bbox: BBox {
                            page: 0,
                            x: 180.0,
                            y: 300.0,
                            width: 220.0,
                            height: 30.0,
                        },
                        column: None,
                        text: Some("Attention(Q, K, V) = softmax( QKT / √dk )V".to_string()),
                        lines: Vec::new(),
                        confidence: 0.93,
                    },
                    LayoutBlock {
                        id: Uuid::new_v4(),
                        kind: crate::layout::BlockKind::Text,
                        bbox: BBox {
                            page: 0,
                            x: 520.0,
                            y: 308.0,
                            width: 20.0,
                            height: 12.0,
                        },
                        column: None,
                        text: Some("(1)".to_string()),
                        lines: Vec::new(),
                        confidence: 0.9,
                    },
                ],
            }],
        };

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("Sample"), "file").unwrap();
        bundle
            .write_derived_json(
                "layout.json",
                &layout,
                "layout",
                serde_json::json!({"pipeline_version": LAYOUT_PIPELINE_VERSION, "status": "complete"}),
            )
            .unwrap();
        crate::objects::run_objects_stage(&bundle).unwrap();

        let artifacts = run_equations_stage(&bundle).unwrap();
        assert_eq!(artifacts.len(), 1, "{artifacts:#?}");
        assert_eq!(artifacts[0].number.as_deref(), Some("1"));
        assert!(artifacts[0].latex.is_none());

        // Artifact persisted at equations/<uuid>.json.
        let path = root.join(format!("equations/{}.json", artifacts[0].object_id));
        assert!(path.is_file());

        let metadata = bundle.metadata().unwrap();
        assert_eq!(
            metadata.pipeline.stages["enrichment_parsing"]["status"],
            "complete"
        );
    }
}
