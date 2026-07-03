//! Community contributions (v5): PR-style change sets over a paper's
//! knowledge object.
//!
//! A proposal packages what changed since a `base_revision`: journal entries
//! to union-merge (the format's native diff) and content-addressed file adds.
//! Proposals are created offline and queued under `contributions/proposals/`;
//! the provenance journal (`contributions/provenance.jsonl`) is the
//! append-only history every review/merge/revert event lands in, and
//! revisions are the hash chain over that journal.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::{now_rfc3339, sha256_bytes, Bundle, BundleError};

type Result<T> = std::result::Result<T, BundleError>;

pub const PROVENANCE_JOURNAL: &str = "contributions/provenance.jsonl";
/// Revision id of a bundle with no provenance history.
pub const GENESIS_REVISION: &str = "genesis";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Author {
    /// Registry identity id (or a local placeholder before first sign-in).
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// One changed path inside a proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProposalChange {
    /// Bundle-relative path the change applies to.
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ChangeKind {
    /// Entries to union into an append-only journal (the native diff).
    JournalAppend { entries: Vec<serde_json::Value> },
    /// A whole-file add/replace, content stored at
    /// `contributions/proposals/<id>/files/<digest>`.
    FileAdd { digest: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    /// Created locally; waiting for connectivity / explicit submission.
    Queued,
    Submitted,
    Merged,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Proposal {
    pub id: String,
    /// Revision of the shared object this change set was computed against.
    pub base_revision: String,
    pub author: Author,
    pub summary: String,
    pub created_at: String,
    pub status: ProposalStatus,
    pub changes: Vec<ProposalChange>,
}

impl Proposal {
    fn dir(bundle: &Bundle, id: &str) -> std::path::PathBuf {
        bundle.root().join("contributions/proposals").join(id)
    }
}

/// Current revision of the shared knowledge object: a hash chain over the
/// provenance journal (`sha256(prev_revision + line)` per event), so any two
/// replicas that have merged the same events agree on the revision.
pub fn current_revision(bundle: &Bundle) -> Result<String> {
    let path = bundle.root().join(PROVENANCE_JOURNAL);
    if !path.exists() {
        return Ok(GENESIS_REVISION.to_string());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| BundleError::Io {
        path: path.clone(),
        source: e,
    })?;
    let mut revision = GENESIS_REVISION.to_string();
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        revision = sha256_bytes(format!("{revision}\n{line}").as_bytes());
    }
    Ok(revision)
}

/// Create a proposal offline: content-address any file payloads, write
/// `proposal.json`, status `Queued`. Nothing touches the shared object until
/// merge.
pub fn create_proposal(
    bundle: &Bundle,
    author: Author,
    summary: &str,
    journal_changes: Vec<(String, Vec<serde_json::Value>)>,
    file_adds: Vec<(String, Vec<u8>)>,
) -> Result<Proposal> {
    let id = Uuid::new_v4().to_string();
    let dir = Proposal::dir(bundle, &id);
    let files_dir = dir.join("files");
    std::fs::create_dir_all(&files_dir).map_err(|e| BundleError::Io {
        path: files_dir.clone(),
        source: e,
    })?;

    let mut changes: Vec<ProposalChange> = journal_changes
        .into_iter()
        .map(|(path, entries)| ProposalChange {
            path,
            kind: ChangeKind::JournalAppend { entries },
        })
        .collect();
    for (path, bytes) in file_adds {
        let digest = sha256_bytes(&bytes);
        let blob = files_dir.join(digest.replace(':', "_"));
        std::fs::write(&blob, &bytes).map_err(|e| BundleError::Io {
            path: blob.clone(),
            source: e,
        })?;
        changes.push(ProposalChange {
            path,
            kind: ChangeKind::FileAdd { digest },
        });
    }

    let proposal = Proposal {
        id: id.clone(),
        base_revision: current_revision(bundle)?,
        author,
        summary: summary.to_string(),
        created_at: now_rfc3339(),
        status: ProposalStatus::Queued,
        changes,
    };
    write_proposal(bundle, &proposal)?;
    Ok(proposal)
}

pub fn write_proposal(bundle: &Bundle, proposal: &Proposal) -> Result<()> {
    let path = Proposal::dir(bundle, &proposal.id).join("proposal.json");
    let json = serde_json::to_vec_pretty(proposal).map_err(|e| BundleError::Json {
        path: path.clone(),
        source: e,
    })?;
    std::fs::write(&path, json).map_err(|e| BundleError::Io { path, source: e })
}

/// Read a proposal's stored file payload by digest.
pub fn read_proposal_blob(bundle: &Bundle, proposal_id: &str, digest: &str) -> Result<Vec<u8>> {
    let path = Proposal::dir(bundle, proposal_id)
        .join("files")
        .join(digest.replace(':', "_"));
    std::fs::read(&path).map_err(|e| BundleError::Io { path, source: e })
}

pub fn list_proposals(bundle: &Bundle) -> Result<Vec<Proposal>> {
    let dir = bundle.root().join("contributions/proposals");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path().join("proposal.json");
        if !path.exists() {
            continue;
        }
        let raw = std::fs::read(&path).map_err(|e| BundleError::Io {
            path: path.clone(),
            source: e,
        })?;
        out.push(serde_json::from_slice(&raw).map_err(|e| BundleError::Json { path, source: e })?);
    }
    out.sort_by(|a: &Proposal, b: &Proposal| a.created_at.cmp(&b.created_at));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    fn bundle() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        let b = Bundle::create(&root, b"%PDF-1.5 x", Paper::new("T"), "file").unwrap();
        (tmp, b)
    }

    fn author() -> Author {
        Author {
            id: "alice@registry".into(),
            display_name: Some("Alice".into()),
        }
    }

    #[test]
    fn proposal_roundtrip_offline_queue() {
        let (_tmp, b) = bundle();
        let created = create_proposal(
            &b,
            author(),
            "better explanation for attention",
            vec![(
                "notes/notes.jsonl".into(),
                vec![serde_json::json!({"text": "improved", "at": "2026-07-02T00:00:00Z"})],
            )],
            vec![("figures/anim.json".into(), b"{\"frames\":[]}".to_vec())],
        )
        .unwrap();

        assert_eq!(created.status, ProposalStatus::Queued);
        assert_eq!(created.base_revision, GENESIS_REVISION);

        let listed = list_proposals(&b).unwrap();
        assert_eq!(listed, vec![created.clone()]);

        // File payload is content-addressed and readable back.
        let digest = match &created.changes[1].kind {
            ChangeKind::FileAdd { digest } => digest.clone(),
            other => panic!("expected file add, got {other:?}"),
        };
        assert_eq!(digest, sha256_bytes(b"{\"frames\":[]}"));
        assert_eq!(
            read_proposal_blob(&b, &created.id, &digest).unwrap(),
            b"{\"frames\":[]}"
        );
    }

    #[test]
    fn revision_advances_with_provenance_events() {
        let (_tmp, b) = bundle();
        assert_eq!(current_revision(&b).unwrap(), GENESIS_REVISION);

        b.journal(PROVENANCE_JOURNAL)
            .append(&serde_json::json!({"event": "merge", "proposal": "x"}))
            .unwrap();
        let r1 = current_revision(&b).unwrap();
        assert_ne!(r1, GENESIS_REVISION);

        b.journal(PROVENANCE_JOURNAL)
            .append(&serde_json::json!({"event": "merge", "proposal": "y"}))
            .unwrap();
        let r2 = current_revision(&b).unwrap();
        assert_ne!(r2, r1, "revision is a chain, not a set");
    }
}

