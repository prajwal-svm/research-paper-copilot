//! Build the bundled sample paper ("Attention Is All You Need"), fully
//! enriched: all pipeline stages + embeddings + resolved citations +
//! pre-generated explanations so first-run works with no key and no network.
//!
//!   cargo run --release -p copilot-core --example build_sample -- <out-dir>

use copilot_core::bundle::{Bundle, Paper};
use copilot_core::objects::{ObjectType, SemanticTreeDocument};
use copilot_core::pipeline::{run, PipelineOptions};

const ARXIV_ID: &str = "1706.03762";

/// Pre-generated explanations keyed by the object's semantic label. Written
/// into `glossary/pregenerated/<object-uuid>.md`; the interaction panel
/// serves these as cached enrichment in no-key mode.
const PREGENERATED: [(&str, &str); 3] = [
    (
        "Equation 1",
        include_str!("sample_content/equation-1-explanation.md"),
    ),
    (
        "Figure 1",
        include_str!("sample_content/figure-1-explanation.md"),
    ),
    (
        "Figure 2",
        include_str!("sample_content/figure-2-explanation.md"),
    ),
];

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: build_sample <out-dir>");
    let out = std::path::Path::new(&out);
    if out.exists() {
        std::fs::remove_dir_all(out).unwrap();
    }

    // Fetch the paper (PDF + metadata) from arXiv.
    eprintln!("fetching arXiv:{ARXIV_ID}…");
    let fetched = copilot_core::arxiv::fetch(ARXIV_ID).expect("arXiv fetch");
    let mut paper = Paper::new(fetched.title.clone());
    paper.authors = fetched.authors.clone();
    paper.abstract_text = fetched.abstract_text.clone();
    paper.extra.insert(
        "identifiers".to_string(),
        serde_json::json!({"arxiv_id": ARXIV_ID}),
    );
    let bundle = Bundle::create(out, &fetched.pdf, paper, "bundled").unwrap();

    // Full pipeline including embeddings.
    run(&bundle, &PipelineOptions::default(), &mut |e| {
        eprintln!("  {e:?}");
    })
    .unwrap();

    // Resolve as many citations as the APIs allow (politeness cap).
    let resolved = copilot_core::citations::resolve_citations(&bundle, 40).unwrap();
    eprintln!("resolved {resolved} citations");

    // Attach pre-generated explanations to their objects by semantic label.
    let tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")
        .unwrap()
        .unwrap();
    let dir = out.join("glossary/pregenerated");
    std::fs::create_dir_all(&dir).unwrap();
    let mut attached = 0;
    for (label, content) in PREGENERATED {
        let object = tree
            .objects
            .iter()
            .filter(|o| {
                matches!(
                    o.object_type,
                    ObjectType::Equation | ObjectType::Figure | ObjectType::Table
                )
            })
            .find(|o| {
                o.semantic_label
                    .as_deref()
                    .is_some_and(|l| l.starts_with(label))
            });
        match object {
            Some(object) => {
                std::fs::write(dir.join(format!("{}.md", object.id)), content).unwrap();
                attached += 1;
            }
            None => eprintln!("WARN: no object found for {label:?}"),
        }
    }
    eprintln!(
        "attached {attached}/{} pre-generated explanations",
        PREGENERATED.len()
    );
    assert!(attached >= 2, "sample must ship with working explanations");
    eprintln!("sample bundle ready at {}", out.display());
    eprintln!(
        "v2: now run `cargo run --release -p copilot-core --example enrich_sample -- {}`\n\
         to add the LLM concept graph + pre-generated lessons/quizzes/flashcards\n\
         (needs the zai-glm key in the keychain).",
        out.display()
    );
}
