//! Code↔paper mapping (v3, code-understanding): which source files/functions
//! implement which paper objects, at line level, with per-link confidence.
//!
//! `reproduction/code_map.json` is derived data with the same honesty rules
//! as extraction: LLM-generated when a provider exists, absent otherwise
//! (the repo browser works fine without it), low confidence styled as such.
//! User corrections are append-only (`notes/code_map_overrides.jsonl`) and
//! re-applied after every re-mapping — never silently reverted.

use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::objects::SemanticTreeDocument;

const CODE_MAP_FILE: &str = "reproduction/code_map.json";
const OVERRIDES_JOURNAL: &str = "notes/code_map_overrides.jsonl";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeLink {
    /// Repo-relative path.
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub object: Uuid,
    pub confidence: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodeMap {
    pub generated_at: String,
    pub links: Vec<CodeLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum MapOverride {
    /// Remove a wrong link (keyed by file + object).
    Delete {
        file: String,
        object: Uuid,
        at: String,
    },
    /// Add or re-point a link (user says "Equation 12 is actually HERE").
    Add { link: CodeLink, at: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CodeMapError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("code map: {0}")]
    Io(#[from] std::io::Error),
}

/// The current map with user corrections applied, or `None` when mapping
/// hasn't run (repo browsing still works — links are an enhancement).
pub fn get(bundle: &Bundle) -> Result<Option<CodeMap>, CodeMapError> {
    let Some(mut map) = bundle.read_derived_json::<CodeMap>(CODE_MAP_FILE)? else {
        return Ok(None);
    };
    apply_overrides(bundle, &mut map)?;
    Ok(Some(map))
}

pub fn record_override(bundle: &Bundle, event: MapOverride) -> Result<(), CodeMapError> {
    bundle.journal(OVERRIDES_JOURNAL).append(&event)?;
    Ok(())
}

fn apply_overrides(bundle: &Bundle, map: &mut CodeMap) -> Result<(), CodeMapError> {
    let events: Vec<MapOverride> = bundle.journal(OVERRIDES_JOURNAL).read_all()?;
    for event in events {
        match event {
            MapOverride::Delete { file, object, .. } => {
                map.links
                    .retain(|l| !(l.file == file && l.object == object));
            }
            MapOverride::Add { link, .. } => {
                // A user link replaces machine links for the same object+file.
                map.links
                    .retain(|l| !(l.file == link.file && l.object == link.object));
                map.links.push(link);
            }
        }
    }
    Ok(())
}

/// Prompt for the mapping pass: repo tree + excerpts of the most relevant
/// source files + the paper's linkable objects (labels + ids).
pub fn mapping_prompt(tree: &SemanticTreeDocument, repo: &Path, max_files: usize) -> String {
    let mut listing = Vec::new();
    collect_source_files(repo, repo, &mut listing);
    listing.sort();
    let file_list = listing.join("\n");

    let mut excerpts = String::new();
    for file in listing.iter().take(max_files) {
        if let Ok(content) = std::fs::read_to_string(repo.join(file)) {
            let numbered: String = content
                .lines()
                .take(200)
                .enumerate()
                .map(|(i, l)| format!("{:>4} {}\n", i + 1, l))
                .collect();
            excerpts.push_str(&format!("\n--- {file} ---\n{numbered}"));
        }
    }

    let objects: String = tree
        .objects
        .iter()
        .filter(|o| {
            matches!(
                o.object_type,
                crate::objects::ObjectType::Equation
                    | crate::objects::ObjectType::Section
                    | crate::objects::ObjectType::Figure
            )
        })
        .map(|o| {
            format!(
                "- {id} | {label}: {text}\n",
                id = o.id,
                label = o.semantic_label.as_deref().unwrap_or("?"),
                text = o.content.text.chars().take(160).collect::<String>(),
            )
        })
        .collect();

    format!(
        "Map this repository's code to the paper objects it implements.\n\
         Respond with JSON only — an array of links:\n\
         [{{\"file\": \"path.py\", \"function\": \"name_or_null\", \"start_line\": 1, \
         \"end_line\": 20, \"object\": \"<uuid from the list>\", \"confidence\": 0.0-1.0}}]\n\
         Only map what you can actually see implemented in the excerpts; omit guesses below 0.3.\n\n\
         Paper objects:\n{objects}\nRepository files:\n{file_list}\n\
         Key file excerpts (line-numbered):\n{excerpts}"
    )
}

fn collect_source_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "__pycache__" {
            continue;
        }
        if path.is_dir() {
            collect_source_files(root, &path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("py" | "rs" | "cu" | "cpp" | "c" | "lua" | "jl")
        ) {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().to_string());
            }
        }
    }
}