// ---------------------------------------------------------------------------
// Provenance events (task 2.2): append-only, ed25519-signed
// ---------------------------------------------------------------------------

/// A registry identity key with a validity window. Verification treats
/// signatures as valid-at-time: an event signed while the key was current
/// stays valid after rotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct KeyWindow {
    /// Hex-encoded ed25519 public key.
    pub public_key: String,
    pub valid_from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind {
    Propose {
        proposal_id: String,
    },
    Review {
        proposal_id: String,
        accepted: bool,
        reason: Option<String>,
    },
    Merge {
        proposal_id: String,
    },
    /// Reverts a previous merge; `merged_revision` names the revision the
    /// offending merge produced. History stays intact — this is a new event.
    Revert {
        proposal_id: String,
        merged_revision: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProvenanceEvent {
    pub at: String,
    pub actor: Author,
    #[serde(flatten)]
    pub kind: EventKind,
    /// Hex ed25519 signature over the canonical unsigned event JSON.
    pub signature: String,
    /// Hex public key that produced the signature.
    pub public_key: String,
}

fn canonical_unsigned(at: &str, actor: &Author, kind: &EventKind) -> Vec<u8> {
    // Canonical form: the serialized event without signature fields. Field
    // order is fixed by construction (serde struct order), so this is stable.
    #[derive(Serialize)]
    struct Unsigned<'a> {
        at: &'a str,
        actor: &'a Author,
        #[serde(flatten)]
        kind: &'a EventKind,
    }
    serde_json::to_vec(&Unsigned { at, actor, kind }).expect("event serializes")
}

/// Sign and append a provenance event. Returns the appended event.
pub fn append_event(
    bundle: &Bundle,
    signing_key: &ed25519_dalek::SigningKey,
    actor: Author,
    kind: EventKind,
) -> Result<ProvenanceEvent> {
    use ed25519_dalek::Signer;
    let at = now_rfc3339();
    let message = canonical_unsigned(&at, &actor, &kind);
    let signature = signing_key.sign(&message);
    let event = ProvenanceEvent {
        at,
        actor,
        kind,
        signature: hex(&signature.to_bytes()),
        public_key: hex(signing_key.verifying_key().as_bytes()),
    };
    bundle.journal(PROVENANCE_JOURNAL).append(&event)?;
    Ok(event)
}

pub fn read_events(bundle: &Bundle) -> Result<Vec<ProvenanceEvent>> {
    bundle.journal(PROVENANCE_JOURNAL).read_all()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventVerdict {
    /// Signature valid and the key was current at the event time.
    Valid,
    /// Signature valid but the key window doesn't cover the event time.
    KeyOutsideWindow,
    /// Signature invalid or key not in the actor's profile.
    Invalid,
}

/// Verify the provenance log against actor key profiles
/// (actor id → key windows). Valid-at-time: old keys stay good for events
/// signed inside their window.
pub fn verify_events(
    events: &[ProvenanceEvent],
    profiles: &std::collections::BTreeMap<String, Vec<KeyWindow>>,
) -> Vec<EventVerdict> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    events
        .iter()
        .map(|event| {
            let Some(windows) = profiles.get(&event.actor.id) else {
                return EventVerdict::Invalid;
            };
            let Some(window) = windows.iter().find(|w| w.public_key == event.public_key) else {
                return EventVerdict::Invalid;
            };
            let (Some(key_bytes), Some(sig_bytes)) =
                (unhex(&event.public_key), unhex(&event.signature))
            else {
                return EventVerdict::Invalid;
            };
            let Ok(key) = VerifyingKey::from_bytes(&match key_bytes.try_into() {
                Ok(array) => array,
                Err(_) => return EventVerdict::Invalid,
            }) else {
                return EventVerdict::Invalid;
            };
            let Ok(signature) = Signature::from_slice(&sig_bytes) else {
                return EventVerdict::Invalid;
            };
            let message = canonical_unsigned(&event.at, &event.actor, &event.kind);
            if key.verify(&message, &signature).is_err() {
                return EventVerdict::Invalid;
            }
            let in_window = event.at.as_str() >= window.valid_from.as_str()
                && window
                    .valid_until
                    .as_deref()
                    .map(|until| event.at.as_str() <= until)
                    .unwrap_or(true);
            if in_window {
                EventVerdict::Valid
            } else {
                EventVerdict::KeyOutsideWindow
            }
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod provenance_tests {
    use super::*;
    use crate::bundle::Paper;
    use std::collections::BTreeMap;

    fn bundle() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        let b = Bundle::create(&root, b"%PDF-1.5 x", Paper::new("T"), "file").unwrap();
        (tmp, b)
    }

    fn key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[7u8; 32])
    }

    fn alice() -> Author {
        Author {
            id: "alice".into(),
            display_name: None,
        }
    }

    #[test]
    fn signed_events_verify_and_tamper_fails() {
        let (_tmp, b) = bundle();
        let signing = key();
        append_event(
            &b,
            &signing,
            alice(),
            EventKind::Propose {
                proposal_id: "p1".into(),
            },
        )
        .unwrap();

        let mut events = read_events(&b).unwrap();
        let profiles: BTreeMap<String, Vec<KeyWindow>> = BTreeMap::from([(
            "alice".to_string(),
            vec![KeyWindow {
                public_key: hex(signing.verifying_key().as_bytes()),
                valid_from: "2000-01-01T00:00:00Z".into(),
                valid_until: None,
            }],
        )]);
        assert_eq!(verify_events(&events, &profiles), vec![EventVerdict::Valid]);

        // Tamper with the summary of the event → Invalid.
        events[0].at = "1999-01-01T00:00:00Z".into();
        assert_eq!(
            verify_events(&events, &profiles),
            vec![EventVerdict::Invalid]
        );
    }

    #[test]
    fn old_key_valid_at_time_after_rotation() {
        let (_tmp, b) = bundle();
        let old_key = key();
        append_event(
            &b,
            &old_key,
            alice(),
            EventKind::Merge {
                proposal_id: "p1".into(),
            },
        )
        .unwrap();
        let events = read_events(&b).unwrap();

        // Key rotated after the event: window still covers the event time.
        let covering = BTreeMap::from([(
            "alice".to_string(),
            vec![KeyWindow {
                public_key: hex(old_key.verifying_key().as_bytes()),
                valid_from: "2000-01-01T00:00:00Z".into(),
                valid_until: Some("2999-01-01T00:00:00Z".into()),
            }],
        )]);
        assert_eq!(verify_events(&events, &covering), vec![EventVerdict::Valid]);

        // Window that ended before the event → flagged, not silently valid.
        let expired = BTreeMap::from([(
            "alice".to_string(),
            vec![KeyWindow {
                public_key: hex(old_key.verifying_key().as_bytes()),
                valid_from: "2000-01-01T00:00:00Z".into(),
                valid_until: Some("2001-01-01T00:00:00Z".into()),
            }],
        )]);
        assert_eq!(
            verify_events(&events, &expired),
            vec![EventVerdict::KeyOutsideWindow]
        );
    }
}

// ---------------------------------------------------------------------------
// Merge / revert (task 2.3)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct MergeOutcome {
    pub merged: bool,
    /// Paths whose whole-file replacement conflicts with existing content;
    /// non-empty ⇒ nothing was written.
    pub conflicts: Vec<String>,
}

