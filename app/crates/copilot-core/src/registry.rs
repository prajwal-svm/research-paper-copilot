//! Knowledge registry client (v5), starting with canonical paper identity.
//!
//! Identity rules (spec: knowledge-registry / Canonical paper identity):
//! - arXiv wins over DOI when both resolve.
//! - arXiv ids are versionless (`1706.03762`, version recorded separately).
//! - DOIs are lowercased with `doi:`/URL prefixes stripped.
//! - No resolvable id ⇒ registry-ineligible; identity is never fabricated.

use serde::{Deserialize, Serialize};

use crate::bundle::{Bundle, BundleError, Metadata};

type Result<T> = std::result::Result<T, BundleError>;

pub const REGISTRY_STATE_FILE: &str = "registry.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CanonicalId {
    Arxiv {
        /// Versionless id, e.g. `1706.03762` or `cs/9901002`.
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<u32>,
    },
    Doi {
        id: String,
    },
}

impl CanonicalId {
    /// Stable string form used as the registry object key.
    pub fn key(&self) -> String {
        match self {
            CanonicalId::Arxiv { id, .. } => format!("arxiv:{id}"),
            CanonicalId::Doi { id } => format!("doi:{id}"),
        }
    }
}

/// Normalize a raw arXiv id: strip a trailing `vN`, keep it separately.
pub fn normalize_arxiv(raw: &str) -> (String, Option<u32>) {
    let raw = raw.trim();
    if let Some(pos) = raw.rfind('v') {
        if pos > 0
            && raw[pos + 1..].chars().all(|c| c.is_ascii_digit())
            && !raw[pos + 1..].is_empty()
        {
            return (raw[..pos].to_string(), raw[pos + 1..].parse().ok());
        }
    }
    (raw.to_string(), None)
}

/// Normalize a raw DOI: strip URL/`doi:` prefixes, lowercase.
pub fn normalize_doi(raw: &str) -> String {
    let mut doi = raw.trim();
    for prefix in [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "doi:",
    ] {
        if let Some(stripped) = doi.strip_prefix(prefix) {
            doi = stripped;
        }
    }
    doi.to_lowercase()
}

/// Resolve the canonical identity from bundle metadata (arXiv > DOI).
/// `None` means registry-ineligible — never a fabricated identity.
pub fn canonical_id(metadata: &Metadata) -> Option<CanonicalId> {
    let get = |key: &str| {
        metadata
            .paper
            .extra
            .get(key)
            .or_else(|| metadata.extra.get(key))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
    };
    if let Some(raw) = get("arxiv_id") {
        let (id, version) = normalize_arxiv(&raw);
        return Some(CanonicalId::Arxiv { id, version });
    }
    if let Some(raw) = get("doi") {
        return Some(CanonicalId::Doi {
            id: normalize_doi(&raw),
        });
    }
    None
}

/// Per-bundle registry state (`registry.json`), written lazily.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegistryState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_id: Option<CanonicalId>,
    /// False ⇔ no resolvable DOI/arXiv id; all local features still work.
    pub eligible: bool,
    /// Manifests of community layers pulled into this bundle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pulled_layers: Vec<serde_json::Value>,
}

/// Resolve and persist the bundle's registry state. Idempotent.
pub fn resolve_state(bundle: &Bundle) -> Result<RegistryState> {
    let metadata = bundle.metadata()?;
    let id = canonical_id(&metadata);
    let state = RegistryState {
        eligible: id.is_some(),
        canonical_id: id,
        pulled_layers: read_state(bundle)?
            .map(|s| s.pulled_layers)
            .unwrap_or_default(),
    };
    bundle.write_user_json(REGISTRY_STATE_FILE, &state)?;
    Ok(state)
}

