//! Spike (v2 task 1.1): heuristic vs LLM concept extraction on a real bundle.
//!
//!   cargo run --release -p copilot-core --example concept_spike -- <bundle> [--llm light|strong]

use copilot_core::ai::{ChatMessage, ModelClass};
use copilot_core::bundle::Bundle;
use copilot_core::concepts::*;
use copilot_core::objects::SemanticTreeDocument;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: concept_spike <bundle> [--llm tier]");
    let bundle = Bundle::open(std::path::Path::new(&path)).unwrap();
    let tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")
        .unwrap()
        .expect("semantic_tree.json");
    let title = bundle.metadata().unwrap().paper.title;

    let heuristic = heuristic_graph(&title, &tree);
    println!(
        "HEURISTIC: {} nodes, {} edges",
        heuristic.nodes.len(),
        heuristic.edges.len()
    );
    for node in &heuristic.nodes {
        println!(
            "  - {} (conf {:.2}, {} anchors)",
            node.name,
            node.confidence,
            node.object_ids.len()
        );
    }

    let tier = match std::env::args().position(|a| a == "--llm") {
        Some(i) => std::env::args()
            .nth(i + 1)
            .unwrap_or_else(|| "light".into()),
        None => return,
    };
    let class = if tier == "strong" {
        ModelClass::Strong
    } else {
        ModelClass::Light
    };

    // Use the zai-glm config directly (spike runs outside the app).
    let config = copilot_core::provider_config::ProviderConfig::from_preset(
        &copilot_core::provider_config::preset("zai-glm").unwrap(),
    );
    let provider = config.provider(class).expect("zai key in keychain");
    println!("\nLLM ({}, {}):", tier, config.model_for(class));

    let prompt = extraction_prompt(&tree, &title);
    println!("prompt: ~{} chars", prompt.len());
    let start = std::time::Instant::now();
    let raw = provider
        .stream_chat(
            &[ChatMessage {
                role: "user".into(),
                content: prompt,
            }],
            &mut |_| {},
        )
        .expect("llm call");
    println!("elapsed: {:?}", start.elapsed());

    match parse_llm_graph(&title, &tree, &raw) {
        Some(graph) => {
            println!(
                "LLM: {} nodes, {} edges",
                graph.nodes.len(),
                graph.edges.len()
            );
            for node in &graph.nodes {
                println!(
                    "  - {} (conf {:.2}, {} anchors) — {}",
                    node.name,
                    node.confidence,
                    node.object_ids.len(),
                    node.description.as_deref().unwrap_or("")
                );
            }
            for edge in &graph.edges {
                let name = |id| {
                    graph
                        .nodes
                        .iter()
                        .find(|n| n.id == id)
                        .map(|n| n.name.as_str())
                        .unwrap_or("?")
                };
                println!(
                    "  {} --{:?}--> {}",
                    name(edge.from),
                    edge.kind,
                    name(edge.to)
                );
            }
        }
        None => println!(
            "LLM output unusable; raw head: {}",
            &raw.chars().take(400).collect::<String>()
        ),
    }
}
