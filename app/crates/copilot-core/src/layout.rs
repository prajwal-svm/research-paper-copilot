//! Ingestion stage 1: PDF layout analysis → `layout.json`.
//!
//! Extracts positioned characters via PDFium, clusters them into lines and
//! blocks, detects columns, derives reading order, and classifies blocks
//! heuristically. Target corpus is arXiv-style ML papers; everything else
//! degrades kindly (scanned pages are flagged `is_scanned`, never an error).
//!
//! Coordinates in the output use PDF points with the origin at the TOP-left
//! of the page (PDFium reports bottom-left; we flip y).

use std::path::{Path, PathBuf};

#[cfg(feature = "native")]
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;

/// Version stamp for this stage's implementation; bump when heuristics change
/// so bundles can be selectively re-run.
/// 0.1.1: graphics regions include form XObjects/shadings (figures embedded
/// as forms were previously invisible → unclickable images).
pub const LAYOUT_PIPELINE_VERSION: &str = "0.1.1";

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[cfg(feature = "native")]
    #[error("pdf engine error: {0}")]
    Pdf(#[from] PdfiumError),
    #[error("pdfium library not found; set PDFIUM_LIB_DIR or run scripts/fetch-pdfium.sh")]
    PdfiumNotFound,
}

// ---------------------------------------------------------------------------
// Output model (mirrors schemas/research-format/v0/layout.schema.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayoutDocument {
    pub pipeline_version: String,
    pub pages: Vec<LayoutPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayoutPage {
    pub index: u32,
    pub width: f32,
    pub height: f32,
    pub rotation: i32,
    pub is_scanned: bool,
    pub blocks: Vec<LayoutBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayoutBlock {
    pub id: Uuid,
    pub kind: BlockKind,
    pub bbox: BBox,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Per-line spans (table grids, fine-grained highlighting).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<LineSpan>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LineSpan {
    pub text: String,
    pub bbox: BBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Text,
    Heading,
    Equation,
    Figure,
    Table,
    Caption,
    Header,
    Footer,
    Footnote,
    ReferenceEntry,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BBox {
    pub page: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

// ---------------------------------------------------------------------------
// PDFium binding
// ---------------------------------------------------------------------------

/// Process-global PDFium instance. pdfium-render permits exactly one live
/// binding per process; the `thread_safe` feature serializes FFI calls so
/// this can be shared across threads.
#[cfg(feature = "native")]
static PDFIUM: std::sync::OnceLock<Option<Pdfium>> = std::sync::OnceLock::new();

/// PDFium is effectively single-threaded: per-call serialization (the
/// `thread_safe` feature) is not enough when document operations interleave
/// across threads. All multi-call PDFium work (load → analyze → render) must
/// hold this lock. The ingestion job runner funnels PDFium work through one
/// worker; tests and tools take the lock directly.
pub static PDFIUM_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire the PDFium serialization lock (poison-tolerant).
#[cfg(feature = "native")]
pub fn pdfium_lock() -> std::sync::MutexGuard<'static, ()> {
    PDFIUM_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Bind PDFium once per process: `PDFIUM_LIB_DIR` → repo `vendor/pdfium/lib`
/// (dev convenience) → system library.
#[cfg(feature = "native")]
pub fn pdfium() -> Result<&'static Pdfium, LayoutError> {
    PDFIUM
        .get_or_init(|| {
            let mut candidates: Vec<PathBuf> = Vec::new();
            if let Ok(dir) = std::env::var("PDFIUM_LIB_DIR") {
                candidates.push(PathBuf::from(dir));
            }
            candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/lib"));

            for dir in &candidates {
                match Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir)) {
                    Ok(bindings) => return Some(Pdfium::new(bindings)),
                    Err(err) => eprintln!("pdfium bind failed at {}: {err}", dir.display()),
                }
            }
            Pdfium::bind_to_system_library().map(Pdfium::new).ok()
        })
        .as_ref()
        .ok_or(LayoutError::PdfiumNotFound)
}

// ---------------------------------------------------------------------------
// Stage entry point
// ---------------------------------------------------------------------------

/// Run stage 1 on a bundle: analyze `original.pdf`, write `layout.json`,
/// record the stage in `metadata.json`.
#[cfg(feature = "native")]
pub fn run_layout_stage(pdfium: &Pdfium, bundle: &Bundle) -> Result<LayoutDocument, LayoutError> {
    let started_at = crate::bundle::now_rfc3339();
    let document = pdfium.load_pdf_from_file(&bundle.original_pdf_path(), None)?;
    let layout = analyze(&document);

    let scanned_pages = layout.pages.iter().filter(|p| p.is_scanned).count();
    let status = if scanned_pages == layout.pages.len() && !layout.pages.is_empty() {
        "degraded"
    } else {
        "complete"
    };
    let mut stage = serde_json::json!({
        "pipeline_version": LAYOUT_PIPELINE_VERSION,
        "status": status,
        "started_at": started_at,
        "completed_at": crate::bundle::now_rfc3339(),
    });
    if status == "degraded" {
        stage["failure_reason"] = serde_json::Value::String(
            "No text layer found — this looks like a scanned PDF. The paper opens in raw view; \
             object extraction is limited."
                .to_string(),
        );
    }
    bundle.write_derived_json("layout.json", &layout, "layout", stage)?;
    Ok(layout)
}

/// Analyze a loaded PDF into a layout document (pure; no bundle IO).
#[cfg(feature = "native")]
pub fn analyze(document: &PdfDocument) -> LayoutDocument {
    let pages = document
        .pages()
        .iter()
        .enumerate()
        .map(|(index, page)| analyze_page(index as u32, &page))
        .collect();
    LayoutDocument {
        pipeline_version: LAYOUT_PIPELINE_VERSION.to_string(),
        pages,
    }
}

// ---------------------------------------------------------------------------
// Per-page analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[cfg(feature = "native")]
struct Char {
    ch: char,
    x: f32,
    /// Top-left-origin y.
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Clone)]
#[cfg(feature = "native")]
struct Line {
    chars: Vec<Char>,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
}