/// Merge an accepted proposal into the bundle. Journal changes union in
/// (idempotent, stale-base-safe); file adds land unless the target exists
/// with different content — then the whole merge aborts with the conflicts
/// surfaced for the reviewer. Replaced files are backed up per-proposal so a
/// revert can restore them without rewriting history.
pub fn merge_proposal(
    bundle: &Bundle,
    signing_key: &ed25519_dalek::SigningKey,
    reviewer: Author,
    proposal_id: &str,
) -> Result<MergeOutcome> {
    let mut proposal = list_proposals(bundle)?
        .into_iter()
        .find(|p| p.id == proposal_id)
        .ok_or_else(|| BundleError::NotABundle(Proposal::dir(bundle, proposal_id)))?;

    // Conflict scan first: no partial merges.
    let mut conflicts = Vec::new();
    for change in &proposal.changes {
        if let ChangeKind::FileAdd { digest } = &change.kind {
            let target = bundle.root().join(&change.path);
            if target.exists() {
                let existing = std::fs::read(&target).map_err(|e| BundleError::Io {
                    path: target.clone(),
                    source: e,
                })?;
                if &sha256_bytes(&existing) != digest {
                    conflicts.push(change.path.clone());
                }
            }
        }
    }
    if !conflicts.is_empty() {
        return Ok(MergeOutcome {
            merged: false,
            conflicts,
        });
    }

    let backup_dir = Proposal::dir(bundle, proposal_id).join("backup");
    for change in &proposal.changes {
        let target = bundle.root().join(&change.path);
        match &change.kind {
            ChangeKind::JournalAppend { entries } => {
                let existing = std::fs::read_to_string(&target).unwrap_or_default();
                let mut incoming = String::new();
                for entry in entries {
                    incoming.push_str(&serde_json::to_string(entry).expect("entry serializes"));
                    incoming.push('\n');
                }
                let merged = crate::sync::merge::merge_journals(&existing, &incoming);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(&target, merged).map_err(|e| BundleError::Io {
                    path: target.clone(),
                    source: e,
                })?;
            }
            ChangeKind::FileAdd { digest } => {
                if target.exists() {
                    // Same-content overwrite; still back up for revert symmetry.
                    std::fs::create_dir_all(&backup_dir).ok();
                    std::fs::copy(&target, backup_dir.join(digest.replace(':', "_"))).ok();
                }
                let payload = read_proposal_blob(bundle, proposal_id, digest)?;
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(&target, payload).map_err(|e| BundleError::Io {
                    path: target.clone(),
                    source: e,
                })?;
            }
        }
    }

    append_event(
        bundle,
        signing_key,
        reviewer,
        EventKind::Merge {
            proposal_id: proposal_id.to_string(),
        },
    )?;
    proposal.status = ProposalStatus::Merged;
    write_proposal(bundle, &proposal)?;
    Ok(MergeOutcome {
        merged: true,
        conflicts: Vec::new(),
    })
}

