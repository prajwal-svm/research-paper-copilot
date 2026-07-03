//! Object-anchored notes and bookmarks (task 7.1) + Markdown export (7.2).
//!
//! Storage is event-sourced over append-only journals (`notes/notes.jsonl`,
//! `bookmarks/bookmarks.jsonl`): create/update/delete events, materialized on
//! read. Append-only keeps crash-safety and the CRDT-upgradeable shape cloud
//! sync needs; anchors are object UUID + content hash, never page offsets, so
//! re-parsing re-attaches notes (UUIDs are deterministic across re-ingestion;
//! a changed content hash flags the anchor for user reattachment).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;

const NOTES_JOURNAL: &str = "notes/notes.jsonl";
const BOOKMARKS_JOURNAL: &str = "bookmarks/bookmarks.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum NoteEvent {
    Upsert {
        note_id: Uuid,
        object_id: Uuid,
        /// Content hash of the anchor object at write time.
        anchor_hash: String,
        markdown: String,
        /// Graph nodes this note is about (v2): auto-linked from the anchor
        /// object's concepts at save time, so notes surface in graph/lesson
        /// views. Absent in v1 journals (default empty).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        concepts: Vec<Uuid>,
        at: String,
    },
    Delete {
        note_id: Uuid,
        at: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum BookmarkEvent {
    Add {
        object_id: Uuid,
        anchor_hash: String,
        at: String,
    },
    Remove {
        object_id: Uuid,
        at: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub note_id: Uuid,
    pub object_id: Uuid,
    pub anchor_hash: String,
    pub markdown: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concepts: Vec<Uuid>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub object_id: Uuid,
    pub anchor_hash: String,
    pub added_at: String,
}

type Result<T> = std::result::Result<T, crate::bundle::BundleError>;

// ---------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------

/// All live notes (latest upsert per id, deletes applied), oldest first.
pub fn notes(bundle: &Bundle) -> Result<Vec<Note>> {
    let events: Vec<NoteEvent> = bundle.journal(NOTES_JOURNAL).read_all()?;
    let mut live: BTreeMap<Uuid, Note> = BTreeMap::new();
    for event in events {
        match event {
            NoteEvent::Upsert {
                note_id,
                object_id,
                anchor_hash,
                markdown,
                concepts,
                at,
            } => {
                live.insert(
                    note_id,
                    Note {
                        note_id,
                        object_id,
                        anchor_hash,
                        markdown,
                        concepts,
                        updated_at: at,
                    },
                );
            }
            NoteEvent::Delete { note_id, .. } => {
                live.remove(&note_id);
            }
        }
    }
    let mut notes: Vec<Note> = live.into_values().collect();
    notes.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
    Ok(notes)
}

/// Create or update a note. Saving is one journal append — effectively
/// instant, which is what the < 100 ms inline-editor budget needs.
/// `concepts`: graph nodes covering the anchor object (auto-linked by the
/// caller from the paper's knowledge graph; empty when no graph exists).
pub fn save_note(
    bundle: &Bundle,
    note_id: Uuid,
    object_id: Uuid,
    anchor_hash: &str,
    markdown: &str,
    concepts: Vec<Uuid>,
) -> Result<()> {
    bundle.journal(NOTES_JOURNAL).append(&NoteEvent::Upsert {
        note_id,
        object_id,
        anchor_hash: anchor_hash.to_string(),
        markdown: markdown.to_string(),
        concepts,
        at: crate::bundle::now_rfc3339(),
    })
}

pub fn delete_note(bundle: &Bundle, note_id: Uuid) -> Result<()> {
    bundle.journal(NOTES_JOURNAL).append(&NoteEvent::Delete {
        note_id,
        at: crate::bundle::now_rfc3339(),
    })
}

// ---------------------------------------------------------------------------
// Bookmarks
// ---------------------------------------------------------------------------

pub fn bookmarks(bundle: &Bundle) -> Result<Vec<Bookmark>> {
    let events: Vec<BookmarkEvent> = bundle.journal(BOOKMARKS_JOURNAL).read_all()?;
    let mut live: BTreeMap<Uuid, Bookmark> = BTreeMap::new();
    for event in events {
        match event {
            BookmarkEvent::Add {
                object_id,
                anchor_hash,
                at,
            } => {
                live.insert(
                    object_id,
                    Bookmark {
                        object_id,
                        anchor_hash,
                        added_at: at,
                    },
                );
            }
            BookmarkEvent::Remove { object_id, .. } => {
                live.remove(&object_id);
            }
        }
    }
    let mut bookmarks: Vec<Bookmark> = live.into_values().collect();
    bookmarks.sort_by(|a, b| a.added_at.cmp(&b.added_at));
    Ok(bookmarks)
}

/// Toggle a bookmark; returns the new state (true = bookmarked).
pub fn toggle_bookmark(bundle: &Bundle, object_id: Uuid, anchor_hash: &str) -> Result<bool> {
    let currently = bookmarks(bundle)?.iter().any(|b| b.object_id == object_id);
    let journal = bundle.journal(BOOKMARKS_JOURNAL);
    if currently {
        journal.append(&BookmarkEvent::Remove {
            object_id,
            at: crate::bundle::now_rfc3339(),
        })?;
        Ok(false)
    } else {
        journal.append(&BookmarkEvent::Add {
            object_id,
            anchor_hash: anchor_hash.to_string(),
            at: crate::bundle::now_rfc3339(),
        })?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Ink (freehand drawing) annotations
// ---------------------------------------------------------------------------

const INK_JOURNAL: &str = "notes/ink.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum InkEvent {
    Add(Box<InkStroke>),
    Delete { stroke_id: Uuid, at: String },
}

/// One freehand stroke, page-anchored in PDF points (top-left origin) so it
/// is zoom- and DPI-independent. Points carry optional pressure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InkStroke {
    pub stroke_id: Uuid,
    pub page: u32,
    /// "pen" | "highlighter"
    pub tool: String,
    /// CSS color.
    pub color: String,
    /// Base stroke size in PDF points.
    pub size: f32,
    /// [x, y, pressure] triples in PDF points.
    pub points: Vec<[f32; 3]>,
    pub at: String,
}

/// All live strokes (deletes applied), in draw order.
pub fn ink_strokes(bundle: &Bundle) -> Result<Vec<InkStroke>> {
    let events: Vec<InkEvent> = bundle.journal(INK_JOURNAL).read_all()?;
    let mut live: Vec<InkStroke> = Vec::new();
    for event in events {
        match event {
            InkEvent::Add(stroke) => live.push(*stroke),
            InkEvent::Delete { stroke_id, .. } => live.retain(|s| s.stroke_id != stroke_id),
        }
    }
    Ok(live)
}

/// Persist one stroke (single journal append — instant).
pub fn ink_add(bundle: &Bundle, stroke: InkStroke) -> Result<()> {
    bundle
        .journal(INK_JOURNAL)
        .append(&InkEvent::Add(Box::new(stroke)))
}

/// Delete a stroke (eraser / undo). Append-only: history is preserved.
pub fn ink_delete(bundle: &Bundle, stroke_id: Uuid) -> Result<()> {
    bundle.journal(INK_JOURNAL).append(&InkEvent::Delete {
        stroke_id,
        at: crate::bundle::now_rfc3339(),
    })
}

// ---------------------------------------------------------------------------
// Markdown export (task 7.2)
// ---------------------------------------------------------------------------

/// Export notes and bookmarks as a single Markdown document, notes grouped
/// by section, each linking back to its anchor context (quoted text).
pub fn export_markdown(bundle: &Bundle) -> Result<String> {
    use crate::objects::{ObjectType, SemanticTreeDocument};

    let title = bundle.metadata()?.paper.title;
    let tree: Option<SemanticTreeDocument> = bundle.read_derived_json("semantic_tree.json")?;
    let notes = notes(bundle)?;
    let bookmarks = bookmarks(bundle)?;

    let find = |id: Uuid| {
        tree.as_ref()
            .and_then(|t| t.objects.iter().find(|o| o.id == id))
    };
    // Section for an object: walk belongs_to relationships upward.
    let section_of = |id: Uuid| -> String {
        let mut current = id;
        for _ in 0..6 {
            let Some(object) = find(current) else { break };
            if object.object_type == ObjectType::Section {
                return object.content.text.clone();
            }
            let Some(parent) = object.relationships.iter().find_map(|r| {
                matches!(
                    r.relationship_type,
                    crate::objects::RelationshipType::BelongsTo
                )
                .then_some(r.target)
            }) else {
                break;
            };
            current = parent;
        }
        "Ungrouped".to_string()
    };

    let mut out = format!("# Notes — {title}\n");

    if !notes.is_empty() {
        let mut by_section: BTreeMap<String, Vec<&Note>> = BTreeMap::new();
        for note in &notes {
            by_section
                .entry(section_of(note.object_id))
                .or_default()
                .push(note);
        }
        for (section, section_notes) in by_section {
            out.push_str(&format!("\n## {section}\n"));
            for note in section_notes {
                if let Some(object) = find(note.object_id) {
                    let quoted: String = object.content.text.chars().take(200).collect();
                    let anchor_moved = object.content_hash != note.anchor_hash;
                    out.push_str(&format!(
                        "\n> {quoted}{ellipsis}\n{moved}\n{markdown}\n",
                        ellipsis = if object.content.text.chars().count() > 200 {
                            "…"
                        } else {
                            ""
                        },
                        moved = if anchor_moved {
                            "*(the anchored passage has changed since this note was written)*\n"
                        } else {
                            ""
                        },
                        markdown = note.markdown,
                    ));
                } else {
                    out.push_str(&format!(
                        "\n> (anchor not found — object {})\n\n{}\n",
                        note.object_id, note.markdown
                    ));
                }
            }
        }
    } else {
        out.push_str("\n*No notes yet.*\n");
    }

    if !bookmarks.is_empty() {
        out.push_str("\n## Bookmarks\n\n");
        for bookmark in &bookmarks {
            let label = find(bookmark.object_id)
                .map(|o| {
                    o.semantic_label
                        .clone()
                        .unwrap_or_else(|| o.content.text.chars().take(80).collect())
                })
                .unwrap_or_else(|| bookmark.object_id.to_string());
            out.push_str(&format!("- {label}\n"));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    fn bundle() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle =
            Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("Test Paper"), "file").unwrap();
        (tmp, bundle)
    }

    #[test]
    fn note_lifecycle_upsert_edit_delete() {
        let (_tmp, bundle) = bundle();
        let note_id = Uuid::new_v4();
        let object_id = Uuid::new_v4();

        save_note(
            &bundle,
            note_id,
            object_id,
            "sha256:aaa",
            "first draft",
            vec![],
        )
        .unwrap();
        save_note(
            &bundle,
            note_id,
            object_id,
            "sha256:aaa",
            "**edited**",
            vec![],
        )
        .unwrap();
        let notes_now = notes(&bundle).unwrap();
        assert_eq!(notes_now.len(), 1);
        assert_eq!(notes_now[0].markdown, "**edited**");

        delete_note(&bundle, note_id).unwrap();
        assert!(notes(&bundle).unwrap().is_empty());

        // Journal keeps full history (append-only) even after delete.
        let raw = std::fs::read_to_string(bundle.root().join(NOTES_JOURNAL)).unwrap();
        assert_eq!(raw.lines().count(), 3);
    }

    #[test]
    fn ink_stroke_lifecycle() {
        let (_tmp, bundle) = bundle();
        let stroke_id = Uuid::new_v4();
        ink_add(
            &bundle,
            InkStroke {
                stroke_id,
                page: 2,
                tool: "pen".into(),
                color: "#3b82f6".into(),
                size: 2.5,
                points: vec![[10.0, 10.0, 0.5], [20.0, 14.0, 0.6], [30.0, 22.0, 0.7]],
                at: crate::bundle::now_rfc3339(),
            },
        )
        .unwrap();
        let strokes = ink_strokes(&bundle).unwrap();
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].page, 2);
        assert_eq!(strokes[0].points.len(), 3);

        ink_delete(&bundle, stroke_id).unwrap();
        assert!(ink_strokes(&bundle).unwrap().is_empty());
        // Append-only journal keeps full history.
        let raw = std::fs::read_to_string(bundle.root().join(INK_JOURNAL)).unwrap();
        assert_eq!(raw.lines().count(), 2);
    }

    #[test]
    fn bookmark_toggle_roundtrip() {
        let (_tmp, bundle) = bundle();
        let object_id = Uuid::new_v4();
        assert!(toggle_bookmark(&bundle, object_id, "sha256:aaa").unwrap());
        assert_eq!(bookmarks(&bundle).unwrap().len(), 1);
        assert!(!toggle_bookmark(&bundle, object_id, "sha256:aaa").unwrap());
        assert!(bookmarks(&bundle).unwrap().is_empty());
    }

    #[test]
    fn export_groups_notes_and_quotes_anchor() {
        use crate::layout::BBox;
        use crate::objects::*;

        let (_tmp, bundle) = bundle();
        // Tree: section containing a paragraph.
        let section_id = Uuid::new_v4();
        let paragraph_id = Uuid::new_v4();
        let paragraph_text =
            "Attention mechanisms have become an integral part of sequence modeling.";
        let tree = SemanticTreeDocument {
            pipeline_version: "0.1.0".into(),
            objects: vec![
                Object {
                    id: section_id,
                    object_type: ObjectType::Section,
                    regions: vec![BBox {
                        page: 0,
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    }],
                    content: Content {
                        text: "1 Introduction".into(),
                        latex: None,
                        caption: None,
                    },
                    semantic_label: Some("Section — 1 Introduction".into()),
                    relationships: vec![],
                    embedding: None,
                    content_hash: crate::bundle::sha256_bytes(b"1 Introduction"),
                    confidence: 0.9,
                },
                Object {
                    id: paragraph_id,
                    object_type: ObjectType::Paragraph,
                    regions: vec![BBox {
                        page: 0,
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    }],
                    content: Content {
                        text: paragraph_text.into(),
                        latex: None,
                        caption: None,
                    },
                    semantic_label: None,
                    relationships: vec![Relationship {
                        relationship_type: RelationshipType::BelongsTo,
                        target: section_id,
                        confidence: None,
                    }],
                    embedding: None,
                    content_hash: crate::bundle::sha256_bytes(paragraph_text.as_bytes()),
                    confidence: 0.9,
                },
            ],
            tree: vec![],
        };
        bundle
            .write_derived_json(
                "semantic_tree.json",
                &tree,
                "objects",
                serde_json::json!({"pipeline_version": "0.1.0", "status": "complete"}),
            )
            .unwrap();

        let anchor_hash = crate::bundle::sha256_bytes(paragraph_text.as_bytes());
        save_note(
            &bundle,
            Uuid::new_v4(),
            paragraph_id,
            &anchor_hash,
            "Key insight!",
            vec![],
        )
        .unwrap();
        toggle_bookmark(&bundle, section_id, "sha256:x").unwrap();

        let markdown = export_markdown(&bundle).unwrap();
        assert!(markdown.contains("# Notes — Test Paper"));
        assert!(markdown.contains("## 1 Introduction"));
        assert!(markdown.contains("> Attention mechanisms"));
        assert!(markdown.contains("Key insight!"));
        assert!(markdown.contains("## Bookmarks"));
        assert!(markdown.contains("Section — 1 Introduction"));
        // Anchor unchanged → no moved warning.
        assert!(!markdown.contains("has changed since"));
    }
}
