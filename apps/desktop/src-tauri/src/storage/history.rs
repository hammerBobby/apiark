use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::models::request::KeyValuePair;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub method: String,
    pub url: String,
    pub status: Option<i32>,
    pub status_text: Option<String>,
    pub time_ms: Option<i64>,
    pub size_bytes: Option<i64>,
    pub timestamp: String,
    pub collection_path: Option<String>,
    pub request_name: Option<String>,
    /// Full request snapshot (headers, params, body, scripts, ...).
    pub request_json: String,
    /// Full response snapshot (status, headers, cookies, body, ...).
    pub response_json: Option<String>,
}

/// Read the column names of a table (used to upgrade databases created by
/// older versions of the app).
fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| format!("Failed to inspect {table} schema: {e}"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("Failed to inspect {table} schema: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to inspect {table} schema: {e}"))?;
    Ok(columns)
}

/// Convert the legacy `request_headers` JSON object into key/value rows.
fn legacy_headers(raw: Option<String>) -> Vec<KeyValuePair> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    map.into_iter()
        .map(|(key, value)| {
            KeyValuePair::new(key, value.as_str().unwrap_or("").to_string(), true)
        })
        .collect()
}

/// Move data from the old `request_headers` / `request_body` / `response_body`
/// columns into the JSON columns used by the current version, so old history
/// entries keep their headers, body and response instead of showing up blank.
fn backfill_legacy_history(conn: &Connection, columns: &[String]) -> Result<(), String> {
    let has_headers = columns.iter().any(|c| c == "request_headers");
    let has_request_body = columns.iter().any(|c| c == "request_body");
    let has_response_body = columns.iter().any(|c| c == "response_body");
    if !has_headers && !has_request_body && !has_response_body {
        return Ok(());
    }

    let header_col = if has_headers { "request_headers" } else { "NULL" };
    let body_col = if has_request_body { "request_body" } else { "NULL" };
    let response_col = if has_response_body { "response_body" } else { "NULL" };
    let sql = format!(
        "SELECT id, url, {header_col}, {body_col}, {response_col}, status, status_text, time_ms, size_bytes
         FROM history WHERE request_json IS NULL OR request_json = ''"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed to read legacy history: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i32>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<i64>>(8)?,
            ))
        })
        .map_err(|e| format!("Failed to read legacy history: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read legacy history: {e}"))?;
    drop(stmt);

    for (id, url, headers, request_body, response_body, status, status_text, time_ms, size_bytes) in
        rows
    {
        let body_json = request_body
            .filter(|s| !s.is_empty())
            .map(|content| {
                serde_json::json!({ "type": "raw", "content": content, "formData": [] })
            });
        let request_json = serde_json::json!({
            "url": url,
            "headers": legacy_headers(headers),
            "params": [],
            "body": body_json,
        })
        .to_string();

        let response_json = (status.is_some() || response_body.is_some()).then(|| {
            serde_json::json!({
                "status": status.unwrap_or(0),
                "statusText": status_text.unwrap_or_default(),
                "headers": [],
                "cookies": [],
                "body": response_body.unwrap_or_default(),
                "timeMs": time_ms.unwrap_or(0),
                "sizeBytes": size_bytes.unwrap_or(0),
            })
            .to_string()
        });

        conn.execute(
            "UPDATE history SET request_json = ?1, response_json = COALESCE(response_json, ?2) WHERE id = ?3",
            params![request_json, response_json, id],
        )
        .map_err(|e| format!("Failed to backfill history entry {id}: {e}"))?;
    }

    Ok(())
}

pub struct HistoryDb {
    conn: Mutex<Connection>,
}

