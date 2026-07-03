//! Hostile-PDF corpus (task 8.2): scanned, malformed, truncated, empty, and
//! wrong-format inputs must degrade kindly — bundle intact, raw view
//! available where possible, plain-language failure reasons recorded, and
//! never a panic.

use copilot_core::bundle::Paper;
use copilot_core::pipeline::{import_pdf, PipelineOptions, ProgressEvent};

fn run(
    name: &str,
    bytes: &[u8],
) -> (
    tempfile::TempDir,
    copilot_core::bundle::Bundle,
    bool,
    Vec<String>,
) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join(format!("{name}.research"));
    let mut usable = false;
    let mut failures = Vec::new();
    let bundle = import_pdf(
        bytes,
        &root,
        Paper::new(name),
        "file",
        &PipelineOptions::local(false),
        &mut |event| match event {
            ProgressEvent::PipelineFinished { usable: u } => usable = u,
            ProgressEvent::StageFailed { reason, .. }
            | ProgressEvent::StageDegraded { reason, .. } => failures.push(reason),
            _ => {}
        },
    )
    .expect("bundle creation must never fail");
    (tmp, bundle, usable, failures)
}

#[test]
fn not_a_pdf_at_all() {
    let (_tmp, bundle, usable, failures) = run("garbage", b"this is just text, not a pdf");
    assert!(!usable);
    assert!(!failures.is_empty(), "failure reason must be reported");
    // Original preserved; metadata intact; delete-able bundle.
    assert!(bundle.original_pdf_path().is_file());
    assert!(bundle.metadata().is_ok());
}

#[test]
fn empty_file() {
    let (_tmp, _bundle, usable, failures) = run("empty", b"");
    assert!(!usable);
    assert!(!failures.is_empty());
}

#[test]
fn truncated_pdf() {
    let _lock = copilot_core::layout::pdfium_lock();
    // Take a valid PDF and cut it off mid-body.
    let pdfium = copilot_core::layout::pdfium().expect("pdfium");
    let mut document = pdfium.create_new_pdf().unwrap();
    let font = document.fonts_mut().helvetica();
    {
        use pdfium_render::prelude::*;
        let mut page = document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::a4())
            .unwrap();
        let h = page.height().value;
        page.objects_mut()
            .create_text_object(
                PdfPoints::new(72.0),
                PdfPoints::new(h - 100.0),
                "This document will be truncated.",
                font,
                PdfPoints::new(12.0),
            )
            .unwrap();
    }
    let full = document.save_to_bytes().unwrap();
    drop(document);
    drop(_lock);
    let truncated = &full[..full.len() / 3];

    let (_tmp, _bundle, _usable, _failures) = run("truncated", truncated);
    // Outcome may be usable (PDFium can salvage some truncations) or not;
    // the contract is simply: no panic, bundle intact, reasons when failed.
}

#[test]
fn scanned_image_only_pdf_flagged_not_failed() {
    let _lock = copilot_core::layout::pdfium_lock();
    let pdfium = copilot_core::layout::pdfium().expect("pdfium");
    let document = {
        let mut document = pdfium.create_new_pdf().unwrap();
        // Pages with no text layer at all.
        for _ in 0..3 {
            use pdfium_render::prelude::*;
            document
                .pages_mut()
                .create_page_at_end(PdfPagePaperSize::a4())
                .unwrap();
        }
        document.save_to_bytes().unwrap()
    };
    drop(_lock);

    let (_tmp, bundle, usable, failures) = run("scanned", &document);
    assert!(usable, "scanned PDFs stay usable (raw view)");
    assert!(
        failures.iter().any(|f| f.contains("scanned")),
        "plain-language scanned explanation expected: {failures:?}"
    );
    // Raw view data (layout.json with is_scanned pages) exists.
    let layout: copilot_core::layout::LayoutDocument = bundle
        .read_derived_json("layout.json")
        .unwrap()
        .expect("layout.json present for raw view");
    assert!(layout.pages.iter().all(|p| p.is_scanned));
}

#[test]
fn binary_junk_with_pdf_magic() {
    // Starts like a PDF, then junk — parser must not panic.
    let mut bytes = b"%PDF-1.7\n".to_vec();
    bytes.extend((0..4096u32).flat_map(|i| i.to_le_bytes()));
    let (_tmp, _bundle, _usable, _failures) = run("magic-junk", &bytes);
}