/// Revert a merged proposal: journal entries the proposal added are
/// subtracted (exact-line set difference), file adds are restored from the
/// merge-time backup (or removed if they didn't exist before). History stays
/// intact — a signed Revert event is appended, never rewritten.
pub fn revert_proposal(
    bundle: &Bundle,
    signing_key: &ed25519_dalek::SigningKey,
    actor: Author,
    proposal_id: &str,
) -> Result<()> {
    let merged_revision = current_revision(bundle)?;
    let mut proposal = list_proposals(bundle)?
        .into_iter()
        .find(|p| p.id == proposal_id)
        .ok_or_else(|| BundleError::NotABundle(Proposal::dir(bundle, proposal_id)))?;

    let backup_dir = Proposal::dir(bundle, proposal_id).join("backup");
    for change in &proposal.changes {
        let target = bundle.root().join(&change.path);
        match &change.kind {
            ChangeKind::JournalAppend { entries } => {
                let existing = std::fs::read_to_string(&target).unwrap_or_default();
                let added: std::collections::BTreeSet<String> = entries
                    .iter()
                    .map(|e| serde_json::to_string(e).expect("entry serializes"))
                    .collect();
                let kept: String = existing
                    .lines()
                    .filter(|line| !added.contains(*line))
                    .map(|line| format!("{line}\n"))
                    .collect();
                std::fs::write(&target, kept).map_err(|e| BundleError::Io {
                    path: target.clone(),
                    source: e,
                })?;
            }
            ChangeKind::FileAdd { digest } => {
                let backup = backup_dir.join(digest.replace(':', "_"));
                if backup.exists() {
                    std::fs::copy(&backup, &target).map_err(|e| BundleError::Io {
                        path: target.clone(),
                        source: e,
                    })?;
                } else {
                    std::fs::remove_file(&target).ok();
                }
            }
        }
    }

    append_event(
        bundle,
        signing_key,
        actor,
        EventKind::Revert {
            proposal_id: proposal_id.to_string(),
            merged_revision,
        },
    )?;
    proposal.status = ProposalStatus::Rejected;
    write_proposal(bundle, &proposal)?;
    Ok(())
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use crate::bundle::Paper;

    fn bundle() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        let b = Bundle::create(&root, b"%PDF-1.5 x", Paper::new("T"), "file").unwrap();
        (tmp, b)
    }

    fn key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[9u8; 32])
    }

    fn alice() -> Author {
        Author {
            id: "alice".into(),
            display_name: None,
        }
    }

    #[test]
    fn merge_unions_journals_even_from_stale_base() {
        let (_tmp, b) = bundle();
        // Local activity lands the same journal line the proposal carries,
        // plus one more — union keeps exactly one copy of each.
        let shared = serde_json::json!({"at": "2026-01-01T00:00:00Z", "text": "shared"});
        let local = serde_json::json!({"at": "2026-01-02T00:00:00Z", "text": "local"});
        b.journal("notes/notes.jsonl").append(&shared).unwrap();
        b.journal("notes/notes.jsonl").append(&local).unwrap();

        let proposal = create_proposal(
            &b,
            alice(),
            "stale base",
            vec![(
                "notes/notes.jsonl".into(),
                vec![
                    shared.clone(),
                    serde_json::json!({"at": "2026-01-03T00:00:00Z", "text": "proposed"}),
                ],
            )],
            vec![],
        )
        .unwrap();
        // Provenance advances after the proposal was cut → stale base.
        append_event(
            &b,
            &key(),
            alice(),
            EventKind::Propose {
                proposal_id: proposal.id.clone(),
            },
        )
        .unwrap();

        let outcome = merge_proposal(&b, &key(), alice(), &proposal.id).unwrap();
        assert!(outcome.merged);

        let lines: Vec<serde_json::Value> = b.journal("notes/notes.jsonl").read_all().unwrap();
        assert_eq!(lines.len(), 3, "union dedupes the shared entry: {lines:?}");
    }

    #[test]
    fn conflicting_file_add_aborts_whole_merge() {
        let (_tmp, b) = bundle();
        std::fs::write(b.root().join("figures/anim.json"), b"{\"theirs\":1}").unwrap();
        let proposal = create_proposal(
            &b,
            alice(),
            "conflict",
            vec![(
                "notes/notes.jsonl".into(),
                vec![serde_json::json!({"at":"t","x":1})],
            )],
            vec![("figures/anim.json".into(), b"{\"mine\":2}".to_vec())],
        )
        .unwrap();

        let outcome = merge_proposal(&b, &key(), alice(), &proposal.id).unwrap();
        assert!(!outcome.merged);
        assert_eq!(outcome.conflicts, vec!["figures/anim.json".to_string()]);
        // Nothing was written — not even the journal part.
        let lines: Vec<serde_json::Value> = b.journal("notes/notes.jsonl").read_all().unwrap();
        assert!(lines.is_empty(), "aborted merge must not partially apply");
    }

    #[test]
    fn revert_restores_content_and_keeps_history() {
        let (_tmp, b) = bundle();
        let proposal = create_proposal(
            &b,
            alice(),
            "to revert",
            vec![(
                "notes/notes.jsonl".into(),
                vec![serde_json::json!({"at":"t","x":1})],
            )],
            vec![("figures/anim.json".into(), b"{\"new\":1}".to_vec())],
        )
        .unwrap();
        merge_proposal(&b, &key(), alice(), &proposal.id).unwrap();
        assert!(b.root().join("figures/anim.json").exists());

        revert_proposal(&b, &key(), alice(), &proposal.id).unwrap();

        assert!(
            !b.root().join("figures/anim.json").exists(),
            "file add removed"
        );
        let lines: Vec<serde_json::Value> = b.journal("notes/notes.jsonl").read_all().unwrap();
        assert!(lines.is_empty(), "journal entries subtracted");
        // History intact: merge AND revert events both present.
        let events = read_events(&b).unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Merge { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Revert { .. })));
    }
}