pub fn read_state(bundle: &Bundle) -> Result<Option<RegistryState>> {
    let path = bundle.root().join(REGISTRY_STATE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(&path).map_err(|e| BundleError::Io {
        path: path.clone(),
        source: e,
    })?;
    Ok(Some(
        serde_json::from_slice(&raw).map_err(|e| BundleError::Json { path, source: e })?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    fn bundle_with(extra: &[(&str, &str)]) -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        let mut paper = Paper::new("T");
        for (k, v) in extra {
            paper.extra.insert((*k).into(), serde_json::json!(v));
        }
        let b = Bundle::create(&root, b"%PDF-1.5 x", paper, "file").unwrap();
        (tmp, b)
    }

    #[test]
    fn same_paper_converges_across_import_paths() {
        // URL import records a versioned id; local-PDF import a bare one.
        let (_t1, via_url) = bundle_with(&[("arxiv_id", "1706.03762v5")]);
        let (_t2, via_pdf) = bundle_with(&[("arxiv_id", "1706.03762")]);
        let a = resolve_state(&via_url).unwrap();
        let b = resolve_state(&via_pdf).unwrap();
        assert_eq!(
            a.canonical_id.as_ref().unwrap().key(),
            b.canonical_id.as_ref().unwrap().key()
        );
        assert_eq!(a.canonical_id.unwrap().key(), "arxiv:1706.03762");
    }

    #[test]
    fn arxiv_wins_over_doi_and_doi_normalizes() {
        let (_t, both) = bundle_with(&[("arxiv_id", "2001.00001v2"), ("doi", "10.1000/XYZ")]);
        let state = resolve_state(&both).unwrap();
        assert_eq!(state.canonical_id.unwrap().key(), "arxiv:2001.00001");

        let (_t2, doi_only) = bundle_with(&[("doi", "https://doi.org/10.1000/XYZ")]);
        let state = resolve_state(&doi_only).unwrap();
        assert_eq!(state.canonical_id.unwrap().key(), "doi:10.1000/xyz");
    }

    #[test]
    fn unresolvable_is_ineligible_never_fabricated() {
        let (_t, plain) = bundle_with(&[]);
        let state = resolve_state(&plain).unwrap();
        assert!(!state.eligible);
        assert!(state.canonical_id.is_none());
        // Old-style ids like "vol2" must not be mangled into arxiv ids.
        let reread = read_state(&plain).unwrap().unwrap();
        assert_eq!(reread, state);
    }
}

// ---------------------------------------------------------------------------
// Layer format (task 3.2): content-addressed tarball + manifest
// ---------------------------------------------------------------------------

use crate::bundle::sha256_bytes;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayerArtifact {
    /// Bundle-relative path this artifact lands at on pull.
    pub path: String,
    pub digest: String,
    pub size: u64,
}

/// Manifest of one published enrichment layer. The blob is a plain tar of
/// the artifacts; everything is content-addressed so pulls are verifiable
/// end to end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayerManifest {
    /// Canonical paper key, e.g. `arxiv:1706.03762`.
    pub canonical_id: String,
    /// Monotonic per-paper layer version, assigned by the registry.
    pub version: u64,
    pub format_major: u64,
    pub publisher: String,
    pub created_at: String,
    /// sha256 of the layer tar blob.
    pub blob_digest: String,
    pub artifacts: Vec<LayerArtifact>,
    /// Provenance excerpt supporting this layer (signed events).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance: Vec<serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum LayerError {
    #[error("layer {version} for {canonical_id} failed verification: {reason}")]
    Integrity {
        canonical_id: String,
        version: u64,
        reason: String,
    },
    #[error("layer build: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Bundle(#[from] BundleError),
}

/// Build a layer from bundle-relative paths: tar in memory, digest every
/// artifact and the blob, return (manifest, blob).
pub fn build_layer(
    bundle: &Bundle,
    canonical_id: &str,
    version: u64,
    publisher: &str,
    paths: &[String],
) -> Result2<(LayerManifest, Vec<u8>), LayerError> {
    let mut tarball = tar::Builder::new(Vec::new());
    let mut artifacts = Vec::new();
    for path in paths {
        let full = bundle.root().join(path);
        let bytes = std::fs::read(&full)?;
        artifacts.push(LayerArtifact {
            path: path.clone(),
            digest: sha256_bytes(&bytes),
            size: bytes.len() as u64,
        });
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tarball.append_data(&mut header, path, bytes.as_slice())?;
    }
    let blob = tarball.into_inner()?;
    let manifest = LayerManifest {
        canonical_id: canonical_id.to_string(),
        version,
        format_major: crate::bundle::FORMAT_MAJOR,
        publisher: publisher.to_string(),
        created_at: crate::bundle::now_rfc3339(),
        blob_digest: sha256_bytes(&blob),
        artifacts,
        provenance: Vec::new(),
    };
    Ok((manifest, blob))
}

type Result2<T, E> = std::result::Result<T, E>;

/// Verify a pulled layer strictly: blob digest, then per-entry digests, and
/// no entries beyond the manifest. Any mismatch discards the layer with a
/// visible error — a failed layer never merges.
pub fn verify_layer(manifest: &LayerManifest, blob: &[u8]) -> Result2<(), LayerError> {
    let integrity = |reason: String| LayerError::Integrity {
        canonical_id: manifest.canonical_id.clone(),
        version: manifest.version,
        reason,
    };
    if sha256_bytes(blob) != manifest.blob_digest {
        return Err(integrity("blob digest mismatch".into()));
    }
    let expected: std::collections::BTreeMap<&str, &str> = manifest
        .artifacts
        .iter()
        .map(|a| (a.path.as_str(), a.digest.as_str()))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut archive = tar::Archive::new(blob);
    for entry in archive.entries().map_err(|e| integrity(e.to_string()))? {
        let mut entry = entry.map_err(|e| integrity(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| integrity(e.to_string()))?
            .to_string_lossy()
            .to_string();
        let Some(expected_digest) = expected.get(path.as_str()) else {
            return Err(integrity(format!(
                "unexpected entry not in manifest: {path}"
            )));
        };
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|e| integrity(e.to_string()))?;
        if &sha256_bytes(&bytes) != expected_digest {
            return Err(integrity(format!("digest mismatch for {path}")));
        }
        seen.insert(path);
    }
    if seen.len() != expected.len() {
        return Err(integrity(
            "manifest lists artifacts missing from blob".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod layer_tests {
    use super::*;
    use crate::bundle::Paper;

    fn bundle() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        let b = Bundle::create(&root, b"%PDF-1.5 x", Paper::new("T"), "file").unwrap();
        (tmp, b)
    }

    #[test]
    fn layer_roundtrip_verifies() {
        let (_tmp, b) = bundle();
        std::fs::write(b.root().join("glossary/terms.json"), b"{\"attention\":1}").unwrap();
        b.journal("notes/notes.jsonl")
            .append(&serde_json::json!({"at": "t", "text": "n"}))
            .unwrap();
        let (manifest, blob) = build_layer(
            &b,
            "arxiv:1706.03762",
            1,
            "alice@registry",
            &["glossary/terms.json".into(), "notes/notes.jsonl".into()],
        )
        .unwrap();
        assert_eq!(manifest.artifacts.len(), 2);
        verify_layer(&manifest, &blob).unwrap();
    }

    #[test]
    fn tampered_blob_is_discarded_with_visible_error() {
        let (_tmp, b) = bundle();
        std::fs::write(b.root().join("glossary/terms.json"), b"{\"a\":1}").unwrap();
        let (manifest, mut blob) =
            build_layer(&b, "arxiv:x", 3, "p", &["glossary/terms.json".into()]).unwrap();
        // Flip one byte inside the tar payload region.
        let idx = blob.len() / 2;
        blob[idx] ^= 0xFF;
        let err = verify_layer(&manifest, &blob).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("failed verification"), "{text}");
        assert!(
            text.contains("arxiv:x") && text.contains('3'),
            "names the layer: {text}"
        );
    }

    #[test]
    fn smuggled_extra_entry_is_rejected() {
        let (_tmp, b) = bundle();
        std::fs::write(b.root().join("glossary/terms.json"), b"{\"a\":1}").unwrap();
        let (manifest, _blob) =
            build_layer(&b, "arxiv:x", 1, "p", &["glossary/terms.json".into()]).unwrap();
        // Rebuild a blob with an extra file the manifest doesn't declare,
        // fixing up blob_digest so only the entry check can catch it.
        let mut tarball = tar::Builder::new(Vec::new());
        for path in ["glossary/terms.json", "evil.bin"] {
            let bytes: &[u8] = if path == "evil.bin" {
                b"%PDF-1.4"
            } else {
                b"{\"a\":1}"
            };
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tarball.append_data(&mut header, path, bytes).unwrap();
        }
        let evil_blob = tarball.into_inner().unwrap();
        let mut evil_manifest = manifest;
        evil_manifest.blob_digest = sha256_bytes(&evil_blob);
        let err = verify_layer(&evil_manifest, &evil_blob).unwrap_err();
        assert!(err.to_string().contains("unexpected entry"), "{err}");
    }
}

// ---------------------------------------------------------------------------
// Publish path (task 3.3): allowlist, preview, client-side policy, upload
// ---------------------------------------------------------------------------

/// Enrichment the registry accepts, as an explicit allowlist of bundle
/// paths. Everything else is excluded with a reason — notably publisher
/// content (original.pdf, page images, full-text extraction) and personal
/// state. Community layers are enrichment ONLY.
const PUBLISH_ALLOWLIST: [&str; 7] = [
    "knowledge_graph.json",
    "glossary",
    "quizzes",
    "flashcards",
    "notes",
    "research",
    "contributions",
];

/// Paths that are banned with a specific, user-visible reason.
fn exclusion_reason(first: &str, name: &str) -> Option<&'static str> {
    match () {
        _ if first == "original.pdf" => Some("publisher-owned source PDF"),
        _ if first == "pages" || first == "figures" || first == "tables" => {
            Some("page imagery derived from the publisher PDF")
        }
        _ if first == "layout.json" || first == "semantic_tree.json" => {
            Some("full-text extraction of the paper")
        }
        _ if name == "embeddings.bin" || name == "graph.db" => Some("machine-local index"),
        _ if name == "reading_state.json" => Some("personal reading state"),
        _ if first == "registry.json" || first == "plugins" => Some("local configuration"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct PublishPreview {
    /// Exactly what would upload.
    pub included: Vec<String>,
    /// What was held back, with the reason shown to the user.
    pub excluded: Vec<(String, String)>,
}

/// Compute the publish preview for a bundle: allowlisted enrichment in,
/// everything else out with its reason. The preview IS the upload set —
/// publish uses this same function.
pub fn publish_preview(bundle: &Bundle) -> std::io::Result<PublishPreview> {
    let mut included = Vec::new();
    let mut excluded = Vec::new();
    let root = bundle.root().to_path_buf();

    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        included: &mut Vec<String>,
        excluded: &mut Vec<(String, String)>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let first = rel.split('/').next().unwrap_or("").to_string();
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(reason) = exclusion_reason(&first, &name) {
                excluded.push((rel, reason.to_string()));
                continue;
            }
            if path.is_dir() {
                walk(root, &path, included, excluded)?;
            } else if PUBLISH_ALLOWLIST.contains(&first.as_str()) {
                if name.ends_with(".tmp") || name.contains(".conflict") {
                    continue;
                }
                included.push(rel);
            } else {
                excluded.push((rel, "not in the enrichment allowlist".to_string()));
            }
        }
        Ok(())
    }
    walk(&root, &root, &mut included, &mut excluded)?;
    included.sort();
    excluded.sort();
    Ok(PublishPreview { included, excluded })
}

/// Client-side enrichment-only gate: runs over the exact upload set right
/// before publish. A crafted set that skips the preview still can't smuggle
/// PDF bytes.
pub fn validate_publish(bundle: &Bundle, paths: &[String]) -> Result2<(), LayerError> {
    for path in paths {
        let first = path.split('/').next().unwrap_or("");
        let name = path.rsplit('/').next().unwrap_or("");
        if let Some(reason) = exclusion_reason(first, name) {
            return Err(LayerError::Integrity {
                canonical_id: String::new(),
                version: 0,
                reason: format!("policy violation at {path}: {reason}"),
            });
        }
        if !PUBLISH_ALLOWLIST.contains(&first) {
            return Err(LayerError::Integrity {
                canonical_id: String::new(),
                version: 0,
                reason: format!("policy violation at {path}: not in the enrichment allowlist"),
            });
        }
        let bytes = std::fs::read(bundle.root().join(path))?;
        if bytes.starts_with(b"%PDF") {
            return Err(LayerError::Integrity {
                canonical_id: String::new(),
                version: 0,
                reason: format!("policy violation at {path}: payload is a PDF"),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod publish_tests {
    use super::*;
    use crate::bundle::Paper;

    #[test]
    fn preview_excludes_publisher_and_personal_content_with_reasons() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        let b = Bundle::create(&root, b"%PDF-1.5 x", Paper::new("T"), "file").unwrap();
        std::fs::write(root.join("glossary/terms.json"), b"{}").unwrap();
        std::fs::write(root.join("layout.json"), b"{\"pages\":[]}").unwrap();
        b.write_user_json("reading_state.json", &serde_json::json!({"scroll": 1}))
            .unwrap();
        b.journal("notes/notes.jsonl")
            .append(&serde_json::json!({"at":"t"}))
            .unwrap();

        let preview = publish_preview(&b).unwrap();
        assert!(preview
            .included
            .contains(&"glossary/terms.json".to_string()));
        assert!(preview.included.contains(&"notes/notes.jsonl".to_string()));
        let excluded: std::collections::BTreeMap<_, _> = preview.excluded.into_iter().collect();
        assert_eq!(excluded["original.pdf"], "publisher-owned source PDF");
        assert_eq!(excluded["layout.json"], "full-text extraction of the paper");
        assert_eq!(excluded["reading_state.json"], "personal reading state");
        assert!(!preview.included.iter().any(|p| p.contains("original.pdf")));
    }

    #[test]
    fn crafted_upload_set_cannot_smuggle_pdf() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        let b = Bundle::create(&root, b"%PDF-1.5 x", Paper::new("T"), "file").unwrap();
        // Direct path to the banned file:
        let err = validate_publish(&b, &["original.pdf".into()]).unwrap_err();
        assert!(err.to_string().contains("publisher-owned"), "{err}");
        // PDF bytes hidden under an allowlisted path:
        std::fs::write(root.join("glossary/innocent.json"), b"%PDF-1.4 sneaky").unwrap();
        let err = validate_publish(&b, &["glossary/innocent.json".into()]).unwrap_err();
        assert!(err.to_string().contains("payload is a PDF"), "{err}");
    }
}

// ---------------------------------------------------------------------------
// Registry HTTP client (tasks 3.3/3.4): token-authenticated publish + pull
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum RegistryHttpError {
    #[error("registry unreachable: {0}")]
    Unreachable(String),
    #[error("registry rejected the request ({status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("registry response was not understood: {0}")]
    BadResponse(String),
}

/// Client for one configured registry. `token` is required for publish,
/// optional for pull (public reads).
#[cfg(feature = "native")]
pub struct RegistryClient {
    pub base_url: String,
    pub token: Option<String>,
}

#[cfg(feature = "native")]
impl RegistryClient {
    fn request(&self, method: &str, path: &str) -> ureq::Request {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), path);
        let mut req = ureq::request(method, &url).timeout(std::time::Duration::from_secs(30));
        if let Some(token) = &self.token {
            req = req.set("authorization", &format!("Bearer {token}"));
        }
        req
    }

    fn rejected(response: ureq::Response) -> RegistryHttpError {
        let status = response.status();
        let body = response.into_string().unwrap_or_default();
        RegistryHttpError::Rejected { status, body }
    }

    /// Publish: manifest first (server validates policy + assigns the next
    /// monotonic version), then the blob (server re-verifies digests).
    /// Returns the assigned version.
    pub fn publish(
        &self,
        manifest: &LayerManifest,
        blob: &[u8],
    ) -> Result2<u64, RegistryHttpError> {
        let key = &manifest.canonical_id;
        let response = self
            .request("POST", &format!("v1/papers/{key}/layers"))
            .set("content-type", "application/json")
            .send_string(&serde_json::to_string(manifest).expect("manifest serializes"))
            .map_err(|e| match e {
                ureq::Error::Status(_, r) => Self::rejected(r),
                other => RegistryHttpError::Unreachable(other.to_string()),
            })?;
        let assigned: serde_json::Value = response
            .into_json()
            .map_err(|e| RegistryHttpError::BadResponse(e.to_string()))?;
        let version = assigned["version"]
            .as_u64()
            .ok_or_else(|| RegistryHttpError::BadResponse("missing version".into()))?;

        self.request("PUT", &format!("v1/papers/{key}/layers/{version}/blob"))
            .set("content-type", "application/octet-stream")
            .send_bytes(blob)
            .map_err(|e| match e {
                ureq::Error::Status(_, r) => Self::rejected(r),
                other => RegistryHttpError::Unreachable(other.to_string()),
            })?;
        Ok(version)
    }

    /// All layer manifests for a paper (empty = none published).
    pub fn layers(&self, canonical_key: &str) -> Result2<Vec<LayerManifest>, RegistryHttpError> {
        let response = self
            .request("GET", &format!("v1/papers/{canonical_key}/layers"))
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(404, _) => {
                    return RegistryHttpError::Rejected {
                        status: 404,
                        body: String::new(),
                    }
                }
                ureq::Error::Status(_, r) => Self::rejected(r),
                other => RegistryHttpError::Unreachable(other.to_string()),
            });
        match response {
            Ok(r) => r
                .into_json()
                .map_err(|e| RegistryHttpError::BadResponse(e.to_string())),
            Err(RegistryHttpError::Rejected { status: 404, .. }) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Fetch one layer blob (verify with [`verify_layer`] before use).
    pub fn blob(&self, canonical_key: &str, version: u64) -> Result2<Vec<u8>, RegistryHttpError> {
        let response = self
            .request(
                "GET",
                &format!("v1/papers/{canonical_key}/layers/{version}/blob"),
            )
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(_, r) => Self::rejected(r),
                other => RegistryHttpError::Unreachable(other.to_string()),
            })?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes)
            .map_err(|e| RegistryHttpError::BadResponse(e.to_string()))?;
        Ok(bytes)
    }
}

// ---------------------------------------------------------------------------
// Pull (task 3.4): merge a verified community layer into the local bundle
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, schemars::JsonSchema)]
pub struct PullReport {
    /// Journals union-merged with local entries.
    pub merged_journals: Vec<String>,
    /// New files added (didn't exist locally).
    pub added: Vec<String>,
    /// Local files kept untouched (community version NOT applied — the
    /// user's own artifact always wins).
    pub kept_local: Vec<String>,
    /// Anchors referencing objects the local copy lacks (explicit
    /// degradation, never silent).
    pub unresolved_anchors: Vec<String>,
}

