//! arXiv / DOI import support: resolve an identifier to paper metadata and
//! PDF bytes. Network errors surface as typed errors with plain-language
//! messages — the import UI shows them directly.

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ImportFetchError {
    #[error("that doesn't look like an arXiv URL, arXiv id, or DOI: {0}")]
    Unrecognized(String),
    #[error("network unavailable or the server did not respond: {0}")]
    Network(String),
    #[error("no paper found for {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchedPaper {
    pub title: String,
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstract_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arxiv_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    pub pdf: Vec<u8>,
}

/// Parse user input into an arXiv id: accepts "1706.03762", "1706.03762v7",
/// "arxiv.org/abs/1706.03762", "arxiv.org/pdf/1706.03762", "arXiv:1706.03762".
pub fn parse_arxiv_id(input: &str) -> Option<String> {
    let input = input.trim();
    let candidate = if let Some(pos) = input.find("/abs/") {
        &input[pos + 5..]
    } else if let Some(pos) = input.find("/pdf/") {
        &input[pos + 5..]
    } else if let Some(rest) = input
        .strip_prefix("arXiv:")
        .or_else(|| input.strip_prefix("arxiv:"))
    {
        rest
    } else {
        input
    };
    let id: String = candidate
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == 'v')
        .collect();
    let base = id.split('v').next().unwrap_or(&id);
    (base.len() >= 9 && base.contains('.') && base.chars().filter(|c| *c == '.').count() == 1)
        .then(|| base.to_string())
}

/// Is this a DOI ("10.xxxx/…") or a doi.org URL?
pub fn parse_doi(input: &str) -> Option<String> {
    let input = input.trim();
    let candidate = if let Some(pos) = input.find("doi.org/") {
        &input[pos + 8..]
    } else {
        input
    };
    candidate
        .starts_with("10.")
        .then(|| candidate.trim_end_matches('/').to_string())
}

/// Fetch a paper by arXiv URL/id or DOI.
pub fn fetch(input: &str) -> Result<FetchedPaper, ImportFetchError> {
    if let Some(arxiv_id) = parse_arxiv_id(input) {
        return fetch_arxiv(&arxiv_id);
    }
    if let Some(doi) = parse_doi(input) {
        return fetch_doi(&doi);
    }
    Err(ImportFetchError::Unrecognized(input.to_string()))
}

fn http_get(url: &str) -> Result<ureq::Response, ImportFetchError> {
    ureq::get(url)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => ImportFetchError::NotFound(url.to_string()),
            other => ImportFetchError::Network(other.to_string()),
        })
}

fn fetch_arxiv(arxiv_id: &str) -> Result<FetchedPaper, ImportFetchError> {
    // Metadata from the Atom API.
    let meta_url = format!("https://export.arxiv.org/api/query?id_list={arxiv_id}&max_results=1");
    let body = http_get(&meta_url)?
        .into_string()
        .map_err(|e| ImportFetchError::Network(e.to_string()))?;
    let entry = body
        .split("<entry>")
        .nth(1)
        .ok_or_else(|| ImportFetchError::NotFound(format!("arXiv:{arxiv_id}")))?;
    let title = xml_text(entry, "title")
        .ok_or_else(|| ImportFetchError::NotFound(format!("arXiv:{arxiv_id}")))?;
    let abstract_text = xml_text(entry, "summary");
    let authors: Vec<String> = entry
        .split("<author>")
        .skip(1)
        .filter_map(|a| xml_text(a, "name"))
        .collect();

    // The PDF itself.
    let pdf_url = format!("https://arxiv.org/pdf/{arxiv_id}");
    let mut reader = http_get(&pdf_url)?.into_reader();
    let mut pdf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut pdf)
        .map_err(|e| ImportFetchError::Network(e.to_string()))?;
    if !pdf.starts_with(b"%PDF") {
        return Err(ImportFetchError::NotFound(format!(
            "arXiv:{arxiv_id} (no PDF)"
        )));
    }

    Ok(FetchedPaper {
        title,
        authors,
        abstract_text,
        arxiv_id: Some(arxiv_id.to_string()),
        doi: None,
        pdf,
    })
}

fn fetch_doi(doi: &str) -> Result<FetchedPaper, ImportFetchError> {
    // Crossref metadata; PDF only when an open-access link is present.
    let url = format!("https://api.crossref.org/works/{doi}");
    let response: serde_json::Value = http_get(&url)?
        .into_json()
        .map_err(|e| ImportFetchError::Network(e.to_string()))?;
    let message = &response["message"];
    let title = message["title"][0]
        .as_str()
        .ok_or_else(|| ImportFetchError::NotFound(doi.to_string()))?
        .to_string();

    // If Crossref points at arXiv, prefer the arXiv path (PDF guaranteed).
    if let Some(arxiv_id) = message["URL"]
        .as_str()
        .and_then(crate::citations::find_arxiv_id)
    {
        return fetch_arxiv(&arxiv_id);
    }

    let pdf_link = message["link"]
        .as_array()
        .and_then(|links| {
            links
                .iter()
                .find(|l| l["content-type"].as_str() == Some("application/pdf"))
        })
        .and_then(|l| l["URL"].as_str())
        .ok_or_else(|| {
            ImportFetchError::NotFound(format!(
                "{doi} has no open-access PDF; download it manually and import the file"
            ))
        })?;

    let mut reader = http_get(pdf_link)?.into_reader();
    let mut pdf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut pdf)
        .map_err(|e| ImportFetchError::Network(e.to_string()))?;
    if !pdf.starts_with(b"%PDF") {
        return Err(ImportFetchError::NotFound(format!("{doi} (no usable PDF)")));
    }

    Ok(FetchedPaper {
        title,
        authors: message["author"]
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
            .unwrap_or_default(),
        abstract_text: None,
        arxiv_id: None,
        doi: Some(doi.to_string()),
        pdf,
    })
}

fn xml_text(hay: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = hay.find(&open)? + open.len();
    let end = hay[start..].find(&close)? + start;
    let text = hay[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arxiv_inputs() {
        for input in [
            "1706.03762",
            "1706.03762v7",
            "arXiv:1706.03762",
            "https://arxiv.org/abs/1706.03762",
            "https://arxiv.org/pdf/1706.03762v7",
        ] {
            assert_eq!(
                parse_arxiv_id(input).as_deref(),
                Some("1706.03762"),
                "{input}"
            );
        }
        assert_eq!(parse_arxiv_id("not a paper"), None);
        assert_eq!(parse_arxiv_id("10.1000/xyz"), None);
    }

    #[test]
    fn parses_dois() {
        assert_eq!(
            parse_doi("https://doi.org/10.1162/neco.1997.9.8.1735").as_deref(),
            Some("10.1162/neco.1997.9.8.1735")
        );
        assert_eq!(
            parse_doi("10.48550/arXiv.1706.03762").as_deref(),
            Some("10.48550/arXiv.1706.03762")
        );
        assert_eq!(parse_doi("1706.03762"), None);
    }

    #[test]
    fn unrecognized_input_is_a_clear_error() {
        match fetch("my favorite paper") {
            Err(ImportFetchError::Unrecognized(_)) => {}
            other => panic!("expected Unrecognized, got {other:?}"),
        }
    }
}
