//! Ingestion stage 3 (figures & tables slice): `figures/` and `tables/`.
//!
//! Figures: each figure object's region is rendered from the original PDF to
//! `figures/<uuid>.png` (the caption travels in the object). Tables: layout
//! blocks inside the table's region band are clustered into rows/columns as
//! best-effort structured data in `tables/<uuid>.json`; extraction confidence
//! is honest and low-confidence tables degrade to raw text in the UI.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::layout::{BBox, LayoutDocument};
use crate::objects::{ObjectType, SemanticTreeDocument};

pub const FIGURES_TABLES_PIPELINE_VERSION: &str = "0.1.0";

/// Render scale for figure crops (2× keeps text in figures legible).
const FIGURE_RENDER_SCALE: f32 = 2.0;

#[derive(Debug, thiserror::Error)]
pub enum FiguresTablesError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error(transparent)]
    Layout(#[from] crate::layout::LayoutError),
    #[error("semantic_tree.json or layout.json missing — run earlier stages first")]
    PrerequisiteMissing,
    #[error("pdf engine error: {0}")]
    Pdf(#[from] pdfium_render::prelude::PdfiumError),
    #[error("image encode error: {0}")]
    Image(#[from] image::ImageError),
}

/// Per-table artifact stored at `tables/<object_id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableArtifact {
    pub object_id: Uuid,
    pub region: BBox,
    pub caption: String,
    /// Best-effort structured data; `None` when clustering found no grid.
    pub data: Option<TableData>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableData {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Summary returned by the stage.
#[derive(Debug, Default)]
pub struct FiguresTablesReport {
    pub figures_rendered: usize,
    pub tables_extracted: usize,
    pub tables_without_grid: usize,
}

/// Run the figures/tables slice of stage 3.
pub fn run_figures_tables_stage(
    bundle: &Bundle,
) -> Result<FiguresTablesReport, FiguresTablesError> {
    let started_at = crate::bundle::now_rfc3339();
    let tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")?
        .ok_or(FiguresTablesError::PrerequisiteMissing)?;
    let layout: LayoutDocument = bundle
        .read_derived_json("layout.json")?
        .ok_or(FiguresTablesError::PrerequisiteMissing)?;

    let mut report = FiguresTablesReport::default();

    // Figures: render each figure region to PNG.
    let pdfium = crate::layout::pdfium()?;
    let document = pdfium.load_pdf_from_file(&bundle.original_pdf_path(), None)?;
    for object in tree
        .objects
        .iter()
        .filter(|o| o.object_type == ObjectType::Figure)
    {
        let Some(region) = object.regions.first() else {
            continue;
        };
        if render_region_png(
            &document,
            *region,
            bundle,
            &format!("figures/{}.png", object.id),
        )
        .is_ok()
        {
            report.figures_rendered += 1;
        }
    }
    drop(document);

    // Tables: cluster layout blocks in the region band under each caption.
    for object in tree
        .objects
        .iter()
        .filter(|o| o.object_type == ObjectType::Table)
    {
        let Some(region) = object.regions.first() else {
            continue;
        };
        let data = extract_table_grid(&layout, *region);
        let confidence = match &data {
            Some(d) if d.rows.len() >= 2 => 0.7,
            Some(_) => 0.5,
            None => 0.3,
        };
        if data.is_none() {
            report.tables_without_grid += 1;
        }
        let artifact = TableArtifact {
            object_id: object.id,
            region: *region,
            caption: object.content.caption.clone().unwrap_or_default(),
            data,
            confidence,
        };
        bundle.write_derived_json(
            &format!("tables/{}.json", object.id),
            &artifact,
            "enrichment_parsing",
            serde_json::json!({
                "pipeline_version": FIGURES_TABLES_PIPELINE_VERSION,
                "status": "running",
                "started_at": started_at,
            }),
        )?;
        report.tables_extracted += 1;
    }

    let mut metadata = bundle.metadata()?;
    metadata.pipeline.stages.insert(
        "enrichment_parsing".to_string(),
        serde_json::json!({
            "pipeline_version": FIGURES_TABLES_PIPELINE_VERSION,
            "status": "complete",
            "started_at": started_at,
            "completed_at": crate::bundle::now_rfc3339(),
        }),
    );
    bundle.write_metadata(&metadata)?;
    Ok(report)
}

/// Render a page region to a PNG inside the bundle.
fn render_region_png(
    document: &pdfium_render::prelude::PdfDocument,
    region: BBox,
    bundle: &Bundle,
    relative_path: &str,
) -> Result<(), FiguresTablesError> {
    use pdfium_render::prelude::*;

    let page = document.pages().get(region.page as i32)?;
    let page_width = page.width().value;
    let page_height = page.height().value;

    let config = PdfRenderConfig::new()
        .set_target_width((page_width * FIGURE_RENDER_SCALE) as Pixels)
        .set_maximum_height((page_height * FIGURE_RENDER_SCALE) as Pixels);
    let bitmap = page.render_with_config(&config)?;
    let full = bitmap.as_image()?;

    let scale_x = full.width() as f32 / page_width;
    let scale_y = full.height() as f32 / page_height;
    // Small margin so strokes on the region edge aren't clipped.
    let margin = 4.0;
    let x = ((region.x - margin) * scale_x).max(0.0) as u32;
    let y = ((region.y - margin) * scale_y).max(0.0) as u32;
    let w = (((region.width + margin * 2.0) * scale_x) as u32).min(full.width() - x);
    let h = (((region.height + margin * 2.0) * scale_y) as u32).min(full.height() - y);
    if w == 0 || h == 0 {
        return Ok(()); // degenerate region; skip silently
    }

    let crop = full.crop_imm(x, y, w, h);
    let path = bundle.root().join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            FiguresTablesError::Bundle(crate::bundle::BundleError::Io {
                path: parent.to_path_buf(),
                source: e,
            })
        })?;
    }
    crop.save_with_format(&path, image::ImageFormat::Png)?;
    Ok(())
}

