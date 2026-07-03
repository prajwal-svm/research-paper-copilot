//! v4 section 7: collaboration features over the real sync engine
//! (MemoryRemote): shared libraries, object-anchored threads with
//! authorship, presence, reading-group cohort progress, lab-mode
//! attributed experiment runs.

use copilot_core::bundle::{now_rfc3339, Bundle, Paper};
use copilot_core::collab::*;
use copilot_core::sync::engine::derive_remote_key;
use copilot_core::sync::remote::MemoryRemote;
use uuid::Uuid;

struct Member {
    _tmp: tempfile::TempDir,
    library: std::path::PathBuf,
    id: Uuid,
    name: &'static str,
    device: String,
}

fn member(name: &'static str) -> Member {
    let tmp = tempfile::tempdir().unwrap();
    Member {
        library: tmp.path().to_path_buf(),
        _tmp: tmp,
        id: Uuid::new_v4(),
        name,
        device: format!("device-{name}"),
    }
}

fn sync(m: &Member, workspace: Uuid, remote: &MemoryRemote, passphrase: &str) {
    let key = derive_remote_key(remote, passphrase).unwrap();
    sync_workspace(&m.library, workspace, &m.device, key, remote).unwrap();
}

#[test]
fn shared_workspace_threads_presence_and_privacy_converge() {
    let remote = MemoryRemote::default();
    let alice = member("alice");
    let bob = member("bob");

    // Alice creates the workspace and seeds membership.
    let ws = create_workspace(&alice.library, "attention reading group", "reading_group").unwrap();
    for (m, role) in [(&alice, "instructor"), (&bob, "member")] {
        append_member_event(
            &alice.library,
            ws.id,
            &MemberEvent::Join {
                member_id: m.id,
                name: m.name.to_string(),
                role: role.to_string(),
                at: now_rfc3339(),
            },
        )
        .unwrap();
    }

    // Alice shares a paper; her personal reading position must not travel.
    let bundle_dir = "attention.research";
    let bundle = Bundle::create(
        &alice.library.join(bundle_dir),
        b"%PDF-1.5 attention",
        Paper::new("Attention Is All You Need"),
        "file",
    )
    .unwrap();
    bundle
        .write_user_json("reading_state.json", &serde_json::json!({"scroll_top": 42}))
        .unwrap();
    let shared = share_paper(&alice.library, ws.id, bundle_dir).unwrap();
    assert!(shared.join("metadata.json").exists());
    assert!(
        shared.join("original.pdf").exists(),
        "group shares the paper"
    );
    assert!(
        !shared.join("reading_state.json").exists(),
        "personal reading position never enters the workspace"
    );

    // Object-anchored thread + presence, then sync up.
    let anchor = Uuid::new_v4();
    append_thread_message(
        &alice.library,
        ws.id,
        anchor,
        &ThreadMessage {
            id: Uuid::new_v4(),
            author_id: alice.id,
            author_name: "alice".into(),
            content: "does eq. 1 assume softmax?".into(),
            at: now_rfc3339(),
        },
    )
    .unwrap();
    record_presence(&alice.library, ws.id, alice.id, "alice").unwrap();
    sync(&alice, ws.id, &remote, "group-pass");

    // Bob joins by syncing the same remote: papers, members, thread arrive.
    sync(&bob, ws.id, &remote, "group-pass");
    let bob_thread = thread(&bob.library, ws.id, anchor).unwrap();
    assert_eq!(bob_thread.len(), 1);
    assert_eq!(
        bob_thread[0].author_name, "alice",
        "authorship always visible"
    );
    assert!(bob
        .library
        .join(format!(
            "workspaces/{}/papers/{bundle_dir}/original.pdf",
            ws.id
        ))
        .exists());
    assert_eq!(members(&bob.library, ws.id).unwrap().len(), 2);

    // Bob replies offline; both sides converge after the next sync round.
    append_thread_message(
        &bob.library,
        ws.id,
        anchor,
        &ThreadMessage {
            id: Uuid::new_v4(),
            author_id: bob.id,
            author_name: "bob".into(),
            content: "yes — see 3.2.1".into(),
            at: now_rfc3339(),
        },
    )
    .unwrap();
    record_presence(&bob.library, ws.id, bob.id, "bob").unwrap();
    sync(&bob, ws.id, &remote, "group-pass");
    sync(&alice, ws.id, &remote, "group-pass");

    let alice_thread = thread(&alice.library, ws.id, anchor).unwrap();
    let bob_thread = thread(&bob.library, ws.id, anchor).unwrap();
    assert_eq!(alice_thread.len(), 2);
    let fold = |t: &[ThreadMessage]| -> Vec<(Uuid, String)> {
        t.iter().map(|m| (m.id, m.content.clone())).collect()
    };
    assert_eq!(fold(&alice_thread), fold(&bob_thread), "identical folds");

    // Presence: both heartbeated recently; sync-cadence presence sees both.
    let now = now_rfc3339();
    let active = active_members(&alice.library, ws.id, &now).unwrap();
    assert_eq!(active.len(), 2, "{active:?}");
    // A heartbeat outside the window drops out.
    let later =
        time::OffsetDateTime::now_utc() + time::Duration::minutes(PRESENCE_WINDOW_MINUTES + 5);
    let later = later
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    assert!(active_members(&alice.library, ws.id, &later)
        .unwrap()
        .is_empty());

    // Privacy boundary: nothing under the workspace resembles learner memory.
    assert!(!bob
        .library
        .join(format!("workspaces/{}/learning_state", ws.id))
        .exists());
}