// ---------------------------------------------------------------------------
// Reputation + trust levels + gates (tasks 2.4, 2.5)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Reputation {
    pub accepted: u64,
    pub rejected: u64,
    pub reverted: u64,
    pub reviews_performed: u64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    New,
    Trusted,
    Maintainer,
}

/// Reputation is a pure fold over the provenance log — deterministic and
/// recomputable; there is no stored score to drift from the record.
pub fn reputation(events: &[ProvenanceEvent]) -> std::collections::BTreeMap<String, Reputation> {
    use std::collections::BTreeMap;
    let mut by_actor: BTreeMap<String, Reputation> = BTreeMap::new();
    // proposal id → author id (from Propose events).
    let mut authors: BTreeMap<String, String> = BTreeMap::new();
    for event in events {
        match &event.kind {
            EventKind::Propose { proposal_id } => {
                authors.insert(proposal_id.clone(), event.actor.id.clone());
            }
            EventKind::Review {
                proposal_id,
                accepted,
                ..
            } => {
                by_actor
                    .entry(event.actor.id.clone())
                    .or_default()
                    .reviews_performed += 1;
                if !accepted {
                    if let Some(author) = authors.get(proposal_id) {
                        by_actor.entry(author.clone()).or_default().rejected += 1;
                    }
                }
            }
            EventKind::Merge { proposal_id } => {
                if let Some(author) = authors.get(proposal_id) {
                    by_actor.entry(author.clone()).or_default().accepted += 1;
                }
            }
            EventKind::Revert { proposal_id, .. } => {
                if let Some(author) = authors.get(proposal_id) {
                    by_actor.entry(author.clone()).or_default().reverted += 1;
                }
            }
        }
    }
    by_actor
}