/// Merge a verified layer: journals union in, new files add, existing
/// non-journal files stay untouched (community copy is dropped, reported).
/// The layer manifest lands in `registry.json` as the provenance tag.
/// Verification failure aborts before any write.
pub fn pull_layer(
    bundle: &Bundle,
    manifest: &LayerManifest,
    blob: &[u8],
) -> Result2<PullReport, LayerError> {
    verify_layer(manifest, blob)?;

    // Local object ids (for anchor resolution), when a tree exists.
    let local_objects: std::collections::BTreeSet<String> = bundle
        .read_derived_json::<serde_json::Value>("semantic_tree.json")
        .ok()
        .flatten()
        .map(|tree| {
            let mut ids = std::collections::BTreeSet::new();
            collect_ids(&tree, &mut ids);
            ids
        })
        .unwrap_or_default();

    let mut report = PullReport::default();
    let mut archive = tar::Archive::new(blob);
    for entry in archive.entries().map_err(LayerError::Io)? {
        let mut entry = entry.map_err(LayerError::Io)?;
        let path = entry
            .path()
            .map_err(LayerError::Io)?
            .to_string_lossy()
            .to_string();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(LayerError::Io)?;
        let target = bundle.root().join(&path);

        if path.ends_with(".jsonl") {
            let existing = std::fs::read_to_string(&target).unwrap_or_default();
            let merged =
                crate::sync::merge::merge_journals(&existing, &String::from_utf8_lossy(&bytes));
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, merged)?;
            report.merged_journals.push(path.clone());
            // Anchor resolution for object-anchored entries.
            if !local_objects.is_empty() {
                for line in String::from_utf8_lossy(&bytes).lines() {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                        if let Some(object_id) = value.get("object_id").and_then(|v| v.as_str()) {
                            if !local_objects.contains(object_id) {
                                report
                                    .unresolved_anchors
                                    .push(format!("{path}: {object_id}"));
                            }
                        }
                    }
                }
            }
        } else if target.exists() {
            report.kept_local.push(path);
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, &bytes)?;
            report.added.push(path);
        }
    }

    // Provenance tag: the pulled manifest is recorded in registry.json.
    let mut state = read_state(bundle)?.unwrap_or_default();
    state
        .pulled_layers
        .push(serde_json::to_value(manifest).expect("manifest serializes"));
    bundle.write_user_json(REGISTRY_STATE_FILE, &state)?;
    Ok(report)
}

