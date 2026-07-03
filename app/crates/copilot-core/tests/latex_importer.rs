//! The shipped LaTeX importer through the real plugin host: source in,
//! schema-valid bundle out, with explicit page-geometry degradation.

use copilot_core::bundle::{Bundle, Paper};
use copilot_core::plugin::{cover_pdf, discover, run_plugin, PluginStatus};

const SAMPLE_TEX: &str = r#"
\documentclass{article}
\title{Sparse Attention at Scale}
\author{Ada Lovelace \and Alan Turing}
\begin{document}
\maketitle
\begin{abstract}
We study sparse attention. % comment stripped
\end{abstract}
\section{Introduction}
Attention is expensive.
\section{Method}
We prune heads.
\end{document}
"#;

#[test]
fn latex_source_becomes_a_valid_bundle() {
    let plugins = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let importer = discover(&plugins)
        .into_iter()
        .find(|p| p.manifest.name == "latex-importer")
        .expect("shipped importer present");
    assert_eq!(importer.status, PluginStatus::Compatible);

    let input = serde_json::json!({ "source": SAMPLE_TEX });
    let report = run_plugin(&importer, input.to_string().as_bytes(), &Default::default()).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&report.output).unwrap();

    assert_eq!(parsed["metadata"]["title"], "Sparse Attention at Scale");
    assert_eq!(parsed["metadata"]["abstract"], "We study sparse attention.");
    assert_eq!(parsed["metadata"]["authors"][1], "Alan Turing");
    assert_eq!(parsed["sections"][1]["heading"], "Method");

    // Host-side assembly: cover PDF (explicit degradation) + metadata +
    // sections as an outline note → validates against the schemas.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("imported.research");
    let mut paper = Paper::new(parsed["metadata"]["title"].as_str().unwrap());
    paper.abstract_text = parsed["metadata"]["abstract"].as_str().map(str::to_string);
    let pdf = cover_pdf(
        paper.title.as_str(),
        "imported from LaTeX source — page geometry unavailable",
    );
    let bundle = Bundle::create(&root, &pdf, paper, "latex").unwrap();
    let mut outline = String::new();
    for section in parsed["sections"].as_array().unwrap() {
        outline.push_str(&format!(
            "## {}\n\n{}\n\n",
            section["heading"].as_str().unwrap_or(""),
            section["text"].as_str().unwrap_or("")
        ));
    }
    std::fs::create_dir_all(root.join("research")).unwrap();
    std::fs::write(root.join("research/imported-outline.md"), outline).unwrap();

    let violations = copilot_core::schemas::validate_bundle(&root).unwrap();
    assert!(violations.is_empty(), "{violations:?}");
    assert!(bundle.metadata().unwrap().paper.abstract_text.is_some());
}