#[cfg(feature = "native")]
impl Line {
    fn center_y(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }

    fn text(&self) -> String {
        let mut chars = self.chars.clone();
        chars.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut out = String::new();
        let mut prev_end: Option<f32> = None;
        for c in &chars {
            if let Some(end) = prev_end {
                // Loose bounds include the advance width, so intra-word gaps
                // are ~0 and a space shows up as a gap of ~0.25× char height.
                if c.x - end > (c.height * 0.15).max(0.5) && !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            out.push(c.ch);
            prev_end = Some(c.x + c.width);
        }
        out.trim().to_string()
    }
}

#[derive(Debug)]
#[cfg(feature = "native")]
struct Block {
    lines: Vec<Line>,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
}

#[cfg(feature = "native")]
fn analyze_page(index: u32, page: &PdfPage) -> LayoutPage {
    let width = page.width().value;
    let height = page.height().value;
    let rotation = match page.rotation() {
        Ok(PdfPageRenderRotation::Degrees90) => 90,
        Ok(PdfPageRenderRotation::Degrees180) => 180,
        Ok(PdfPageRenderRotation::Degrees270) => 270,
        _ => 0,
    };

    let chars = extract_chars(page, height);
    if chars.is_empty() {
        return LayoutPage {
            index,
            width,
            height,
            rotation,
            is_scanned: true,
            blocks: Vec::new(),
        };
    }

    let lines = cluster_lines(chars);
    let blocks = cluster_blocks(lines);
    let median_char_height = median(
        blocks
            .iter()
            .flat_map(|b| {
                b.lines
                    .iter()
                    .flat_map(|l| l.chars.iter().map(|c| c.height))
            })
            .collect(),
    );

    let ordered = reading_order(blocks, width);
    let mut blocks: Vec<LayoutBlock> = ordered
        .into_iter()
        .map(|(block, column)| classify(index, block, column, width, height, median_char_height))
        .collect();
    blocks.extend(graphics_regions(index, page, height));

    LayoutPage {
        index,
        width,
        height,
        rotation,
        is_scanned: false,
        blocks,
    }
}

/// Detect graphics-dense regions (figures, charts) from PDFium page objects.
/// Everything non-text counts as graphic content: raw images and paths, but
/// also Form XObjects and shadings — many PDF producers wrap raster figures
/// in forms, which is why image-only detection misses them. Bounds are
/// unioned into regions and the substantial ones become `figure` blocks
/// (no text) that stage 2 matches captions against.
#[cfg(feature = "native")]
fn graphics_regions(page_index: u32, page: &PdfPage, page_height: f32) -> Vec<LayoutBlock> {
    let mut rects: Vec<(f32, f32, f32, f32)> = Vec::new(); // x0, y0, x1, y1 (top-left origin)
    for object in page.objects().iter() {
        if object.object_type() == PdfPageObjectType::Text {
            continue;
        }
        let Ok(bounds) = object.bounds() else {
            continue;
        };
        let x0 = bounds.left().value;
        let x1 = bounds.right().value;
        let y0 = page_height - bounds.top().value;
        let y1 = page_height - bounds.bottom().value;
        let (w, h) = (x1 - x0, y1 - y0);
        // Drop hairline rules and page-wide separators.
        if w < 4.0 || h < 4.0 || w * h < 100.0 {
            continue;
        }
        rects.push((x0, y0, x1, y1));
    }

    // Union transitively-overlapping/nearby rects.
    const GAP: f32 = 12.0;
    let mut merged: Vec<(f32, f32, f32, f32)> = Vec::new();
    'outer: for rect in rects {
        for m in &mut merged {
            let overlaps = rect.0 < m.2 + GAP
                && m.0 < rect.2 + GAP
                && rect.1 < m.3 + GAP
                && m.1 < rect.3 + GAP;
            if overlaps {
                m.0 = m.0.min(rect.0);
                m.1 = m.1.min(rect.1);
                m.2 = m.2.max(rect.2);
                m.3 = m.3.max(rect.3);
                continue 'outer;
            }
        }
        merged.push(rect);
    }
    // Merging can make previously-separate regions overlap; one more pass.
    let mut stable: Vec<(f32, f32, f32, f32)> = Vec::new();
    'outer2: for rect in merged {
        for m in &mut stable {
            let overlaps = rect.0 < m.2 + GAP
                && m.0 < rect.2 + GAP
                && rect.1 < m.3 + GAP
                && m.1 < rect.3 + GAP;
            if overlaps {
                m.0 = m.0.min(rect.0);
                m.1 = m.1.min(rect.1);
                m.2 = m.2.max(rect.2);
                m.3 = m.3.max(rect.3);
                continue 'outer2;
            }
        }
        stable.push(rect);
    }

    stable
        .into_iter()
        .filter(|(x0, y0, x1, y1)| {
            let (w, h) = (x1 - x0, y1 - y0);
            // A real figure occupies meaningful area and isn't a thin rule.
            w > 40.0 && h > 40.0 && w * h > 8_000.0
        })
        .map(|(x0, y0, x1, y1)| LayoutBlock {
            id: Uuid::new_v4(),
            kind: BlockKind::Figure,
            bbox: BBox {
                page: page_index,
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            },
            column: None,
            text: None,
            lines: Vec::new(),
            confidence: 0.6,
        })
        .collect()
}

