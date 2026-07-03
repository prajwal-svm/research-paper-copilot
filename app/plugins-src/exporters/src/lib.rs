//! Reference exporters, one wasm module per capability, selected by the
//! `format` field the host puts in the input JSON:
//!   { "format": "anki" | "obsidian" | "latex", "view": <bundle view> }
//!
//! Output contract (JSON):
//!   { "files": { "<relative path>": "<content>" } }
//! The host writes files where the user chooses. Uses ONLY the public ABI.

use std::alloc::{alloc as raw_alloc, Layout};

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    unsafe { raw_alloc(Layout::from_size_align(size as usize, 1).unwrap()) as i32 }
}

#[no_mangle]
pub extern "C" fn run(ptr: i32, len: i32) -> i64 {
    let input = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let output = export(input).unwrap_or_else(|e| {
        serde_json::json!({ "error": e }).to_string().into_bytes()
    });
    let out_ptr = alloc(output.len() as i32);
    unsafe {
        std::ptr::copy_nonoverlapping(output.as_ptr(), out_ptr as *mut u8, output.len());
    }
    ((out_ptr as u32 as i64) << 32) | (output.len() as u32 as i64)
}

fn export(input: &[u8]) -> Result<Vec<u8>, String> {
    let doc: serde_json::Value = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let format = doc["format"].as_str().unwrap_or("anki");
    let view = &doc["view"];
    let title = view["metadata"]["paper"]["title"].as_str().unwrap_or("paper");
    let files = match format {
        "anki" => anki(view, title),
        "obsidian" => obsidian(view, title),
        "latex" => latex(view, title),
        other => return Err(format!("unknown format {other}")),
    };
    Ok(serde_json::json!({ "files": files }).to_string().into_bytes())
}

fn slug(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Anki: tab-separated deck file (front, back, tags), tags carry the
/// concept anchors so cards stay traceable to the paper.
fn anki(view: &serde_json::Value, title: &str) -> serde_json::Value {
    let mut lines = vec!["#separator:tab".to_string(), "#html:false".to_string(), "#columns:Front\tBack\tTags".to_string()];
    if let Some(cards) = view["flashcards"]["cards"].as_array() {
        for card in cards {
            let front = card["front"].as_str().unwrap_or_default().replace(['\t', '\n'], " ");
            let back = card["back"].as_str().unwrap_or_default().replace(['\t', '\n'], " ");
            let anchor = card["concept_id"].as_str().unwrap_or("unanchored");
            lines.push(format!("{front}\t{back}\t{}::{anchor}", slug(title)));
        }
    }
    serde_json::json!({ format!("{}.txt", slug(title)): lines.join("\n") + "\n" })
}

/// Obsidian: one note per concept with [[backlinks]] along graph edges,
/// plus an index note.
fn obsidian(view: &serde_json::Value, title: &str) -> serde_json::Value {
    let mut files = serde_json::Map::new();
    let empty = Vec::new();
    let nodes = view["knowledge_graph"]["nodes"].as_array().unwrap_or(&empty);
    let edges = view["knowledge_graph"]["edges"].as_array().unwrap_or(&empty);
    let name_of = |id: &str| -> Option<String> {
        nodes
            .iter()
            .find(|n| n["id"].as_str() == Some(id))
            .and_then(|n| n["name"].as_str())
            .map(str::to_string)
    };
    let mut index = format!("# {title}\n\n");
    for node in nodes {
        let name = node["name"].as_str().unwrap_or("concept");
        let mut body = format!("# {name}\n\n");
        if let Some(explanation) = node["explanation"].as_str() {
            body.push_str(explanation);
            body.push('\n');
        }
        let related: Vec<String> = edges
            .iter()
            .filter_map(|e| {
                let (from, to) = (e["from"].as_str()?, e["to"].as_str()?);
                if name_of(from)? == name {
                    name_of(to)
                } else if name_of(to)? == name {
                    name_of(from)
                } else {
                    None
                }
            })
            .collect();
        if !related.is_empty() {
            body.push_str("\n## Related\n");
            for r in related {
                body.push_str(&format!("- [[{r}]]\n"));
            }
        }
        index.push_str(&format!("- [[{name}]]\n"));
        files.insert(format!("{}/{name}.md", slug(title)), serde_json::json!(body));
    }
    files.insert(format!("{}/{title}.md", slug(title)), serde_json::json!(index));
    serde_json::Value::Object(files)
}

/// LaTeX: annotated notes file (title, concepts as sections, notes as items).
fn latex(view: &serde_json::Value, title: &str) -> serde_json::Value {
    let mut tex = format!(
        "\\documentclass{{article}}\n\\title{{Notes: {title}}}\n\\begin{{document}}\n\\maketitle\n"
    );
    let empty = Vec::new();
    for node in view["knowledge_graph"]["nodes"].as_array().unwrap_or(&empty) {
        if let Some(name) = node["name"].as_str() {
            tex.push_str(&format!("\\section{{{name}}}\n"));
            if let Some(explanation) = node["explanation"].as_str() {
                tex.push_str(&format!("{explanation}\n"));
            }
        }
    }
    let notes = view["notes"].as_array().unwrap_or(&empty);
    if !notes.is_empty() {
        tex.push_str("\\section{Notes}\n\\begin{itemize}\n");
        for note in notes {
            if let Some(text) = note["text"].as_str() {
                tex.push_str(&format!("\\item {text}\n"));
            }
        }
        tex.push_str("\\end{itemize}\n");
    }
    tex.push_str("\\end{document}\n");
    serde_json::json!({ format!("{}.tex", slug(title)): tex })
}
