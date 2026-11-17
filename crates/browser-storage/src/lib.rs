//! Persistent browser data.
//!
//! Storage owns file layout and durability; domain types live in
//! `browser-core`. Writes are atomic (temporary file + rename) so a crash never
//! leaves a half-written session behind.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use browser_core::SessionSnapshot;
use thiserror::Error;

/// Environment override used by automated probes and power users.
pub const DATA_DIR_ENV: &str = "GPUI_BROWSER_DATA_DIR";
const APP_DIR_NAME: &str = "GPUI Browser";
const SESSION_FILE: &str = "session.json";

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("could not determine the application data directory")]
    NoDataDirectory,
    #[error("session file is corrupt: {0}")]
    CorruptSession(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Root directory for browser data plus typed accessors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Uses `GPUI_BROWSER_DATA_DIR` when set, otherwise
    /// `~/Library/Application Support/GPUI Browser` on macOS and
    /// `$XDG_DATA_HOME/gpui-browser` elsewhere.
    pub fn open_default() -> Result<Self, StorageError> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
            return Ok(Self::new(PathBuf::from(dir)));
        }
        if cfg!(target_os = "macos") {
            let home = std::env::var_os("HOME").ok_or(StorageError::NoDataDirectory)?;
            return Ok(Self::new(
                PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join(APP_DIR_NAME),
            ));
        }
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .ok_or(StorageError::NoDataDirectory)?;
        Ok(Self::new(base.join("gpui-browser")))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn session_path(&self) -> PathBuf {
        self.root.join(SESSION_FILE)
    }

    /// Loads the last session, if one exists.
    ///
    /// A corrupt file is reported, never silently treated as empty: the caller
    /// decides whether to start fresh while preserving the bad file for
    /// diagnosis.
    pub fn load_session(&self) -> Result<Option<SessionSnapshot>, StorageError> {
        let path = self.session_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(StorageError::Io(error)),
        };
        let snapshot: SessionSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| StorageError::CorruptSession(error.to_string()))?;
        Ok(Some(snapshot.sanitized()))
    }

    /// Atomically replaces the session file.
    pub fn save_session(&self, session: &SessionSnapshot) -> Result<(), StorageError> {
        fs::create_dir_all(&self.root)?;
        let json = serde_json::to_vec_pretty(&session.clone().sanitized())
            .map_err(|error| StorageError::CorruptSession(error.to_string()))?;
        let temporary = self.root.join(format!("{SESSION_FILE}.tmp"));
        fs::write(&temporary, &json)?;
        fs::rename(&temporary, self.session_path())?;
        Ok(())
    }

    /// Removes the session, for example when the window was closed with no tabs.
    pub fn clear_session(&self) -> Result<(), StorageError> {
        match fs::remove_file(self.session_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StorageError::Io(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use browser_core::SessionTab;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_storage() -> Storage {
        let unique = format!(
            "gpui-browser-storage-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(unique);
        let _ = fs::remove_dir_all(&root);
        Storage::new(root)
    }

    fn sample_session() -> SessionSnapshot {
        SessionSnapshot {
            tabs: vec![
                SessionTab {
                    url: "https://example.com/".into(),
                    title: "Example".into(),
                },
                SessionTab {
                    url: "https://rust-lang.org/".into(),
                    title: "Rust".into(),
                },
            ],
            active: 1,
        }
    }

    #[test]
    fn missing_session_is_none() {
        let storage = temp_storage();
        assert!(storage.load_session().expect("load").is_none());
    }

    #[test]
    fn session_round_trips_atomically() {
        let storage = temp_storage();
        storage.save_session(&sample_session()).expect("save");
        let loaded = storage.load_session().expect("load").expect("present");
        assert_eq!(loaded, sample_session());
        assert!(!storage.root().join("session.json.tmp").exists());
        let _ = fs::remove_dir_all(storage.root());
    }

    #[test]
    fn corrupt_session_is_reported() {
        let storage = temp_storage();
        fs::create_dir_all(storage.root()).expect("mkdir");
        fs::write(storage.session_path(), b"{not-json").expect("write");
        match storage.load_session() {
            Err(StorageError::CorruptSession(message)) => {
                assert!(message.contains("expected") || !message.is_empty())
            }
            other => panic!("expected corrupt session error, got {other:?}"),
        }
        let _ = fs::remove_dir_all(storage.root());
    }

    #[test]
    fn clearing_removes_the_file() {
        let storage = temp_storage();
        storage.save_session(&sample_session()).expect("save");
        storage.clear_session().expect("clear");
        assert!(storage.load_session().expect("load").is_none());
        let _ = fs::remove_dir_all(storage.root());
    }

    #[test]
    fn env_override_controls_the_root() {
        let storage = temp_storage();
        // Safety: this test is single-threaded with respect to the variable it
        // sets and runs in its own process's test binary.
        unsafe { std::env::set_var(DATA_DIR_ENV, storage.root()) };
        let opened = Storage::open_default().expect("open default");
        unsafe { std::env::remove_var(DATA_DIR_ENV) };
        assert_eq!(opened.root(), storage.root());
    }
}