/// Parse the LLM's response into validated links: object ids must exist in
/// the tree, files must exist in the repo, line ranges must be sane.
pub fn parse_map(tree: &SemanticTreeDocument, repo: &Path, raw: &str) -> Option<CodeMap> {
    let json = raw
        .find('[')
        .and_then(|s| raw.rfind(']').map(|e| &raw[s..=e]))?;
    let parsed: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    let links: Vec<CodeLink> = parsed
        .into_iter()
        .filter_map(|v| {
            let object: Uuid = v["object"].as_str()?.parse().ok()?;
            if !tree.objects.iter().any(|o| o.id == object) {
                return None;
            }
            let file = v["file"].as_str()?.to_string();
            if !repo.join(&file).is_file() {
                return None;
            }
            let start_line = v["start_line"].as_u64()? as u32;
            let end_line = v["end_line"].as_u64()? as u32;
            if start_line == 0 || end_line < start_line {
                return None;
            }
            Some(CodeLink {
                file,
                function: v["function"].as_str().map(|s| s.to_string()),
                start_line,
                end_line,
                object,
                confidence: (v["confidence"].as_f64().unwrap_or(0.5) as f32).clamp(0.0, 1.0),
            })
        })
        .collect();
    (!links.is_empty()).then(|| CodeMap {
        generated_at: crate::bundle::now_rfc3339(),
        links,
    })
}

/// Run the mapping pass and persist (overrides re-applied on read). `None`
/// from the LLM (no key) leaves any existing map untouched.
pub fn run_mapping(
    bundle: &Bundle,
    tree: &SemanticTreeDocument,
    repo: &Path,
    llm: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<CodeMap>, CodeMapError> {
    let prompt = mapping_prompt(tree, repo, 8);
    let Some(raw) = llm(&prompt) else {
        return get(bundle);
    };
    let Some(map) = parse_map(tree, repo, raw.as_str()) else {
        return get(bundle);
    };
    std::fs::create_dir_all(bundle.root().join(crate::reproduction::REPRODUCTION_DIR))?;
    bundle.write_derived_json(
        CODE_MAP_FILE,
        &map,
        "code_map",
        serde_json::json!({"pipeline_version": "0.1.0", "status": "complete"}),
    )?;
    get(bundle)
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;
    use crate::layout::BBox;
    use crate::objects::{Content, Object, ObjectType};

    fn setup() -> (
        tempfile::TempDir,
        Bundle,
        SemanticTreeDocument,
        Uuid,
        tempfile::TempDir,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("T"), "file").unwrap();
        let object_id = Uuid::new_v4();
        let tree = SemanticTreeDocument {
            pipeline_version: "0.1.0".to_string(),
            objects: vec![Object {
                id: object_id,
                object_type: ObjectType::Equation,
                regions: vec![BBox {
                    page: 0,
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                }],
                content: Content {
                    text: "softmax(QK^T/sqrt(dk))V".to_string(),
                    latex: None,
                    caption: None,
                },
                semantic_label: Some("Equation 12".to_string()),
                relationships: vec![],
                embedding: None,
                content_hash: crate::bundle::sha256_bytes(b"eq"),
                confidence: 0.9,
            }],
            tree: vec![],
        };
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(
            repo.path().join("attention.py"),
            "def sdpa(q, k, v):\n    pass\n",
        )
        .unwrap();
        (tmp, bundle, tree, object_id, repo)
    }

    #[test]
    fn mapping_validates_ids_files_and_lines() {
        let (_tmp, bundle, tree, object, repo) = setup();
        let raw = format!(
            r#"[
              {{"file": "attention.py", "function": "sdpa", "start_line": 1, "end_line": 2, "object": "{object}", "confidence": 0.9}},
              {{"file": "missing.py", "start_line": 1, "end_line": 2, "object": "{object}", "confidence": 0.9}},
              {{"file": "attention.py", "start_line": 5, "end_line": 2, "object": "{object}", "confidence": 0.9}},
              {{"file": "attention.py", "start_line": 1, "end_line": 2, "object": "{other}", "confidence": 0.9}}
            ]"#,
            other = Uuid::new_v4(),
        );
        let map = run_mapping(&bundle, &tree, repo.path(), &|_| Some(raw.clone()))
            .unwrap()
            .expect("map");
        assert_eq!(map.links.len(), 1, "invalid file/lines/ids all rejected");
        assert_eq!(map.links[0].function.as_deref(), Some("sdpa"));

        // No key: existing map served untouched.
        let unchanged = run_mapping(&bundle, &tree, repo.path(), &|_| None)
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.links.len(), 1);
    }

    #[test]
    fn corrections_survive_remapping() {
        let (_tmp, bundle, tree, object, repo) = setup();
        let machine = format!(
            r#"[{{"file": "attention.py", "start_line": 1, "end_line": 1, "object": "{object}", "confidence": 0.6}}]"#
        );
        run_mapping(&bundle, &tree, repo.path(), &|_| Some(machine.clone())).unwrap();

        // User re-points the link to the right lines.
        record_override(
            &bundle,
            MapOverride::Add {
                link: CodeLink {
                    file: "attention.py".into(),
                    function: Some("sdpa".into()),
                    start_line: 1,
                    end_line: 2,
                    object,
                    confidence: 1.0,
                },
                at: crate::bundle::now_rfc3339(),
            },
        )
        .unwrap();
        let corrected = get(&bundle).unwrap().unwrap();
        assert_eq!(corrected.links.len(), 1);
        assert_eq!(corrected.links[0].end_line, 2, "user link wins");

        // Re-mapping regenerates the machine map — the correction re-applies.
        run_mapping(&bundle, &tree, repo.path(), &|_| Some(machine.clone())).unwrap();
        let after = get(&bundle).unwrap().unwrap();
        assert_eq!(after.links[0].end_line, 2, "correction survived re-mapping");
        assert_eq!(after.links[0].confidence, 1.0);
    }
}
