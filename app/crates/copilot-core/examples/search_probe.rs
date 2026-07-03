//! Dev probe: run combined search against a bundle.
use copilot_core::bundle::Bundle;
use copilot_core::embeddings::Embedder;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: search_probe <bundle> <query...>");
    let query = std::env::args().skip(2).collect::<Vec<_>>().join(" ");
    let bundle = Bundle::open(std::path::Path::new(&path)).unwrap();
    let embedder = Embedder::load().ok();
    let start = std::time::Instant::now();
    let results = copilot_core::search::search(&bundle, embedder.as_ref(), &query, 5).unwrap();
    println!(
        "search took {:?} (semantic_available={})",
        start.elapsed(),
        results.semantic_available
    );
    for hit in &results.exact {
        println!(
            "  exact  {:.2} {}",
            hit.score,
            hit.snippet.chars().take(70).collect::<String>()
        );
    }
    for hit in &results.semantic {
        println!(
            "  sem    {:.2} {}",
            hit.score,
            hit.snippet.chars().take(70).collect::<String>()
        );
    }
}
