//! `.research` bundle read/write layer.
//!
//! A bundle is a directory in the library (zip only on export). Invariants
//! (see `schemas/research-format/v0/README.md`):
//! - `original.pdf` is immutable and content-addressed.
//! - Derived data is regenerable; user data is append-only JSONL.
//! - Files this app version does not understand are preserved verbatim:
//!   nothing here ever enumerates-and-deletes, and metadata rewrites are
//!   atomic single-file replaces.
//! - Readers open any bundle of the same major `format_version` and refuse
//!   newer majors without touching the bundle.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Format version written by this app. Bump per semver rules; readers accept
/// any bundle whose major matches [`FORMAT_MAJOR`].
// 0.2.0 (v2): activated the reserved learning dirs (`learning_state/`,
// `quizzes/`, `flashcards/`, `glossary/lessons/`) and added the derived
// `knowledge_graph.json`.
// 0.3.0 (v3): activates `implementations/` and `experiments/`, adds
// `reproduction/` (repo reference, env plan, code map, report) and
// `consents.jsonl` (sandbox consent journal).
// 0.4.0 (v4): adds `research/` (weaknesses, hypothesis cards, outline,
// draft); library-level `reviews/`, `gaps/`, `workspaces/`. Additive only —
// same major, so older readers open newer bundles and preserve the new
// files untouched (unknown-file rule).
// 0.5.0 (v5): adds `contributions/` (proposal change sets + provenance
// journal), `plugins/` (per-plugin consent journal), and a lazily-written
// `registry.json` (canonical paper identity + pulled-layer manifests).
// Additive only, same major.
pub const FORMAT_VERSION: &str = "0.5.0";
pub const FORMAT_MAJOR: u64 = 0;

/// Derived directories created on ingestion.
const DERIVED_DIRS: [&str; 4] = ["equations", "figures", "tables", "glossary"];
/// User-data directories: append-only, sync-mergeable.
const USER_DIRS: [&str; 5] = ["notes", "bookmarks", "chats", "contributions", "plugins"];

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid json in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("not a .research bundle (missing metadata.json): {0}")]
    NotABundle(PathBuf),
    #[error(
        "this bundle uses format {found}, newer than this app supports (major {supported}); \
         update Research Paper Copilot to open it — the bundle has not been modified"
    )]
    NewerFormatVersion { found: String, supported: u64 },
    #[error("unparseable format_version: {0}")]
    BadFormatVersion(String),
    #[error("content hash mismatch for {path}: expected {expected}, found {found}")]
    HashMismatch {
        path: PathBuf,
        expected: String,
        found: String,
    },
}

impl BundleError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        BundleError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

type Result<T> = std::result::Result<T, BundleError>;

