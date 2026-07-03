//! Context-efficiency check (v2 task 3.3): graph-first assembly must cut
//! approximate prompt tokens by ≥60% vs the v1 object+relationships baseline
//! on a scripted question set over the sample paper.
//!
//! Runs against the bundled sample (offline, heuristic graph built
//! in-memory — the resource bundle is never written to).

use copilot_core::bundle::Bundle;
use copilot_core::concepts::heuristic_graph;
use copilot_core::context::{assemble, assemble_graph, Action, GraphInputs};
use copilot_core::learning::LearnerSnapshot;
use copilot_core::objects::SemanticTreeDocument;

#[test]
fn graph_assembly_cuts_prompt_tokens_by_60_percent() {
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src-tauri/resources/sample/attention-is-all-you-need.research");
    assert!(
        sample.is_dir(),
        "sample bundle missing: {}",
        sample.display()
    );
    let bundle = Bundle::open(&sample).unwrap();
    let tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")
        .unwrap()
        .expect("semantic_tree.json");
    let title = bundle.metadata().unwrap().paper.title;

    let graph = heuristic_graph(&title, &tree);
    assert!(graph.nodes.len() >= 5, "heuristic graph too small to test");
    let snapshot = LearnerSnapshot::default();
    let inputs = GraphInputs {
        graph: &graph,
        snapshot: &snapshot,
        episodes: &[],
        node_globals: None,
    };

    // Scripted set: every concept-anchored object, one Explain and one Ask.
    let anchors: Vec<uuid::Uuid> = graph
        .nodes
        .iter()
        .flat_map(|n| n.object_ids.iter().copied())
        .collect();
    assert!(anchors.len() >= 5);

    let questions = [
        (Action::Explain, None),
        (
            Action::Ask,
            Some("how does this relate to the rest of the paper?"),
        ),
    ];

    // Generous budget so we compare natural assembly sizes, not the cap.
    let budget = 150_000;
    let mut v1_tokens = 0usize;
    let mut graph_tokens = 0usize;
    let mut compared = 0usize;
    for &anchor in &anchors {
        for (action, question) in questions {
            let v1 = assemble(&tree, &title, anchor, action, question, &[], None, budget)
                .expect("anchor exists");
            let Some(v2) = assemble_graph(
                &tree,
                &title,
                anchor,
                action,
                question,
                &[],
                None,
                budget,
                &inputs,
            ) else {
                continue;
            };
            v1_tokens += v1.approx_tokens;
            graph_tokens += v2.approx_tokens;
            compared += 1;
        }
    }
    assert!(compared >= 10, "only {compared} comparable prompts");

    let reduction = 1.0 - (graph_tokens as f64 / v1_tokens as f64);
    eprintln!(
        "scripted set: {compared} prompts, v1 {v1_tokens} tokens → graph {graph_tokens} tokens \
         ({:.0}% reduction)",
        reduction * 100.0
    );
    assert!(
        reduction >= 0.60,
        "context reduction {:.0}% below the 60% target (v1 {v1_tokens} → graph {graph_tokens})",
        reduction * 100.0
    );
}
