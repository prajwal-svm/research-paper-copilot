//! Collaborative workspace data models (v4, section 6): the journals that
//! `add-cloud-sync` will merge. Local-only until sync lands — no sharing UI,
//! just sync-ready shapes and the privacy boundary, both tested.
//!
//! Shapes: append-only, UUID-keyed, author-attributed. Folding SORTS events
//! by (at, content-hash) first, so two devices' interleaved appends fold to
//! the identical state regardless of the order sync unioned them — the
//! merge-determinism collaboration depends on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;

pub const WORKSPACES_DIR: &str = "workspaces";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    /// "reading_group" | "lab"
    pub mode: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum MemberEvent {
    Join {
        member_id: Uuid,
        name: String,
        /// "instructor" | "member"
        role: String,
        at: String,
    },
    Leave {
        member_id: Uuid,
        at: String,
    },
}

/// One message in an object-anchored discussion thread (chat-journal shape
/// plus authorship — authorship is always visible in collaboration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMessage {
    pub id: Uuid,
    pub author_id: Uuid,
    pub author_name: String,
    pub content: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub id: Uuid,
    pub paper_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiz_node: Option<Uuid>,
    pub assigned_by: Uuid,
    pub at: String,
}

/// Opt-in progress sharing: the record itself states exactly what is
/// shared, so consent is auditable in the data, not just the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum ProgressEvent {
    OptIn {
        member_id: Uuid,
        /// Exactly what this member agreed to share — a closed description,
        /// e.g. "assignment_completion,quiz_outcomes". Never raw learner
        /// memory (that set is unshareable by construction, see below).
        shares: String,
        at: String,
    },
    OptOut {
        member_id: Uuid,
        at: String,
    },
    Completion {
        member_id: Uuid,
        assignment_id: Uuid,
        /// "completed" | "in_progress"
        status: String,
        /// Shared quiz outcome (SM-2 quality) — only present under opt-in.
        #[serde(skip_serializing_if = "Option::is_none")]
        quiz_quality: Option<u8>,
        at: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum CollabError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("workspace: {0}")]
    Io(#[from] std::io::Error),
}

fn dir(library_root: &Path, id: Uuid) -> PathBuf {
    library_root.join(WORKSPACES_DIR).join(id.to_string())
}

pub fn create_workspace(
    library_root: &Path,
    name: &str,
    mode: &str,
) -> Result<Workspace, CollabError> {
    let workspace = Workspace {
        id: Uuid::new_v4(),
        name: name.to_string(),
        mode: mode.to_string(),
        created_at: crate::bundle::now_rfc3339(),
    };
    let d = dir(library_root, workspace.id);
    std::fs::create_dir_all(&d)?;
    std::fs::write(
        d.join("workspace.json"),
        serde_json::to_vec_pretty(&workspace).expect("serializable"),
    )?;
    Ok(workspace)
}

fn journal(library_root: &Path, workspace: Uuid, file: &str) -> crate::bundle::Journal {
    crate::bundle::Journal::at(dir(library_root, workspace).join(file))
}

pub fn append_member_event(
    library_root: &Path,
    workspace: Uuid,
    event: &MemberEvent,
) -> Result<(), CollabError> {
    journal(library_root, workspace, "membership.jsonl").append(event)?;
    Ok(())
}

pub fn append_thread_message(
    library_root: &Path,
    workspace: Uuid,
    anchor: Uuid,
    message: &ThreadMessage,
) -> Result<(), CollabError> {
    journal(library_root, workspace, &format!("threads/{anchor}.jsonl")).append(message)?;
    Ok(())
}

pub fn append_assignment(
    library_root: &Path,
    workspace: Uuid,
    assignment: &Assignment,
) -> Result<(), CollabError> {
    journal(library_root, workspace, "assignments.jsonl").append(assignment)?;
    Ok(())
}

pub fn append_progress(
    library_root: &Path,
    workspace: Uuid,
    event: &ProgressEvent,
) -> Result<(), CollabError> {
    journal(library_root, workspace, "progress.jsonl").append(event)?;
    Ok(())
}

/// Deterministic fold order: (timestamp, serialized-entry). Sync merges by
/// union, so folds must be independent of on-disk interleaving.
fn sorted_events<T: Serialize + DeserializeOwned>(
    library_root: &Path,
    workspace: Uuid,
    file: &str,
    at_of: impl Fn(&T) -> String,
) -> Result<Vec<T>, CollabError> {
    let mut events: Vec<T> = journal(library_root, workspace, file).read_all()?;
    events.sort_by_key(|e| (at_of(e), serde_json::to_string(e).unwrap_or_default()));
    Ok(events)
}