#[cfg(feature = "native")]
fn extract_chars(page: &PdfPage, page_height: f32) -> Vec<Char> {
    let Ok(text) = page.text() else {
        return Vec::new();
    };
    let mut chars = Vec::new();
    for c in text.chars().iter() {
        let Some(ch) = c.unicode_char() else { continue };
        if ch.is_control() || ch.is_whitespace() {
            continue;
        }
        let Ok(bounds) = c.loose_bounds() else {
            continue;
        };
        let h = (bounds.top() - bounds.bottom()).value;
        let w = (bounds.right() - bounds.left()).value;
        if h <= 0.0 || w <= 0.0 {
            continue;
        }
        chars.push(Char {
            ch,
            x: bounds.left().value,
            y: page_height - bounds.top().value,
            width: w,
            height: h,
        });
    }
    chars
}

/// Greedy vertical clustering of chars into lines.
#[cfg(feature = "native")]
fn cluster_lines(mut chars: Vec<Char>) -> Vec<Line> {
    chars.sort_by(|a, b| {
        (a.y + a.height / 2.0)
            .total_cmp(&(b.y + b.height / 2.0))
            .then(a.x.total_cmp(&b.x))
    });

    let mut lines: Vec<Line> = Vec::new();
    for c in chars {
        let center = c.y + c.height / 2.0;
        let matched = lines
            .iter_mut()
            .rev()
            .take(4)
            .find(|line| (center - line.center_y()).abs() < (c.height * 0.6).max(2.0));
        match matched {
            Some(line) => {
                line.x0 = line.x0.min(c.x);
                line.x1 = line.x1.max(c.x + c.width);
                line.y0 = line.y0.min(c.y);
                line.y1 = line.y1.max(c.y + c.height);
                line.chars.push(c);
            }
            None => lines.push(Line {
                x0: c.x,
                x1: c.x + c.width,
                y0: c.y,
                y1: c.y + c.height,
                chars: vec![c],
            }),
        }
    }
    let mut lines: Vec<Line> = lines.into_iter().flat_map(split_line_on_gaps).collect();
    lines.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    lines
}

