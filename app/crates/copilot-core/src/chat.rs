//! Per-object persistent chats: append-only JSONL journals in the bundle's
//! `chats/` directory, keyed by object UUID. Reopening an object resumes its
//! conversation; a crash mid-write never loses committed messages (journal
//! semantics from `bundle::Journal`). Honest failure: interrupted assistant
//! responses are persisted with `incomplete: true`, never silently dropped.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai::ChatMessage;
use crate::bundle::Bundle;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StoredChatMessage {
    /// Stable id for edits/deletes. Absent in v1 journals — materialization
    /// assigns a deterministic id (UUID v5 over object + line index) so old
    /// messages stay editable and edits survive re-reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub role: String, // "user" | "assistant"
    pub content: String,
    /// The action that produced this exchange (user messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// True when the assistant response was cut off mid-stream.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub incomplete: bool,
    /// True when the content was edited after the fact (derived at read).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edited: bool,
    pub at: String,
}

/// Journal entries: messages plus append-only correction events. Old
/// journals contain bare messages; `op`-tagged lines are corrections.
/// Untagged deserialization tries corrections first (they require `op`,
/// which messages never have).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
enum ChatEntry {
    Correction(ChatCorrection),
    Message(StoredChatMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "op")]
enum ChatCorrection {
    Edit {
        target: Uuid,
        content: String,
        at: String,
    },
    Delete {
        target: Uuid,
        at: String,
    },
}

fn journal_path(object_id: Uuid) -> String {
    format!("chats/{object_id}.jsonl")
}

/// Namespace for deterministic ids of pre-v2 messages (no stored id).
const CHAT_MESSAGE_NAMESPACE: Uuid = Uuid::from_bytes([
    0x8c, 0x1f, 0x2a, 0x3b, 0x4c, 0x5d, 0x6e, 0x7f, 0x80, 0x91, 0xa2, 0xb3, 0xc4, 0xd5, 0xe6, 0xf7,
]);

/// Full conversation for an object (empty when none), with edits and
/// deletes applied. Committed messages always load; torn trailing writes
/// are skipped by the journal. The journal itself stays append-only — the
/// original text is never erased on disk, only superseded.
pub fn history(
    bundle: &Bundle,
    object_id: Uuid,
) -> Result<Vec<StoredChatMessage>, crate::bundle::BundleError> {
    let entries: Vec<ChatEntry> = bundle.journal(&journal_path(object_id)).read_all()?;
    let mut messages: Vec<StoredChatMessage> = Vec::new();
    let mut index = 0usize;
    for entry in entries {
        match entry {
            ChatEntry::Message(mut message) => {
                if message.id.is_none() {
                    let key = format!("{object_id}:{index}");
                    message.id = Some(Uuid::new_v5(&CHAT_MESSAGE_NAMESPACE, key.as_bytes()));
                }
                index += 1;
                messages.push(message);
            }
            ChatEntry::Correction(ChatCorrection::Edit {
                target, content, ..
            }) => {
                if let Some(m) = messages.iter_mut().find(|m| m.id == Some(target)) {
                    m.content = content;
                    m.edited = true;
                }
            }
            ChatEntry::Correction(ChatCorrection::Delete { target, .. }) => {
                messages.retain(|m| m.id != Some(target));
            }
        }
    }
    Ok(messages)
}

/// Edit a message's content (user or assistant) — an append-only event; the
/// original line stays on disk.
pub fn edit_message(
    bundle: &Bundle,
    object_id: Uuid,
    message_id: Uuid,
    content: String,
) -> Result<(), crate::bundle::BundleError> {
    bundle
        .journal(&journal_path(object_id))
        .append(&ChatCorrection::Edit {
            target: message_id,
            content,
            at: crate::bundle::now_rfc3339(),
        })
}

/// Delete a message from the conversation view (append-only tombstone).
pub fn delete_message(
    bundle: &Bundle,
    object_id: Uuid,
    message_id: Uuid,
) -> Result<(), crate::bundle::BundleError> {
    bundle
        .journal(&journal_path(object_id))
        .append(&ChatCorrection::Delete {
            target: message_id,
            at: crate::bundle::now_rfc3339(),
        })
}

