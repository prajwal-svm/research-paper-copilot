//! Dev tool: run stages 1–2 on a PDF and dump artifacts or a summary.
//!
//!   cargo run -p copilot-core --example pipeline_dump -- paper.pdf [--layout|--tree]

use copilot_core::layout::{analyze, pdfium};
use copilot_core::objects::{build_semantic_tree, ObjectType};

fn main() {
    let path = std::env::args().nth(1).expect("usage: pipeline_dump <pdf>");
    let pdfium = pdfium().expect("pdfium missing — run scripts/fetch-pdfium.sh");
    let document = pdfium
        .load_pdf_from_file(&path, None)
        .expect("failed to load pdf");

    let layout = analyze(&document);
    let tree = build_semantic_tree(&layout);

    if std::env::args().any(|a| a == "--layout") {
        println!("{}", serde_json::to_string_pretty(&layout).unwrap());
        return;
    }
    if std::env::args().any(|a| a == "--tree") {
        println!("{}", serde_json::to_string_pretty(&tree).unwrap());
        return;
    }

    let count = |t: ObjectType| tree.objects.iter().filter(|o| o.object_type == t).count();
    println!(
        "pages: {}  objects: {}  sections: {}  paragraphs: {}  sentences: {}",
        layout.pages.len(),
        tree.objects.len(),
        count(ObjectType::Section),
        count(ObjectType::Paragraph),
        count(ObjectType::Sentence),
    );
    println!("\nsection outline:");
    fn walk(
        nodes: &[copilot_core::objects::TreeNode],
        tree: &copilot_core::objects::SemanticTreeDocument,
        depth: usize,
    ) {
        for node in nodes {
            if let Some(object) = tree.objects.iter().find(|o| o.id == node.object) {
                if object.object_type == ObjectType::Section {
                    println!("{:indent$}{}", "", object.content.text, indent = depth * 2);
                }
            }
            walk(&node.children, tree, depth + 1);
        }
    }
    walk(&tree.tree, &tree, 0);
}