/// Current members: (member_id, name, role), joins/leaves folded in
/// deterministic order.
pub fn members(
    library_root: &Path,
    workspace: Uuid,
) -> Result<Vec<(Uuid, String, String)>, CollabError> {
    let events =
        sorted_events::<MemberEvent>(library_root, workspace, "membership.jsonl", |e| match e {
            MemberEvent::Join { at, .. } | MemberEvent::Leave { at, .. } => at.clone(),
        })?;
    let mut live: BTreeMap<Uuid, (String, String)> = BTreeMap::new();
    for event in events {
        match event {
            MemberEvent::Join {
                member_id,
                name,
                role,
                ..
            } => {
                live.insert(member_id, (name, role));
            }
            MemberEvent::Leave { member_id, .. } => {
                live.remove(&member_id);
            }
        }
    }
    Ok(live.into_iter().map(|(id, (n, r))| (id, n, r)).collect())
}

/// Thread for one anchor, deterministically ordered.
pub fn thread(
    library_root: &Path,
    workspace: Uuid,
    anchor: Uuid,
) -> Result<Vec<ThreadMessage>, CollabError> {
    sorted_events::<ThreadMessage>(
        library_root,
        workspace,
        &format!("threads/{anchor}.jsonl"),
        |m| m.at.clone(),
    )
}

/// The paths within a library that a workspace MAY share, by construction.
/// Learner memory is excluded here — not by filtering it out, but by never
/// being a candidate: the shareable set is an explicit allowlist.
pub fn workspace_shareable_dirs() -> &'static [&'static str] {
    &[
        WORKSPACES_DIR,
        // Shared paper content classes (papers themselves join per-share):
        "notes",
        "bookmarks",
        "chats",
        // NEVER: learning_state (mastery/preferences/episodes), telemetry.
    ]
}

/// True when a path is eligible for workspace sharing. `learning_state/`
/// and telemetry can never pass — the privacy boundary is an allowlist.
pub fn is_workspace_shareable(relative_path: &Path) -> bool {
    let Some(first) = relative_path.components().next() else {
        return false;
    };
    let first = first.as_os_str().to_string_lossy();
    workspace_shareable_dirs().contains(&first.as_ref())
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_appends_fold_deterministically() {
        // Two "devices" append the same logical events in different orders
        // (what a sync union produces); folds must agree.
        let anchor = Uuid::new_v4();
        let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
        let message = |author: Uuid, name: &str, content: &str, at: &str| ThreadMessage {
            id: Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!("{author}{content}").as_bytes(),
            ),
            author_id: author,
            author_name: name.to_string(),
            content: content.to_string(),
            at: at.to_string(),
        };
        let a1 = message(alice, "Alice", "why softmax here?", "2026-07-01T10:00:00Z");
        let b1 = message(bob, "Bob", "scaling, see eq 1", "2026-07-01T10:05:00Z");
        let a2 = message(alice, "Alice", "got it", "2026-07-01T10:06:00Z");

        let order_one = tempfile::tempdir().unwrap();
        let order_two = tempfile::tempdir().unwrap();
        let ws1 = create_workspace(order_one.path(), "G", "reading_group").unwrap();
        let ws2 = create_workspace(order_two.path(), "G", "reading_group").unwrap();
        for m in [&a1, &b1, &a2] {
            append_thread_message(order_one.path(), ws1.id, anchor, m).unwrap();
        }
        for m in [&a2, &a1, &b1] {
            append_thread_message(order_two.path(), ws2.id, anchor, m).unwrap();
        }
        let t1: Vec<String> = thread(order_one.path(), ws1.id, anchor)
            .unwrap()
            .iter()
            .map(|m| m.content.clone())
            .collect();
        let t2: Vec<String> = thread(order_two.path(), ws2.id, anchor)
            .unwrap()
            .iter()
            .map(|m| m.content.clone())
            .collect();
        assert_eq!(t1, t2, "fold independent of on-disk interleaving");
        assert_eq!(t1, ["why softmax here?", "scaling, see eq 1", "got it"]);
    }

    #[test]
    fn membership_folds_with_roles_and_leaves() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = create_workspace(tmp.path(), "Lab", "lab").unwrap();
        let (instructor, student) = (Uuid::new_v4(), Uuid::new_v4());
        append_member_event(
            tmp.path(),
            ws.id,
            &MemberEvent::Join {
                member_id: instructor,
                name: "Prof".into(),
                role: "instructor".into(),
                at: "2026-07-01T09:00:00Z".into(),
            },
        )
        .unwrap();
        append_member_event(
            tmp.path(),
            ws.id,
            &MemberEvent::Join {
                member_id: student,
                name: "Stu".into(),
                role: "member".into(),
                at: "2026-07-01T09:01:00Z".into(),
            },
        )
        .unwrap();
        append_member_event(
            tmp.path(),
            ws.id,
            &MemberEvent::Leave {
                member_id: student,
                at: "2026-07-02T09:00:00Z".into(),
            },
        )
        .unwrap();
        let members = members(tmp.path(), ws.id).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].2, "instructor");
    }

    #[test]
    fn learner_memory_is_unshareable_by_construction() {
        use std::path::PathBuf;
        // The shareable set is an allowlist; learner memory isn't on it.
        assert!(!is_workspace_shareable(&PathBuf::from(
            "learning_state/mastery.jsonl"
        )));
        assert!(!is_workspace_shareable(&PathBuf::from(
            "learning_state/episodes.jsonl"
        )));
        assert!(!is_workspace_shareable(&PathBuf::from(
            "telemetry/events.jsonl"
        )));
        assert!(is_workspace_shareable(&PathBuf::from("notes/notes.jsonl")));
        assert!(is_workspace_shareable(&PathBuf::from(
            "workspaces/x/threads/y.jsonl"
        )));
        // And the opt-in record states exactly what is shared.
        let opt_in = ProgressEvent::OptIn {
            member_id: Uuid::new_v4(),
            shares: "assignment_completion,quiz_outcomes".into(),
            at: crate::bundle::now_rfc3339(),
        };
        let json = serde_json::to_string(&opt_in).unwrap();
        assert!(json.contains("assignment_completion,quiz_outcomes"));
        assert!(!json.to_lowercase().contains("mastery"));
    }
}