/// Split a line at large horizontal gaps. In two-column layouts, text in both
/// columns shares a baseline and would otherwise merge into one page-wide
/// "line", destroying column detection.
#[cfg(feature = "native")]
fn split_line_on_gaps(line: Line) -> Vec<Line> {
    let mut chars = line.chars;
    chars.sort_by(|a, b| a.x.total_cmp(&b.x));
    let char_height = median(chars.iter().map(|c| c.height).collect()).max(1.0);
    let gap_threshold = (char_height * 2.0).max(12.0);

    let mut segments: Vec<Vec<Char>> = Vec::new();
    let mut current: Vec<Char> = Vec::new();
    let mut prev_end = f32::MIN;
    for c in chars {
        if !current.is_empty() && c.x - prev_end > gap_threshold {
            segments.push(std::mem::take(&mut current));
        }
        prev_end = c.x + c.width;
        current.push(c);
    }
    if !current.is_empty() {
        segments.push(current);
    }

    segments
        .into_iter()
        .map(|chars| {
            let x0 = chars.iter().map(|c| c.x).fold(f32::MAX, f32::min);
            let x1 = chars.iter().map(|c| c.x + c.width).fold(f32::MIN, f32::max);
            let y0 = chars.iter().map(|c| c.y).fold(f32::MAX, f32::min);
            let y1 = chars
                .iter()
                .map(|c| c.y + c.height)
                .fold(f32::MIN, f32::max);
            Line {
                chars,
                x0,
                x1,
                y0,
                y1,
            }
        })
        .collect()
}

/// Merge consecutive lines into blocks when vertically close and horizontally
/// overlapping. Columns separate naturally (no horizontal overlap).
#[cfg(feature = "native")]
fn cluster_blocks(lines: Vec<Line>) -> Vec<Block> {
    let line_heights: Vec<f32> = lines.iter().map(|l| l.y1 - l.y0).collect();
    let median_line_height = median(line_heights).max(1.0);

    let mut blocks: Vec<Block> = Vec::new();
    for line in lines {
        let matched = blocks.iter_mut().rev().take(6).find(|block| {
            let gap = line.y0 - block.y1;
            let x_overlap = line.x1.min(block.x1) - line.x0.max(block.x0);
            let narrower = (line.x1 - line.x0).min(block.x1 - block.x0);
            gap > -median_line_height
                && gap < median_line_height * 0.9
                && x_overlap > narrower * 0.3
        });
        match matched {
            Some(block) => {
                block.x0 = block.x0.min(line.x0);
                block.x1 = block.x1.max(line.x1);
                block.y0 = block.y0.min(line.y0);
                block.y1 = block.y1.max(line.y1);
                block.lines.push(line);
            }
            None => blocks.push(Block {
                x0: line.x0,
                x1: line.x1,
                y0: line.y0,
                y1: line.y1,
                lines: vec![line],
            }),
        }
    }
    blocks
}

