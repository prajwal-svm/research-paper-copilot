//! Published JSON Schemas for the `.research` format, generated from the
//! core serde types — the single source of truth. Third parties build
//! against `schemas/generated/<format-major>/`; the `generated_schemas_do_
//! not_drift` test fails whenever code and committed schemas diverge
//! (regenerate with `UPDATE_SCHEMAS=1 cargo test -p copilot-core schemas`).

use std::fs;
use std::io;
use std::path::Path;

use schemars::{schema_for, Schema};

use crate::bundle::FORMAT_MAJOR;

/// Every schema'd bundle file kind: (file kind name, schema).
/// Kind names match the bundle file they describe.
pub fn schema_set() -> Vec<(&'static str, Schema)> {
    vec![
        ("metadata", schema_for!(crate::bundle::Metadata)),
        ("layout", schema_for!(crate::layout::LayoutDocument)),
        (
            "semantic_tree",
            schema_for!(crate::objects::SemanticTreeDocument),
        ),
        (
            "citations",
            schema_for!(crate::citations::CitationsDocument),
        ),
        (
            "knowledge_graph",
            schema_for!(crate::concepts::KnowledgeGraph),
        ),
        ("chat_message", schema_for!(crate::chat::StoredChatMessage)),
    ]
}

fn render(schema: &Schema) -> String {
    let mut text = serde_json::to_string_pretty(schema).expect("schema serializes");
    text.push('\n');
    text
}

/// Write the full schema set into `dir` (one `<kind>.schema.json` each).
pub fn emit(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for (kind, schema) in schema_set() {
        fs::write(dir.join(format!("{kind}.schema.json")), render(&schema))?;
    }
    Ok(())
}

/// One schema violation found by [`validate_bundle`].
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct Violation {
    /// Bundle-relative file the violation is in.
    pub file: String,
    /// JSON path to the offending value (e.g. `/paper/authors/0`).
    pub json_path: String,
    /// Human-readable description of what the schema expected.
    pub message: String,
}

/// Which bundle files each schema kind describes.
fn file_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "metadata" => Some("metadata.json"),
        "layout" => Some("layout.json"),
        "semantic_tree" => Some("semantic_tree.json"),
        "citations" => Some("citations.json"),
        "knowledge_graph" => Some("knowledge_graph.json"),
        // JSONL kinds (chat_message) are validated per-line below.
        _ => None,
    }
}

/// Validate a bundle directory against the published schemas. Reports every
/// violation by file and JSON path; files a bundle legitimately lacks (not
/// yet derived) are skipped. Unknown files are never flagged — the format's
/// unknown-file rule applies to validation too.
pub fn validate_bundle(root: &Path) -> io::Result<Vec<Violation>> {
    let mut violations = Vec::new();
    for (kind, schema) in schema_set() {
        let compiled = jsonschema::validator_for(
            &serde_json::to_value(&schema).expect("schema is valid json"),
        )
        .expect("generated schemas always compile");

        if let Some(file) = file_for_kind(kind) {
            let path = root.join(file);
            if !path.exists() {
                continue;
            }
            let raw = fs::read_to_string(&path)?;
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(doc) => {
                    for error in compiled.iter_errors(&doc) {
                        violations.push(Violation {
                            file: file.to_string(),
                            json_path: error.instance_path().to_string(),
                            message: error.to_string(),
                        });
                    }
                }
                Err(e) => violations.push(Violation {
                    file: file.to_string(),
                    json_path: String::new(),
                    message: format!("not valid JSON: {e}"),
                }),
            }
        } else if kind == "chat_message" {
            let chats = root.join("chats");
            let Ok(entries) = fs::read_dir(&chats) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let file = format!("chats/{}", entry.file_name().to_string_lossy());
                for (index, line) in fs::read_to_string(&path)?.lines().enumerate() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<serde_json::Value>(line) {
                        Ok(doc) => {
                            // Correction events share the journal; only
                            // message-shaped lines are schema-checked.
                            if doc.get("op").is_some() {
                                continue;
                            }
                            for error in compiled.iter_errors(&doc) {
                                violations.push(Violation {
                                    file: file.clone(),
                                    json_path: format!(
                                        "line {}{}",
                                        index + 1,
                                        error.instance_path()
                                    ),
                                    message: error.to_string(),
                                });
                            }
                        }
                        Err(e) => violations.push(Violation {
                            file: file.clone(),
                            json_path: format!("line {}", index + 1),
                            message: format!("not valid JSON: {e}"),
                        }),
                    }
                }
            }
        }
    }
    Ok(violations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn published_dir() -> PathBuf {
        // crates/copilot-core → app/schemas/generated/<major>
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/generated")
            .join(FORMAT_MAJOR.to_string())
    }

    #[test]
    fn core_written_bundle_validates_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        crate::bundle::Bundle::create(
            &root,
            b"%PDF-1.5 fake",
            crate::bundle::Paper::new("Valid Paper"),
            "file",
        )
        .unwrap();
        let violations = validate_bundle(&root).unwrap();
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn corrupted_fixture_reports_exact_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        crate::bundle::Bundle::create(
            &root,
            b"%PDF-1.5 fake",
            crate::bundle::Paper::new("Broken Paper"),
            "file",
        )
        .unwrap();
        // Break metadata: title must be a string.
        let path = root.join("metadata.json");
        let mut doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        doc["paper"]["title"] = serde_json::json!(42);
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();

        let violations = validate_bundle(&root).unwrap();
        assert!(
            violations
                .iter()
                .any(|v| v.file == "metadata.json" && v.json_path == "/paper/title"),
            "expected /paper/title violation, got {violations:?}"
        );
    }

    /// Bless pattern: `UPDATE_SCHEMAS=1` regenerates the published files;
    /// a normal run proves code and published schemas are byte-identical.
    #[test]
    fn generated_schemas_do_not_drift() {
        let dir = published_dir();
        if std::env::var("UPDATE_SCHEMAS").is_ok() {
            emit(&dir).expect("write schemas");
            return;
        }
        for (kind, schema) in schema_set() {
            let path = dir.join(format!("{kind}.schema.json"));
            let published = std::fs::read_to_string(&path).unwrap_or_else(|_| {
                panic!(
                    "missing published schema {} — run UPDATE_SCHEMAS=1 cargo test -p copilot-core schemas",
                    path.display()
                )
            });
            assert_eq!(
                published,
                render(&schema),
                "schema drift for {kind}: regenerate with UPDATE_SCHEMAS=1 cargo test -p copilot-core schemas"
            );
        }
    }
}