// ---------------------------------------------------------------------------
// Section 7: collaboration features over sync (activated by cloud-sync)
// ---------------------------------------------------------------------------

/// Presence is sync-cadence, not realtime: members heartbeat into a journal
/// on activity; "present" = heartbeat within this window at fold time.
pub const PRESENCE_WINDOW_MINUTES: i64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceEvent {
    pub member_id: Uuid,
    pub name: String,
    pub at: String,
}

pub fn record_presence(
    library_root: &Path,
    workspace: Uuid,
    member_id: Uuid,
    name: &str,
) -> Result<(), CollabError> {
    journal(library_root, workspace, "presence.jsonl").append(&PresenceEvent {
        member_id,
        name: name.to_string(),
        at: crate::bundle::now_rfc3339(),
    })?;
    Ok(())
}

/// Members whose latest heartbeat falls within the presence window of `now`
/// (RFC3339). Fold is deterministic (latest per member, then name order).
pub fn active_members(
    library_root: &Path,
    workspace: Uuid,
    now: &str,
) -> Result<Vec<(Uuid, String)>, CollabError> {
    use time::format_description::well_known::Rfc3339;
    let Ok(now) = time::OffsetDateTime::parse(now, &Rfc3339) else {
        return Ok(Vec::new());
    };
    let events = sorted_events::<PresenceEvent>(library_root, workspace, "presence.jsonl", |e| {
        e.at.clone()
    })?;
    let mut latest: BTreeMap<Uuid, (String, time::OffsetDateTime)> = BTreeMap::new();
    for event in events {
        if let Ok(at) = time::OffsetDateTime::parse(&event.at, &Rfc3339) {
            latest.insert(event.member_id, (event.name, at));
        }
    }
    let mut out: Vec<(Uuid, String)> = latest
        .into_iter()
        .filter(|(_, (_, at))| (now - *at).whole_minutes() < PRESENCE_WINDOW_MINUTES && *at <= now)
        .map(|(id, (name, _))| (id, name))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

/// Copy a paper bundle into the workspace's shared `papers/` mirror,
/// skipping personal and machine-local files: `reading_state.json`
/// (personal position), sync-excluded artifacts (embeddings.bin, graph.db,
/// repos/, telemetry/, sync_state/, .trash/), and torn/conflict files.
/// Learner memory is library-level and never inside a bundle — the privacy
/// boundary holds by construction.
pub fn share_paper(
    library_root: &Path,
    workspace: Uuid,
    bundle_dir: &str,
) -> Result<PathBuf, CollabError> {
    const SKIP_FILES: [&str; 3] = ["embeddings.bin", "graph.db", "reading_state.json"];
    const SKIP_DIRS: [&str; 4] = ["repos", "sync_state", "telemetry", ".trash"];

    fn copy_filtered(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if path.is_dir() {
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                copy_filtered(&path, &dst.join(&name))?;
            } else {
                if SKIP_FILES.contains(&name.as_str())
                    || name.ends_with(".tmp")
                    || name.ends_with(".conflict")
                {
                    continue;
                }
                std::fs::copy(&path, dst.join(&name))?;
            }
        }
        Ok(())
    }

    let src = library_root.join(bundle_dir);
    let dst = dir(library_root, workspace).join("papers").join(bundle_dir);
    copy_filtered(&src, &dst)?;
    Ok(dst)
}

