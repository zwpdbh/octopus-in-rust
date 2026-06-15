use crate::types::SourceReference;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Persistent SQLite store for docref.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open or create the store at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path).context("failed to open SQLite database")?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Open an in-memory store (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory SQLite")?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS docs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                last_scanned_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS snippets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
                source_path TEXT NOT NULL,
                item_name TEXT NOT NULL,
                doc_line INTEGER NOT NULL,
                annotation TEXT,
                snippet_body TEXT NOT NULL,
                snippet_checksum TEXT NOT NULL,
                current_line INTEGER,
                current_checksum TEXT,
                last_verified_at INTEGER,
                drift_detected_at INTEGER
            );

            CREATE INDEX IF NOT EXISTS idx_snippets_source ON snippets(source_path);
            CREATE INDEX IF NOT EXISTS idx_snippets_doc ON snippets(doc_id);
            ",
        )?;
        Ok(())
    }

    pub fn record_scan(&self, doc_path: &Path, refs: &[SourceReference]) -> Result<()> {
        let now = now_secs();
        self.conn.execute(
            "INSERT INTO docs (path, last_scanned_at) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET last_scanned_at=excluded.last_scanned_at",
            params![doc_path.to_string_lossy(), now],
        )?;

        let doc_id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM docs WHERE path = ?1",
                params![doc_path.to_string_lossy()],
                |row| row.get(0),
            )
            .context("failed to retrieve doc id")?;

        // Remove existing snippets for this doc and re-insert.
        self.conn
            .execute("DELETE FROM snippets WHERE doc_id = ?1", params![doc_id])?;

        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO snippets
                 (doc_id, source_path, item_name, doc_line, annotation, snippet_body, snippet_checksum)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for r in refs {
                stmt.execute(params![
                    doc_id,
                    r.source_path.to_string_lossy(),
                    r.item_name,
                    r.doc_line as i64,
                    r.annotation.as_deref().unwrap_or(""),
                    &r.snippet_body,
                    &r.snippet_checksum,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Return all snippets that reference any of the given source paths.
    pub fn get_snippets_for_sources(&self, sources: &[PathBuf]) -> Result<Vec<StoredSnippet>> {
        if sources.is_empty() {
            return Ok(Vec::new());
        }

        let paths: Vec<String> = sources
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let placeholders: Vec<String> = (1..=paths.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT
                s.id,
                d.path,
                s.source_path,
                s.item_name,
                s.doc_line,
                s.annotation,
                s.snippet_body,
                s.snippet_checksum,
                s.current_line,
                s.current_checksum,
                s.last_verified_at,
                s.drift_detected_at
             FROM snippets s
             JOIN docs d ON d.id = s.doc_id
             WHERE s.source_path IN ({})",
            placeholders.join(",")
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(paths.iter()), |row| {
            Ok(StoredSnippet {
                id: row.get(0)?,
                doc_path: PathBuf::from(row.get::<_, String>(1)?),
                source_path: PathBuf::from(row.get::<_, String>(2)?),
                item_name: row.get(3)?,
                doc_line: row.get::<_, i64>(4)? as usize,
                annotation: row.get::<_, Option<String>>(5)?,
                snippet_body: row.get(6)?,
                snippet_checksum: row.get(7)?,
                current_line: row.get::<_, Option<i64>>(8)?.map(|n| n as usize),
                current_checksum: row.get::<_, Option<String>>(9)?,
                last_verified_at: row.get::<_, Option<i64>>(10)?,
                drift_detected_at: row.get::<_, Option<i64>>(11)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Return every stored snippet.
    pub fn get_all_snippets(&self) -> Result<Vec<StoredSnippet>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                s.id,
                d.path,
                s.source_path,
                s.item_name,
                s.doc_line,
                s.annotation,
                s.snippet_body,
                s.snippet_checksum,
                s.current_line,
                s.current_checksum,
                s.last_verified_at,
                s.drift_detected_at
             FROM snippets s
             JOIN docs d ON d.id = s.doc_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StoredSnippet {
                id: row.get(0)?,
                doc_path: PathBuf::from(row.get::<_, String>(1)?),
                source_path: PathBuf::from(row.get::<_, String>(2)?),
                item_name: row.get(3)?,
                doc_line: row.get::<_, i64>(4)? as usize,
                annotation: row.get::<_, Option<String>>(5)?,
                snippet_body: row.get(6)?,
                snippet_checksum: row.get(7)?,
                current_line: row.get::<_, Option<i64>>(8)?.map(|n| n as usize),
                current_checksum: row.get::<_, Option<String>>(9)?,
                last_verified_at: row.get::<_, Option<i64>>(10)?,
                drift_detected_at: row.get::<_, Option<i64>>(11)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn update_verification(
        &self,
        snippet_id: i64,
        current_line: Option<usize>,
        current_checksum: Option<&str>,
        drift_detected: bool,
    ) -> Result<()> {
        let now = now_secs();
        let drift_at = if drift_detected { Some(now) } else { None };
        self.conn.execute(
            "UPDATE snippets
             SET current_line = ?1,
                 current_checksum = ?2,
                 last_verified_at = ?3,
                 drift_detected_at = ?4
             WHERE id = ?5",
            params![
                current_line.map(|n| n as i64),
                current_checksum,
                now,
                drift_at,
                snippet_id,
            ],
        )?;
        Ok(())
    }

    pub fn doc_id(&self, doc_path: &Path) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT id FROM docs WHERE path = ?1",
                params![doc_path.to_string_lossy()],
                |row| row.get(0),
            )
            .optional()
            .context("failed to look up doc")
    }

    pub fn get_doc_paths(&self) -> Result<Vec<PathBuf>> {
        let mut stmt = self.conn.prepare("SELECT path FROM docs")?;
        let rows = stmt.query_map([], |row| {
            let s: String = row.get(0)?;
            Ok(PathBuf::from(s))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct StoredSnippet {
    pub id: i64,
    pub doc_path: PathBuf,
    pub source_path: PathBuf,
    pub item_name: String,
    pub doc_line: usize,
    pub annotation: Option<String>,
    pub snippet_body: String,
    pub snippet_checksum: String,
    pub current_line: Option<usize>,
    pub current_checksum: Option<String>,
    pub last_verified_at: Option<i64>,
    pub drift_detected_at: Option<i64>,
}

impl StoredSnippet {
    pub fn to_source_reference(&self) -> SourceReference {
        SourceReference {
            doc_path: self.doc_path.clone(),
            source_path: self.source_path.clone(),
            doc_line: self.doc_line,
            item_name: self.item_name.clone(),
            annotation: self.annotation.clone(),
            snippet_body: self.snippet_body.clone(),
            snippet_checksum: self.snippet_checksum.clone(),
        }
    }
}

pub fn checksum(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
