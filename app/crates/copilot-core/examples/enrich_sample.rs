//! Enrich an existing sample bundle for v2's zero-setup rule: LLM concept
//! graph + pre-generated lessons, quizzes, and flashcards for the first N
//! course entries, so reading mode works first-run with no key.
//!
//!   cargo run --release -p copilot-core --example enrich_sample -- <bundle> [n]
//!
//! Uses the zai-glm preset (key from the OS keychain): light tier for
//! extraction/quizzes/flashcards, strong tier for lesson prose.

use copilot_core::ai::{ChatMessage, ModelClass};
use copilot_core::bundle::Bundle;
use copilot_core::objects::SemanticTreeDocument;
use copilot_core::provider_config::{preset, ProviderConfig};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: enrich_sample <bundle> [n]");
    let n: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let bundle = Bundle::open(std::path::Path::new(&path)).unwrap();
    let tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")
        .unwrap()
        .expect("semantic_tree.json");

    let config = ProviderConfig::from_preset(&preset("zai-glm").expect("zai-glm preset"));
    let call = |class: ModelClass, prompt: &str| -> Option<String> {
        let provider = config.provider(class).ok()?;
        let messages = [ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
            images: vec![],
        }];
        provider.stream_chat(&messages, &mut |_| {}).ok()
    };
    let light = |prompt: &str| call(ModelClass::Light, prompt);
    let strong = |prompt: &str| call(ModelClass::Strong, prompt);

    eprintln!("extracting concept graph (light tier)…");
    let graph = copilot_core::concepts::run_concepts_stage(&bundle, Some(&light)).unwrap();
    eprintln!(
        "  {} extraction: {} nodes, {} edges",
        graph.extraction,
        graph.nodes.len(),
        graph.edges.len()
    );
    assert_eq!(graph.extraction, "llm", "sample must ship an LLM graph");

    let sequence = copilot_core::lessons::lesson_sequence(&graph, &|_| false);
    for (i, entry) in sequence.iter().take(n).enumerate() {
        eprintln!("[{}/{n}] {}…", i + 1, entry.name);
        let lesson =
            copilot_core::lessons::lesson_generate(&bundle, &graph, &tree, entry.node, &strong)
                .unwrap();
        eprintln!(
            "    lesson: {}",
            if lesson.is_some() { "ok" } else { "SKIPPED" }
        );
        let quiz = copilot_core::lessons::quiz_generate(&bundle, &graph, &tree, entry.node, &light)
            .unwrap();
        eprintln!(
            "    quiz: {}",
            quiz.map(|q| format!("{} items", q.items.len()))
                .unwrap_or_else(|| "SKIPPED".to_string())
        );
        let deck = copilot_core::lessons::deck_generate(&bundle, &graph, &tree, entry.node, &light)
            .unwrap();
        eprintln!(
            "    flashcards: {}",
            deck.map(|d| format!("{} cards", d.cards.len()))
                .unwrap_or_else(|| "SKIPPED".to_string())
        );
    }
    eprintln!("sample enriched at {path}");
}
