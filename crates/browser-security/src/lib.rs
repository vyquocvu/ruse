//! Browser security policy: URL handling, navigation decisions and download
//! destination validation.
//!
//! This crate is deliberately pure and heavily tested; the engine and UI apply
//! its decisions rather than inventing policy locally.

use std::path::{Path, PathBuf};

/// Maximum length we accept for a suggested download filename.
const MAX_FILENAME_LEN: usize = 180;
const FALLBACK_FILENAME: &str = "download";

/// Sanitises a server-suggested filename into a single safe path component.
///
/// Strips directory separators and control characters, prevents `.`/`..`,
/// trims leading dots (hidden files), bounds the length and falls back to
/// `download` when nothing usable remains. The extension is preserved.
pub fn sanitize_download_filename(suggested: &str) -> String {
    let base = suggested
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(suggested)
        .trim();
    if base.is_empty() || base == "." || base == ".." {
        return FALLBACK_FILENAME.to_owned();
    }
    let cleaned: String = base
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '\0' | '?' | '*' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    let cleaned = cleaned.trim_start_matches(['.', '_']).trim();
    if cleaned.is_empty() {
        return FALLBACK_FILENAME.to_owned();
    }
    if cleaned.chars().count() <= MAX_FILENAME_LEN {
        return cleaned.to_owned();
    }

    // Preserve the extension while truncating the stem.
    let path = Path::new(cleaned);
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{}", &extension[..extension.len().min(16)]))
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(FALLBACK_FILENAME);
    let available = MAX_FILENAME_LEN.saturating_sub(extension.chars().count());
    let mut truncated: String = stem.chars().take(available).collect();
    if truncated.is_empty() {
        truncated = FALLBACK_FILENAME.to_owned();
    }
    format!("{truncated}{extension}")
}

/// Returns a non-colliding path inside `directory` for `filename`.
///
/// Collisions get ` (1)`, ` (2)`, ... before the extension. The returned path is
/// always a direct child of `directory`.
pub fn unique_download_path(directory: &Path, filename: &str) -> PathBuf {
    let filename = sanitize_download_filename(filename);
    let candidate = directory.join(&filename);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(&filename);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(FALLBACK_FILENAME);
    let extension = path.extension().and_then(|extension| extension.to_str());
    for index in 1..=9999 {
        let name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = directory.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{stem}-{}", std::process::id()))
}

/// Bounds how many new windows a page can request in a short interval.
///
/// Every `window.open`/`target=_blank` opens a tab in this browser, so a
/// malicious page could otherwise exhaust memory and attention.
#[derive(Debug, Clone)]
pub struct PopupPolicy {
    max_per_window: usize,
    window: std::time::Duration,
    timestamps: Vec<std::time::Instant>,
}

impl PopupPolicy {
    pub fn new(max_per_window: usize, window: std::time::Duration) -> Self {
        Self {
            max_per_window,
            window,
            timestamps: Vec::new(),
        }
    }

    /// Records a request and returns whether it may open a new tab.
    pub fn allow(&mut self, now: std::time::Instant) -> bool {
        self.timestamps
            .retain(|timestamp| now.duration_since(*timestamp) < self.window);
        if self.timestamps.len() >= self.max_per_window {
            return false;
        }
        self.timestamps.push(now);
        true
    }
}

impl Default for PopupPolicy {
    fn default() -> Self {
        Self::new(5, std::time::Duration::from_secs(3))
    }
}

/// Whether a URL may be navigated by the browser itself.
pub fn is_allowed_navigation_scheme(scheme: &str) -> bool {
    matches!(
        scheme,
        "http" | "https" | "about" | "file" | "data" | "blob"
    )
}

/// Whether a URL must be handed to the operating system instead of rendered.
pub fn requires_external_handler(scheme: &str) -> bool {
    !is_allowed_navigation_scheme(scheme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn strips_directories_and_dangerous_characters() {
        assert_eq!(sanitize_download_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_download_filename("a/b\\c.txt"), "c.txt");
        assert_eq!(sanitize_download_filename("bad:name?.txt"), "bad_name_.txt");
        assert_eq!(sanitize_download_filename("..\u{0}evil"), "evil");
    }

    #[test]
    fn hides_leading_dots_and_rejects_empty() {
        assert_eq!(sanitize_download_filename(".hidden"), "hidden");
        assert_eq!(sanitize_download_filename(""), "download");
        assert_eq!(sanitize_download_filename(".."), "download");
        assert_eq!(sanitize_download_filename("   "), "download");
    }

    #[test]
    fn bounds_length_but_keeps_extension() {
        let long = format!("{}.txt", "a".repeat(400));
        let sanitized = sanitize_download_filename(&long);
        assert!(sanitized.chars().count() <= 180);
        assert!(sanitized.ends_with(".txt"));
    }

    #[test]
    fn resolves_collisions() {
        let directory = std::env::temp_dir().join(format!(
            "gpui-browser-download-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).expect("mkdir");
        let first = unique_download_path(&directory, "file.txt");
        assert_eq!(first.file_name().unwrap(), "file.txt");
        fs::write(&first, b"x").expect("write");
        let second = unique_download_path(&directory, "file.txt");
        assert_eq!(second.file_name().unwrap(), "file (1).txt");
        assert_eq!(second.parent(), Some(directory.as_path()));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn popup_policy_rate_limits_and_recovers() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut policy = PopupPolicy::new(3, Duration::from_secs(2));
        assert!(policy.allow(start));
        assert!(policy.allow(start + Duration::from_millis(100)));
        assert!(policy.allow(start + Duration::from_millis(200)));
        assert!(!policy.allow(start + Duration::from_millis(300)));
        assert!(policy.allow(start + Duration::from_secs(3)));
    }

    #[test]
    fn schemes_are_classified() {
        assert!(is_allowed_navigation_scheme("https"));
        assert!(is_allowed_navigation_scheme("about"));
        assert!(!is_allowed_navigation_scheme("mailto"));
        assert!(requires_external_handler("tel"));
        assert!(!requires_external_handler("https"));
    }
}