/// Sync the workspace with its shared remote: the workspace directory IS a
/// small library, so the existing engine (union-merge, E2E encryption,
/// tombstones, conflict copies) applies unchanged.
pub fn sync_workspace(
    library_root: &Path,
    workspace: Uuid,
    device_id: &str,
    key: crate::sync::crypto::LibraryKey,
    remote: &dyn crate::sync::remote::Remote,
) -> Result<crate::sync::engine::SyncOutcome, crate::sync::engine::SyncError> {
    let root = dir(library_root, workspace);
    std::fs::create_dir_all(&root).ok();
    let engine = crate::sync::engine::SyncEngine {
        library_root: &root,
        device_id: device_id.to_string(),
        key,
        remote,
    };
    engine.sync(&mut |_| {})
}

/// One member's row in the cohort view (reading-group mode).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CohortRow {
    pub member_id: Uuid,
    /// assignment id → ("completed"|"in_progress", shared quiz quality).
    pub completions: BTreeMap<Uuid, (String, Option<u8>)>,
}

/// Cohort progress honoring opt-in: ONLY members whose latest opt event is
/// OptIn appear, and only their completion/quiz-outcome records — never
/// mastery, episodes, or chat. Deterministic fold.
pub fn cohort_progress(
    library_root: &Path,
    workspace: Uuid,
) -> Result<Vec<CohortRow>, CollabError> {
    let events =
        sorted_events::<ProgressEvent>(library_root, workspace, "progress.jsonl", |e| match e {
            ProgressEvent::OptIn { at, .. }
            | ProgressEvent::OptOut { at, .. }
            | ProgressEvent::Completion { at, .. } => at.clone(),
        })?;
    let mut opted: BTreeMap<Uuid, bool> = BTreeMap::new();
    let mut rows: BTreeMap<Uuid, CohortRow> = BTreeMap::new();
    for event in &events {
        match event {
            ProgressEvent::OptIn { member_id, .. } => {
                opted.insert(*member_id, true);
            }
            ProgressEvent::OptOut { member_id, .. } => {
                opted.insert(*member_id, false);
            }
            ProgressEvent::Completion {
                member_id,
                assignment_id,
                status,
                quiz_quality,
                ..
            } => {
                rows.entry(*member_id)
                    .or_insert_with(|| CohortRow {
                        member_id: *member_id,
                        completions: BTreeMap::new(),
                    })
                    .completions
                    .insert(*assignment_id, (status.clone(), *quiz_quality));
            }
        }
    }
    Ok(rows
        .into_values()
        .filter(|row| opted.get(&row.member_id).copied().unwrap_or(false))
        .collect())
}

/// All assignments, deterministically ordered.
pub fn assignments(library_root: &Path, workspace: Uuid) -> Result<Vec<Assignment>, CollabError> {
    sorted_events::<Assignment>(library_root, workspace, "assignments.jsonl", |a| {
        a.at.clone()
    })
}

/// All workspaces under a library root.
pub fn list_workspaces(library_root: &Path) -> Vec<Workspace> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(library_root.join(WORKSPACES_DIR)) else {
        return out;
    };
    for entry in entries.flatten() {
        if let Ok(bytes) = std::fs::read(entry.path().join("workspace.json")) {
            if let Ok(ws) = serde_json::from_slice::<Workspace>(&bytes) {
                out.push(ws);
            }
        }
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    out
}
