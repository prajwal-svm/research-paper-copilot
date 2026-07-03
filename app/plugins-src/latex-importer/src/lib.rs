//! Reference importer: LaTeX source → structured import JSON the host
//! assembles into a schema-valid bundle. Input: { "source": "<tex>" }.
//! Output: { "metadata": { title, abstract, authors }, "sections":
//! [{ "heading", "text" }] }. Page geometry is absent by nature — the
//! host degrades explicitly (cover-page PDF), never silently.

use std::alloc::{alloc as raw_alloc, Layout};

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    unsafe { raw_alloc(Layout::from_size_align(size as usize, 1).unwrap()) as i32 }
}

#[no_mangle]
pub extern "C" fn run(ptr: i32, len: i32) -> i64 {
    let input = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let output = import(input).unwrap_or_else(|e| {
        serde_json::json!({ "error": e }).to_string().into_bytes()
    });
    let out_ptr = alloc(output.len() as i32);
    unsafe { std::ptr::copy_nonoverlapping(output.as_ptr(), out_ptr as *mut u8, output.len()) };
    ((out_ptr as u32 as i64) << 32) | (output.len() as u32 as i64)
}

/// `\command{...}` argument, brace-balanced.
fn command_arg(source: &str, command: &str) -> Option<String> {
    let start = source.find(&format!("\\{command}{{"))? + command.len() + 2;
    let mut depth = 1;
    let mut out = String::new();
    for c in source[start..].chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(out);
                }
            }
            _ => {}
        }
        out.push(c);
    }
    None
}

fn environment(source: &str, name: &str) -> Option<String> {
    let begin = format!("\\begin{{{name}}}");
    let end = format!("\\end{{{name}}}");
    let start = source.find(&begin)? + begin.len();
    let stop = source[start..].find(&end)? + start;
    Some(source[start..stop].trim().to_string())
}

fn strip_tex(text: &str) -> String {
    // Light cleanup: drop comments and collapse whitespace; keep math as-is.
    text.lines()
        .map(|l| l.split('%').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn import(input: &[u8]) -> Result<Vec<u8>, String> {
    let doc: serde_json::Value = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let source = doc["source"]
        .as_str()
        .or_else(|| doc["view"]["source"].as_str())
        .ok_or("missing source")?;

    let title = command_arg(source, "title").map(|t| strip_tex(&t));
    let abstract_text = environment(source, "abstract").map(|a| strip_tex(&a));
    let authors: Vec<String> = command_arg(source, "author")
        .map(|a| {
            a.split("\\and")
                .map(|s| strip_tex(s))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    // Sections: split on \section{...}.
    let mut sections = Vec::new();
    let mut rest = source;
    while let Some(pos) = rest.find("\\section{") {
        rest = &rest[pos..];
        let heading = command_arg(rest, "section").unwrap_or_default();
        let body_start = rest.find('}').map(|i| i + 1).unwrap_or(0);
        let body_end = rest[body_start..]
            .find("\\section{")
            .map(|i| i + body_start)
            .unwrap_or(rest.len());
        let body = rest[body_start..body_end]
            .split("\\end{document}")
            .next()
            .unwrap_or("");
        sections.push(serde_json::json!({
            "heading": strip_tex(&heading),
            "text": strip_tex(body),
        }));
        rest = &rest[body_end..];
    }

    Ok(serde_json::json!({
        "metadata": { "title": title, "abstract": abstract_text, "authors": authors },
        "sections": sections,
    })
    .to_string()
    .into_bytes())
}
