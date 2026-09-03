//! Response history (spec section 10): "Stored in a local SQLite DB,
//! outside Git-native collection storage (per-user, ephemeral, may
//! contain sensitive response data — never synced or committed)."
//! "Retention: last N runs per request (default ~20, configurable),
//! manual clear option."
//!
//! `request_key` is caller-defined — the desktop app uses a saved
//! request's file path, or a stable per-tab id for ad-hoc requests, so
//! history stays scoped to "this request" rather than merging everything
//! that happened to share a URL. This module doesn't know or care which
//! scheme a caller picked; it just groups and prunes by whatever string
//! it's given.
//!
//! Every send is recorded here, including the one that just happened —
//! which is what lets "diff the live response against a past run"
//! (`crate::diff`) be nothing more than "diff two history rows": the live
//! response's own row is simply the newest one.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::EngineError;

pub struct NewHistoryEntry {
    pub request_key: String,
    pub request_label: String,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub status_text: String,
    /// Pre-serialized by the caller -- this module doesn't depend on
    /// `IndexMap` or any particular header representation.
    pub headers_json: String,
    /// JSON array of raw `Set-Cookie` values, kept separate from
    /// `headers_json` for the same reason `http::ResponseSummary` keeps
    /// them separate -- folding multiple into one map would keep only
    /// the last.
    pub cookies_json: String,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEntrySummary {
    pub id: i64,
    pub request_label: String,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub status_text: String,
    pub elapsed_ms: u64,
    pub sent_at: i64,
    pub size_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub id: i64,
    pub request_key: String,
    pub request_label: String,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub status_text: String,
    pub headers_json: String,
    pub cookies_json: String,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub elapsed_ms: u64,
    pub sent_at: i64,
}

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS response_history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        request_key TEXT NOT NULL,
        request_label TEXT NOT NULL,
        method TEXT NOT NULL,
        url TEXT NOT NULL,
        status INTEGER NOT NULL,
        status_text TEXT NOT NULL,
        headers_json TEXT NOT NULL,
        cookies_json TEXT NOT NULL DEFAULT '[]',
        body BLOB NOT NULL,
        content_type TEXT,
        elapsed_ms INTEGER NOT NULL,
        sent_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_history_request_key ON response_history(request_key, sent_at DESC);
";

impl HistoryStore {
    /// Opens (creating if needed) a SQLite file at `path`. The desktop
    /// app points this at its app-data directory, deliberately outside
    /// any Git-tracked collection folder.
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(HistoryStore { conn: Mutex::new(conn) })
    }

