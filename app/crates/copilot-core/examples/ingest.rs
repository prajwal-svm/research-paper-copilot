//! Dev tool: run the full implemented pipeline (stages 1–3) on a PDF,
//! producing a real bundle.
//!
//!   cargo run -p copilot-core --example ingest -- paper.pdf out.research

use copilot_core::bundle::{Bundle, Paper};
use copilot_core::citations::{resolve_citations, run_citations_stage};
use copilot_core::equations::run_equations_stage;
use copilot_core::figures_tables::run_figures_tables_stage;
use copilot_core::layout::{pdfium, pdfium_lock, run_layout_stage};
use copilot_core::objects::run_objects_stage;

fn main() {
    let mut args = std::env::args().skip(1);
    let pdf_path = args.next().expect("usage: ingest <pdf> <out.research>");
    let out = args.next().expect("usage: ingest <pdf> <out.research>");

    let bytes = std::fs::read(&pdf_path).expect("failed to read pdf");
    let title = std::path::Path::new(&pdf_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".to_string());
    let bundle = Bundle::create(
        std::path::Path::new(&out),
        &bytes,
        Paper::new(title),
        "file",
    )
    .unwrap();

    let _lock = pdfium_lock();
    let pdfium = pdfium().expect("pdfium missing — run scripts/fetch-pdfium.sh");

    let start = std::time::Instant::now();
    let layout = run_layout_stage(pdfium, &bundle).unwrap();
    println!(
        "stage 1 layout: {} pages ({:?})",
        layout.pages.len(),
        start.elapsed()
    );

    let t = std::time::Instant::now();
    let tree = run_objects_stage(&bundle).unwrap();
    println!(
        "stage 2 objects: {} objects ({:?})",
        tree.objects.len(),
        t.elapsed()
    );

    let t = std::time::Instant::now();
    let equations = run_equations_stage(&bundle).unwrap();
    println!(
        "stage 3 equations: {} artifacts ({:?})",
        equations.len(),
        t.elapsed()
    );

    let t = std::time::Instant::now();
    let report = run_figures_tables_stage(&bundle).unwrap();
    println!(
        "stage 3 figures/tables: {} figures rendered, {} tables ({} no grid) ({:?})",
        report.figures_rendered,
        report.tables_extracted,
        report.tables_without_grid,
        t.elapsed()
    );
    let t = std::time::Instant::now();
    let citations = run_citations_stage(&bundle).unwrap();
    println!(
        "stage 3 citations: {} entries parsed ({:?})",
        citations.entries.len(),
        t.elapsed()
    );

    if std::env::args().any(|a| a == "--resolve") {
        let t = std::time::Instant::now();
        let resolved = resolve_citations(&bundle, 5).unwrap();
        println!(
            "citation resolution: {resolved} resolved ({:?})",
            t.elapsed()
        );
    }

    if std::env::args().any(|a| a == "--embed") {
        use copilot_core::embeddings::{run_embeddings_stage, Embedder, EmbeddingStore};
        let t = std::time::Instant::now();
        let embedder = Embedder::load().unwrap();
        println!("stage 4 model load: ({:?})", t.elapsed());
        let t = std::time::Instant::now();
        let count = run_embeddings_stage(&bundle, &embedder).unwrap();
        println!("stage 4 embeddings: {count} vectors ({:?})", t.elapsed());

        // Semantic search smoke test (the spec scenario).
        let store = EmbeddingStore::open(&bundle).unwrap().unwrap();
        let query = embedder
            .embed(&["why do they scale the dot product"])
            .unwrap();
        let t = std::time::Instant::now();
        let results = store.search(&query[0], 3);
        let search_time = t.elapsed();
        let tree: copilot_core::objects::SemanticTreeDocument = bundle
            .read_derived_json("semantic_tree.json")
            .unwrap()
            .unwrap();
        println!("semantic search ({search_time:?}):");
        for (id, score) in results {
            if let Some(o) = tree.objects.iter().find(|o| o.id == id) {
                let preview: String = o.content.text.chars().take(70).collect();
                println!("  {score:.3} {preview}");
            }
        }
    }

    println!("total: {:?} → {}", start.elapsed(), out);
}