// ---------------------------------------------------------------------------
// Metadata model (mirrors schemas/research-format/v0/metadata.schema.json).
// Unknown fields are captured via `extra` maps so a same-major bundle written
// by a newer app round-trips losslessly.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Metadata {
    pub format_version: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub paper: Paper,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    pub content_hashes: serde_json::Map<String, serde_json::Value>,
    pub pipeline: Pipeline,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Paper {
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(rename = "abstract", skip_serializing_if = "Option::is_none")]
    pub abstract_text: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Source {
    pub imported_from: String,
    pub imported_at: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Pipeline {
    pub stages: serde_json::Map<String, serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// `abstract` is reserved in Rust; keep the JSON name from the schema.
impl Paper {
    pub fn new(title: impl Into<String>) -> Self {
        Paper {
            title: title.into(),
            authors: Vec::new(),
            abstract_text: None,
            extra: serde_json::Map::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Hashing
// ---------------------------------------------------------------------------

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|e| BundleError::io(path, e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| BundleError::io(path, e))?;
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC3339 formatting of the current time cannot fail")
}

fn parse_major(version: &str) -> Result<u64> {
    version
        .split('.')
        .next()
        .and_then(|major| major.parse().ok())
        .ok_or_else(|| BundleError::BadFormatVersion(version.to_string()))
}

// ---------------------------------------------------------------------------
// Bundle
// ---------------------------------------------------------------------------

pub struct Bundle {
    root: PathBuf,
}

impl Bundle {
    /// Create a new bundle directory around a PDF. Copies the PDF in as
    /// `original.pdf`, records its content hash, and writes `metadata.json`.
    pub fn create(
        root: &Path,
        pdf_bytes: &[u8],
        paper: Paper,
        imported_from: &str,
    ) -> Result<Self> {
        fs::create_dir_all(root).map_err(|e| BundleError::io(root, e))?;
        for dir in DERIVED_DIRS.iter().chain(USER_DIRS.iter()) {
            let path = root.join(dir);
            fs::create_dir_all(&path).map_err(|e| BundleError::io(&path, e))?;
        }

        let pdf_path = root.join("original.pdf");
        write_atomic(&pdf_path, pdf_bytes)?;

        let now = now_rfc3339();
        let mut content_hashes = serde_json::Map::new();
        content_hashes.insert(
            "original.pdf".to_string(),
            serde_json::Value::String(sha256_bytes(pdf_bytes)),
        );

        let metadata = Metadata {
            format_version: FORMAT_VERSION.to_string(),
            created_at: now.clone(),
            updated_at: None,
            paper,
            source: Some(Source {
                imported_from: imported_from.to_string(),
                imported_at: now,
                extra: serde_json::Map::new(),
            }),
            content_hashes,
            pipeline: Pipeline::default(),
            embedding_model: None,
            extra: serde_json::Map::new(),
        };

        let bundle = Bundle {
            root: root.to_path_buf(),
        };
        bundle.write_metadata(&metadata)?;
        Ok(bundle)
    }

    /// Open an existing bundle, refusing newer major format versions without
    /// modifying anything.
    pub fn open(root: &Path) -> Result<Self> {
        let metadata_path = root.join("metadata.json");
        if !metadata_path.is_file() {
            return Err(BundleError::NotABundle(root.to_path_buf()));
        }
        // Version check reads only format_version so even otherwise-invalid
        // newer bundles get the "update required" message, not a parse error.
        #[derive(Deserialize)]
        struct VersionOnly {
            format_version: String,
        }
        let version: VersionOnly = read_json(&metadata_path)?;
        let major = parse_major(&version.format_version)?;
        if major > FORMAT_MAJOR {
            return Err(BundleError::NewerFormatVersion {
                found: version.format_version,
                supported: FORMAT_MAJOR,
            });
        }
        Ok(Bundle {
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn metadata(&self) -> Result<Metadata> {
        read_json(&self.root.join("metadata.json"))
    }

    /// Atomically replace `metadata.json` (temp file + rename), stamping
    /// `updated_at`. Never touches any other file — unknown files survive.
    pub fn write_metadata(&self, metadata: &Metadata) -> Result<()> {
        let mut metadata = metadata.clone();
        metadata.updated_at = Some(now_rfc3339());
        let path = self.root.join("metadata.json");
        let json = serde_json::to_vec_pretty(&metadata).map_err(|e| BundleError::Json {
            path: path.clone(),
            source: e,
        })?;
        write_atomic(&path, &json)
    }

    /// Atomically write a derived JSON artifact (e.g. `layout.json`) and
    /// record its content hash + the producing stage in `metadata.json`.
    pub fn write_derived_json<T: Serialize>(
        &self,
        relative_path: &str,
        value: &T,
        stage_name: &str,
        stage: serde_json::Value,
    ) -> Result<()> {
        let path = self.root.join(relative_path);
        let json = serde_json::to_vec_pretty(value).map_err(|e| BundleError::Json {
            path: path.clone(),
            source: e,
        })?;
        write_atomic(&path, &json)?;

        let mut metadata = self.metadata()?;
        metadata.content_hashes.insert(
            relative_path.to_string(),
            serde_json::Value::String(sha256_bytes(&json)),
        );
        metadata
            .pipeline
            .stages
            .insert(stage_name.to_string(), stage);
        self.write_metadata(&metadata)
    }

    /// Read a derived JSON artifact; `Ok(None)` when the stage hasn't run.
    pub fn read_derived_json<T: DeserializeOwned>(&self, relative_path: &str) -> Result<Option<T>> {
        let path = self.root.join(relative_path);
        if !path.is_file() {
            return Ok(None);
        }
        read_json(&path).map(Some)
    }

    /// Atomically write a small user-data JSON file (e.g. reading_state.json).
    /// Unlike derived artifacts it does not touch metadata — user data is not
    /// content-hashed or stage-tracked.
    pub fn write_user_json<T: Serialize>(&self, relative_path: &str, value: &T) -> Result<()> {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| BundleError::io(parent, e))?;
        }
        let json = serde_json::to_vec_pretty(value).map_err(|e| BundleError::Json {
            path: path.clone(),
            source: e,
        })?;
        write_atomic(&path, &json)
    }

    pub fn original_pdf_path(&self) -> PathBuf {
        self.root.join("original.pdf")
    }

    /// Verify `original.pdf` against the hash recorded at import.
    pub fn verify_original(&self) -> Result<()> {
        let metadata = self.metadata()?;
        let pdf_path = self.root.join("original.pdf");
        let expected = metadata
            .content_hashes
            .get("original.pdf")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let found = sha256_file(&pdf_path)?;
        if expected != found {
            return Err(BundleError::HashMismatch {
                path: pdf_path,
                expected,
                found,
            });
        }
        Ok(())
    }

    /// Append-only journal under the bundle, e.g. `chats/<uuid>.jsonl`.
    pub fn journal(&self, relative_path: &str) -> Journal {
        Journal {
            path: self.root.join(relative_path),
        }
    }
}

// ---------------------------------------------------------------------------
// Append-only JSONL journal
// ---------------------------------------------------------------------------

/// One JSON document per line; a torn trailing line (crash mid-write) is
/// ignored on read so committed entries always load.
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    /// Journal at an absolute path — for stores that live outside a bundle
    /// (e.g. the library-level `learning_state/` journals).
    pub fn at(path: PathBuf) -> Journal {
        Journal { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append<T: Serialize>(&self, entry: &T) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| BundleError::io(parent, e))?;
        }
        let mut line = serde_json::to_vec(entry).map_err(|e| BundleError::Json {
            path: self.path.clone(),
            source: e,
        })?;
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| BundleError::io(&self.path, e))?;
        // Heal a torn trailing line from a previous crash: terminate it so the
        // new entry starts on its own line instead of merging into garbage.
        if !ends_with_newline(&self.path)? {
            file.write_all(b"\n")
                .map_err(|e| BundleError::io(&self.path, e))?;
        }
        file.write_all(&line)
            .map_err(|e| BundleError::io(&self.path, e))?;
        file.sync_data()
            .map_err(|e| BundleError::io(&self.path, e))?;
        Ok(())
    }

    /// Read all committed entries. An unparseable line is treated as a torn
    /// write and skipped (torn lines are healed, not removed, on the next
    /// append, so they can appear mid-file); committed entries always load.
    pub fn read_all<T: DeserializeOwned>(&self) -> Result<Vec<T>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&self.path).map_err(|e| BundleError::io(&self.path, e))?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader
            .lines()
            .collect::<std::io::Result<_>>()
            .map_err(|e| BundleError::io(&self.path, e))?;
        let mut entries = Vec::with_capacity(lines.len());
        for line in &lines {
            if line.trim().is_empty() {
                continue;
            }
            // Unparseable lines are torn writes from crashes; committed
            // entries stand. Torn lines are preserved on disk, never erased.
            if let Ok(entry) = serde_json::from_str(line) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| BundleError::io(path, e))?;
    serde_json::from_slice(&bytes).map_err(|e| BundleError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

fn ends_with_newline(path: &Path) -> Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = File::open(path).map_err(|e| BundleError::io(path, e))?;
    let len = file.metadata().map_err(|e| BundleError::io(path, e))?.len();
    if len == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::End(-1))
        .map_err(|e| BundleError::io(path, e))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)
        .map_err(|e| BundleError::io(path, e))?;
    Ok(last[0] == b'\n')
}