/// Column-aware reading order. Full-width blocks split the page into bands;
/// within a band, columns read left → right, top → bottom.
#[cfg(feature = "native")]
fn reading_order(mut blocks: Vec<Block>, page_width: f32) -> Vec<(Block, Option<u32>)> {
    blocks.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let center = page_width / 2.0;

    let column_of = |block: &Block| -> Option<u32> {
        let w = block.x1 - block.x0;
        if w > page_width * 0.6 {
            return None; // spans columns
        }
        if block.x1 < center + page_width * 0.05 {
            Some(0)
        } else if block.x0 > center - page_width * 0.05 {
            Some(1)
        } else {
            None
        }
    };

    let mut out: Vec<(Block, Option<u32>)> = Vec::new();
    let mut band: Vec<(Block, Option<u32>)> = Vec::new();
    let flush = |band: &mut Vec<(Block, Option<u32>)>, out: &mut Vec<(Block, Option<u32>)>| {
        band.sort_by(|(a, ca), (b, cb)| {
            ca.unwrap_or(0)
                .cmp(&cb.unwrap_or(0))
                .then(a.y0.total_cmp(&b.y0))
        });
        out.append(band);
    };

    for block in blocks {
        match column_of(&block) {
            None => {
                flush(&mut band, &mut out);
                out.push((block, None));
            }
            col => band.push((block, col)),
        }
    }
    flush(&mut band, &mut out);
    out
}

#[cfg(feature = "native")]
fn classify(
    page_index: u32,
    block: Block,
    column: Option<u32>,
    _page_width: f32,
    page_height: f32,
    median_char_height: f32,
) -> LayoutBlock {
    let text = block
        .lines
        .iter()
        .map(|l| l.text())
        .collect::<Vec<_>>()
        .join(" ");
    let char_height = median(
        block
            .lines
            .iter()
            .flat_map(|l| l.chars.iter().map(|c| c.height))
            .collect(),
    );

    let line_count = block.lines.len();
    let kind = if block.y1 < page_height * 0.07 && line_count <= 1 {
        BlockKind::Header
    } else if block.y0 > page_height * 0.93 && line_count <= 2 {
        BlockKind::Footer
    } else if block.y0 > page_height * 0.8 && char_height < median_char_height * 0.85 {
        BlockKind::Footnote
    } else if char_height > median_char_height * 1.15 && line_count <= 2 {
        BlockKind::Heading
    } else {
        BlockKind::Text
    };

    // Confidence heuristic: long clean text blocks are trustworthy; tiny
    // fragments (often math debris or ligature fallout) are not.
    let confidence = if text.chars().count() >= 10 {
        0.95
    } else {
        0.7
    };

    let lines = block
        .lines
        .iter()
        .map(|l| LineSpan {
            text: l.text(),
            bbox: BBox {
                page: page_index,
                x: l.x0,
                y: l.y0,
                width: l.x1 - l.x0,
                height: l.y1 - l.y0,
            },
        })
        .filter(|l| !l.text.is_empty())
        .collect();

    LayoutBlock {
        id: Uuid::new_v4(),
        kind,
        bbox: BBox {
            page: page_index,
            x: block.x0,
            y: block.y0,
            width: block.x1 - block.x0,
            height: block.y1 - block.y0,
        },
        column,
        text: if text.is_empty() { None } else { Some(text) },
        lines,
        confidence,
    }
}

