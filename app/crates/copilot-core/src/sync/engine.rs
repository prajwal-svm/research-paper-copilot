//! The sync engine: a resumable reconcile loop over a dumb encrypted remote.
//!
//! Remote layout (all values ciphertext except `key.salt`):
//!   key.salt                    Argon2id salt (public by design)
//!   manifest-{generation:012}   encrypted manifest JSON (create-only puts)
//!   <hmac-name>                 encrypted blob for one (path, hash) version
//!
//! Order of operations makes partial failure invisible: blobs first, the
//! manifest swap last — other devices never see a state whose blobs aren't
//! fully uploaded. Deletions travel as manifest tombstones; receiving
//! devices move bundles to a local `.trash/` (grace period). The remote is
//! only ever garbage-collected by an explicit action.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::crypto::{self, LibraryKey};
use super::manifest::{self, Layer, Manifest};
use super::merge;
use super::remote::{Remote, RemoteError};

pub const SYNC_STATE_DIR: &str = "sync_state";
const LAYOUT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Remote(#[from] RemoteError),
    #[error("sync: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "Wrong passphrase for this remote (or corrupted remote data). Nothing was changed locally."
    )]
    WrongPassphrase,
    #[error("sync: another device kept winning the manifest race — try again")]
    RetriesExhausted,
}

/// Local, cache-class engine state (never source of truth; losing it only
/// costs re-checking, never data).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SyncState {
    last_generation: u64,
    /// (path@hash) blob versions known to be on the remote — the
    /// resume-without-reupload set.
    uploaded: BTreeSet<String>,
    /// Local pending tombstones (bundle dir names) not yet in a pushed
    /// manifest.
    pending_tombstones: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncOutcome {
    pub pulled_files: usize,
    pub merged_journals: usize,
    pub conflict_copies: Vec<String>,
    pub pushed_blobs: usize,
    pub trashed_papers: Vec<String>,
    pub generation: u64,
}

pub struct SyncEngine<'a> {
    pub library_root: &'a Path,
    pub device_id: String,
    pub key: LibraryKey,
    pub remote: &'a dyn Remote,
}

impl<'a> SyncEngine<'a> {
    fn state_path(&self) -> PathBuf {
        self.library_root.join(SYNC_STATE_DIR).join("state.json")
    }

    fn load_state(&self) -> SyncState {
        std::fs::read(self.state_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, state: &SyncState) -> Result<(), SyncError> {
        std::fs::create_dir_all(self.library_root.join(SYNC_STATE_DIR))?;
        let tmp = self.state_path().with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(state).expect("serializable"),
        )?;
        std::fs::rename(&tmp, self.state_path())?;
        Ok(())
    }