    #[cfg(test)]
    fn open_in_memory() -> Result<Self, EngineError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(HistoryStore { conn: Mutex::new(conn) })
    }

    /// Records a send and prunes older rows for the same `request_key`
    /// beyond `retention`. Returns the new row's id.
    pub fn record(&self, entry: NewHistoryEntry, retention: u32) -> Result<i64, EngineError> {
        let conn = self.conn.lock().unwrap();
        // Milliseconds, not seconds -- several sends can easily land in
        // the same second (a quick manual re-send, a scripted loop), and
        // `sent_at` is what newest-first ordering and retention pruning
        // both key off.
        let sent_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        conn.execute(
            "INSERT INTO response_history
                (request_key, request_label, method, url, status, status_text, headers_json, cookies_json, body, content_type, elapsed_ms, sent_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                entry.request_key,
                entry.request_label,
                entry.method,
                entry.url,
                entry.status,
                entry.status_text,
                entry.headers_json,
                entry.cookies_json,
                entry.body,
                entry.content_type,
                entry.elapsed_ms,
                sent_at,
            ],
        )?;
        let id = conn.last_insert_rowid();

        conn.execute(
            "DELETE FROM response_history
             WHERE request_key = ?1
               AND id NOT IN (
                 SELECT id FROM response_history WHERE request_key = ?1 ORDER BY sent_at DESC, id DESC LIMIT ?2
               )",
            params![entry.request_key, retention],
        )?;

        Ok(id)
    }

    /// Newest-first summaries for a request -- no body, so listing is
    /// cheap even with many/large past responses.
    pub fn list_for_request(&self, request_key: &str, limit: u32) -> Result<Vec<HistoryEntrySummary>, EngineError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, request_label, method, url, status, status_text, elapsed_ms, sent_at, length(body)
             FROM response_history WHERE request_key = ?1 ORDER BY sent_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![request_key, limit], |row| {
            Ok(HistoryEntrySummary {
                id: row.get(0)?,
                request_label: row.get(1)?,
                method: row.get(2)?,
                url: row.get(3)?,
                status: row.get::<_, i64>(4)? as u16,
                status_text: row.get(5)?,
                elapsed_ms: row.get::<_, i64>(6)? as u64,
                sent_at: row.get(7)?,
                size_bytes: row.get::<_, i64>(8)? as usize,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(EngineError::from)
    }

    /// The full row, body included -- for opening a past response or
    /// diffing it against another.
    pub fn get(&self, id: i64) -> Result<Option<HistoryEntry>, EngineError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, request_key, request_label, method, url, status, status_text, headers_json, cookies_json, body, content_type, elapsed_ms, sent_at
             FROM response_history WHERE id = ?1",
            params![id],
            |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    request_key: row.get(1)?,
                    request_label: row.get(2)?,
                    method: row.get(3)?,
                    url: row.get(4)?,
                    status: row.get::<_, i64>(5)? as u16,
                    status_text: row.get(6)?,
                    headers_json: row.get(7)?,
                    cookies_json: row.get(8)?,
                    body: row.get(9)?,
                    content_type: row.get(10)?,
                    elapsed_ms: row.get::<_, i64>(11)? as u64,
                    sent_at: row.get(12)?,
                })
            },
        )
        .optional()
        .map_err(EngineError::from)
    }

    pub fn clear_for_request(&self, request_key: &str) -> Result<(), EngineError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM response_history WHERE request_key = ?1", params![request_key])?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<(), EngineError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM response_history", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(request_key: &str, status: u16) -> NewHistoryEntry {
        NewHistoryEntry {
            request_key: request_key.to_string(),
            request_label: "Get user".to_string(),
            method: "GET".to_string(),
            url: "https://example.com".to_string(),
            status,
            status_text: "OK".to_string(),
            headers_json: "{}".to_string(),
            cookies_json: "[]".to_string(),
            body: b"{\"ok\":true}".to_vec(),
            content_type: Some("application/json".to_string()),
            elapsed_ms: 42,
        }
    }

    #[test]
    fn records_and_lists_newest_first() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.record(entry("req-1", 200), 20).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store.record(entry("req-1", 500), 20).unwrap();

        let list = store.list_for_request("req-1", 20).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].status, 500); // most recent first
        assert_eq!(list[1].status, 200);
    }

    #[test]
    fn get_returns_full_entry_including_body() {
        let store = HistoryStore::open_in_memory().unwrap();
        let id = store.record(entry("req-1", 200), 20).unwrap();

        let full = store.get(id).unwrap().unwrap();
        assert_eq!(full.body, b"{\"ok\":true}");
        assert_eq!(full.request_key, "req-1");
    }

    #[test]
    fn get_missing_id_returns_none() {
        let store = HistoryStore::open_in_memory().unwrap();
        assert!(store.get(999).unwrap().is_none());
    }

    #[test]
    fn retention_prunes_oldest_beyond_limit() {
        let store = HistoryStore::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..5 {
            ids.push(store.record(entry("req-1", 200 + i), 3).unwrap());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let list = store.list_for_request("req-1", 100).unwrap();
        assert_eq!(list.len(), 3, "only the retention limit should remain");
        // The two oldest (first inserted) should be gone.
        assert!(store.get(ids[0]).unwrap().is_none());
        assert!(store.get(ids[1]).unwrap().is_none());
        assert!(store.get(ids[4]).unwrap().is_some());
    }

    #[test]
    fn different_request_keys_do_not_interfere() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.record(entry("req-a", 200), 20).unwrap();
        store.record(entry("req-b", 404), 20).unwrap();

        assert_eq!(store.list_for_request("req-a", 20).unwrap().len(), 1);
        assert_eq!(store.list_for_request("req-b", 20).unwrap().len(), 1);
    }

    #[test]
    fn clear_for_request_only_clears_that_key() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.record(entry("req-a", 200), 20).unwrap();
        store.record(entry("req-b", 200), 20).unwrap();

        store.clear_for_request("req-a").unwrap();

        assert_eq!(store.list_for_request("req-a", 20).unwrap().len(), 0);
        assert_eq!(store.list_for_request("req-b", 20).unwrap().len(), 1);
    }

    #[test]
    fn clear_all_clears_everything() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.record(entry("req-a", 200), 20).unwrap();
        store.record(entry("req-b", 200), 20).unwrap();

        store.clear_all().unwrap();

        assert_eq!(store.list_for_request("req-a", 20).unwrap().len(), 0);
        assert_eq!(store.list_for_request("req-b", 20).unwrap().len(), 0);
    }
}
