//! Sync manifests: classify every library/bundle file by the format's
//! layer table and record what a consistent remote state looks like.
//!
//! Exclusion rules are the design's decision 3: heavy re-derivable caches
//! (`embeddings.bin`, `graph.db`, `repos/`, `sync_state/`, telemetry) never
//! sync; small derived JSONs do (cheap, saves re-ingestion); user data
//! always does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// Immutable, content-addressed (original.pdf).
    Source,
    /// Regenerable; synced because it's small and saves work.
    Derived,
    /// Journals + documents — the valuable part; always syncs.
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Library-relative path (e.g. "paper.research/notes/notes.jsonl").
    pub path: String,
    pub layer: Layer,
    /// sha256 of the plaintext content.
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    /// Remote layout version (future evolution).
    pub layout_version: u32,
    /// Monotonic generation for compare-and-swap.
    pub generation: u64,
    pub device_id: String,
    pub written_at: String,
    /// path → entry (BTreeMap for stable serialization/diffing).
    pub entries: BTreeMap<String, ManifestEntry>,
    /// Tombstoned bundle directories (deleted papers), path → deleted_at.
    #[serde(default)]
    pub tombstones: BTreeMap<String, String>,
}

/// Directory names (library-level or in-bundle) that never sync.
const EXCLUDED_DIRS: [&str; 4] = ["repos", "sync_state", "telemetry", ".trash"];
/// File names that never sync: heavy re-derivable caches, plus SQLite WAL
/// sidecars (transient mid-write state; the main db file is the artifact).
const EXCLUDED_FILES: [&str; 4] = [
    "embeddings.bin",
    "graph.db",
    "workspace.db-wal",
    "workspace.db-shm",
];

fn classify(relative: &Path) -> Option<Layer> {
    let first = relative.components().next()?.as_os_str().to_string_lossy();
    let name = relative.file_name()?.to_string_lossy();
    if EXCLUDED_FILES.contains(&name.as_ref())
        || name.ends_with(".tmp")
        || name.ends_with(".conflict")
    {
        return None;
    }
    if EXCLUDED_DIRS.contains(&first.as_ref()) {
        return None;
    }
    // In-bundle paths: component after "<paper>.research".
    let in_bundle = relative
        .components()
        .nth(1)
        .map(|c| c.as_os_str().to_string_lossy().to_string());
    if name == "original.pdf" {
        return Some(Layer::Source);
    }
    let user_dirs = [
        "notes",
        "bookmarks",
        "chats",
        "implementations",
        "experiments",
        "research",
        "contributions",
        "plugins",
    ];
    let user_files = ["consents.jsonl", "reading_state.json", "registry.json"];
    if let Some(dir) = &in_bundle {
        if user_dirs.contains(&dir.as_str()) || user_files.contains(&dir.as_str()) {
            return Some(Layer::User);
        }
    }
    // Library-level user stores. The second group are workspace-internal
    // names: workspace collaboration syncs with the engine rooted AT the
    // workspace directory (collab::sync_workspace), so its journals arrive
    // here as top-level paths. A main library root never contains these
    // names at top level, so the two shapes coexist in one classifier.
    let library_user = [
        "learning_state",
        "concepts.jsonl",
        "reviews",
        "gaps",
        "workspaces",
        "workspace.json",
        "membership.jsonl",
        "assignments.jsonl",
        "progress.jsonl",
        "presence.jsonl",
        "threads",
        "papers",
        // The workspace store (notes/canvases/chat threads) is user data.
        "workspace.db",
    ];
    if library_user.contains(&first.as_ref()) {
        return Some(Layer::User);
    }
    // Everything else that survives exclusion is derived (layout.json,
    // semantic_tree.json, knowledge_graph.json, figures/, reproduction/, …).
    Some(Layer::Derived)
}

/// Walk a library root and build its manifest entries (generation and
/// device fields are the engine's business).
pub fn build_entries(library_root: &Path) -> std::io::Result<BTreeMap<String, ManifestEntry>> {
    let mut entries = BTreeMap::new();
    walk(library_root, library_root, &mut entries)?;
    Ok(entries)
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, ManifestEntry>) -> std::io::Result<()> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let relative: PathBuf = path.strip_prefix(root).expect("under root").to_path_buf();
        if path.is_dir() {
            // Prune excluded directories before descending.
            let first = relative
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .unwrap_or_default();
            if EXCLUDED_DIRS.contains(&first.as_str()) {
                continue;
            }
            walk(root, &path, out)?;
        } else if let Some(layer) = classify(&relative) {
            let bytes = std::fs::read(&path)?;
            let key = relative.to_string_lossy().replace('\\', "/");
            out.insert(
                key.clone(),
                ManifestEntry {
                    path: key,
                    layer,
                    hash: crate::bundle::sha256_bytes(&bytes),
                    size: bytes.len() as u64,
                },
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str, content: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn layers_and_exclusions_match_the_format_table() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // A miniature library.
        touch(root, "p.research/original.pdf", b"%PDF");
        touch(root, "p.research/semantic_tree.json", b"{}");
        touch(root, "p.research/knowledge_graph.json", b"{}");
        touch(root, "p.research/figures/f1.png", b"png");
        touch(root, "p.research/embeddings.bin", b"heavy");
        touch(root, "p.research/notes/notes.jsonl", b"{}");
        touch(root, "p.research/consents.jsonl", b"{}");
        touch(root, "p.research/research/hypotheses.jsonl", b"{}");
        touch(root, "learning_state/mastery.jsonl", b"{}");
        touch(root, "concepts.jsonl", b"{}");
        touch(root, "graph.db", b"cache");
        touch(root, "repos/abc/file.py", b"clone");
        touch(root, "sync_state/last.json", b"{}");
        touch(root, "reviews/r1/document.md", b"# r");

        let entries = build_entries(root).unwrap();
        let layer = |p: &str| entries.get(p).map(|e| e.layer);

        assert_eq!(layer("p.research/original.pdf"), Some(Layer::Source));
        assert_eq!(layer("p.research/semantic_tree.json"), Some(Layer::Derived));
        assert_eq!(layer("p.research/figures/f1.png"), Some(Layer::Derived));
        assert_eq!(layer("p.research/notes/notes.jsonl"), Some(Layer::User));
        assert_eq!(layer("p.research/consents.jsonl"), Some(Layer::User));
        assert_eq!(
            layer("p.research/research/hypotheses.jsonl"),
            Some(Layer::User)
        );
        assert_eq!(layer("learning_state/mastery.jsonl"), Some(Layer::User));
        assert_eq!(layer("concepts.jsonl"), Some(Layer::User));
        assert_eq!(layer("reviews/r1/document.md"), Some(Layer::User));

        // The exclusion rules: heavy caches never listed.
        assert!(!entries.contains_key("p.research/embeddings.bin"));
        assert!(!entries.contains_key("graph.db"));
        assert!(!entries.keys().any(|k| k.starts_with("repos/")));
        assert!(!entries.keys().any(|k| k.starts_with("sync_state/")));

        // Hashes are real content hashes.
        assert!(entries["p.research/original.pdf"]
            .hash
            .starts_with("sha256:"));
    }
}