    /// Record a paper deletion for propagation (called by delete flows).
    pub fn record_tombstone(library_root: &Path, bundle_dir: &str) -> std::io::Result<()> {
        let path = library_root.join(SYNC_STATE_DIR).join("state.json");
        let mut state: SyncState = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        state
            .pending_tombstones
            .insert(bundle_dir.to_string(), crate::bundle::now_rfc3339());
        std::fs::create_dir_all(path.parent().expect("state dir"))?;
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&state).expect("serializable"),
        )
    }

    fn blob_key(&self, path: &str, hash: &str) -> String {
        crypto::blob_name(&self.key, &format!("{path}@{hash}"))
    }

    /// Latest remote manifest, if any. Wrong key → typed error, no changes.
    fn fetch_remote_manifest(&self) -> Result<Option<Manifest>, SyncError> {
        let mut names = self.remote.list("manifest-")?;
        names.sort();
        let Some(latest) = names.last() else {
            return Ok(None);
        };
        let Some(blob) = self.remote.get(latest)? else {
            return Ok(None);
        };
        let plaintext =
            crypto::decrypt(&self.key, &blob).map_err(|_| SyncError::WrongPassphrase)?;
        let manifest: Manifest =
            serde_json::from_slice(&plaintext).map_err(|_| SyncError::WrongPassphrase)?;
        Ok(Some(manifest))
    }

    /// One full reconcile: pull → merge → push → swap. Resumable and
    /// interruptible; a failed push leaves the previous consistent state
    /// visible to other devices.
    pub fn sync(&self, on_progress: &mut dyn FnMut(&str)) -> Result<SyncOutcome, SyncError> {
        let mut state = self.load_state();
        let mut outcome = SyncOutcome::default();

        for attempt in 0..3 {
            if attempt > 0 {
                on_progress("manifest race lost — re-merging and retrying");
            }
            let remote_manifest = self.fetch_remote_manifest()?;
            let remote_generation = remote_manifest.as_ref().map(|m| m.generation).unwrap_or(0);

            // ---- PULL ----
            if let Some(remote_manifest) = &remote_manifest {
                self.pull(remote_manifest, &mut state, &mut outcome, on_progress)?;
            }

            // ---- Local view after pull ----
            let entries = manifest::build_entries(self.library_root)?;
            let mut tombstones = remote_manifest
                .as_ref()
                .map(|m| m.tombstones.clone())
                .unwrap_or_default();
            for (dir, at) in &state.pending_tombstones {
                tombstones.entry(dir.clone()).or_insert_with(|| at.clone());
            }
            // A tombstoned bundle's files never re-enter the manifest.
            let entries: BTreeMap<String, manifest::ManifestEntry> = entries
                .into_iter()
                .filter(|(path, _)| {
                    !tombstones
                        .keys()
                        .any(|dir| path.starts_with(&format!("{dir}/")))
                })
                .collect();

            // ---- PUSH blobs (before any manifest becomes visible) ----
            let remote_entries = remote_manifest
                .as_ref()
                .map(|m| m.entries.clone())
                .unwrap_or_default();
            for (path, entry) in &entries {
                let already_remote = remote_entries
                    .get(path)
                    .map(|r| r.hash == entry.hash)
                    .unwrap_or(false);
                let version = format!("{path}@{}", entry.hash);
                if already_remote || state.uploaded.contains(&version) {
                    continue;
                }
                let plaintext = std::fs::read(self.library_root.join(path))?;
                let blob = crypto::encrypt(&self.key, &plaintext);
                on_progress(&format!("uploading {path}"));
                self.remote.put(&self.blob_key(path, &entry.hash), &blob)?;
                state.uploaded.insert(version);
                outcome.pushed_blobs += 1;
                self.save_state(&state)?; // resume point after every blob
            }

            // ---- Manifest swap (create-only on the next generation) ----
            let next = Manifest {
                layout_version: LAYOUT_VERSION,
                generation: remote_generation + 1,
                device_id: self.device_id.clone(),
                written_at: crate::bundle::now_rfc3339(),
                entries,
                tombstones,
            };
            let name = format!("manifest-{:012}", next.generation);
            let blob =
                crypto::encrypt(&self.key, &serde_json::to_vec(&next).expect("serializable"));
            match self.remote.put_if_absent(&name, &blob) {
                Ok(()) => {
                    state.last_generation = next.generation;
                    state.pending_tombstones.clear();
                    self.save_state(&state)?;
                    outcome.generation = next.generation;
                    return Ok(outcome);
                }
                Err(RemoteError::Conflict) => continue, // another writer won
                Err(e) => return Err(e.into()),
            }
        }
        Err(SyncError::RetriesExhausted)
    }

    fn pull(
        &self,
        remote_manifest: &Manifest,
        state: &mut SyncState,
        outcome: &mut SyncOutcome,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<(), SyncError> {
        // Tombstones first: deleted papers move to local trash (grace
        // period), never silently destroyed.
        for (dir, deleted_at) in &remote_manifest.tombstones {
            let local = self.library_root.join(dir);
            if local.is_dir() && !state.pending_tombstones.contains_key(dir) {
                let trash = self
                    .library_root
                    .join(".trash")
                    .join(format!("{dir}-{}", deleted_at.replace(':', "-")));
                std::fs::create_dir_all(trash.parent().expect("trash"))?;
                std::fs::rename(&local, &trash)?;
                on_progress(&format!("moved deleted paper to trash: {dir}"));
                outcome.trashed_papers.push(dir.clone());
            }
        }

        let local_entries = manifest::build_entries(self.library_root)?;
        for (path, remote_entry) in &remote_manifest.entries {
            if remote_manifest
                .tombstones
                .keys()
                .any(|dir| path.starts_with(&format!("{dir}/")))
            {
                continue;
            }
            let local = local_entries.get(path);
            if local.map(|l| l.hash == remote_entry.hash).unwrap_or(false) {
                continue; // identical
            }
            let local_path = self.library_root.join(path);
            let fetch = || -> Result<Vec<u8>, SyncError> {
                let blob = self
                    .remote
                    .get(&self.blob_key(path, &remote_entry.hash))?
                    .ok_or_else(|| RemoteError::Backend(format!("blob missing for {path}")))?;
                crypto::decrypt(&self.key, &blob).map_err(|_| SyncError::WrongPassphrase)
            };
            match (local, remote_entry.layer) {
                // New file locally absent → write it.
                (None, _) => {
                    let content = fetch()?;
                    if let Some(parent) = local_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&local_path, content)?;
                    outcome.pulled_files += 1;
                    // The pulled version is on the remote by definition.
                    state
                        .uploaded
                        .insert(format!("{path}@{}", remote_entry.hash));
                }
                // Divergent journal → union-merge (order-independent).
                (Some(_), Layer::User) if merge::is_journal(path) => {
                    let remote_content = String::from_utf8_lossy(&fetch()?).to_string();
                    let local_content = std::fs::read_to_string(&local_path)?;
                    let merged = merge::merge_journals(&local_content, &remote_content);
                    std::fs::write(&local_path, merged)?;
                    outcome.merged_journals += 1;
                    on_progress(&format!("merged journal {path}"));
                }
                // Divergent user document → LWW + conflict copy.
                (Some(_), Layer::User) => {
                    let incoming = fetch()?;
                    let local_mtime = std::fs::metadata(&local_path)
                        .and_then(|m| m.modified())
                        .ok();
                    let incoming_newer = match local_mtime {
                        Some(mtime) => {
                            let local_at: time::OffsetDateTime = mtime.into();
                            let local_str = local_at
                                .format(&time::format_description::well_known::Rfc3339)
                                .unwrap_or_default();
                            remote_manifest.written_at > local_str
                        }
                        None => true,
                    };
                    if let Some(conflict) = merge::lww_with_conflict(
                        &local_path,
                        &incoming,
                        incoming_newer,
                        &remote_manifest.device_id,
                    )? {
                        outcome
                            .conflict_copies
                            .push(conflict.to_string_lossy().to_string());
                        on_progress(&format!("conflict copy created for {path}"));
                    }
                    outcome.pulled_files += 1;
                }
                // Source is immutable: a hash mismatch means different
                // papers under one name — keep local, never overwrite.
                (Some(_), Layer::Source) => {}
                // Derived diverging: local regenerates anyway; keep local.
                (Some(_), Layer::Derived) => {}
            }
        }
        Ok(())
    }

    /// Explicit remote GC: delete blobs unreferenced by the latest manifest
    /// and manifests older than the last three. Never runs implicitly.
    pub fn clean_remote(&self) -> Result<usize, SyncError> {
        let Some(latest) = self.fetch_remote_manifest()? else {
            return Ok(0);
        };
        let referenced: BTreeSet<String> = latest
            .entries
            .iter()
            .map(|(path, e)| self.blob_key(path, &e.hash))
            .collect();
        let mut manifests = self.remote.list("manifest-")?;
        manifests.sort();
        let keep_manifests: BTreeSet<String> = manifests.iter().rev().take(3).cloned().collect();
        let mut removed = 0;
        for key in self.remote.list("")? {
            if key == "key.salt" {
                continue;
            }
            let is_manifest = key.starts_with("manifest-");
            let keep = if is_manifest {
                keep_manifests.contains(&key)
            } else {
                referenced.contains(&key)
            };
            if !keep {
                self.remote.delete(&key)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// Remote initialization / join: publish the salt on first use, derive the
/// key from the passphrase + remote salt. Wrong passphrase surfaces on the
/// first manifest read, cleanly, with no partial local state.
pub fn derive_remote_key(remote: &dyn Remote, passphrase: &str) -> Result<LibraryKey, SyncError> {
    let salt = match remote.get("key.salt")? {
        Some(salt) => salt,
        None => {
            let salt = crypto::random_salt().to_vec();
            // Create-only: two first-time devices converge on one salt.
            match remote.put_if_absent("key.salt", &salt) {
                Ok(()) => salt,
                Err(RemoteError::Conflict) => remote
                    .get("key.salt")?
                    .ok_or_else(|| RemoteError::Backend("salt vanished".into()))?,
                Err(e) => return Err(e.into()),
            }
        }
    };
    crypto::derive_key(passphrase, &salt).map_err(|_| SyncError::WrongPassphrase)
}
