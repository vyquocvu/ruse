//! Settings persistence (atomic JSON).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use browser_core::BrowserSettings;

use crate::StorageError;

const SETTINGS_FILE: &str = "settings.json";

/// Atomic JSON store for browser settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn in_directory(directory: impl AsRef<Path>) -> Self {
        Self::new(directory.as_ref().join(SETTINGS_FILE))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads settings, returning defaults when the file is absent. A corrupt
    /// file is reported to the caller, which can fall back to defaults while
    /// preserving the file for inspection.
    pub fn load(&self) -> Result<Option<BrowserSettings>, StorageError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(StorageError::Io(error)),
        };
        let settings: BrowserSettings = serde_json::from_slice(&bytes)
            .map_err(|error| StorageError::CorruptSession(error.to_string()))?;
        Ok(Some(settings.sanitized()))
    }

    pub fn save(&self, settings: &BrowserSettings) -> Result<(), StorageError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(&settings.clone().sanitized())
            .map_err(|error| StorageError::CorruptSession(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, json)?;
        fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use browser_core::{SearchEngine, ThemeMode};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_store() -> SettingsStore {
        let unique = format!(
            "gpui-browser-settings-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let directory = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&directory);
        SettingsStore::in_directory(directory)
    }

    #[test]
    fn missing_file_is_none() {
        let store = temp_store();
        assert!(store.load().expect("load").is_none());
    }

    #[test]
    fn round_trips_settings() {
        let store = temp_store();
        let settings = BrowserSettings {
            homepage: "https://example.org".into(),
            search_engine: SearchEngine::new("Kagi", "https://kagi.com/search?q={query}"),
            theme: ThemeMode::Dark,
        };
        store.save(&settings).expect("save");
        let loaded = store.load().expect("load").expect("present");
        assert_eq!(loaded, settings);
        assert!(!store.path().with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(store.path().parent().unwrap());
    }

    #[test]
    fn corrupt_file_is_reported() {
        let store = temp_store();
        std::fs::create_dir_all(store.path().parent().unwrap()).expect("mkdir");
        std::fs::write(store.path(), b"{oops").expect("write");
        assert!(store.load().is_err());
        let _ = std::fs::remove_dir_all(store.path().parent().unwrap());
    }
}
