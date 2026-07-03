//! Paper library: the directory of `.research` bundles the app manages.
//!
//! The library is a plain directory; each child ending in `.research` is a
//! bundle. Summaries are read from each bundle's `metadata.json` — cheap
//! enough to scan hundreds of papers well inside the 1.5 s cold-start budget
//! (measured in the perf suite).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bundle::Bundle;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("paper not found: {0}")]
    NotFound(String),
}

/// Ingestion status distilled from per-stage records for the list UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestionStatus {
    /// All recorded stages complete.
    Ready,
    /// Some stage pending/running — paper may already be readable.
    Processing,
    /// A stage degraded; readable with flagged limitations.
    Degraded,
    /// Layout failed; raw view only.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperSummary {
    /// Bundle directory name; stable id for all commands.
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub status: IngestionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_opened: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arxiv_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default)]
    pub starred: bool,
    /// User-set reading priority ("high" | "medium" | "low"), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}

pub struct Library {
    root: PathBuf,
}

impl Library {
    /// Open (creating if needed) the library at `root`.
    pub fn open(root: &Path) -> Result<Self, LibraryError> {
        std::fs::create_dir_all(root).map_err(|e| LibraryError::Io {
            path: root.to_path_buf(),
            source: e,
        })?;
        Ok(Library {
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn bundle_path(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// A filesystem-safe, unique bundle directory name from a title.
    pub fn new_bundle_id(&self, title: &str) -> String {
        let slug: String = title
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
            .chars()
            .take(60)
            .collect();
        let base = if slug.is_empty() { "paper" } else { &slug };
        let mut id = format!("{base}.research");
        let mut n = 2;
        while self.root.join(&id).exists() {
            id = format!("{base}-{n}.research");
            n += 1;
        }
        id
    }

    /// List all bundles, most recently imported first. Unreadable bundles are
    /// skipped (never block the library on one bad directory).
    pub fn list(&self) -> Result<Vec<PaperSummary>, LibraryError> {
        let entries = std::fs::read_dir(&self.root).map_err(|e| LibraryError::Io {
            path: self.root.clone(),
            source: e,
        })?;
        let mut papers = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || path.extension().and_then(|e| e.to_str()) != Some("research") {
                continue;
            }
            let Some(summary) = self.summarize(&path) else {
                continue;
            };
            papers.push(summary);
        }
        papers.sort_by(|a, b| b.imported_at.cmp(&a.imported_at));
        Ok(papers)
    }

    fn summarize(&self, path: &Path) -> Option<PaperSummary> {
        let bundle = Bundle::open(path).ok()?;
        let metadata = bundle.metadata().ok()?;

        let mut status = IngestionStatus::Ready;
        let stages = &metadata.pipeline.stages;
        if stages.is_empty() {
            status = IngestionStatus::Processing;
        }
        for (name, record) in stages {
            match record["status"].as_str().unwrap_or("pending") {
                "failed" if name == "layout" => {
                    status = IngestionStatus::Failed;
                    break;
                }
                "failed" | "degraded" => status = IngestionStatus::Degraded,
                "pending" | "running" if status == IngestionStatus::Ready => {
                    status = IngestionStatus::Processing;
                }
                _ => {}
            }
        }

        let extra_str = |map: &serde_json::Map<String, serde_json::Value>, key: &str| {
            map.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
        };
        let identifiers = metadata.paper.extra.get("identifiers").cloned();
        let id_str = |key: &str| {
            identifiers
                .as_ref()
                .and_then(|v| v.get(key))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        };

        Some(PaperSummary {
            id: path.file_name()?.to_string_lossy().to_string(),
            title: metadata.paper.title.clone(),
            authors: metadata.paper.authors.clone(),
            status,
            imported_at: metadata.source.as_ref().map(|s| s.imported_at.clone()),
            last_opened: extra_str(&metadata.extra, "last_opened"),
            arxiv_id: id_str("arxiv_id"),
            doi: id_str("doi"),
            published_at: extra_str(&metadata.paper.extra, "published_at"),
            starred: metadata
                .extra
                .get("starred")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            priority: extra_str(&metadata.extra, "priority"),
        })
    }

    /// Star/unstar a paper (favorite). Returns the new state.
    pub fn toggle_starred(&self, id: &str) -> Result<bool, LibraryError> {
        let bundle = self.get(id)?;
        let mut metadata = bundle.metadata()?;
        let now = !metadata
            .extra
            .get("starred")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        metadata
            .extra
            .insert("starred".to_string(), serde_json::Value::Bool(now));
        bundle.write_metadata(&metadata)?;
        Ok(now)
    }

    /// Set (or clear) the reading priority.
    pub fn set_priority(&self, id: &str, priority: Option<&str>) -> Result<(), LibraryError> {
        let bundle = self.get(id)?;
        let mut metadata = bundle.metadata()?;
        match priority {
            Some(p) => {
                metadata
                    .extra
                    .insert("priority".to_string(), serde_json::Value::String(p.into()));
            }
            None => {
                metadata.extra.remove("priority");
            }
        }
        bundle.write_metadata(&metadata)?;
        Ok(())
    }

    /// Record that a paper was opened (drives "last opened" ordering).
    pub fn touch_opened(&self, id: &str) -> Result<(), LibraryError> {
        let bundle = self.get(id)?;
        let mut metadata = bundle.metadata()?;
        metadata.extra.insert(
            "last_opened".to_string(),
            serde_json::Value::String(crate::bundle::now_rfc3339()),
        );
        bundle.write_metadata(&metadata)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Bundle, LibraryError> {
        let path = self.bundle_path(id);
        if !path.is_dir() {
            return Err(LibraryError::NotFound(id.to_string()));
        }
        Ok(Bundle::open(&path)?)
    }

    /// Delete a bundle. Only ever removes the bundle directory inside the
    /// library — the user's original PDF elsewhere on disk is untouched.
    pub fn delete(&self, id: &str) -> Result<(), LibraryError> {
        let path = self.bundle_path(id);
        if !path.is_dir() {
            return Err(LibraryError::NotFound(id.to_string()));
        }
        // Refuse to delete anything that isn't a .research directory in our
        // root (paranoia against path tricks in `id`).
        let canonical = path.canonicalize().map_err(|e| LibraryError::Io {
            path: path.clone(),
            source: e,
        })?;
        let root = self.root.canonicalize().map_err(|e| LibraryError::Io {
            path: self.root.clone(),
            source: e,
        })?;
        if !canonical.starts_with(&root)
            || canonical.extension().and_then(|e| e.to_str()) != Some("research")
        {
            return Err(LibraryError::NotFound(id.to_string()));
        }
        std::fs::remove_dir_all(&canonical).map_err(|e| LibraryError::Io {
            path: canonical,
            source: e,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    fn make_paper(library: &Library, title: &str) -> String {
        let id = library.new_bundle_id(title);
        Bundle::create(
            &library.bundle_path(&id),
            b"%PDF-1.5 fake",
            Paper::new(title),
            "file",
        )
        .unwrap();
        id
    }

    #[test]
    fn list_shows_imported_papers_with_status() {
        let tmp = tempfile::tempdir().unwrap();
        let library = Library::open(tmp.path()).unwrap();
        let id = make_paper(&library, "Attention Is All You Need");

        let papers = library.list().unwrap();
        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].id, id);
        assert_eq!(papers[0].title, "Attention Is All You Need");
        assert_eq!(papers[0].status, IngestionStatus::Processing); // no stages yet

        // Junk in the library directory is ignored.
        std::fs::create_dir(tmp.path().join("not-a-bundle")).unwrap();
        std::fs::write(tmp.path().join("stray.txt"), b"x").unwrap();
        assert_eq!(library.list().unwrap().len(), 1);
    }

    #[test]
    fn ids_are_unique_slugs() {
        let tmp = tempfile::tempdir().unwrap();
        let library = Library::open(tmp.path()).unwrap();
        let a = make_paper(&library, "Same Title!");
        let b = make_paper(&library, "Same Title!");
        assert_eq!(a, "same-title.research");
        assert_eq!(b, "same-title-2.research");
    }

    #[test]
    fn delete_removes_only_the_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let library = Library::open(tmp.path()).unwrap();
        let id = make_paper(&library, "Doomed");
        // A sibling the delete must not touch.
        let sibling = make_paper(&library, "Survivor");

        library.delete(&id).unwrap();
        assert!(!library.bundle_path(&id).exists());
        assert!(library.bundle_path(&sibling).exists());

        assert!(matches!(
            library.delete("../../etc"),
            Err(LibraryError::NotFound(_))
        ));
        assert!(matches!(
            library.delete(&id),
            Err(LibraryError::NotFound(_))
        ));
    }

    #[test]
    fn touch_opened_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let library = Library::open(tmp.path()).unwrap();
        let id = make_paper(&library, "Read Me");
        assert!(library.list().unwrap()[0].last_opened.is_none());
        library.touch_opened(&id).unwrap();
        assert!(library.list().unwrap()[0].last_opened.is_some());
    }
}