/// Cluster layout blocks under a table caption into a row/column grid.
/// Returns `None` when no plausible grid is found (single column of text).
fn extract_table_grid(layout: &LayoutDocument, caption_region: BBox) -> Option<TableData> {
    let page = layout
        .pages
        .iter()
        .find(|p| p.index == caption_region.page)?;

    // Candidate cells: per-line spans of text blocks in the band below the
    // caption (tables usually follow their caption in papers using
    // "Table n:" style). Lines give real cell granularity — vertical block
    // clustering merges table rows into per-column blocks.
    let band_top = caption_region.y + caption_region.height;
    let band_bottom = band_top + 260.0;
    let mut cells: Vec<(f32, f32, String)> = page
        .blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .filter(|l| {
            let ly = l.bbox.y;
            ly >= band_top - 2.0 && ly < band_bottom
        })
        .map(|l| (l.bbox.y, l.bbox.x, l.text.trim().to_string()))
        .filter(|(_, _, t)| !t.is_empty())
        .collect();
    if cells.len() < 4 {
        return None;
    }
    cells.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));

    // Group into rows by y proximity.
    let mut rows: Vec<Vec<(f32, String)>> = Vec::new();
    let mut current_y = f32::MIN;
    for (y, x, text) in cells {
        if (y - current_y).abs() > 6.0 {
            rows.push(Vec::new());
            current_y = y;
        }
        rows.last_mut().unwrap().push((x, text));
    }
    // A grid needs at least two rows with more than one cell.
    let multi_cell_rows = rows.iter().filter(|r| r.len() > 1).count();
    if multi_cell_rows < 2 {
        return None;
    }

    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut iter = rows.into_iter();
    let header: Vec<String> = iter.next()?.into_iter().map(|(_, t)| t).collect();
    let mut columns = header;
    columns.resize(width, String::new());

    let data_rows: Vec<Vec<String>> = iter
        .map(|row| {
            let mut cells: Vec<String> = row.into_iter().map(|(_, t)| t).collect();
            cells.resize(width, String::new());
            cells
        })
        .collect();

    Some(TableData {
        columns,
        rows: data_rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;
    use crate::layout::{BlockKind, LayoutBlock, LayoutPage, LAYOUT_PIPELINE_VERSION};

    fn text_block(text: &str, x: f32, y: f32, w: f32) -> LayoutBlock {
        let bbox = BBox {
            page: 0,
            x,
            y,
            width: w,
            height: 12.0,
        };
        LayoutBlock {
            id: Uuid::new_v4(),
            kind: BlockKind::Text,
            bbox,
            column: None,
            text: Some(text.to_string()),
            lines: vec![crate::layout::LineSpan {
                text: text.to_string(),
                bbox,
            }],
            confidence: 0.9,
        }
    }

    #[test]
    fn clusters_table_blocks_into_grid() {
        let caption = BBox {
            page: 0,
            x: 100.0,
            y: 100.0,
            width: 300.0,
            height: 14.0,
        };
        let layout = LayoutDocument {
            pipeline_version: LAYOUT_PIPELINE_VERSION.to_string(),
            pages: vec![LayoutPage {
                index: 0,
                width: 612.0,
                height: 792.0,
                rotation: 0,
                is_scanned: false,
                blocks: vec![
                    // header row
                    text_block("Model", 100.0, 130.0, 60.0),
                    text_block("BLEU EN-DE", 220.0, 130.0, 80.0),
                    // data rows
                    text_block("ByteNet", 100.0, 150.0, 60.0),
                    text_block("23.75", 220.0, 150.0, 40.0),
                    text_block("Transformer (big)", 100.0, 170.0, 110.0),
                    text_block("28.4", 220.0, 170.0, 40.0),
                ],
            }],
        };

        let data = extract_table_grid(&layout, caption).expect("grid expected");
        assert_eq!(data.columns, vec!["Model", "BLEU EN-DE"]);
        assert_eq!(data.rows.len(), 2);
        assert_eq!(data.rows[1], vec!["Transformer (big)", "28.4"]);
    }

    #[test]
    fn no_grid_for_prose_band() {
        let caption = BBox {
            page: 0,
            x: 100.0,
            y: 100.0,
            width: 300.0,
            height: 14.0,
        };
        let layout = LayoutDocument {
            pipeline_version: LAYOUT_PIPELINE_VERSION.to_string(),
            pages: vec![LayoutPage {
                index: 0,
                width: 612.0,
                height: 792.0,
                rotation: 0,
                is_scanned: false,
                blocks: vec![
                    text_block("Just a paragraph of following text.", 100.0, 130.0, 300.0),
                    text_block("Another paragraph, one per line.", 100.0, 150.0, 300.0),
                ],
            }],
        };
        assert!(extract_table_grid(&layout, caption).is_none());
    }

    #[test]
    fn stage_renders_figure_pngs_and_writes_table_artifacts() {
        use pdfium_render::prelude::*;

        let _lock = crate::layout::pdfium_lock();
        let pdfium = crate::layout::pdfium().expect("pdfium missing");
        // Build a PDF whose text yields a figure caption and a table caption
        // with a grid beneath it.
        let bytes = {
            let mut document = pdfium.create_new_pdf().unwrap();
            let font = document.fonts_mut().helvetica();
            let mut page = document
                .pages_mut()
                .create_page_at_end(PdfPagePaperSize::a4())
                .unwrap();
            let page_height = page.height().value;
            let add = |page: &mut PdfPage, text: &str, x: f32, y_top: f32, size: f32| {
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
            add(&mut page, "Figure 1: A test figure.", 100.0, 200.0, 10.0);
            add(&mut page, "Table 1: A test table.", 100.0, 400.0, 10.0);
            add(&mut page, "Model", 100.0, 430.0, 10.0);
            add(&mut page, "Score", 300.0, 430.0, 10.0);
            add(&mut page, "Alpha", 100.0, 450.0, 10.0);
            add(&mut page, "1.0", 300.0, 450.0, 10.0);
            add(&mut page, "Beta", 100.0, 470.0, 10.0);
            add(&mut page, "2.0", 300.0, 470.0, 10.0);
            document.save_to_bytes().unwrap()
        };

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, &bytes, Paper::new("Sample"), "file").unwrap();
        crate::layout::run_layout_stage(pdfium, &bundle).unwrap();
        crate::objects::run_objects_stage(&bundle).unwrap();

        let report = run_figures_tables_stage(&bundle).unwrap();
        assert_eq!(report.figures_rendered, 1);
        assert_eq!(report.tables_extracted, 1);

        // A PNG exists for the figure object.
        let tree: SemanticTreeDocument = bundle
            .read_derived_json("semantic_tree.json")
            .unwrap()
            .unwrap();
        let figure = tree
            .objects
            .iter()
            .find(|o| o.object_type == ObjectType::Figure)
            .expect("figure object");
        assert!(root.join(format!("figures/{}.png", figure.id)).is_file());

        // Table artifact holds the extracted grid.
        let table = tree
            .objects
            .iter()
            .find(|o| o.object_type == ObjectType::Table)
            .expect("table object");
        let artifact: TableArtifact = bundle
            .read_derived_json(&format!("tables/{}.json", table.id))
            .unwrap()
            .unwrap();
        let data = artifact.data.expect("grid expected");
        assert_eq!(data.columns, vec!["Model", "Score"]);
        assert_eq!(data.rows, vec![vec!["Alpha", "1.0"], vec!["Beta", "2.0"]]);
    }
}
