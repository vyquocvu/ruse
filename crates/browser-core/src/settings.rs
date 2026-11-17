//! User-configurable browser settings.

use serde::{Deserialize, Serialize};

use crate::search::SearchEngine;

/// Default home page: a small, fast, standards-compliant page.
pub const DEFAULT_HOMEPAGE: &str = "https://example.com";

/// Settings the browser reads at startup and when resolving address-bar input.
///
/// Persistence lives in `browser-storage` (M3); the model and its defaults are
/// here so navigation behaviour is testable without touching the file system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSettings {
    /// Page opened in new windows.
    pub homepage: String,
    /// Engine used for address-bar search queries.
    pub search_engine: SearchEngine,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            homepage: DEFAULT_HOMEPAGE.to_owned(),
            search_engine: SearchEngine::duckduckgo(),
        }
    }
}

impl BrowserSettings {
    /// Rejects an empty homepage and falls back to the default.
    pub fn homepage_or_default(&self) -> &str {
        if self.homepage.trim().is_empty() {
            DEFAULT_HOMEPAGE
        } else {
            self.homepage.trim()
        }
    }

    /// Keeps settings compatible with older or hand-edited files.
    pub fn sanitized(mut self) -> Self {
        if self.search_engine.url_template.trim().is_empty() {
            self.search_engine = SearchEngine::duckduckgo();
        }
        if self.homepage.trim().is_empty() {
            self.homepage = DEFAULT_HOMEPAGE.to_owned();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe() {
        let settings = BrowserSettings::default();
        assert_eq!(settings.homepage, DEFAULT_HOMEPAGE);
        assert_eq!(settings.search_engine.name, "DuckDuckGo");
    }

    #[test]
    fn empty_values_are_sanitized() {
        let settings = BrowserSettings {
            homepage: "  ".into(),
            search_engine: SearchEngine::new("broken", ""),
        }
        .sanitized();
        assert_eq!(settings.homepage, DEFAULT_HOMEPAGE);
        assert_eq!(settings.search_engine.name, "DuckDuckGo");
    }

    #[test]
    fn settings_round_trip_through_json() {
        let settings = BrowserSettings {
            homepage: "https://news.ycombinator.com".into(),
            search_engine: SearchEngine::new("Kagi", "https://kagi.com/search?q={query}"),
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let decoded: BrowserSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(settings, decoded);
    }
}
