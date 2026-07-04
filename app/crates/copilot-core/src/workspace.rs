//! Workspace store: `workspace.db` (SQLite) inside the library root — the
//! home of every paper-independent entity (notes, canvases, chat threads).
//!
//! Design contract (openspec/changes/workspace-store):
//! - `items` is the generic registry (listing + recency); feature changes
//!   add their content tables via migrations and join against it.
//! - `refs` is the generic backlink table — the knowledge-graph seed —
//!   linking any workspace entity to papers/objects/URLs/files.
//! - Deletes are tombstones (`deleted_at`), rows carry RFC 3339
//!   `updated_at`: merge-ready for a future sync layer, no CRDTs.
//! - `.research` bundles stay untouched; nothing paper-anchored lives here.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub const WORKSPACE_DB_FILE: &str = "workspace.db";

/// Bump per migration; `migrate` applies every step above the stored
/// `user_version` in order, inside a transaction.
const SCHEMA_VERSION: i64 = 4;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace store: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("workspace store io at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "this workspace was created by a newer version of the app \
         (schema v{found}, this app understands v{supported}) — update the app"
    )]
    VersionTooNew { found: i64, supported: i64 },
    #[error("no workspace item with id {0}")]
    NotFound(Uuid),
}

type Result<T> = std::result::Result<T, WorkspaceError>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceItem {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceRef {
    pub id: Uuid,
    pub source_kind: String,
    pub source_id: Uuid,
    pub target_kind: String, // paper | object | url | file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paper_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NoteDoc {
    pub item: WorkspaceItem,
    /// BlockNote document JSON — the source of truth.
    pub content: String,
    /// Derived markdown mirror (export, search).
    pub markdown: String,
}

/// A paper/object mention as the note editor reports it. Also the shape a
/// pinned canvas element reports for backlink reconciliation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MentionRef {
    pub paper_id: Option<String>,
    pub object_id: Option<Uuid>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CanvasDoc {
    pub item: WorkspaceItem,
    /// Excalidraw scene JSON ({elements, appState, files}) — source of truth.
    pub scene: String,
    /// Rendered PNG (data URL) for the library card; empty when blank.
    pub thumbnail: String,
}

/// One turn in a chat thread. Mirrors the per-object chat's honesty:
/// partial answers persist marked `incomplete`; edits are marked, not erased.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessageRow {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub role: String, // user | assistant
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    pub incomplete: bool,
    pub edited: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug)]
pub struct WorkspaceStore {
    conn: Connection,
}