/// Trust is earned from the same fold: thresholds are policy, the inputs are
/// the public record.
pub fn trust_level(rep: &Reputation) -> TrustLevel {
    let net = rep.accepted.saturating_sub(rep.reverted);
    if net >= 20 && rep.reviews_performed >= 10 {
        TrustLevel::Maintainer
    } else if net >= 5 {
        TrustLevel::Trusted
    } else {
        TrustLevel::New
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GateError {
    #[error("proposal contains disallowed content ({path}): {reason}")]
    PolicyViolation { path: String, reason: String },
    #[error("contributor trust level {level:?} requires review before merge")]
    ReviewRequired { level: TrustLevel },
}

/// Enrichment-only policy: publisher content never enters a proposal.
/// Checked at submission — violations never reach reviewers.
pub fn validate_policy(bundle: &Bundle, proposal: &Proposal) -> std::result::Result<(), GateError> {
    const BANNED_PATHS: [&str; 2] = ["original.pdf", "pages"];
    for change in &proposal.changes {
        let first = change.path.split('/').next().unwrap_or("");
        if BANNED_PATHS.contains(&first) {
            return Err(GateError::PolicyViolation {
                path: change.path.clone(),
                reason: "publisher-owned content (source PDF / page images) is never shared".into(),
            });
        }
        if let ChangeKind::FileAdd { digest } = &change.kind {
            if let Ok(payload) = read_proposal_blob(bundle, &proposal.id, digest) {
                if payload.starts_with(b"%PDF") {
                    return Err(GateError::PolicyViolation {
                        path: change.path.clone(),
                        reason: "payload is a PDF (magic bytes)".into(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Moderation gate: direct merge is a maintainer privilege; everyone else
/// goes through review. Policy validation runs first, always.
pub fn can_merge_directly(
    bundle: &Bundle,
    proposal: &Proposal,
) -> std::result::Result<(), GateError> {
    validate_policy(bundle, proposal)?;
    let events = read_events(bundle).unwrap_or_default();
    let reps = reputation(&events);
    let level = reps
        .get(&proposal.author.id)
        .map(trust_level)
        .unwrap_or(TrustLevel::New);
    if level < TrustLevel::Maintainer {
        return Err(GateError::ReviewRequired { level });
    }
    Ok(())
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use crate::bundle::Paper;

    fn bundle() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        let b = Bundle::create(&root, b"%PDF-1.5 x", Paper::new("T"), "file").unwrap();
        (tmp, b)
    }

    fn key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[3u8; 32])
    }

    #[test]
    fn reputation_recompute_is_deterministic() {
        let (_tmp, b) = bundle();
        let alice = Author {
            id: "alice".into(),
            display_name: None,
        };
        let bob = Author {
            id: "bob".into(),
            display_name: None,
        };
        append_event(
            &b,
            &key(),
            alice.clone(),
            EventKind::Propose {
                proposal_id: "p1".into(),
            },
        )
        .unwrap();
        append_event(
            &b,
            &key(),
            bob.clone(),
            EventKind::Review {
                proposal_id: "p1".into(),
                accepted: true,
                reason: None,
            },
        )
        .unwrap();
        append_event(
            &b,
            &key(),
            bob.clone(),
            EventKind::Merge {
                proposal_id: "p1".into(),
            },
        )
        .unwrap();

        let events = read_events(&b).unwrap();
        let first = reputation(&events);
        let second = reputation(&events);
        assert_eq!(first, second);
        assert_eq!(first["alice"].accepted, 1);
        assert_eq!(first["bob"].reviews_performed, 1);
    }

    #[test]
    fn new_contributor_requires_review() {
        let (_tmp, b) = bundle();
        let mallory = Author {
            id: "mallory".into(),
            display_name: None,
        };
        let proposal = create_proposal(&b, mallory, "first ever", vec![], vec![]).unwrap();
        match can_merge_directly(&b, &proposal) {
            Err(GateError::ReviewRequired {
                level: TrustLevel::New,
            }) => {}
            other => panic!("expected review gate, got {other:?}"),
        }
    }

    #[test]
    fn pdf_payload_blocked_before_review() {
        let (_tmp, b) = bundle();
        let alice = Author {
            id: "alice".into(),
            display_name: None,
        };
        let proposal = create_proposal(
            &b,
            alice,
            "smuggled pdf",
            vec![],
            vec![("figures/paper.bin".into(), b"%PDF-1.4 sneaky".to_vec())],
        )
        .unwrap();
        match validate_policy(&b, &proposal) {
            Err(GateError::PolicyViolation { path, .. }) => assert_eq!(path, "figures/paper.bin"),
            other => panic!("expected policy violation, got {other:?}"),
        }
    }

    #[test]
    fn banned_path_blocked() {
        let (_tmp, b) = bundle();
        let alice = Author {
            id: "alice".into(),
            display_name: None,
        };
        let proposal = create_proposal(
            &b,
            alice,
            "replace the pdf",
            vec![],
            vec![("original.pdf".into(), b"not even a pdf".to_vec())],
        )
        .unwrap();
        assert!(matches!(
            validate_policy(&b, &proposal),
            Err(GateError::PolicyViolation { .. })
        ));
    }
}
