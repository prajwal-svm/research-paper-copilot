//! Reference-context extraction for chat threads: turn a URL or a raw PDF
//! into readable text for the model. Papers/objects and attachments are
//! resolved elsewhere (semantic tree, attachment pipeline); this module
//! covers the two new external sources.

#[derive(Debug, thiserror::Error)]
pub enum RefContextError {
    #[error("could not fetch {0}")]
    Fetch(String),
    #[error("nothing readable extracted from {0}")]
    Empty(String),
    #[error("not a PDF: {0}")]
    NotPdf(String),
    #[error("io: {0}")]
    Io(String),
}

/// Clamp so a single reference never dominates the context window.
const MAX_CHARS: usize = 40_000;

/// Fetch a URL and reduce its HTML to readable text: drop script/style
/// bodies, strip tags, decode a few common entities, collapse whitespace.
/// A lightweight readability — good enough for context, upgradeable to a
/// dedicated crate without changing this signature.
#[cfg(feature = "native")]
pub fn fetch_url_text(url: &str) -> Result<String, RefContextError> {
    let body = ureq::get(url)
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| RefContextError::Fetch(e.to_string()))?
        .into_string()
        .map_err(|e| RefContextError::Fetch(e.to_string()))?;
    let text = html_to_text(&body);
    if text.trim().is_empty() {
        return Err(RefContextError::Empty(url.to_string()));
    }
    Ok(clamp(&text))
}

/// Strip HTML to readable text. Not a full parser — removes script/style
/// regions, then all tags, decodes common entities, collapses whitespace.
pub fn html_to_text(html: &str) -> String {
    let without_blocks = strip_regions(html, &["script", "style", "noscript", "svg"]);
    let mut out = String::with_capacity(without_blocks.len());
    let mut in_tag = false;
    for ch in without_blocks.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' '); // tag boundary → whitespace, so words don't fuse
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Remove `<tag>…</tag>` regions (case-insensitive) whole, including bodies.
fn strip_regions(html: &str, tags: &[&str]) -> String {
    let mut result = html.to_string();
    for tag in tags {
        let lower_open = format!("<{tag}");
        let lower_close = format!("</{tag}>");
        loop {
            let hay = result.to_lowercase();
            let Some(start) = hay.find(&lower_open) else {
                break;
            };
            let end = match hay[start..].find(&lower_close) {
                Some(rel) => start + rel + lower_close.len(),
                None => result.len(),
            };
            result.replace_range(start..end, " ");
        }
    }
    result
}

/// Extract text from a PDF file (all pages) via pdfium.
#[cfg(feature = "native")]
pub fn extract_pdf_text(path: &std::path::Path) -> Result<String, RefContextError> {
    let bytes = std::fs::read(path).map_err(|e| RefContextError::Io(e.to_string()))?;
    if !bytes.starts_with(b"%PDF") {
        return Err(RefContextError::NotPdf(path.display().to_string()));
    }
    let _lock = crate::layout::pdfium_lock();
    let pdfium = crate::layout::pdfium().map_err(|e| RefContextError::Io(e.to_string()))?;
    let document = pdfium
        .load_pdf_from_byte_slice(&bytes, None)
        .map_err(|e| RefContextError::Io(e.to_string()))?;
    let mut text = String::new();
    for page in document.pages().iter() {
        if let Ok(page_text) = page.text() {
            text.push_str(&page_text.all());
            text.push('\n');
        }
        if text.len() > MAX_CHARS {
            break;
        }
    }
    if text.trim().is_empty() {
        return Err(RefContextError::Empty(path.display().to_string()));
    }
    Ok(clamp(&text))
}

fn clamp(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        text.to_string()
    } else {
        text.chars().take(MAX_CHARS).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_reduces_to_readable_text() {
        let html = "<html><head><style>.x{color:red}</style>\
                    <script>var a=1;</script></head>\
                    <body><h1>Title</h1><p>Hello&nbsp;world &amp; more.</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world & more."));
        assert!(!text.contains("color:red"));
        assert!(!text.contains("var a"));
    }

    #[test]
    fn tags_become_word_boundaries() {
        assert_eq!(html_to_text("<b>one</b><b>two</b>"), "one two");
    }
}