/// Write via temp file + fsync + rename so readers never observe a partial file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut file = File::create(&tmp).map_err(|e| BundleError::io(&tmp, e))?;
        file.write_all(bytes)
            .map_err(|e| BundleError::io(&tmp, e))?;
        file.sync_data().map_err(|e| BundleError::io(&tmp, e))?;
    }
    fs::rename(&tmp, path).map_err(|e| BundleError::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bundle(dir: &Path) -> Bundle {
        Bundle::create(
            dir,
            b"%PDF-1.5 fake pdf bytes",
            Paper::new("Attention Is All You Need"),
            "file",
        )
        .unwrap()
    }

    #[test]
    fn create_open_roundtrip_and_hash_verify() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        sample_bundle(&root);

        let bundle = Bundle::open(&root).unwrap();
        let metadata = bundle.metadata().unwrap();
        assert_eq!(metadata.format_version, FORMAT_VERSION);
        assert_eq!(metadata.paper.title, "Attention Is All You Need");
        bundle.verify_original().unwrap();

        for dir in DERIVED_DIRS.iter().chain(USER_DIRS.iter()) {
            assert!(root.join(dir).is_dir(), "missing {dir}/");
        }
    }

    #[test]
    fn tampered_pdf_fails_verification() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = sample_bundle(&root);
        fs::write(root.join("original.pdf"), b"tampered").unwrap();
        assert!(matches!(
            bundle.verify_original(),
            Err(BundleError::HashMismatch { .. })
        ));
    }

    #[test]
    fn newer_major_version_is_refused_without_modification() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        sample_bundle(&root);

        let metadata_path = root.join("metadata.json");
        let mut doc: serde_json::Value =
            serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
        doc["format_version"] = serde_json::Value::String("99.0.0".into());
        fs::write(&metadata_path, serde_json::to_vec(&doc).unwrap()).unwrap();
        let before = fs::read(&metadata_path).unwrap();

        match Bundle::open(&root) {
            Err(BundleError::NewerFormatVersion { found, supported }) => {
                assert_eq!(found, "99.0.0");
                assert_eq!(supported, FORMAT_MAJOR);
            }
            other => panic!(
                "expected NewerFormatVersion, got {other:?}",
                other = other.err()
            ),
        }
        assert_eq!(
            fs::read(&metadata_path).unwrap(),
            before,
            "bundle was modified"
        );
    }

    #[test]
    fn older_minor_bundles_open_across_bumps() {
        // v2 bumped 0.1.0 → 0.2.0, v3 → 0.3.0, v4 → 0.4.0, v5 → 0.5.0 (all
        // additive). Same major → bundles written by any older app version
        // open unmodified, and the newer versions' extra artifacts are
        // exactly the unknown-file case for an older reader (covered below).
        for older in ["0.1.0", "0.2.0", "0.3.0", "0.4.0"] {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("paper.research");
            sample_bundle(&root);

            let metadata_path = root.join("metadata.json");
            let mut doc: serde_json::Value =
                serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
            doc["format_version"] = serde_json::Value::String(older.into());
            fs::write(&metadata_path, serde_json::to_vec(&doc).unwrap()).unwrap();

            let bundle = Bundle::open(&root).expect("same-major older minor opens");
            assert_eq!(bundle.metadata().unwrap().format_version, older);
        }
    }

    #[test]
    fn unknown_files_survive_metadata_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = sample_bundle(&root);

        // A directory from some future app version.
        let future_dir = root.join("learning_state");
        fs::create_dir_all(&future_dir).unwrap();
        fs::write(future_dir.join("mastery.json"), b"{\"score\": 1}").unwrap();

        let metadata = bundle.metadata().unwrap();
        bundle.write_metadata(&metadata).unwrap();

        assert_eq!(
            fs::read(future_dir.join("mastery.json")).unwrap(),
            b"{\"score\": 1}"
        );
    }

    #[test]
    fn metadata_unknown_fields_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = sample_bundle(&root);

        // Simulate a newer same-major app having written an extra field.
        let metadata_path = root.join("metadata.json");
        let mut doc: serde_json::Value =
            serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
        doc["future_field"] = serde_json::json!({"kept": true});
        fs::write(&metadata_path, serde_json::to_vec(&doc).unwrap()).unwrap();

        let metadata = bundle.metadata().unwrap();
        bundle.write_metadata(&metadata).unwrap();

        let reread: serde_json::Value =
            serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
        assert_eq!(reread["future_field"]["kept"], serde_json::json!(true));
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
    struct ChatMessage {
        role: String,
        text: String,
    }

    #[test]
    fn journal_append_and_read() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = sample_bundle(&root);

        let journal = bundle.journal("chats/b71c9d2e-4a3f-4c5d-9e8f-1a2b3c4d5e02.jsonl");
        for i in 0..3 {
            journal
                .append(&ChatMessage {
                    role: "user".into(),
                    text: format!("message {i}"),
                })
                .unwrap();
        }
        let entries: Vec<ChatMessage> = journal.read_all().unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].text, "message 2");
    }

    #[test]
    fn journal_survives_torn_trailing_write() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = sample_bundle(&root);

        let journal = bundle.journal("chats/thread.jsonl");
        journal
            .append(&ChatMessage {
                role: "user".into(),
                text: "committed".into(),
            })
            .unwrap();

        // Simulate a crash mid-append: partial JSON, no trailing newline.
        let mut file = OpenOptions::new()
            .append(true)
            .open(journal.path())
            .unwrap();
        file.write_all(b"{\"role\": \"assistant\", \"te").unwrap();
        drop(file);

        let entries: Vec<ChatMessage> = journal.read_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "committed");

        // Appending after the crash heals the torn line; nothing is lost.
        journal
            .append(&ChatMessage {
                role: "user".into(),
                text: "after crash".into(),
            })
            .unwrap();
        let entries: Vec<ChatMessage> = journal.read_all().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].text, "after crash");
    }
}