#[test]
fn cohort_progress_is_opt_in_only() {
    let alice = member("alice");
    let ws = create_workspace(&alice.library, "course", "reading_group").unwrap();
    let assignment = Uuid::new_v4();
    let (optin_member, silent_member) = (Uuid::new_v4(), Uuid::new_v4());

    append_progress(
        &alice.library,
        ws.id,
        &ProgressEvent::OptIn {
            member_id: optin_member,
            shares: "assignment_completion,quiz_outcomes".into(),
            at: now_rfc3339(),
        },
    )
    .unwrap();
    for member_id in [optin_member, silent_member] {
        append_progress(
            &alice.library,
            ws.id,
            &ProgressEvent::Completion {
                member_id,
                assignment_id: assignment,
                status: "completed".into(),
                quiz_quality: Some(4),
                at: now_rfc3339(),
            },
        )
        .unwrap();
    }

    let rows = cohort_progress(&alice.library, ws.id).unwrap();
    assert_eq!(rows.len(), 1, "only the opted-in member appears: {rows:?}");
    assert_eq!(rows[0].member_id, optin_member);
    assert_eq!(
        rows[0].completions[&assignment],
        ("completed".to_string(), Some(4))
    );
}

#[test]
fn lab_mode_shared_run_carries_attribution() {
    use copilot_core::experiments::{create, record_run, runs, ExperimentRun, ParameterSpec};
    let alice = member("alice");
    let ws = create_workspace(&alice.library, "lab", "lab").unwrap();

    // Shared paper bundle inside the workspace is a regular bundle.
    let dir = "paper.research";
    Bundle::create(
        &alice.library.join(dir),
        b"%PDF-1.5 x",
        Paper::new("P"),
        "file",
    )
    .unwrap();
    let shared_dir = share_paper(&alice.library, ws.id, dir).unwrap();
    let shared = Bundle::open(&shared_dir).unwrap();

    let experiment = create(
        &shared,
        "batch sweep",
        Uuid::new_v4(),
        copilot_core::implementations::Language::Python,
        vec![ParameterSpec {
            name: "batch".into(),
            kind: "number".into(),
            default: "32".into(),
        }],
    )
    .unwrap();
    record_run(
        &shared,
        experiment.id,
        &ExperimentRun {
            run_id: Uuid::new_v4(),
            params: Default::default(),
            metrics: std::collections::BTreeMap::from([("loss".to_string(), 0.9)]),
            stdout_tail: String::new(),
            duration_ms: 12,
            status: "completed".into(),
            prediction: None,
            run_by: Some("bob".into()),
            at: now_rfc3339(),
        },
    )
    .unwrap();

    let all = runs(&shared, experiment.id).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(
        all[0].run_by.as_deref(),
        Some("bob"),
        "attribution in the record"
    );
}