fn collect_ids(value: &serde_json::Value, out: &mut std::collections::BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(id) = map.get("id").and_then(|v| v.as_str()) {
                out.insert(id.to_string());
            }
            for v in map.values() {
                collect_ids(v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                collect_ids(v, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod pull_tests {
    use super::*;
    use crate::bundle::Paper;

    fn bundle() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        let b = Bundle::create(&root, b"%PDF-1.5 x", Paper::new("T"), "file").unwrap();
        (tmp, b)
    }

    fn community_layer(paths: &[(&str, &[u8])]) -> (LayerManifest, Vec<u8>) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("c.research");
        let b = Bundle::create(&root, b"%PDF-1.5 c", Paper::new("C"), "file").unwrap();
        for (path, bytes) in paths {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, bytes).unwrap();
        }
        build_layer(
            &b,
            "arxiv:1",
            1,
            "community",
            &paths.iter().map(|(p, _)| p.to_string()).collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn pull_merges_adds_and_never_overwrites() {
        let (_tmp, b) = bundle();
        // Local user data that must survive untouched.
        b.journal("notes/notes.jsonl")
            .append(&serde_json::json!({"at": "2026-01-01T00:00:00Z", "text": "mine"}))
            .unwrap();
        std::fs::write(b.root().join("glossary/terms.json"), b"{\"local\":true}").unwrap();

        let (manifest, blob) = community_layer(&[
            (
                "notes/notes.jsonl",
                br#"{"at": "2026-01-02T00:00:00Z", "text": "community"}"# as &[u8],
            ),
            ("glossary/terms.json", b"{\"community\":true}"),
            ("glossary/lessons.json", b"{\"new\":true}"),
        ]);
        let report = pull_layer(&b, &manifest, &blob).unwrap();

        // Journal unioned: both entries present.
        let notes: Vec<serde_json::Value> = b.journal("notes/notes.jsonl").read_all().unwrap();
        assert_eq!(notes.len(), 2, "{notes:?}");
        // Existing file untouched; new file added.
        assert_eq!(
            std::fs::read(b.root().join("glossary/terms.json")).unwrap(),
            b"{\"local\":true}"
        );
        assert!(report
            .kept_local
            .contains(&"glossary/terms.json".to_string()));
        assert!(report.added.contains(&"glossary/lessons.json".to_string()));
        // Provenance tag recorded.
        let state = read_state(&b).unwrap().unwrap();
        assert_eq!(state.pulled_layers.len(), 1);
        assert_eq!(state.pulled_layers[0]["publisher"], "community");
    }

    #[test]
    fn corrupted_layer_aborts_without_touching_the_bundle() {
        let (_tmp, b) = bundle();
        b.journal("notes/notes.jsonl")
            .append(&serde_json::json!({"at": "t", "text": "mine"}))
            .unwrap();
        let (manifest, mut blob) =
            community_layer(&[("notes/notes.jsonl", br#"{"at":"u","text":"c"}"# as &[u8])]);
        let idx = blob.len() / 2;
        blob[idx] ^= 0xFF;

        assert!(pull_layer(&b, &manifest, &blob).is_err());
        let notes: Vec<serde_json::Value> = b.journal("notes/notes.jsonl").read_all().unwrap();
        assert_eq!(notes.len(), 1, "bundle unchanged after failed verification");
        assert!(
            read_state(&b).unwrap().is_none(),
            "no provenance tag recorded"
        );
    }

    #[test]
    fn foreign_anchors_are_reported_unresolved() {
        let (_tmp, b) = bundle();
        b.write_derived_json(
            "semantic_tree.json",
            &serde_json::json!({"nodes": [{"id": "11111111-1111-1111-1111-111111111111"}]}),
            "objects",
            serde_json::json!({"status": "completed"}),
        )
        .unwrap();
        let (manifest, blob) = community_layer(&[(
            "notes/notes.jsonl",
            br#"{"at":"t","object_id":"99999999-9999-9999-9999-999999999999","text":"x"}"#
                as &[u8],
        )]);
        let report = pull_layer(&b, &manifest, &blob).unwrap();
        assert_eq!(report.unresolved_anchors.len(), 1, "{report:?}");
        assert!(
            report.unresolved_anchors[0].contains("9999"),
            "explicit, never silent"
        );
    }
}
