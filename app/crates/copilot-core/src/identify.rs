//! Best-effort paper identification for file / direct-PDF imports: the
//! PDF's embedded metadata gives a title candidate; Crossref fills the real
//! title, authors, DOI, and published date. Conservative by design — a hit
//! is applied only on an exact normalized-title match, so a wrong network
//! guess never overwrites user data. Everything here is optional: failures
//! fall back to the filename title and the import proceeds.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Identified {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub doi: Option<String>,
    pub published_at: Option<String>,
}

/// The PDF's embedded DocInfo title, when present and plausible.
pub fn pdf_metadata_title(pdf: &[u8]) -> Option<String> {
    use pdfium_render::prelude::*;
    let _lock = crate::layout::pdfium_lock();
    let pdfium = crate::layout::pdfium().ok()?;
    let document = pdfium.load_pdf_from_byte_slice(pdf, None).ok()?;
    let title = document
        .metadata()
        .get(PdfDocumentMetadataTagType::Title)?
        .value()
        .trim()
        .to_string();
    plausible_title(&title).then_some(title)
}

/// Filter out the junk PDF producers leave in the Title field.
fn plausible_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    title.len() >= 4
        && !lower.starts_with("microsoft word")
        && !lower.starts_with("untitled")
        && !lower.ends_with(".pdf")
        && !lower.ends_with(".tex")
        && !lower.ends_with(".dvi")
        && !lower.ends_with(".indd")
}

fn normalized(title: &str) -> String {
    title
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c.is_whitespace() {
                Some(' ')
            } else {
                None
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Crossref lookup by title. Applied only when the top results contain an
/// exact normalized-title match — no fuzzy guessing.
pub fn crossref_identify(title: &str) -> Option<Identified> {
    if normalized(title).len() < 12 {
        return None; // too little signal to match safely
    }
    let response: serde_json::Value = ureq::get("https://api.crossref.org/works")
        .query("query.bibliographic", title)
        .query("rows", "3")
        .timeout(std::time::Duration::from_secs(6))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    let items = response["message"]["items"].as_array()?;
    let wanted = normalized(title);
    for item in items {
        let hit_title = item["title"][0].as_str()?;
        if normalized(hit_title) != wanted {
            continue;
        }
        let authors = item["author"]
            .as_array()
            .map(|authors| {
                authors
                    .iter()
                    .filter_map(|a| {
                        let given = a["given"].as_str().unwrap_or_default();
                        let family = a["family"].as_str()?;
                        Some(format!("{given} {family}").trim().to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Some(Identified {
            title: Some(hit_title.to_string()),
            authors,
            doi: item["DOI"].as_str().map(str::to_string),
            published_at: issued_date(item),
        });
    }
    None
}

/// Crossref `issued.date-parts` → "YYYY[-MM[-DD]]".
pub(crate) fn issued_date(item: &serde_json::Value) -> Option<String> {
    let parts = item["issued"]["date-parts"][0].as_array()?;
    let n = |i: usize| parts.get(i).and_then(|v| v.as_i64());
    let date = match (n(0), n(1), n(2)) {
        (Some(y), Some(m), Some(d)) => format!("{y:04}-{m:02}-{d:02}"),
        (Some(y), Some(m), None) => format!("{y:04}-{m:02}"),
        (Some(y), _, _) => format!("{y:04}"),
        _ => return None,
    };
    Some(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_ignores_punctuation_and_case() {
        assert_eq!(
            normalized("Patchwork: A Traffic-Capture Platform!"),
            "patchwork a trafficcapture platform"
        );
    }

    #[test]
    fn junk_titles_are_rejected() {
        assert!(!plausible_title("Microsoft Word - draft3.docx"));
        assert!(!plausible_title("untitled"));
        assert!(!plausible_title("paper.dvi"));
        assert!(plausible_title("Attention Is All You Need"));
    }
}
