//! History storage — SQLite at `~/.apiark/data.db`, ported from
//! `apps/desktop/src-tauri/src/storage/history.rs`.
//!
//! Same database file as the Tauri version, so switching clients preserves history.

use crate::models::HistoryEntry;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

impl HistoryStore {
    /// Open (or create) the history database, creating the schema if needed.
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {e}"))?;
        conn.execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(|e| e.to_string())?;

        // Corruption guard: if integrity check fails, back up and start fresh.
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap_or_else(|_| "error".into());
        let conn = if integrity != "ok" {
            tracing::warn!("History DB corrupt — backing up and recreating");
            let backup = db_path.with_extension("db.corrupt");
            let _ = std::fs::rename(db_path, &backup);
            Connection::open(db_path).map_err(|e| e.to_string())?
        } else {
            conn
        };

        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    /// Open the default database (`~/.apiark/data.db`).
    pub fn open_default() -> Result<Self, String> {
        let path = dirs::home_dir()
            .ok_or("Could not determine home directory")?
            .join(".apiark")
            .join("data.db");
        Self::open(&path)
    }

    fn migrate(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                method TEXT NOT NULL,
                url TEXT NOT NULL,
                status INTEGER,
                status_text TEXT,
                time_ms INTEGER,
                size_bytes INTEGER,
                timestamp TEXT NOT NULL,
                collection_path TEXT,
                request_name TEXT,
                request_json TEXT NOT NULL DEFAULT '{}',
                response_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_history_url ON history(url);
            CREATE INDEX IF NOT EXISTS idx_history_method ON history(method);",
        )
        .map_err(|e| format!("Migration failed: {e}"))
    }

    fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
        Ok(HistoryEntry {
            id: row.get(0)?,
            method: row.get(1)?,
            url: row.get(2)?,
            status: row.get(3)?,
            status_text: row.get(4)?,
            time_ms: row.get(5)?,
            size_bytes: row.get(6)?,
            timestamp: row.get(7)?,
            collection_path: row.get(8)?,
            request_name: row.get(9)?,
            request_json: row.get(10)?,
            response_json: row.get(11)?,
        })
    }

    const SELECT_COLUMNS: &'static str =
        "id, method, url, status, status_text, time_ms, size_bytes, timestamp, \
         collection_path, request_name, request_json, response_json";

    /// Insert a history entry and return its id.
    pub fn insert(&self, entry: &HistoryEntry) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO history (method, url, status, status_text, time_ms, size_bytes,
                                  timestamp, collection_path, request_name, request_json, response_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.method,
                entry.url,
                entry.status,
                entry.status_text,
                entry.time_ms,
                entry.size_bytes,
                entry.timestamp,
                entry.collection_path,
                entry.request_name,
                entry.request_json,
                entry.response_json,
            ],
        )
        .map_err(|e| format!("Failed to insert history: {e}"))?;
        let id = conn.last_insert_rowid();
        // Keep the database bounded: each row can hold a few hundred KB of
        // request/response JSON, so an unbounded table grows without limit.
        conn.execute(
            "DELETE FROM history WHERE id NOT IN (
                SELECT id FROM history ORDER BY id DESC LIMIT 2000
             )",
            [],
        )
        .map_err(|e| format!("Failed to trim history: {e}"))?;
        Ok(id)
    }

    /// List recent entries (newest first).
    pub fn list(&self, limit: i64, offset: i64) -> Result<Vec<HistoryEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM history ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2",
            Self::SELECT_COLUMNS
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit, offset], |row| Self::row_to_entry(row))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Search by URL / method / request name substring.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<HistoryEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let pattern = format!("%{query}%");
        let sql = format!(
            "SELECT {} FROM history
             WHERE url LIKE ?1 OR method LIKE ?1 OR request_name LIKE ?1
             ORDER BY timestamp DESC LIMIT ?2",
            Self::SELECT_COLUMNS
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![pattern, limit], |row| Self::row_to_entry(row))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn delete(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM history", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_query_history() {
        let store = HistoryStore::open(Path::new(":memory:")).unwrap();
        let entry = HistoryEntry {
            id: 0,
            method: "GET".into(),
            url: "https://example.com".into(),
            status: Some(200),
            status_text: Some("OK".into()),
            time_ms: Some(42),
            size_bytes: Some(128),
            timestamp: "2026-01-01T00:00:00Z".into(),
            collection_path: None,
            request_name: Some("Test".into()),
            request_json: "{}".into(),
            response_json: None,
        };
        let id = store.insert(&entry).unwrap();
        assert!(id > 0);

        let list = store.list(10, 0).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].url, "https://example.com");

        let found = store.search("exam", 10).unwrap();
        assert_eq!(found.len(), 1);

        store.delete(id).unwrap();
        assert!(store.list(10, 0).unwrap().is_empty());
    }
}
