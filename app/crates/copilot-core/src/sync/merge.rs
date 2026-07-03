//! Merge semantics — the no-CRDT decision, made safe by tests.
//!
//! Journals (nearly all user data) merge by **entry-set union**: dedupe on
//! exact line content, order deterministically by (parsed `at`, line). Every
//! reader in the codebase folds edit/delete *events* rather than mutating in
//! place, so union is semantically correct by construction. Property tests
//! assert commutativity and idempotence through real readers.
//!
//! The few non-journal user files resolve last-writer-wins with the losing
//! version preserved as a visible conflict copy — never silently dropped.

use std::collections::BTreeSet;
use std::path::Path;

/// Union-merge two journal files' contents. Lines are treated as opaque
/// committed entries (torn trailing lines were already excluded by journal
/// read semantics on each device; here we defensively drop non-JSON lines).
/// Result is deterministic regardless of argument order.
pub fn merge_journals(a: &str, b: &str) -> String {
    let mut entries: BTreeSet<(String, String)> = BTreeSet::new();
    for line in a.lines().chain(b.lines()) {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        // Only committed JSON entries participate (torn writes stay local).
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let at = value
            .get("at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        entries.insert((at, line.to_string()));
    }
    let mut out = String::new();
    for (_, line) in entries {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Is this path a journal (union-merged) as opposed to an LWW document?
pub fn is_journal(path: &str) -> bool {
    path.ends_with(".jsonl")
}

/// LWW with conflict copy: writes `incoming` over `local_path` when the
/// incoming version is newer, preserving the loser beside it as
/// `<name>.conflict-<device>-<date>`. Returns the conflict-copy path when
/// one was created. Identical content is a no-op.
pub fn lww_with_conflict(
    local_path: &Path,
    incoming: &[u8],
    incoming_is_newer: bool,
    other_device: &str,
) -> std::io::Result<Option<std::path::PathBuf>> {
    let local = std::fs::read(local_path).ok();
    match local {
        None => {
            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(local_path, incoming)?;
            Ok(None)
        }
        Some(existing) if existing == incoming => Ok(None),
        Some(existing) => {
            let date = crate::bundle::now_rfc3339()
                .chars()
                .take(10)
                .collect::<String>();
            let conflict_name = format!(
                "{}.conflict-{other_device}-{date}",
                local_path.file_name().unwrap_or_default().to_string_lossy()
            );
            let conflict_path = local_path.with_file_name(conflict_name);
            if incoming_is_newer {
                // Incoming wins; local preserved as the conflict copy.
                std::fs::write(&conflict_path, &existing)?;
                std::fs::write(local_path, incoming)?;
            } else {
                // Local wins; incoming preserved as the conflict copy.
                std::fs::write(&conflict_path, incoming)?;
            }
            Ok(Some(conflict_path))
        }
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{Bundle, Paper};

    #[test]
    fn union_is_commutative_idempotent_and_deduplicating() {
        let a = "{\"at\":\"2026-07-01T10:00:00Z\",\"note\":\"from A\"}\n{\"at\":\"2026-07-01T11:00:00Z\",\"note\":\"shared\"}\n";
        let b = "{\"at\":\"2026-07-01T11:00:00Z\",\"note\":\"shared\"}\n{\"at\":\"2026-07-01T10:30:00Z\",\"note\":\"from B\"}\n";
        let ab = merge_journals(a, b);
        let ba = merge_journals(b, a);
        assert_eq!(ab, ba, "commutative");
        assert_eq!(merge_journals(&ab, b), ab, "idempotent");
        assert_eq!(ab.lines().count(), 3, "shared entry deduplicated");
        let order: Vec<&str> = ab.lines().collect();
        assert!(order[0].contains("from A") && order[1].contains("from B"));
    }

    /// The load-bearing property: any interleaving of two devices' appends,
    /// merged in any order, folds identically through the REAL readers.
    #[test]
    fn merged_journals_fold_identically_through_real_readers() {
        use crate::annotations::{notes, save_note};
        // Device A and device B each save notes offline.
        let make_device = || {
            let tmp = tempfile::tempdir().unwrap();
            let bundle = Bundle::create(
                &tmp.path().join("p.research"),
                b"%PDF-1.5 fake",
                Paper::new("S"),
                "file",
            )
            .unwrap();
            (tmp, bundle)
        };
        let (_ta, device_a) = make_device();
        let (_tb, device_b) = make_device();
        let object = uuid::Uuid::new_v4();
        let note_a = uuid::Uuid::new_v4();
        let note_b = uuid::Uuid::new_v4();
        save_note(&device_a, note_a, object, "sha256:x", "note from A", vec![]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        save_note(&device_b, note_b, object, "sha256:x", "note from B", vec![]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        // A edits its note (an event, not an in-place mutation).
        save_note(
            &device_a,
            note_a,
            object,
            "sha256:x",
            "note from A (edited)",
            vec![],
        )
        .unwrap();

        let journal_a = std::fs::read_to_string(device_a.root().join("notes/notes.jsonl")).unwrap();
        let journal_b = std::fs::read_to_string(device_b.root().join("notes/notes.jsonl")).unwrap();

        // Merge in both orders onto fresh devices; fold through the reader.
        let fold = |merged: &str| -> Vec<(uuid::Uuid, String)> {
            let (_t, device) = make_device();
            std::fs::write(device.root().join("notes/notes.jsonl"), merged).unwrap();
            notes(&device)
                .unwrap()
                .into_iter()
                .map(|n| (n.note_id, n.markdown))
                .collect()
        };
        let ab = fold(&merge_journals(&journal_a, &journal_b));
        let ba = fold(&merge_journals(&journal_b, &journal_a));
        assert_eq!(ab, ba, "fold independent of merge order");
        assert_eq!(ab.len(), 2, "both devices' notes present");
        assert!(
            ab.iter()
                .any(|(id, md)| *id == note_a && md == "note from A (edited)"),
            "A's edit event applied after union: {ab:?}"
        );
        assert!(ab.iter().any(|(id, _)| *id == note_b));
    }

    #[test]
    fn mastery_journals_union_cleanly_too() {
        use crate::learning::{LearnerModel, MasteryEvent};
        let concept = uuid::Uuid::new_v4();
        let event = |quality: u8, at: &str| MasteryEvent {
            concept,
            object: None,
            quality,
            source: "quiz".into(),
            at: at.into(),
        };
        let build = |events: &[&MasteryEvent]| -> String {
            let tmp = tempfile::tempdir().unwrap();
            let model = LearnerModel::open(tmp.path());
            for e in events {
                model.record_mastery(e).unwrap();
            }
            std::fs::read_to_string(tmp.path().join("learning_state/mastery.jsonl")).unwrap()
        };
        let a = build(&[&event(2, "2026-07-01T10:00:00Z")]);
        let b = build(&[&event(5, "2026-07-01T12:00:00Z")]);
        let merged = merge_journals(&a, &b);

        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("learning_state")).unwrap();
        std::fs::write(tmp.path().join("learning_state/mastery.jsonl"), &merged).unwrap();
        let snapshot = LearnerModel::open(tmp.path()).snapshot().unwrap();
        let mastery = snapshot.mastery_of(concept).unwrap();
        assert_eq!(mastery.signals, 2, "both devices' events counted");
        assert_eq!(mastery.repetitions, 1, "SM-2 folded in timestamp order");
    }

    #[test]
    fn lww_preserves_the_loser_as_conflict_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("reviews/r1/document.md");
        // First write: no conflict.
        assert!(lww_with_conflict(&path, b"v1 from A", true, "device-b")
            .unwrap()
            .is_none());
        // Identical content: no-op.
        assert!(lww_with_conflict(&path, b"v1 from A", true, "device-b")
            .unwrap()
            .is_none());
        // Divergent incoming, newer → wins, local preserved.
        let conflict = lww_with_conflict(&path, b"v2 from B", true, "device-b")
            .unwrap()
            .expect("conflict copy created");
        assert_eq!(std::fs::read(&path).unwrap(), b"v2 from B");
        assert_eq!(std::fs::read(&conflict).unwrap(), b"v1 from A");
        assert!(conflict.to_string_lossy().contains(".conflict-device-b-"));

        // Divergent incoming, older → local wins, incoming preserved.
        let conflict2 = lww_with_conflict(&path, b"stale from C", false, "device-c")
            .unwrap()
            .expect("conflict copy created");
        assert_eq!(std::fs::read(&path).unwrap(), b"v2 from B", "local kept");
        assert_eq!(std::fs::read(&conflict2).unwrap(), b"stale from C");
    }
}
