//! Golden-corpus regression test (task 2.7): ≥95% of the arXiv ML corpus
//! must produce a *usable* bundle. "Usable" is defined below and is
//! deliberately about the reading experience, not parsing perfection.
//!
//! Ignored by default (downloads PDFs). Run:
//!   cargo test -p copilot-core --test golden_corpus -- --ignored --nocapture
//!
//! PDFs are cached in target/golden-corpus/ so re-runs are offline.

use copilot_core::bundle::Paper;
use copilot_core::objects::{ObjectType, SemanticTreeDocument};
use copilot_core::pipeline::{import_pdf, PipelineOptions, ProgressEvent};

/// arXiv ML papers across layout styles (NIPS two-column-ish, CVPR
/// two-column, single-column reports).
const CORPUS: [(&str, &str); 6] = [
    ("1706.03762", "Attention Is All You Need"),
    ("1512.03385", "Deep Residual Learning (ResNet)"),
    ("1810.04805", "BERT"),
    (
        "1409.0473",
        "Neural MT by Jointly Learning to Align (Bahdanau)",
    ),
    ("2010.11929", "An Image is Worth 16x16 Words (ViT)"),
    (
        "1301.3781",
        "Efficient Estimation of Word Representations (word2vec)",
    ),
];

struct Usability {
    arxiv_id: &'static str,
    usable: bool,
    problems: Vec<String>,
}

fn assess(arxiv_id: &'static str, pdf: &[u8], workdir: &std::path::Path) -> Usability {
    let mut problems = Vec::new();
    let root = workdir.join(format!("{arxiv_id}.research"));

    let mut finished_usable = false;
    let bundle = import_pdf(
        pdf,
        &root,
        Paper::new(arxiv_id),
        "file",
        &PipelineOptions::local(false),
        &mut |e| {
            if let ProgressEvent::PipelineFinished { usable } = e {
                finished_usable = usable;
            }
        },
    )
    .expect("bundle creation");

    if !finished_usable {
        problems.push("pipeline reported not usable".to_string());
        return Usability {
            arxiv_id,
            usable: false,
            problems,
        };
    }

    let layout: copilot_core::layout::LayoutDocument = bundle
        .read_derived_json("layout.json")
        .unwrap()
        .expect("layout.json");
    let pages_with_blocks = layout.pages.iter().filter(|p| !p.blocks.is_empty()).count();
    if (pages_with_blocks as f32) < layout.pages.len() as f32 * 0.6 {
        problems.push(format!(
            "only {pages_with_blocks}/{} pages have blocks",
            layout.pages.len()
        ));
    }

    let tree: SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")
        .unwrap()
        .expect("semantic_tree.json");
    let sections = tree
        .objects
        .iter()
        .filter(|o| o.object_type == ObjectType::Section)
        .count();
    let paragraphs = tree
        .objects
        .iter()
        .filter(|o| o.object_type == ObjectType::Paragraph)
        .count();
    if sections < 3 {
        problems.push(format!("only {sections} sections detected"));
    }
    if paragraphs < 20 {
        problems.push(format!("only {paragraphs} paragraphs detected"));
    }

    let citations: copilot_core::citations::CitationsDocument = bundle
        .read_derived_json("citations.json")
        .unwrap()
        .expect("citations.json");
    if citations.entries.len() < 5 {
        problems.push(format!(
            "only {} bibliography entries parsed",
            citations.entries.len()
        ));
    }

    // Concept graph (v2 stage 5): the corpus runs keyless, so the heuristic
    // fallback must still produce a non-trivial, honestly-flagged graph.
    let graph: copilot_core::concepts::KnowledgeGraph = bundle
        .read_derived_json("knowledge_graph.json")
        .unwrap()
        .expect("knowledge_graph.json");
    if graph.nodes.len() < 3 {
        problems.push(format!("only {} concept nodes", graph.nodes.len()));
    }
    if graph.extraction == "heuristic"
        && graph
            .nodes
            .iter()
            .any(|n| n.confidence > 0.4 + f32::EPSILON)
    {
        problems.push("heuristic concept confidence above 0.4 cap".to_string());
    }
    // Sample paper: core concepts must surface even in the degraded graph.
    if arxiv_id == "1706.03762" {
        for expected in ["attention", "encoder"] {
            if !graph
                .nodes
                .iter()
                .any(|n| n.name.to_lowercase().contains(expected))
            {
                problems.push(format!("core concept '{expected}' missing from graph"));
            }
        }
    }

    Usability {
        arxiv_id,
        usable: problems.is_empty(),
        problems,
    }
}

#[test]
#[ignore = "downloads the corpus; run explicitly"]
fn golden_corpus_95_percent_usable() {
    let cache = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/golden-corpus");
    std::fs::create_dir_all(&cache).unwrap();
    let workdir = tempfile::tempdir().unwrap();

    let mut results = Vec::new();
    for (arxiv_id, title) in CORPUS {
        let pdf_path = cache.join(format!("{arxiv_id}.pdf"));
        if !pdf_path.is_file() {
            let url = format!("https://arxiv.org/pdf/{arxiv_id}");
            eprintln!("fetching {url}");
            let mut reader = ureq::get(&url)
                .timeout(std::time::Duration::from_secs(60))
                .call()
                .unwrap_or_else(|e| panic!("download {arxiv_id}: {e}"))
                .into_reader();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut reader, &mut bytes).unwrap();
            std::fs::write(&pdf_path, &bytes).unwrap();
        }
        let pdf = std::fs::read(&pdf_path).unwrap();
        let result = assess(arxiv_id, &pdf, workdir.path());
        eprintln!(
            "{} {title}: {}",
            if result.usable { "ok  " } else { "FAIL" },
            if result.problems.is_empty() {
                "usable".to_string()
            } else {
                result.problems.join("; ")
            }
        );
        results.push(result);
    }

    let usable = results.iter().filter(|r| r.usable).count();
    let ratio = usable as f32 / results.len() as f32;
    eprintln!(
        "\ngolden corpus: {usable}/{} usable ({:.0}%)",
        results.len(),
        ratio * 100.0
    );
    assert!(
        ratio >= 0.95,
        "corpus usability below 95%: {:#?}",
        results
            .iter()
            .filter(|r| !r.usable)
            .map(|r| (r.arxiv_id, r.problems.clone()))
            .collect::<Vec<_>>()
    );
}
