//! SQLite-backed bookmarks and folders.

use std::path::{Path, PathBuf};

use browser_core::{Bookmark, BookmarkFolder};
use rusqlite::{Connection, OptionalExtension, params};

use crate::StorageError;

const DB_FILE: &str = "browser.sqlite3";

/// Bookmark database handle. Opens a connection per call so it is
/// `Send + Sync` and usable from the background executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkStore {
    path: PathBuf,
}

impl BookmarkStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

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
                "PRAGMA foreign_keys=ON;
                 CREATE TABLE IF NOT EXISTS bookmark_folders (
                     id INTEGER PRIMARY KEY,
                     name TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS bookmarks (
                     id INTEGER PRIMARY KEY,
                     url TEXT NOT NULL UNIQUE,
                     title TEXT NOT NULL DEFAULT '',
                     folder_id INTEGER REFERENCES bookmark_folders(id) ON DELETE SET NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS bookmarks_folder ON bookmarks(folder_id);",
            )
            .map_err(|error| StorageError::Database(error.to_string()))?;
        Ok(connection)
    }

    /// Saves a URL, moving it into `folder_id` if it already exists.
    pub fn add(
        &self,
        url: &str,
        title: &str,
        folder_id: Option<i64>,
        created_at: i64,
    ) -> Result<i64, StorageError> {
        let connection = self.open()?;
        connection
            .execute(
                "INSERT INTO bookmarks (url, title, folder_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(url) DO UPDATE SET
                     title = CASE WHEN excluded.title != '' THEN excluded.title ELSE bookmarks.title END,
                     folder_id = excluded.folder_id",
                params![url, title, folder_id, created_at],
            )
            .map_err(|error| StorageError::Database(error.to_string()))?;
        connection
            .query_row(
                "SELECT id FROM bookmarks WHERE url = ?1",
                params![url],
                |row| row.get(0),
            )
            .map_err(|error| StorageError::Database(error.to_string()))
    }

    pub fn remove(&self, id: i64) -> Result<bool, StorageError> {
        self.remove_where("DELETE FROM bookmarks WHERE id = ?1", params![id])
    }

    pub fn remove_url(&self, url: &str) -> Result<bool, StorageError> {
        self.remove_where("DELETE FROM bookmarks WHERE url = ?1", params![url])
    }

    fn remove_where(
        &self,
        sql: &str,
        parameters: impl rusqlite::Params,
    ) -> Result<bool, StorageError> {
        let connection = self.open()?;
        let removed = connection
            .execute(sql, parameters)
            .map_err(|error| StorageError::Database(error.to_string()))?;
        Ok(removed > 0)
    }

    pub fn find_by_url(&self, url: &str) -> Result<Option<Bookmark>, StorageError> {
        let connection = self.open()?;
        connection
            .query_row(
                "SELECT id, url, title, folder_id, created_at FROM bookmarks WHERE url = ?1",
                params![url],
                row_to_bookmark,
            )
            .optional()
            .map_err(|error| StorageError::Database(error.to_string()))
    }

    /// All bookmarks, unfiled first, then by folder and creation time.
    pub fn list(&self) -> Result<Vec<Bookmark>, StorageError> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare(
                "SELECT id, url, title, folder_id, created_at FROM bookmarks
                 ORDER BY (folder_id IS NULL) DESC, folder_id ASC, created_at DESC",
            )
            .map_err(|error| StorageError::Database(error.to_string()))?;
        let rows = statement
            .query_map([], row_to_bookmark)
            .map_err(|error| StorageError::Database(error.to_string()))?;
        let mut bookmarks = Vec::new();
        for row in rows {
            bookmarks.push(row.map_err(|error| StorageError::Database(error.to_string()))?);
        }
        Ok(bookmarks)
    }

    pub fn move_to_folder(&self, id: i64, folder_id: Option<i64>) -> Result<(), StorageError> {
        let connection = self.open()?;
        connection
            .execute(
                "UPDATE bookmarks SET folder_id = ?1 WHERE id = ?2",
                params![folder_id, id],
            )
            .map_err(|error| StorageError::Database(error.to_string()))?;
        Ok(())
    }

    pub fn create_folder(&self, name: &str, created_at: i64) -> Result<i64, StorageError> {
        let connection = self.open()?;
        connection
            .execute(
                "INSERT INTO bookmark_folders (name, created_at) VALUES (?1, ?2)",
                params![name, created_at],
            )
            .map_err(|error| StorageError::Database(error.to_string()))?;
        Ok(connection.last_insert_rowid())
    }

    pub fn rename_folder(&self, id: i64, name: &str) -> Result<(), StorageError> {
        let connection = self.open()?;
        connection
            .execute(
                "UPDATE bookmark_folders SET name = ?1 WHERE id = ?2",
                params![name, id],
            )
            .map_err(|error| StorageError::Database(error.to_string()))?;
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<BookmarkFolder>, StorageError> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare("SELECT id, name FROM bookmark_folders ORDER BY name COLLATE NOCASE ASC")
            .map_err(|error| StorageError::Database(error.to_string()))?;
        let rows = statement
            .query_map([], |row| {
                Ok(BookmarkFolder {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })
            .map_err(|error| StorageError::Database(error.to_string()))?;
        let mut folders = Vec::new();
        for row in rows {
            folders.push(row.map_err(|error| StorageError::Database(error.to_string()))?);
        }
        Ok(folders)
    }

    /// Deletes a folder; its bookmarks move to the unfiled section via
    /// `ON DELETE SET NULL`.
    pub fn delete_folder(&self, id: i64) -> Result<bool, StorageError> {
        self.remove_where("DELETE FROM bookmark_folders WHERE id = ?1", params![id])
    }
}

fn row_to_bookmark(row: &rusqlite::Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        folder_id: row.get(3)?,
        created_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_store() -> BookmarkStore {
        let unique = format!(
            "gpui-browser-bookmarks-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&dir);
        BookmarkStore::in_directory(dir)
    }

    fn cleanup(store: &BookmarkStore) {
        let _ = std::fs::remove_dir_all(store.path().parent().unwrap());
    }

    #[test]
    fn adds_lists_and_finds_bookmarks() {
        let store = temp_store();
        store
            .add("https://example.com/", "Example", None, 1)
            .expect("add");
        let list = store.list().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Example");
        assert!(
            store
                .find_by_url("https://example.com/")
                .expect("find")
                .is_some()
        );
        cleanup(&store);
    }

    #[test]
    fn adding_the_same_url_moves_and_updates_it() {
        let store = temp_store();
        let folder = store.create_folder("Reading", 1).expect("folder");
        store.add("https://a.test/", "A", None, 1).expect("add");
        store
            .add("https://a.test/", "A better title", Some(folder), 2)
            .expect("add again");
        let list = store.list().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].folder_id, Some(folder));
        assert_eq!(list[0].title, "A better title");
        cleanup(&store);
    }

    #[test]
    fn empty_titles_do_not_erase_titles() {
        let store = temp_store();
        store
            .add("https://a.test/", "Useful", None, 1)
            .expect("add");
        store.add("https://a.test/", "", None, 2).expect("add");
        assert_eq!(store.list().expect("list")[0].title, "Useful");
        cleanup(&store);
    }

    #[test]
    fn folders_sort_and_deleting_moves_children_to_unfiled() {
        let store = temp_store();
        let reading = store.create_folder("Reading", 1).expect("folder");
        store.create_folder("Alpha", 2).expect("folder");
        store
            .add("https://a.test/", "A", Some(reading), 3)
            .expect("add");
        let folders = store.folders().expect("folders");
        assert_eq!(folders[0].name, "Alpha");
        assert!(store.delete_folder(reading).expect("delete folder"));
        let bookmark = store
            .find_by_url("https://a.test/")
            .expect("find")
            .expect("present");
        assert_eq!(bookmark.folder_id, None);
        cleanup(&store);
    }

    #[test]
    fn removes_by_id_and_url() {
        let store = temp_store();
        let id = store.add("https://a.test/", "A", None, 1).expect("add");
        store.add("https://b.test/", "B", None, 2).expect("add");
        assert!(store.remove(id).expect("remove"));
        assert!(store.remove_url("https://b.test/").expect("remove url"));
        assert!(store.list().expect("list").is_empty());
        cleanup(&store);
    }
}
