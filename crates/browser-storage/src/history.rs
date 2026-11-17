//! SQLite-backed browsing history.

use std::path::{Path, PathBuf};

use browser_core::HistoryEntry;
use rusqlite::{Connection, params};

use crate::StorageError;

const DB_FILE: &str = "browser.sqlite3";

/// History database handle. Cheap to clone; a connection is opened per call so
/// the store is `Send + Sync` and safe to use from the background executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryStore {
    path: PathBuf,
}

impl HistoryStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Uses `browser.sqlite3` inside the given data directory.
    pub fn in_directory(directory: impl AsRef<Path>) -> Self {
        Self::new(directory.as_ref().join(DB_FILE))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn open(&self) -> Result<Connection, StorageError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&self.path)
            .map_err(|error| StorageError::Database(error.to_string()))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS history (
                     id INTEGER PRIMARY KEY,
                     url TEXT NOT NULL UNIQUE,
                     title TEXT NOT NULL DEFAULT '',
                     last_visited_at INTEGER NOT NULL,
                     visit_count INTEGER NOT NULL DEFAULT 1
                 );
                 CREATE INDEX IF NOT EXISTS history_last_visited
                     ON history(last_visited_at DESC);",
            )
            .map_err(|error| StorageError::Database(error.to_string()))?;
        Ok(connection)
    }

    /// Records a visit, collapsing repeat visits of the same URL.
    pub fn record(&self, url: &str, title: &str, visited_at: i64) -> Result<(), StorageError> {
        let connection = self.open()?;
        connection
            .execute(
                "INSERT INTO history (url, title, last_visited_at, visit_count)
                 VALUES (?1, ?2, ?3, 1)
                 ON CONFLICT(url) DO UPDATE SET
                     title = CASE WHEN excluded.title != '' THEN excluded.title ELSE history.title END,
                     last_visited_at = excluded.last_visited_at,
                     visit_count = history.visit_count + 1",
                params![url, title, visited_at],
            )
            .map_err(|error| StorageError::Database(error.to_string()))?;
        Ok(())
    }

    /// Most recently visited pages.
    pub fn recent(&self, limit: usize) -> Result<Vec<HistoryEntry>, StorageError> {
        self.query(
            "SELECT id, url, title, last_visited_at, visit_count
             FROM history ORDER BY last_visited_at DESC LIMIT ?1",
            params![limit as i64],
        )
    }

    /// Case-insensitive search over URL and title.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<HistoryEntry>, StorageError> {
        let pattern = format!("%{}%", escape_like(query.trim()));
        self.query(
            "SELECT id, url, title, last_visited_at, visit_count
             FROM history
             WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
             ORDER BY last_visited_at DESC LIMIT ?2",
            params![pattern, limit as i64],
        )
    }

    fn query(
        &self,
        sql: &str,
        parameters: impl rusqlite::Params,
    ) -> Result<Vec<HistoryEntry>, StorageError> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare(sql)
            .map_err(|error| StorageError::Database(error.to_string()))?;
        let rows = statement
            .query_map(parameters, |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    last_visited_at: row.get(3)?,
                    visit_count: row.get(4)?,
                })
            })
            .map_err(|error| StorageError::Database(error.to_string()))?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|error| StorageError::Database(error.to_string()))?);
        }
        Ok(entries)
    }

    /// Deletes one entry. Returns whether a row was removed.
    pub fn delete(&self, id: i64) -> Result<bool, StorageError> {
        let connection = self.open()?;
        let removed = connection
            .execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(|error| StorageError::Database(error.to_string()))?;
        Ok(removed > 0)
    }

    /// Removes all history.
    pub fn clear(&self) -> Result<(), StorageError> {
        let connection = self.open()?;
        connection
            .execute("DELETE FROM history", [])
            .map_err(|error| StorageError::Database(error.to_string()))?;
        Ok(())
    }

    pub fn count(&self) -> Result<u64, StorageError> {
        let connection = self.open()?;
        connection
            .query_row("SELECT COUNT(*) FROM history", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count as u64)
            .map_err(|error| StorageError::Database(error.to_string()))
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_store() -> HistoryStore {
        let unique = format!(
            "gpui-browser-history-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&dir);
        HistoryStore::in_directory(dir)
    }

    #[test]
    fn records_and_reads_recent_entries() {
        let store = temp_store();
        store.record("https://a.test/", "A", 1_000).expect("record");
        store.record("https://b.test/", "B", 2_000).expect("record");
        let recent = store.recent(10).expect("recent");
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].url, "https://b.test/");
        assert_eq!(recent[0].title, "B");
        assert_eq!(recent[0].visit_count, 1);
        let _ = std::fs::remove_dir_all(store.path().parent().unwrap());
    }

    #[test]
    fn repeat_visits_collapse_into_one_row() {
        let store = temp_store();
        store
            .record("https://a.test/", "Old", 1_000)
            .expect("record");
        store
            .record("https://a.test/", "New", 3_000)
            .expect("record");
        let recent = store.recent(10).expect("recent");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].title, "New");
        assert_eq!(recent[0].last_visited_at, 3_000);
        assert_eq!(recent[0].visit_count, 2);
        let _ = std::fs::remove_dir_all(store.path().parent().unwrap());
    }

    #[test]
    fn empty_titles_do_not_erase_titles() {
        let store = temp_store();
        store
            .record("https://a.test/", "Useful", 1_000)
            .expect("record");
        store.record("https://a.test/", "", 2_000).expect("record");
        assert_eq!(store.recent(1).expect("recent")[0].title, "Useful");
        let _ = std::fs::remove_dir_all(store.path().parent().unwrap());
    }

    #[test]
    fn search_matches_url_and_title_and_escapes_wildcards() {
        let store = temp_store();
        store
            .record("https://rust-lang.org/", "Rust", 1_000)
            .expect("record");
        store
            .record("https://example.com/", "Example", 2_000)
            .expect("record");
        assert_eq!(store.search("rust", 10).expect("search").len(), 1);
        assert_eq!(store.search("EXAMPLE", 10).expect("search").len(), 1);
        assert_eq!(store.search("%", 10).expect("search").len(), 0);
        let _ = std::fs::remove_dir_all(store.path().parent().unwrap());
    }

    #[test]
    fn deletes_and_clears() {
        let store = temp_store();
        store.record("https://a.test/", "A", 1_000).expect("record");
        store.record("https://b.test/", "B", 2_000).expect("record");
        let entries = store.recent(10).expect("recent");
        assert!(store.delete(entries[0].id).expect("delete"));
        assert_eq!(store.count().expect("count"), 1);
        store.clear().expect("clear");
        assert_eq!(store.count().expect("count"), 0);
        let _ = std::fs::remove_dir_all(store.path().parent().unwrap());
    }
}
