//! Golden tests for the shipped reference exporters: the checked-in wasm
//! runs through the real plugin host over a fixed bundle view — proving
//! the public ABI is sufficient for real exporters (spec: plugin-api).
//! Rebuild the wasm: cd plugins-src/exporters &&
//!   cargo build --release --target wasm32-unknown-unknown
//! then copy target/wasm32-unknown-unknown/release/reference_exporters.wasm
//! to plugins/reference-exporters/plugin.wasm.

use copilot_core::plugin::{discover, run_plugin, PluginStatus};

fn plugins_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn fixed_view() -> serde_json::Value {
    serde_json::json!({
        "metadata": { "paper": { "title": "Attention Is All You Need" } },
        "knowledge_graph": {
            "nodes": [
                { "id": "n1", "name": "Attention", "explanation": "Weighted lookup over values." },
                { "id": "n2", "name": "Softmax", "explanation": "Normalizes scores." }
            ],
            "edges": [ { "from": "n2", "to": "n1", "kind": "prerequisite_of" } ]
        },
        "notes": [ { "text": "Check eq. 1 scaling." } ],
        "flashcards": { "cards": [
            { "front": "What is attention?", "back": "A weighted lookup.", "concept_id": "n1" }
        ] },
        "glossary": null
    })
}

fn run(format: &str) -> serde_json::Value {
    let plugin = discover(&plugins_dir())
        .into_iter()
        .find(|p| p.manifest.name == "reference-exporters")
        .expect("shipped plugin present");
    assert_eq!(plugin.status, PluginStatus::Compatible);
    let input = serde_json::json!({ "format": format, "view": fixed_view() });
    let report = run_plugin(&plugin, input.to_string().as_bytes(), &Default::default()).unwrap();
    assert!(
        report.blocked.is_empty(),
        "exporters declare no permissions"
    );
    serde_json::from_slice(&report.output).unwrap()
}

#[test]
fn anki_deck_carries_anchor_tags() {
    let out = run("anki");
    let deck = out["files"]["attention-is-all-you-need.txt"]
        .as_str()
        .unwrap();
    assert!(deck.contains("#separator:tab"), "{deck}");
    assert!(
        deck.contains("What is attention?\tA weighted lookup.\tattention-is-all-you-need::n1"),
        "anchors as tags: {deck}"
    );
}

#[test]
fn obsidian_vault_has_backlinks() {
    let out = run("obsidian");
    let files = out["files"].as_object().unwrap();
    let attention = files["attention-is-all-you-need/Attention.md"]
        .as_str()
        .unwrap();
    assert!(
        attention.contains("[[Softmax]]"),
        "backlink present: {attention}"
    );
    assert!(files.contains_key("attention-is-all-you-need/Attention Is All You Need.md"));
}

#[test]
fn latex_notes_are_a_complete_document() {
    let out = run("latex");
    let tex = out["files"]["attention-is-all-you-need.tex"]
        .as_str()
        .unwrap();
    assert!(tex.starts_with("\\documentclass"));
    assert!(tex.contains("\\section{Attention}"));
    assert!(tex.contains("\\item Check eq. 1 scaling."));
    assert!(tex.trim_end().ends_with("\\end{document}"));
}
