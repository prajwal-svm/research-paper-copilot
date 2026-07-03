//! Dev tool: run stage-1 layout analysis on a PDF and print a summary.
//!
//!   cargo run -p copilot-core --example layout_dump -- path/to/paper.pdf

use copilot_core::layout::{analyze, pdfium};

fn main() {
    let path = std::env::args().nth(1).expect("usage: layout_dump <pdf>");
    let pdfium = pdfium().expect("pdfium missing — run scripts/fetch-pdfium.sh");
    let document = pdfium
        .load_pdf_from_file(&path, None)
        .expect("failed to load pdf");

    let layout = analyze(&document);
    if std::env::args().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&layout).unwrap());
        return;
    }
    println!(
        "pages: {}  scanned: {}",
        layout.pages.len(),
        layout.pages.iter().filter(|p| p.is_scanned).count()
    );
    for page in &layout.pages {
        println!(
            "\n— page {} ({}×{}): {} blocks",
            page.index,
            page.width,
            page.height,
            page.blocks.len()
        );
        for block in &page.blocks {
            let text = block.text.as_deref().unwrap_or("");
            let preview: String = text.chars().take(70).collect();
            println!(
                "  [{kind:?} col={col:?} conf={conf:.2}] {preview}",
                kind = block.kind,
                col = block.column,
                conf = block.confidence,
            );
        }
    }
}
