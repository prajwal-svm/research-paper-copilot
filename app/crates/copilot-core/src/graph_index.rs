//! Library-level concept-graph index: `graph.db` (SQLite) at the library
//! root, mirroring every bundle's `knowledge_graph.json` for cross-paper
//! queries and O(1) neighborhoods.
//!
//! The index is a rebuildable cache, never a source of truth — the bundle
//! JSON stays the portable contract. A schema-version stamp guards drift:
//! on mismatch the whole file is dropped and rebuilt from bundles.

use std::path::Path;

use rusqlite::Connection;
use uuid::Uuid;

use crate::concepts::KnowledgeGraph;

/// Bump when the table shape changes; a mismatched stamp triggers a rebuild.
const GRAPH_INDEX_SCHEMA: i64 = 1;

pub const GRAPH_DB_FILE: &str = "graph.db";

#[derive(Debug, thiserror::Error)]
pub enum GraphIndexError {
    #[error("graph index: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error(transparent)]
    Library(#[from] crate::library::LibraryError),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexedNode {
    pub paper_id: String,
    pub id: Uuid,
    pub name: String,
    pub confidence: f32,
    /// Hop distance from the query node (0 = the node itself).
    pub distance: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexedEdge {
    pub paper_id: String,
    pub from: Uuid,
    pub to: Uuid,
    pub kind: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Neighborhood {
    pub nodes: Vec<IndexedNode>,
    pub edges: Vec<IndexedEdge>,
}

pub struct GraphIndex {
    conn: Connection,
}

impl GraphIndex {
    /// Open (or create) `graph.db` under the library root. A stale schema
    /// stamp wipes the tables — callers should follow up with a rebuild.
    pub fn open(library_root: &Path) -> Result<Self, GraphIndexError> {
        let conn = Connection::open(library_root.join(GRAPH_DB_FILE))?;
        let mut index = GraphIndex { conn };
        if index.schema_stamp()? != Some(GRAPH_INDEX_SCHEMA) {
            index.reset_schema()?;
        }
        Ok(index)
    }

    #[cfg(test)]
    fn open_in_memory() -> Result<Self, GraphIndexError> {
        let conn = Connection::open_in_memory()?;
        let mut index = GraphIndex { conn };
        index.reset_schema()?;
        Ok(index)
    }

    fn schema_stamp(&self) -> Result<Option<i64>, GraphIndexError> {
        let has_meta: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta')",
            [],
            |r| r.get(0),
        )?;
        if !has_meta {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| {
                r.get(0)
            })
            .ok())
    }

    fn reset_schema(&mut self) -> Result<(), GraphIndexError> {
        self.conn.execute_batch(&format!(
            "DROP TABLE IF EXISTS meta;
             DROP TABLE IF EXISTS nodes;
             DROP TABLE IF EXISTS edges;
             CREATE TABLE meta(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             CREATE TABLE nodes(
               paper_id   TEXT NOT NULL,
               id         TEXT NOT NULL,
               name       TEXT NOT NULL,
               confidence REAL NOT NULL,
               PRIMARY KEY(paper_id, id)
             );
             CREATE INDEX nodes_by_id ON nodes(id);
             CREATE TABLE edges(
               paper_id   TEXT NOT NULL,
               from_id    TEXT NOT NULL,
               to_id      TEXT NOT NULL,
               kind       TEXT NOT NULL,
               confidence REAL NOT NULL
             );
             CREATE INDEX edges_by_from ON edges(from_id);
             CREATE INDEX edges_by_to ON edges(to_id);
             INSERT INTO meta(key, value) VALUES('schema', {GRAPH_INDEX_SCHEMA});"
        ))?;
        Ok(())
    }

    /// Replace one paper's rows with its current graph (idempotent).
    pub fn index_paper(
        &mut self,
        paper_id: &str,
        graph: &KnowledgeGraph,
    ) -> Result<(), GraphIndexError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM nodes WHERE paper_id = ?1", [paper_id])?;
        tx.execute("DELETE FROM edges WHERE paper_id = ?1", [paper_id])?;
        {
            let mut insert_node = tx.prepare(
                "INSERT INTO nodes(paper_id, id, name, confidence) VALUES(?1, ?2, ?3, ?4)",
            )?;
            for node in &graph.nodes {
                insert_node.execute(rusqlite::params![
                    paper_id,
                    node.id.to_string(),
                    node.name,
                    node.confidence,
                ])?;
            }
            let mut insert_edge = tx.prepare(
                "INSERT INTO edges(paper_id, from_id, to_id, kind, confidence)
                 VALUES(?1, ?2, ?3, ?4, ?5)",
            )?;
            for edge in &graph.edges {
                insert_edge.execute(rusqlite::params![
                    paper_id,
                    edge.from.to_string(),
                    edge.to.to_string(),
                    edge.kind.as_str(),
                    edge.confidence,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_paper(&mut self, paper_id: &str) -> Result<(), GraphIndexError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM nodes WHERE paper_id = ?1", [paper_id])?;
        tx.execute("DELETE FROM edges WHERE paper_id = ?1", [paper_id])?;
        tx.commit()?;
        Ok(())
    }

    /// Full rebuild from every bundle's `knowledge_graph.json`. Returns the
    /// number of papers indexed. Bundles without a graph are skipped.
    pub fn rebuild(&mut self, library: &crate::library::Library) -> Result<usize, GraphIndexError> {
        self.reset_schema()?;
        let mut indexed = 0;
        for summary in library.list()? {
            let bundle = library.get(&summary.id)?;
            let Ok(Some(graph)) =
                bundle.read_derived_json::<KnowledgeGraph>("knowledge_graph.json")
            else {
                continue;
            };
            self.index_paper(&summary.id, &graph)?;
            indexed += 1;
        }
        Ok(indexed)
    }

    /// All edges of one kind across every indexed paper (v4 gap analysis:
    /// library-wide `contradicts` edges).
    pub fn edges_of_kind(&self, kind: &str) -> Result<Vec<IndexedEdge>, GraphIndexError> {
        let mut query = self.conn.prepare_cached(
            "SELECT paper_id, from_id, to_id, kind, confidence FROM edges WHERE kind = ?1",
        )?;
        let rows = query.query_map([kind], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, f64>(4)?,
            ))
        })?;
        let mut edges = Vec::new();
        for row in rows {
            let (paper_id, from, to, kind, confidence) = row?;
            edges.push(IndexedEdge {
                paper_id,
                from: Uuid::parse_str(&from).unwrap_or_default(),
                to: Uuid::parse_str(&to).unwrap_or_default(),
                kind,
                confidence: confidence as f32,
            });
        }
        Ok(edges)
    }

    /// Edges of the given kinds incident to a node within one paper (v4
    /// lineage enrichment: how a paper `extends`/`cites` around a concept).
    pub fn incident_edges(
        &self,
        paper_id: &str,
        node: Uuid,
        kinds: &[&str],
    ) -> Result<Vec<IndexedEdge>, GraphIndexError> {
        let node_key = node.to_string();
        let mut query = self.conn.prepare_cached(
            "SELECT paper_id, from_id, to_id, kind, confidence FROM edges
             WHERE paper_id = ?1 AND (from_id = ?2 OR to_id = ?2)",
        )?;
        let rows = query.query_map(rusqlite::params![paper_id, node_key], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, f64>(4)?,
            ))
        })?;
        let mut edges = Vec::new();
        for row in rows {
            let (paper_id, from, to, kind, confidence) = row?;
            if !kinds.is_empty() && !kinds.contains(&kind.as_str()) {
                continue;
            }
            edges.push(IndexedEdge {
                paper_id,
                from: Uuid::parse_str(&from).unwrap_or_default(),
                to: Uuid::parse_str(&to).unwrap_or_default(),
                kind,
                confidence: confidence as f32,
            });
        }
        Ok(edges)
    }

    /// Nodes and edges within `hops` undirected hops of `node_id` (across
    /// papers — node ids are deterministic per paper, so a cross-paper query
    /// walks each paper's copy). Budget: <5 ms (perf suite).
    pub fn neighborhood(&self, node_id: Uuid, hops: u32) -> Result<Neighborhood, GraphIndexError> {
        use std::collections::{HashMap, HashSet};

        let mut distances: HashMap<String, u32> = HashMap::new();
        let mut frontier: Vec<String> = vec![node_id.to_string()];
        distances.insert(node_id.to_string(), 0);

        let mut adjacent = self.conn.prepare_cached(
            "SELECT to_id FROM edges WHERE from_id = ?1
             UNION SELECT from_id FROM edges WHERE to_id = ?1",
        )?;
        for depth in 1..=hops {
            let mut next = Vec::new();
            for id in &frontier {
                let rows = adjacent.query_map([id], |r| r.get::<_, String>(0))?;
                for neighbor in rows {
                    let neighbor = neighbor?;
                    if !distances.contains_key(&neighbor) {
                        distances.insert(neighbor.clone(), depth);
                        next.push(neighbor);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        let ids: HashSet<&String> = distances.keys().collect();
        let mut nodes = Vec::new();
        let mut node_query = self
            .conn
            .prepare_cached("SELECT paper_id, name, confidence FROM nodes WHERE id = ?1")?;
        for (id, distance) in &distances {
            let rows = node_query.query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })?;
            for row in rows {
                let (paper_id, name, confidence) = row?;
                nodes.push(IndexedNode {
                    paper_id,
                    id: Uuid::parse_str(id).unwrap_or_default(),
                    name,
                    confidence: confidence as f32,
                    distance: *distance,
                });
            }
        }
        nodes.sort_by(|a, b| a.distance.cmp(&b.distance).then(a.name.cmp(&b.name)));

        // Induced edges: both ends inside the neighborhood.
        let mut edges = Vec::new();
        let mut edge_query = self.conn.prepare_cached(
            "SELECT paper_id, to_id, kind, confidence FROM edges WHERE from_id = ?1",
        )?;
        for from in &ids {
            let rows = edge_query.query_map([from.as_str()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, f64>(3)?,
                ))
            })?;
            for row in rows {
                let (paper_id, to, kind, confidence) = row?;
                if !ids.contains(&to) {
                    continue;
                }
                edges.push(IndexedEdge {
                    paper_id,
                    from: Uuid::parse_str(from).unwrap_or_default(),
                    to: Uuid::parse_str(&to).unwrap_or_default(),
                    kind,
                    confidence: confidence as f32,
                });
            }
        }
        Ok(Neighborhood { nodes, edges })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::{concept_id, ConceptEdge, ConceptNode, EdgeKind};

    fn graph(paper: &str, names: &[&str], edges: &[(usize, usize)]) -> KnowledgeGraph {
        let nodes: Vec<ConceptNode> = names
            .iter()
            .map(|n| ConceptNode {
                id: concept_id(paper, n),
                name: n.to_string(),
                description: None,
                object_ids: vec![],
                confidence: 0.8,
            })
            .collect();
        let edges = edges
            .iter()
            .map(|&(a, b)| ConceptEdge {
                from: nodes[a].id,
                to: nodes[b].id,
                kind: EdgeKind::DependsOn,
                confidence: 0.8,
            })
            .collect();
        KnowledgeGraph {
            pipeline_version: crate::concepts::CONCEPTS_PIPELINE_VERSION.to_string(),
            extraction: "llm".to_string(),
            nodes,
            edges,
        }
    }

    #[test]
    fn neighborhood_respects_hop_limit() {
        let mut index = GraphIndex::open_in_memory().unwrap();
        // chain: a -> b -> c -> d
        let g = graph("p1", &["a", "b", "c", "d"], &[(0, 1), (1, 2), (2, 3)]);
        index.index_paper("p1", &g).unwrap();

        let hood = index.neighborhood(g.nodes[0].id, 2).unwrap();
        let names: Vec<&str> = hood.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]); // sorted by distance
        assert_eq!(hood.edges.len(), 2); // a->b, b->c; c->d excluded
        assert_eq!(hood.nodes[2].distance, 2);
    }

    #[test]
    fn reindex_replaces_paper_rows() {
        let mut index = GraphIndex::open_in_memory().unwrap();
        index
            .index_paper("p1", &graph("p1", &["a", "b"], &[(0, 1)]))
            .unwrap();
        let updated = graph("p1", &["a"], &[]);
        index.index_paper("p1", &updated).unwrap();

        let hood = index.neighborhood(updated.nodes[0].id, 2).unwrap();
        assert_eq!(hood.nodes.len(), 1);
        assert!(hood.edges.is_empty());
    }

    #[test]
    fn remove_paper_clears_rows() {
        let mut index = GraphIndex::open_in_memory().unwrap();
        let g = graph("p1", &["a", "b"], &[(0, 1)]);
        index.index_paper("p1", &g).unwrap();
        index.remove_paper("p1").unwrap();
        let hood = index.neighborhood(g.nodes[0].id, 2).unwrap();
        assert!(hood.nodes.is_empty());
    }

    #[test]
    fn stale_schema_stamp_resets_on_open() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let index = GraphIndex::open(tmp.path()).unwrap();
            index
                .conn
                .execute("UPDATE meta SET value = 0 WHERE key='schema'", [])
                .unwrap();
        }
        // Reopen: stamp mismatch → fresh schema (no rows, current stamp).
        let index = GraphIndex::open(tmp.path()).unwrap();
        assert_eq!(index.schema_stamp().unwrap(), Some(GRAPH_INDEX_SCHEMA));
        let count: i64 = index
            .conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
