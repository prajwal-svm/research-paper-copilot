//! Run the concepts stage (heuristic, offline) on an existing bundle.
//!   cargo run -p copilot-core --example run_concepts -- <bundle>

use copilot_core::bundle::Bundle;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: run_concepts <bundle>");
    let bundle = Bundle::open(std::path::Path::new(&path)).unwrap();
    let graph = copilot_core::concepts::run_concepts_stage(&bundle, None).unwrap();
    println!(
        "{}: {} nodes, {} edges",
        graph.extraction,
        graph.nodes.len(),
        graph.edges.len()
    );
}