#[cfg(feature = "native")]
fn median(mut values: Vec<f32>) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    fn pdfium_guard() -> std::sync::MutexGuard<'static, ()> {
        pdfium_lock()
    }

    fn test_pdfium() -> &'static Pdfium {
        pdfium().expect("pdfium library missing — run scripts/fetch-pdfium.sh")
    }

    /// Build a simple two-column style PDF with a title and two body texts.
    fn sample_pdf(pdfium: &Pdfium) -> Vec<u8> {
        let mut document = pdfium.create_new_pdf().unwrap();
        let font = document.fonts_mut().helvetica();
        let mut page = document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::a4())
            .unwrap();
        let page_height = page.height().value;

        let add_text = |page: &mut PdfPage, text: &str, x: f32, y_top: f32, size: f32| {
            page.objects_mut()
                .create_text_object(
                    PdfPoints::new(x),
                    PdfPoints::new(page_height - y_top),
                    text,
                    font,
                    PdfPoints::new(size),
                )
                .unwrap();
        };

        add_text(&mut page, "Attention Is All You Need", 150.0, 80.0, 20.0);
        add_text(
            &mut page,
            "Left column body text about attention.",
            60.0,
            200.0,
            10.0,
        );
        add_text(
            &mut page,
            "Right column body text about encoders.",
            320.0,
            200.0,
            10.0,
        );

        document.save_to_bytes().unwrap()
    }

    #[test]
    fn analyzes_text_pdf_into_blocks_with_reading_order() {
        let _lock = pdfium_guard();
        let pdfium = test_pdfium();
        let bytes = sample_pdf(pdfium);
        let document = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
        let layout = analyze(&document);

        assert_eq!(layout.pages.len(), 1);
        let page = &layout.pages[0];
        assert!(!page.is_scanned);
        assert!(page.blocks.len() >= 3, "blocks: {:#?}", page.blocks);

        let texts: Vec<&str> = page
            .blocks
            .iter()
            .filter_map(|b| b.text.as_deref())
            .collect();
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Attention Is All You Need")),
            "title missing from {texts:?}"
        );

        // Title (full width, larger font) reads before the columns; left
        // column before right column.
        let idx = |needle: &str| {
            page.blocks
                .iter()
                .position(|b| b.text.as_deref().is_some_and(|t| t.contains(needle)))
                .unwrap_or(usize::MAX)
        };
        assert!(idx("Attention") < idx("Left column"));
        assert!(idx("Left column") < idx("Right column"));

        // Columns detected.
        let left = &page.blocks[idx("Left column")];
        let right = &page.blocks[idx("Right column")];
        assert_eq!(left.column, Some(0));
        assert_eq!(right.column, Some(1));

        // Title classified as heading (larger font, single line).
        let title = &page.blocks[idx("Attention")];
        assert_eq!(title.kind, BlockKind::Heading);
    }

    #[test]
    fn image_only_pdf_is_flagged_scanned_not_failed() {
        let _lock = pdfium_guard();
        let pdfium = test_pdfium();
        let mut document = pdfium.create_new_pdf().unwrap();
        document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::a4())
            .unwrap();
        let bytes = document.save_to_bytes().unwrap();

        let document = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
        let layout = analyze(&document);
        assert_eq!(layout.pages.len(), 1);
        assert!(layout.pages[0].is_scanned);
        assert!(layout.pages[0].blocks.is_empty());
    }

    #[test]
    fn stage_writes_layout_json_and_metadata_record() {
        let _lock = pdfium_guard();
        let pdfium = test_pdfium();
        let bytes = sample_pdf(pdfium);

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, &bytes, Paper::new("Sample"), "file").unwrap();

        let layout = run_layout_stage(pdfium, &bundle).unwrap();
        assert_eq!(layout.pipeline_version, LAYOUT_PIPELINE_VERSION);

        let reread: Option<LayoutDocument> = bundle.read_derived_json("layout.json").unwrap();
        assert!(reread.is_some());

        let metadata = bundle.metadata().unwrap();
        let stage = &metadata.pipeline.stages["layout"];
        assert_eq!(stage["status"], "complete");
        assert_eq!(stage["pipeline_version"], LAYOUT_PIPELINE_VERSION);
        assert!(metadata.content_hashes.contains_key("layout.json"));
    }
}
