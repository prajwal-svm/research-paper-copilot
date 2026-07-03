//! Per-OS visual regression for the native render path (task 4.3).
//!
//! Renders the first page of a deterministic synthetic PDF via PDFium and
//! compares against a committed per-OS golden PNG (mean absolute pixel
//! difference within tolerance). When the golden for this OS doesn't exist
//! yet, the test writes the candidate next to it and fails with instructions
//! — commit the candidate after eyeballing it (CI uploads it as an artifact).
//!
//! The webview canvas path (pdf.js) uses one rasterizer on all OSes by
//! design; full webview screenshot tests ride on the 8.1 quality-gate
//! harness (tauri-driver).

use copilot_core::layout::{pdfium, pdfium_lock};
use pdfium_render::prelude::*;

const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/goldens");
/// Mean absolute channel difference (0-255) allowed across the image.
const TOLERANCE: f64 = 1.5;

fn os_tag() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

/// Deterministic one-page PDF exercising text, vector paths, and shading.
fn synthetic_pdf(pdfium: &Pdfium) -> Vec<u8> {
    let mut document = pdfium.create_new_pdf().unwrap();
    let font = document.fonts_mut().helvetica();
    let mut page = document
        .pages_mut()
        .create_page_at_end(PdfPagePaperSize::a4())
        .unwrap();
    let height = page.height().value;

    let objects = page.objects_mut();
    objects
        .create_text_object(
            PdfPoints::new(72.0),
            PdfPoints::new(height - 100.0),
            "Render regression: The quick brown fox 0123456789",
            font,
            PdfPoints::new(14.0),
        )
        .unwrap();
    objects
        .create_path_object_rect(
            PdfRect::new(
                PdfPoints::new(height - 320.0),
                PdfPoints::new(72.0),
                PdfPoints::new(height - 180.0),
                PdfPoints::new(300.0),
            ),
            Some(PdfColor::new(30, 34, 72, 255)),
            Some(PdfPoints::new(2.0)),
            Some(PdfColor::new(90, 103, 216, 255)),
        )
        .unwrap();
    objects
        .create_path_object_line(
            PdfPoints::new(72.0),
            PdfPoints::new(height - 400.0),
            PdfPoints::new(500.0),
            PdfPoints::new(height - 340.0),
            PdfColor::new(229, 62, 62, 255),
            PdfPoints::new(3.0),
        )
        .unwrap();
    document.save_to_bytes().unwrap()
}

fn render_page_png(pdfium: &Pdfium, pdf: &[u8]) -> image::DynamicImage {
    let document = pdfium.load_pdf_from_byte_slice(pdf, None).unwrap();
    let page = document.pages().get(0).unwrap();
    let config = PdfRenderConfig::new().set_target_width(1000);
    let rendered = page
        .render_with_config(&config)
        .unwrap()
        .as_image()
        .unwrap();
    rendered
}

#[test]
fn native_render_matches_per_os_golden() {
    let _lock = pdfium_lock();
    let pdfium = pdfium().expect("pdfium missing — run scripts/fetch-pdfium.sh");
    let pdf = synthetic_pdf(pdfium);

    // Rendering must be deterministic on this platform.
    let a = render_page_png(pdfium, &pdf).to_rgba8();
    let b = render_page_png(pdfium, &pdf).to_rgba8();
    assert_eq!(
        a.as_raw(),
        b.as_raw(),
        "rendering is nondeterministic on this platform"
    );

    // Not blank.
    let non_white = a.pixels().filter(|p| p.0[0] < 250).count();
    assert!(
        non_white > 1000,
        "render appears blank ({non_white} dark pixels)"
    );

    std::fs::create_dir_all(GOLDEN_DIR).unwrap();
    let golden_path = format!("{GOLDEN_DIR}/render-{}.png", os_tag());
    if !std::path::Path::new(&golden_path).exists() {
        let candidate = format!("{GOLDEN_DIR}/render-{}.candidate.png", os_tag());
        a.save(&candidate).unwrap();
        panic!(
            "no golden for {os}: candidate written to {candidate} — inspect and commit it as {golden_path}",
            os = os_tag()
        );
    }

    let golden = image::open(&golden_path).unwrap().to_rgba8();
    assert_eq!(
        (golden.width(), golden.height()),
        (a.width(), a.height()),
        "render dimensions changed vs golden"
    );
    let total_diff: u64 = golden
        .as_raw()
        .iter()
        .zip(a.as_raw())
        .map(|(x, y)| (*x as i64 - *y as i64).unsigned_abs())
        .sum();
    let mean = total_diff as f64 / golden.as_raw().len() as f64;
    if mean > TOLERANCE {
        let candidate = format!("{GOLDEN_DIR}/render-{}.candidate.png", os_tag());
        a.save(&candidate).unwrap();
        panic!(
            "render drifted from golden (mean pixel diff {mean:.3} > {TOLERANCE}); candidate at {candidate}"
        );
    }
}