/// Append one message to an object's conversation.
pub fn append(
    bundle: &Bundle,
    object_id: Uuid,
    message: &StoredChatMessage,
) -> Result<(), crate::bundle::BundleError> {
    bundle.journal(&journal_path(object_id)).append(message)
}

/// History as provider messages (for context assembly). Incomplete assistant
/// turns are included with an explicit marker so the model knows the reader
/// saw a partial answer.
pub fn as_thread(history: &[StoredChatMessage]) -> Vec<ChatMessage> {
    history
        .iter()
        .map(|m| ChatMessage {
            images: Vec::new(),
            role: m.role.clone(),
            content: if m.incomplete {
                format!("{} [answer was cut off here]", m.content)
            } else {
                m.content.clone()
            },
        })
        .collect()
}

pub fn user_message(action: &str, content: String) -> StoredChatMessage {
    StoredChatMessage {
        id: Some(Uuid::new_v4()),
        role: "user".to_string(),
        content,
        action: Some(action.to_string()),
        incomplete: false,
        edited: false,
        at: crate::bundle::now_rfc3339(),
    }
}

pub fn assistant_message(content: String, incomplete: bool) -> StoredChatMessage {
    StoredChatMessage {
        id: Some(Uuid::new_v4()),
        role: "assistant".to_string(),
        content,
        action: None,
        incomplete,
        edited: false,
        at: crate::bundle::now_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    #[test]
    fn edits_and_deletes_apply_including_legacy_messages() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("S"), "file").unwrap();
        let object_id = Uuid::new_v4();

        // A v1-era journal line: no id field at all.
        bundle
            .journal(&journal_path(object_id))
            .append(&serde_json::json!({
                "role": "user", "content": "old question", "at": "2025-01-01T00:00:00Z"
            }))
            .unwrap();
        append(
            &bundle,
            object_id,
            &assistant_message("answer with a tpyo".into(), false),
        )
        .unwrap();

        let history_now = history(&bundle, object_id).unwrap();
        assert_eq!(history_now.len(), 2);
        let legacy_id = history_now[0].id.expect("legacy message gets an id");
        let answer_id = history_now[1].id.unwrap();

        // Edit the assistant answer; delete the legacy user message.
        edit_message(&bundle, object_id, answer_id, "answer with a typo".into()).unwrap();
        delete_message(&bundle, object_id, legacy_id).unwrap();

        let after = history(&bundle, object_id).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].content, "answer with a typo");
        assert!(after[0].edited);

        // Deterministic legacy ids: a re-read still honors the delete.
        let again = history(&bundle, object_id).unwrap();
        assert_eq!(again.len(), 1);

        // Append-only: the original text is still on disk, just superseded.
        let raw = std::fs::read_to_string(root.join(journal_path(object_id))).unwrap();
        assert!(raw.contains("tpyo"));
        assert!(raw.contains("old question"));
    }

    #[test]
    fn conversation_resumes_across_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("S"), "file").unwrap();
        let object_id = Uuid::new_v4();

        append(
            &bundle,
            object_id,
            &user_message("ask", "what is Q?".into()),
        )
        .unwrap();
        append(
            &bundle,
            object_id,
            &assistant_message("Q is the query matrix.".into(), false),
        )
        .unwrap();
        drop(bundle);

        // Reopen (simulates three days later).
        let bundle = Bundle::open(&root).unwrap();
        let history = history(&bundle, object_id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].action.as_deref(), Some("ask"));
        assert_eq!(history[1].content, "Q is the query matrix.");

        // Another object's chat is separate.
        assert!(super::history(&bundle, Uuid::new_v4()).unwrap().is_empty());
    }

    #[test]
    fn incomplete_answers_are_marked_and_carried_into_thread() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("S"), "file").unwrap();
        let object_id = Uuid::new_v4();

        append(
            &bundle,
            object_id,
            &user_message("explain", "explain".into()),
        )
        .unwrap();
        append(
            &bundle,
            object_id,
            &assistant_message("The attention mechanism".into(), true),
        )
        .unwrap();

        let history = history(&bundle, object_id).unwrap();
        assert!(history[1].incomplete);
        let thread = as_thread(&history);
        assert!(thread[1].content.contains("[answer was cut off here]"));
    }
}