impl HistoryDb {
    /// Open (or create) the history database at the given path.
    pub fn open(db_path: &Path) -> Result<Self, String> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create data directory: {e}"))?;
        }

        let conn =
            Connection::open(db_path).map_err(|e| format!("Failed to open database: {e}"))?;

        // Enable WAL mode for crash resilience
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| format!("Failed to set WAL mode: {e}"))?;

        // Integrity check
        let integrity: String = conn
            .query_row("PRAGMA integrity_check;", [], |row| row.get(0))
            .map_err(|e| format!("Integrity check failed: {e}"))?;

        if integrity != "ok" {
            tracing::error!("Database integrity check failed: {integrity}");
            return Err(format!("Database corruption detected: {integrity}"));
        }

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Run database migrations.
    fn migrate(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
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
                request_json TEXT NOT NULL,
                response_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_history_url ON history(url);
            CREATE INDEX IF NOT EXISTS idx_history_method ON history(method);

            CREATE TABLE IF NOT EXISTS monitor_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                monitor_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                total_requests INTEGER NOT NULL,
                total_passed INTEGER NOT NULL,
                total_failed INTEGER NOT NULL,
                total_time_ms INTEGER NOT NULL,
                status TEXT NOT NULL,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_monitor_results_monitor ON monitor_results(monitor_id, timestamp DESC);",
        )
        .map_err(|e| format!("Migration failed: {e}"))?;

        // Older databases were created without the JSON columns. Without these
        // ALTERs every new INSERT would fail ("no column named request_json")
        // and the history would silently stop recording new requests.
        let columns = table_columns(&conn, "history")?;
        if !columns.iter().any(|c| c == "request_json") {
            conn.execute_batch(
                "ALTER TABLE history ADD COLUMN request_json TEXT NOT NULL DEFAULT '';",
            )
            .map_err(|e| format!("Failed to add request_json column: {e}"))?;
        }
        if !columns.iter().any(|c| c == "response_json") {
            conn.execute_batch("ALTER TABLE history ADD COLUMN response_json TEXT;")
                .map_err(|e| format!("Failed to add response_json column: {e}"))?;
        }
        backfill_legacy_history(&conn, &columns)?;

        Ok(())
    }

    /// Insert a new history entry.
    pub fn insert(&self, entry: &HistoryEntry) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
        conn.execute(
            "INSERT INTO history (method, url, status, status_text, time_ms, size_bytes, timestamp, collection_path, request_name, request_json, response_json)
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
        Ok(conn.last_insert_rowid())
    }

    /// List recent history entries (most recent first).
    pub fn list(&self, limit: i64, offset: i64) -> Result<Vec<HistoryEntry>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, method, url, status, status_text, time_ms, size_bytes, timestamp, collection_path, request_name, request_json, response_json
                 FROM history ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2",
            )
            .map_err(|e| format!("Query prepare failed: {e}"))?;

        let entries = stmt
            .query_map(params![limit, offset], |row| {
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
            })
            .map_err(|e| format!("Query failed: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Row mapping failed: {e}"))?;

        Ok(entries)
    }

    /// Search history by URL substring.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<HistoryEntry>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
        let pattern = format!("%{query}%");
        let mut stmt = conn
            .prepare(
                "SELECT id, method, url, status, status_text, time_ms, size_bytes, timestamp, collection_path, request_name, request_json, response_json
                 FROM history WHERE url LIKE ?1 OR method LIKE ?1 OR request_name LIKE ?1
                 ORDER BY timestamp DESC LIMIT ?2",
            )
            .map_err(|e| format!("Query prepare failed: {e}"))?;

        let entries = stmt
            .query_map(params![pattern, limit], |row| {
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
            })
            .map_err(|e| format!("Query failed: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Row mapping failed: {e}"))?;

        Ok(entries)
    }

    /// Get a single history entry by ID.
    pub fn get(&self, id: i64) -> Result<HistoryEntry, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
        conn.query_row(
            "SELECT id, method, url, status, status_text, time_ms, size_bytes, timestamp, collection_path, request_name, request_json, response_json
             FROM history WHERE id = ?1",
            params![id],
            |row| {
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
            },
        )
        .map_err(|e| format!("History entry not found: {e}"))
    }

    /// Clear all history.
    pub fn clear(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
        conn.execute("DELETE FROM history", [])
            .map_err(|e| format!("Failed to clear history: {e}"))?;
        Ok(())
    }

    /// Delete a single history entry.
    pub fn delete(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
        conn.execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete history entry: {e}"))?;
        Ok(())
    }

    /// Insert a monitor result.
    pub fn insert_monitor_result(
        &self,
        result: &crate::scheduler::MonitorResult,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
        conn.execute(
            "INSERT INTO monitor_results (monitor_id, timestamp, total_requests, total_passed, total_failed, total_time_ms, status, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                result.monitor_id,
                result.timestamp,
                result.total_requests as i64,
                result.total_passed as i64,
                result.total_failed as i64,
                result.total_time_ms as i64,
                result.status,
                result.error,
            ],
        )
        .map_err(|e| format!("Failed to insert monitor result: {e}"))?;
        Ok(())
    }

    /// Get monitor results for a specific monitor.
    pub fn get_monitor_results(
        &self,
        monitor_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::scheduler::MonitorResult>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, monitor_id, timestamp, total_requests, total_passed, total_failed, total_time_ms, status, error
                 FROM monitor_results WHERE monitor_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
            )
            .map_err(|e| format!("Prepare failed: {e}"))?;

        let results = stmt
            .query_map(params![monitor_id, limit as i64], |row| {
                Ok(crate::scheduler::MonitorResult {
                    id: row.get(0)?,
                    monitor_id: row.get(1)?,
                    timestamp: row.get(2)?,
                    total_requests: row.get::<_, i64>(3)? as usize,
                    total_passed: row.get::<_, i64>(4)? as usize,
                    total_failed: row.get::<_, i64>(5)? as usize,
                    total_time_ms: row.get::<_, i64>(6)? as u64,
                    status: row.get(7)?,
                    error: row.get(8)?,
                })
            })
            .map_err(|e| format!("Query failed: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Row error: {e}"))?;

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_db_path(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "apiark_history_test_{}_{}_{}",
            std::process::id(),
            name,
            id
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("data.db")
    }

    fn sample_entry() -> HistoryEntry {
        HistoryEntry {
            id: 0,
            method: "POST".to_string(),
            url: "https://example.com/history".to_string(),
            status: Some(200),
            status_text: Some("OK".to_string()),
            time_ms: Some(42),
            size_bytes: Some(128),
            timestamp: chrono::Utc::now().to_rfc3339(),
            collection_path: None,
            request_name: Some("Create user".to_string()),
            request_json:
                r#"{"headers":[{"key":"X-Test","value":"1"}],"params":[{"key":"page","value":"2"}]}"#
                    .to_string(),
            response_json: Some(r#"{"status":200,"body":"{\"ok\":true}","headers":[]}"#.to_string()),
        }
    }

    #[test]
    fn history_roundtrip_keeps_request_and_response() {
        let path = temp_db_path("roundtrip");
        let db = HistoryDb::open(&path).unwrap();
        let entry = sample_entry();
        db.insert(&entry).unwrap();

        let listed = db.list(10, 0).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].request_json.contains("X-Test"));
        assert_eq!(listed[0].response_json, entry.response_json);

        let one = db.get(listed[0].id).unwrap();
        let stored_response = one.response_json.unwrap();
        assert!(stored_response.contains("\"status\":200"));
        assert!(stored_response.contains("ok"));

        let found = db.search("example.com", 10).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].response_json.is_some());

        drop(db);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn legacy_history_schema_is_migrated_and_still_writable() {
        let path = temp_db_path("legacy");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    method TEXT NOT NULL, url TEXT NOT NULL,
                    status INTEGER, status_text TEXT, time_ms INTEGER, size_bytes INTEGER,
                    request_headers TEXT, request_body TEXT, response_body TEXT,
                    collection_path TEXT, request_name TEXT,
                    timestamp TEXT NOT NULL DEFAULT (datetime('now'))
                );
                INSERT INTO history (method, url, status, status_text, time_ms, size_bytes,
                                     request_headers, request_body, response_body)
                VALUES ('GET', 'https://legacy.example.com/users', 200, 'OK', 10, 32,
                        '{\"Content-Type\":\"application/json\"}', '{\"a\":1}', '{\"users\":[]}');",
            )
            .unwrap();
        }

        let db = HistoryDb::open(&path).unwrap();
        let entries = db.list(10, 0).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].request_json.contains("Content-Type"));
        assert!(entries[0].request_json.contains("\"type\":\"raw\""));
        assert!(entries[0]
            .response_json
            .as_deref()
            .unwrap_or("")
            .contains("users"));

        // Before the migration new inserts failed with "no column named request_json".
        db.insert(&sample_entry()).unwrap();
        assert_eq!(db.list(10, 0).unwrap().len(), 2);

        drop(db);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