impl WorkspaceStore {
    /// Open (creating if absent) the store inside `library_root` and bring
    /// its schema current. Refuses newer-than-known schemas untouched.
    pub fn open(library_root: &Path) -> Result<Self> {
        std::fs::create_dir_all(library_root).map_err(|e| WorkspaceError::Io {
            path: library_root.to_path_buf(),
            source: e,
        })?;
        let conn = Connection::open(library_root.join(WORKSPACE_DB_FILE))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_millis(5_000))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = WorkspaceStore { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let found: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if found > SCHEMA_VERSION {
            return Err(WorkspaceError::VersionTooNew {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        if found < 1 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS items (
                   id TEXT PRIMARY KEY,
                   kind TEXT NOT NULL,
                   title TEXT NOT NULL,
                   created_at TEXT NOT NULL,
                   updated_at TEXT NOT NULL,
                   deleted_at TEXT
                 );
                 CREATE INDEX IF NOT EXISTS idx_items_kind_updated
                   ON items(kind, updated_at);
                 CREATE TABLE IF NOT EXISTS refs (
                   id TEXT PRIMARY KEY,
                   source_kind TEXT NOT NULL,
                   source_id TEXT NOT NULL,
                   target_kind TEXT NOT NULL,
                   paper_id TEXT,
                   object_id TEXT,
                   url TEXT,
                   path TEXT,
                   label TEXT,
                   created_at TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_refs_source
                   ON refs(source_kind, source_id);
                 CREATE INDEX IF NOT EXISTS idx_refs_paper ON refs(paper_id);
                 CREATE INDEX IF NOT EXISTS idx_refs_object ON refs(object_id);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        if found < 2 {
            // v2 (independent-notes): BlockNote JSON is the source of truth;
            // the markdown mirror serves export and future search.
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS notes (
                   id TEXT PRIMARY KEY REFERENCES items(id),
                   content TEXT NOT NULL,
                   markdown TEXT NOT NULL DEFAULT ''
                 );
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }
        if found < 3 {
            // v3 (independent-canvases): Excalidraw scene JSON is the source
            // of truth; the thumbnail is a rendered PNG for library cards.
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS canvases (
                   id TEXT PRIMARY KEY REFERENCES items(id),
                   scene TEXT NOT NULL DEFAULT '{}',
                   thumbnail TEXT NOT NULL DEFAULT ''
                 );
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }
        if found < 4 {
            // v4 (chat-threads): title/recency/tombstone live on `items`;
            // the chats row is the join, messages carry the transcript.
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS chats (
                   id TEXT PRIMARY KEY REFERENCES items(id)
                 );
                 CREATE TABLE IF NOT EXISTS chat_messages (
                   id TEXT PRIMARY KEY,
                   chat_id TEXT NOT NULL REFERENCES chats(id),
                   role TEXT NOT NULL,
                   content TEXT NOT NULL,
                   action TEXT,
                   incomplete INTEGER NOT NULL DEFAULT 0,
                   edited INTEGER NOT NULL DEFAULT 0,
                   created_at TEXT NOT NULL,
                   updated_at TEXT NOT NULL,
                   deleted_at TEXT
                 );
                 CREATE INDEX IF NOT EXISTS idx_chat_messages_chat
                   ON chat_messages(chat_id, created_at);
                 PRAGMA user_version = 4;
                 COMMIT;",
            )?;
        }
        Ok(())
    }

    // ---- item registry -----------------------------------------------------

    pub fn create_item(&self, kind: &str, title: &str) -> Result<WorkspaceItem> {
        let now = crate::bundle::now_rfc3339();
        let item = WorkspaceItem {
            id: Uuid::new_v4(),
            kind: kind.to_string(),
            title: title.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.conn.execute(
            "INSERT INTO items (id, kind, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                item.id.to_string(),
                item.kind,
                item.title,
                item.created_at,
                item.updated_at
            ],
        )?;
        Ok(item)
    }

    pub fn rename_item(&self, id: Uuid, title: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE items SET title = ?2, updated_at = ?3
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id.to_string(), title, crate::bundle::now_rfc3339()],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(WorkspaceError::NotFound(id))
    }

    /// Bump `updated_at` — called by feature code on any content mutation,
    /// so recency-sorted listings stay honest.
    pub fn touch_item(&self, id: Uuid) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE items SET updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
            params![id.to_string(), crate::bundle::now_rfc3339()],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(WorkspaceError::NotFound(id))
    }

    /// Soft delete: the tombstoned row stays for future sync merge.
    pub fn delete_item(&self, id: Uuid) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE items SET deleted_at = ?2, updated_at = ?2
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id.to_string(), crate::bundle::now_rfc3339()],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(WorkspaceError::NotFound(id))
    }

    /// Live items, newest-updated first; optionally one kind.
    pub fn list_items(&self, kind: Option<&str>) -> Result<Vec<WorkspaceItem>> {
        let mut sql = String::from(
            "SELECT id, kind, title, created_at, updated_at FROM items
             WHERE deleted_at IS NULL",
        );
        if kind.is_some() {
            sql.push_str(" AND kind = ?1");
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let mut statement = self.conn.prepare(&sql)?;
        let map = |row: &rusqlite::Row| -> rusqlite::Result<WorkspaceItem> {
            Ok(WorkspaceItem {
                id: parse_uuid(row.get::<_, String>(0)?),
                kind: row.get(1)?,
                title: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        };
        let rows = match kind {
            Some(k) => statement.query_map(params![k], map)?,
            None => statement.query_map([], map)?,
        };
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_item(&self, id: Uuid) -> Result<Option<WorkspaceItem>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, kind, title, created_at, updated_at FROM items
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id.to_string()],
                |row| {
                    Ok(WorkspaceItem {
                        id: parse_uuid(row.get::<_, String>(0)?),
                        kind: row.get(1)?,
                        title: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    // ---- notes ---------------------------------------------------------

    /// Create a note: registry row + empty BlockNote document.
    pub fn note_create(&self, title: &str) -> Result<WorkspaceItem> {
        let item = self.create_item("note", title)?;
        self.conn.execute(
            "INSERT INTO notes (id, content, markdown) VALUES (?1, '[]', '')",
            params![item.id.to_string()],
        )?;
        Ok(item)
    }

    /// A note's registry row + content. `None` when missing or tombstoned.
    pub fn note_get(&self, id: Uuid) -> Result<Option<NoteDoc>> {
        let Some(item) = self.get_item(id)? else {
            return Ok(None);
        };
        let row = self
            .conn
            .query_row(
                "SELECT content, markdown FROM notes WHERE id = ?1",
                params![id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row.map(|(content, markdown)| NoteDoc {
            item,
            content,
            markdown,
        }))
    }

    /// Persist an autosave: BlockNote JSON + markdown mirror, bump recency.
    pub fn note_save(&self, id: Uuid, content: &str, markdown: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE notes SET content = ?2, markdown = ?3 WHERE id = ?1",
            params![id.to_string(), content, markdown],
        )?;
        if changed != 1 {
            return Err(WorkspaceError::NotFound(id));
        }
        self.touch_item(id)
    }

    /// Reconcile a note's mention refs against the document — see `sync_refs`.
    pub fn note_sync_refs(&self, id: Uuid, mentions: &[MentionRef]) -> Result<()> {
        self.sync_refs("note", id, mentions)
    }

    /// Diff-based, self-healing refs reconciliation for any entity: adds
    /// missing rows, removes stale ones. Called after every autosave; shared
    /// by notes (@-mentions) and canvases (pinned content).
    pub fn sync_refs(&self, source_kind: &str, id: Uuid, mentions: &[MentionRef]) -> Result<()> {
        let existing = self.refs_from(source_kind, id)?;
        let key = |paper: &Option<String>, object: &Option<Uuid>| {
            format!(
                "{}\u{1f}{}",
                paper.as_deref().unwrap_or(""),
                object.map(|o| o.to_string()).unwrap_or_default()
            )
        };
        let wanted: std::collections::HashSet<String> = mentions
            .iter()
            .map(|m| key(&m.paper_id, &m.object_id))
            .collect();
        for r in &existing {
            if !wanted.contains(&key(&r.paper_id, &r.object_id)) {
                self.remove_ref(r.id)?;
            }
        }
        let have: std::collections::HashSet<String> = existing
            .iter()
            .map(|r| key(&r.paper_id, &r.object_id))
            .collect();
        for m in mentions {
            if !have.contains(&key(&m.paper_id, &m.object_id)) {
                self.add_ref(
                    source_kind,
                    id,
                    if m.object_id.is_some() { "object" } else { "paper" },
                    m.paper_id.as_deref(),
                    m.object_id,
                    None,
                    None,
                    m.label.as_deref(),
                )?;
            }
        }
        Ok(())
    }

    // ---- canvases ------------------------------------------------------

    /// Create a canvas: registry row + empty Excalidraw scene.
    pub fn canvas_create(&self, title: &str) -> Result<WorkspaceItem> {
        let item = self.create_item("canvas", title)?;
        self.conn.execute(
            "INSERT INTO canvases (id, scene, thumbnail) VALUES (?1, '{}', '')",
            params![item.id.to_string()],
        )?;
        Ok(item)
    }

    /// A canvas's registry row + scene. `None` when missing or tombstoned.
    pub fn canvas_get(&self, id: Uuid) -> Result<Option<CanvasDoc>> {
        let Some(item) = self.get_item(id)? else {
            return Ok(None);
        };
        let row = self
            .conn
            .query_row(
                "SELECT scene, thumbnail FROM canvases WHERE id = ?1",
                params![id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row.map(|(scene, thumbnail)| CanvasDoc {
            item,
            scene,
            thumbnail,
        }))
    }

    /// Persist an autosave: scene JSON + thumbnail PNG, bump recency.
    pub fn canvas_save(&self, id: Uuid, scene: &str, thumbnail: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE canvases SET scene = ?2, thumbnail = ?3 WHERE id = ?1",
            params![id.to_string(), scene, thumbnail],
        )?;
        if changed != 1 {
            return Err(WorkspaceError::NotFound(id));
        }
        self.touch_item(id)
    }

    /// Reconcile a canvas's pinned-content refs — see `sync_refs`.
    pub fn canvas_sync_refs(&self, id: Uuid, pins: &[MentionRef]) -> Result<()> {
        self.sync_refs("canvas", id, pins)
    }

    // ---- chats ---------------------------------------------------------

    /// Create a chat: registry row + `chats` join row.
    pub fn chat_create(&self, title: &str) -> Result<WorkspaceItem> {
        let item = self.create_item("chat", title)?;
        self.conn.execute(
            "INSERT INTO chats (id) VALUES (?1)",
            params![item.id.to_string()],
        )?;
        Ok(item)
    }

    pub fn chat_get(&self, id: Uuid) -> Result<Option<WorkspaceItem>> {
        // A chat is its registry row plus messages; the join carries nothing
        // extra today. Confirm the chats row exists (not just any item).
        let Some(item) = self.get_item(id)? else {
            return Ok(None);
        };
        let is_chat: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM chats WHERE id = ?1",
                params![id.to_string()],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        Ok(is_chat.then_some(item))
    }

    pub fn chat_set_title(&self, id: Uuid, title: &str) -> Result<()> {
        self.rename_item(id, title)
    }

    /// Live (non-deleted) messages of a chat, oldest first.
    pub fn chat_messages(&self, chat_id: Uuid) -> Result<Vec<ChatMessageRow>> {
        let mut statement = self.conn.prepare(
            "SELECT id, chat_id, role, content, action, incomplete, edited,
                    created_at, updated_at
             FROM chat_messages
             WHERE chat_id = ?1 AND deleted_at IS NULL
             ORDER BY created_at",
        )?;
        let rows = statement.query_map(params![chat_id.to_string()], |row| {
            Ok(ChatMessageRow {
                id: parse_uuid(row.get::<_, String>(0)?),
                chat_id: parse_uuid(row.get::<_, String>(1)?),
                role: row.get(2)?,
                content: row.get(3)?,
                action: row.get(4)?,
                incomplete: row.get::<_, i64>(5)? != 0,
                edited: row.get::<_, i64>(6)? != 0,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Append a turn; bumps chat recency. Returns the new message id.
    pub fn chat_append_message(
        &self,
        chat_id: Uuid,
        role: &str,
        content: &str,
        action: Option<&str>,
        incomplete: bool,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let now = crate::bundle::now_rfc3339();
        self.conn.execute(
            "INSERT INTO chat_messages
               (id, chat_id, role, content, action, incomplete, edited, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7)",
            params![
                id.to_string(),
                chat_id.to_string(),
                role,
                content,
                action,
                incomplete as i64,
                now,
            ],
        )?;
        self.touch_item(chat_id)?;
        Ok(id)
    }

    /// Edit a message (marked `edited`; original text is overwritten but the
    /// journal-style honesty is the `edited` flag, matching per-object chat).
    pub fn chat_edit_message(&self, message_id: Uuid, content: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE chat_messages SET content = ?2, edited = 1, updated_at = ?3
             WHERE id = ?1 AND deleted_at IS NULL",
            params![message_id.to_string(), content, crate::bundle::now_rfc3339()],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(WorkspaceError::NotFound(message_id))
    }

    pub fn chat_delete_message(&self, message_id: Uuid) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE chat_messages SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
            params![message_id.to_string(), crate::bundle::now_rfc3339()],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(WorkspaceError::NotFound(message_id))
    }

    /// Reconcile a chat's reference backlinks — see `sync_refs`.
    pub fn chat_sync_refs(&self, id: Uuid, refs: &[MentionRef]) -> Result<()> {
        self.sync_refs("chat", id, refs)
    }

    // ---- refs (backlinks) --------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn add_ref(
        &self,
        source_kind: &str,
        source_id: Uuid,
        target_kind: &str,
        paper_id: Option<&str>,
        object_id: Option<Uuid>,
        url: Option<&str>,
        path: Option<&str>,
        label: Option<&str>,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        self.conn.execute(
            "INSERT INTO refs (id, source_kind, source_id, target_kind,
                               paper_id, object_id, url, path, label, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id.to_string(),
                source_kind,
                source_id.to_string(),
                target_kind,
                paper_id,
                object_id.map(|o| o.to_string()),
                url,
                path,
                label,
                crate::bundle::now_rfc3339(),
            ],
        )?;
        Ok(id)
    }

    pub fn remove_ref(&self, id: Uuid) -> Result<()> {
        self.conn
            .execute("DELETE FROM refs WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

    pub fn refs_from(&self, source_kind: &str, source_id: Uuid) -> Result<Vec<WorkspaceRef>> {
        self.query_refs(
            "SELECT id, source_kind, source_id, target_kind, paper_id, object_id,
                    url, path, label, created_at
             FROM refs WHERE source_kind = ?1 AND source_id = ?2
             ORDER BY created_at",
            params![source_kind, source_id.to_string()],
        )
    }

    /// Backlinks to a paper — tombstoned sources are excluded (a source
    /// without an `items` row, e.g. a message id, passes through).
    pub fn refs_to_paper(&self, paper_id: &str) -> Result<Vec<WorkspaceRef>> {
        self.query_refs(
            "SELECT r.id, r.source_kind, r.source_id, r.target_kind, r.paper_id,
                    r.object_id, r.url, r.path, r.label, r.created_at
             FROM refs r
             WHERE r.paper_id = ?1
               AND NOT EXISTS (SELECT 1 FROM items i
                               WHERE i.id = r.source_id AND i.deleted_at IS NOT NULL)
             ORDER BY r.created_at",
            params![paper_id],
        )
    }

    pub fn refs_to_object(&self, object_id: Uuid) -> Result<Vec<WorkspaceRef>> {
        self.query_refs(
            "SELECT r.id, r.source_kind, r.source_id, r.target_kind, r.paper_id,
                    r.object_id, r.url, r.path, r.label, r.created_at
             FROM refs r
             WHERE r.object_id = ?1
               AND NOT EXISTS (SELECT 1 FROM items i
                               WHERE i.id = r.source_id AND i.deleted_at IS NOT NULL)
             ORDER BY r.created_at",
            params![object_id.to_string()],
        )
    }

    fn query_refs(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<WorkspaceRef>> {
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params, |row| {
            Ok(WorkspaceRef {
                id: parse_uuid(row.get::<_, String>(0)?),
                source_kind: row.get(1)?,
                source_id: parse_uuid(row.get::<_, String>(2)?),
                target_kind: row.get(3)?,
                paper_id: row.get(4)?,
                object_id: row
                    .get::<_, Option<String>>(5)?
                    .map(|s| parse_uuid(s)),
                url: row.get(6)?,
                path: row.get(7)?,
                label: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- export ------------------------------------------------------------

    /// Full-fidelity JSON for one entity: registry row + its refs. Feature
    /// kinds append their content when they register richer exporters.
    pub fn export_item_json(&self, id: Uuid) -> Result<serde_json::Value> {
        let item = self.get_item(id)?.ok_or(WorkspaceError::NotFound(id))?;
        let refs = self.refs_from(&item.kind, id)?;
        Ok(serde_json::json!({ "item": item, "refs": refs }))
    }

    /// Human-readable markdown for one entity: front matter + content body
    /// (kinds with content tables) + references.
    pub fn export_item_markdown(&self, id: Uuid) -> Result<String> {
        let item = self.get_item(id)?.ok_or(WorkspaceError::NotFound(id))?;
        let refs = self.refs_from(&item.kind, id)?;
        let mut md = format!(
            "---\nkind: {}\ncreated: {}\nupdated: {}\n---\n\n# {}\n",
            item.kind, item.created_at, item.updated_at, item.title
        );
        if item.kind == "note" {
            if let Some(note) = self.note_get(id)? {
                if !note.markdown.trim().is_empty() {
                    md.push('\n');
                    md.push_str(note.markdown.trim());
                    md.push('\n');
                }
            }
        }
        if item.kind == "chat" {
            for message in self.chat_messages(id)? {
                let who = if message.role == "assistant" {
                    "Assistant"
                } else {
                    "User"
                };
                md.push_str(&format!("\n**{who}:** {}\n", message.content.trim()));
            }
        }
        if !refs.is_empty() {
            md.push_str("\n## References\n");
            for r in refs {
                let target = r
                    .label
                    .or(r.url)
                    .or(r.paper_id)
                    .or(r.path)
                    .unwrap_or_else(|| r.target_kind.clone());
                md.push_str(&format!("- {target}\n"));
            }
        }
        Ok(md)
    }

    /// Write every live entity under `dir`, grouped by kind. Returns
    /// (kind → count). No network, no AI.
    pub fn export_all(&self, dir: &Path) -> Result<Vec<(String, usize)>> {
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for item in self.list_items(None)? {
            let kind_dir = dir.join(&item.kind);
            std::fs::create_dir_all(&kind_dir).map_err(|e| WorkspaceError::Io {
                path: kind_dir.clone(),
                source: e,
            })?;
            let base = kind_dir.join(item.id.to_string());
            let json = self.export_item_json(item.id)?;
            let write = |path: PathBuf, contents: String| -> Result<()> {
                std::fs::write(&path, contents)
                    .map_err(|e| WorkspaceError::Io { path, source: e })
            };
            write(
                base.with_extension("json"),
                serde_json::to_string_pretty(&json).unwrap_or_default(),
            )?;
            write(base.with_extension("md"), self.export_item_markdown(item.id)?)?;
            // Canvases also export their scene (.excalidraw) and thumbnail
            // (.png), so the visual survives outside the app.
            if item.kind == "canvas" {
                if let Some(canvas) = self.canvas_get(item.id)? {
                    write(base.with_extension("excalidraw"), canvas.scene)?;
                    if let Some(png) = decode_png_data_url(&canvas.thumbnail) {
                        std::fs::write(base.with_extension("png"), png).map_err(|e| {
                            WorkspaceError::Io {
                                path: base.with_extension("png"),
                                source: e,
                            }
                        })?;
                    }
                }
            }
            *counts.entry(item.kind.clone()).or_default() += 1;
        }
        Ok(counts.into_iter().collect())
    }
}

/// Stored ids are always written by us as UUID strings; a corrupt row maps
/// to the nil UUID rather than poisoning whole listings.
fn parse_uuid(s: String) -> Uuid {
    Uuid::parse_str(&s).unwrap_or(Uuid::nil())
}

/// Decode a `data:image/png;base64,…` URL to raw PNG bytes; `None` for an
/// empty or non-data-URL thumbnail.
fn decode_png_data_url(data_url: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let comma = data_url.find(',')?;
    if !data_url[..comma].contains("base64") {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(&data_url[comma + 1..])
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, WorkspaceStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkspaceStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn creates_and_migrates_fresh_store() {
        let (dir, store) = store();
        assert!(dir.path().join(WORKSPACE_DB_FILE).is_file());
        let version: i64 = store
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        // Reopen: idempotent.
        drop(store);
        WorkspaceStore::open(dir.path()).unwrap();
    }

    #[test]
    fn refuses_newer_schema_untouched() {
        let (dir, store) = store();
        store
            .conn
            .pragma_update(None, "user_version", 99)
            .unwrap();
        drop(store);
        match WorkspaceStore::open(dir.path()) {
            Err(WorkspaceError::VersionTooNew { found: 99, .. }) => {}
            other => panic!("expected VersionTooNew, got {other:?}"),
        }
    }

    #[test]
    fn tombstones_hide_items_but_keep_rows() {
        let (_dir, store) = store();
        let item = store.create_item("note", "My note").unwrap();
        assert_eq!(store.list_items(None).unwrap().len(), 1);
        store.delete_item(item.id).unwrap();
        assert!(store.list_items(None).unwrap().is_empty());
        // The tombstoned row is still there for a future sync merge.
        let raw: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(raw, 1);
        // Double delete → NotFound (already tombstoned).
        assert!(matches!(
            store.delete_item(item.id),
            Err(WorkspaceError::NotFound(_))
        ));
    }

    #[test]
    fn recency_bump_and_kind_filter() {
        let (_dir, store) = store();
        let a = store.create_item("note", "A").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _b = store.create_item("canvas", "B").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        store.touch_item(a.id).unwrap();
        let all = store.list_items(None).unwrap();
        assert_eq!(all[0].id, a.id, "touched item is most recent");
        let notes = store.list_items(Some("note")).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, a.id);
    }

    #[test]
    fn refs_query_both_directions_and_respect_tombstones() {
        let (_dir, store) = store();
        let note = store.create_item("note", "N").unwrap();
        let object = Uuid::new_v4();
        store
            .add_ref(
                "note",
                note.id,
                "paper",
                Some("p1"),
                Some(object),
                None,
                None,
                Some("Attention Is All You Need"),
            )
            .unwrap();
        assert_eq!(store.refs_from("note", note.id).unwrap().len(), 1);
        assert_eq!(store.refs_to_paper("p1").unwrap().len(), 1);
        assert_eq!(store.refs_to_object(object).unwrap().len(), 1);
        store.delete_item(note.id).unwrap();
        assert!(store.refs_to_paper("p1").unwrap().is_empty());
    }

    #[test]
    fn notes_crud_and_recency() {
        let (_dir, store) = store();
        let item = store.note_create("Draft").unwrap();
        let doc = store.note_get(item.id).unwrap().unwrap();
        assert_eq!(doc.content, "[]");
        std::thread::sleep(std::time::Duration::from_millis(5));
        store
            .note_save(item.id, r#"[{"type":"paragraph"}]"#, "Hello world")
            .unwrap();
        let doc = store.note_get(item.id).unwrap().unwrap();
        assert_eq!(doc.markdown, "Hello world");
        assert!(doc.item.updated_at > item.updated_at, "save bumps recency");
        // Tombstoned notes read as gone.
        store.delete_item(item.id).unwrap();
        assert!(store.note_get(item.id).unwrap().is_none());
    }

    #[test]
    fn migrates_v1_store_to_v2() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = WorkspaceStore::open(dir.path()).unwrap();
            // Rewind to v1 by dropping the notes table and version stamp.
            store
                .conn
                .execute_batch("DROP TABLE notes; PRAGMA user_version = 1;")
                .unwrap();
        }
        let store = WorkspaceStore::open(dir.path()).unwrap();
        store.note_create("post-migration note").unwrap();
        assert_eq!(store.list_items(Some("note")).unwrap().len(), 1);
    }

    #[test]
    fn migrates_v2_store_to_v3() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = WorkspaceStore::open(dir.path()).unwrap();
            store
                .conn
                .execute_batch("DROP TABLE canvases; PRAGMA user_version = 2;")
                .unwrap();
        }
        let store = WorkspaceStore::open(dir.path()).unwrap();
        store.canvas_create("post-migration canvas").unwrap();
        assert_eq!(store.list_items(Some("canvas")).unwrap().len(), 1);
    }

    #[test]
    fn canvas_crud_and_recency() {
        let (_dir, store) = store();
        let item = store.canvas_create("Sketch").unwrap();
        let doc = store.canvas_get(item.id).unwrap().unwrap();
        assert_eq!(doc.scene, "{}");
        std::thread::sleep(std::time::Duration::from_millis(5));
        store
            .canvas_save(item.id, r#"{"elements":[]}"#, "data:image/png;base64,AAA")
            .unwrap();
        let doc = store.canvas_get(item.id).unwrap().unwrap();
        assert_eq!(doc.scene, r#"{"elements":[]}"#);
        assert!(doc.item.updated_at > item.updated_at, "save bumps recency");
        store.delete_item(item.id).unwrap();
        assert!(store.canvas_get(item.id).unwrap().is_none());
    }

    #[test]
    fn canvas_pin_refs_reconcile() {
        let (_dir, store) = store();
        let canvas = store.canvas_create("C").unwrap();
        let object = Uuid::new_v4();
        let pin = |paper: &str, object: Option<Uuid>| MentionRef {
            paper_id: Some(paper.to_string()),
            object_id: object,
            label: None,
        };
        store
            .canvas_sync_refs(canvas.id, &[pin("p1", None), pin("p1", Some(object))])
            .unwrap();
        assert_eq!(store.refs_from("canvas", canvas.id).unwrap().len(), 2);
        assert_eq!(store.refs_to_paper("p1").unwrap().len(), 2);
        store.canvas_sync_refs(canvas.id, &[pin("p1", Some(object))]).unwrap();
        assert_eq!(store.refs_from("canvas", canvas.id).unwrap().len(), 1);
    }

    #[test]
    fn migrates_v3_store_to_v4() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = WorkspaceStore::open(dir.path()).unwrap();
            store
                .conn
                .execute_batch(
                    "DROP TABLE chat_messages; DROP TABLE chats; PRAGMA user_version = 3;",
                )
                .unwrap();
        }
        let store = WorkspaceStore::open(dir.path()).unwrap();
        store.chat_create("post-migration chat").unwrap();
        assert_eq!(store.list_items(Some("chat")).unwrap().len(), 1);
    }

    #[test]
    fn chat_messages_append_edit_delete() {
        let (_dir, store) = store();
        let chat = store.chat_create("New chat").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let u = store
            .chat_append_message(chat.id, "user", "What is attention?", Some("ask"), false)
            .unwrap();
        let _a = store
            .chat_append_message(chat.id, "assistant", "It is a mechanism…", None, false)
            .unwrap();
        let msgs = store.chat_messages(chat.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert!(msgs[1].content.starts_with("It is"));
        // Append bumps chat recency.
        let refreshed = store.get_item(chat.id).unwrap().unwrap();
        assert!(refreshed.updated_at > chat.updated_at);
        // Edit marks, delete tombstones.
        store.chat_edit_message(u, "What exactly is attention?").unwrap();
        assert!(store.chat_messages(chat.id).unwrap()[0].edited);
        store.chat_delete_message(u).unwrap();
        assert_eq!(store.chat_messages(chat.id).unwrap().len(), 1);
    }

    #[test]
    fn chat_export_transcript_ordered() {
        let (_dir, store) = store();
        let chat = store.chat_create("Discussion").unwrap();
        store
            .chat_append_message(chat.id, "user", "Hello", None, false)
            .unwrap();
        store
            .chat_append_message(chat.id, "assistant", "Hi there", None, false)
            .unwrap();
        let md = store.export_item_markdown(chat.id).unwrap();
        let user_at = md.find("**User:** Hello").unwrap();
        let asst_at = md.find("**Assistant:** Hi there").unwrap();
        assert!(user_at < asst_at, "turns are ordered");
    }

    #[test]
    fn canvas_export_writes_scene_and_png() {
        let (_dir, store) = store();
        let item = store.canvas_create("My board").unwrap();
        // "hello" base64-encoded PNG stand-in.
        store
            .canvas_save(
                item.id,
                r#"{"elements":[{"type":"rectangle"}]}"#,
                "data:image/png;base64,aGVsbG8=",
            )
            .unwrap();
        let out = tempfile::tempdir().unwrap();
        store.export_all(out.path()).unwrap();
        let base = out.path().join("canvas").join(item.id.to_string());
        assert!(base.with_extension("excalidraw").is_file());
        assert_eq!(
            std::fs::read(base.with_extension("png")).unwrap(),
            b"hello"
        );
    }

    #[test]
    fn mention_refs_reconcile_by_diff() {
        let (_dir, store) = store();
        let note = store.note_create("N").unwrap();
        let object = Uuid::new_v4();
        let mention = |paper: &str, object: Option<Uuid>| MentionRef {
            paper_id: Some(paper.to_string()),
            object_id: object,
            label: None,
        };
        store
            .note_sync_refs(note.id, &[mention("p1", None), mention("p1", Some(object))])
            .unwrap();
        assert_eq!(store.refs_from("note", note.id).unwrap().len(), 2);
        // Removing one mention and re-syncing drops its row, keeps the other.
        store
            .note_sync_refs(note.id, &[mention("p1", Some(object))])
            .unwrap();
        let remaining = store.refs_from("note", note.id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].object_id, Some(object));
        // Idempotent.
        store
            .note_sync_refs(note.id, &[mention("p1", Some(object))])
            .unwrap();
        assert_eq!(store.refs_from("note", note.id).unwrap().len(), 1);
    }

    #[test]
    fn note_export_contains_content() {
        let (_dir, store) = store();
        let item = store.note_create("My study note").unwrap();
        store
            .note_save(item.id, "[]", "## Findings\n\n- alpha\n- beta")
            .unwrap();
        let md = store.export_item_markdown(item.id).unwrap();
        assert!(md.contains("# My study note"));
        assert!(md.contains("- alpha"));
    }

    #[test]
    fn export_all_groups_by_kind_and_counts() {
        let (_dir, store) = store();
        let note = store.create_item("note", "My note").unwrap();
        store.create_item("canvas", "My canvas").unwrap();
        store
            .add_ref("note", note.id, "url", None, None, Some("https://x.y"), None, None)
            .unwrap();
        let out = tempfile::tempdir().unwrap();
        let counts = store.export_all(out.path()).unwrap();
        assert_eq!(
            counts,
            vec![("canvas".to_string(), 1), ("note".to_string(), 1)]
        );
        assert!(out
            .path()
            .join("note")
            .join(format!("{}.md", note.id))
            .is_file());
        let md = std::fs::read_to_string(
            out.path().join("note").join(format!("{}.md", note.id)),
        )
        .unwrap();
        assert!(md.contains("# My note"));
        assert!(md.contains("https://x.y"));
    }
}
