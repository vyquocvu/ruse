//! Session capture and restore.
//!
//! A session is the set of ordinary (non-private) tabs in the last window and
//! which one was active. It intentionally stores only URLs, titles and order:
//! form state, scroll position and navigation history are owned by the engine
//! and are not reconstructed by the browser yet.

use serde::{Deserialize, Serialize};

use crate::tab::TabSet;

/// Upper bound protects startup from a corrupt or malicious session file.
pub const MAX_SESSION_TABS: usize = 100;

/// One restorable tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTab {
    pub url: String,
    pub title: String,
}

/// The restorable window state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub tabs: Vec<SessionTab>,
    /// Index into `tabs`; clamped to a valid index when restored.
    pub active: usize,
}

impl SessionSnapshot {
    /// Captures the current tab set.
    pub fn capture(tabs: &TabSet) -> Self {
        Self {
            tabs: tabs
                .tabs()
                .iter()
                .map(|tab| SessionTab {
                    url: tab.navigation.url.clone(),
                    title: tab.navigation.title.clone(),
                })
                .collect(),
            active: tabs.active_index().unwrap_or(0),
        }
    }

    /// Drops unusable entries and clamps the active index.
    ///
    /// Empty URLs are kept: they represent the GPUI new-tab page, which is a
    /// valid thing to restore.
    pub fn sanitized(mut self) -> Self {
        self.tabs
            .retain(|tab| tab.url.len() <= 8192 && !tab.url.contains('\0'));
        self.tabs.truncate(MAX_SESSION_TABS);
        if self.tabs.is_empty() {
            self.active = 0;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_with_tabs(urls: &[&str]) -> TabSet {
        let mut set = TabSet::new();
        for url in urls {
            set.open(*url);
        }
        set
    }

    #[test]
    fn capture_preserves_order_and_active_tab() {
        let mut set = set_with_tabs(&["https://a.test/", "https://b.test/", "https://c.test/"]);
        set.activate_index(1);
        let snapshot = SessionSnapshot::capture(&set);
        assert_eq!(snapshot.tabs.len(), 3);
        assert_eq!(snapshot.tabs[0].url, "https://a.test/");
        assert_eq!(snapshot.active, 1);
    }

    #[test]
    fn sanitize_clamps_active_index() {
        let snapshot = SessionSnapshot {
            tabs: vec![SessionTab {
                url: "https://a.test/".into(),
                title: "A".into(),
            }],
            active: 7,
        }
        .sanitized();
        assert_eq!(snapshot.active, 0);
    }

    #[test]
    fn sanitize_truncates_oversized_sessions() {
        let tabs = (0..MAX_SESSION_TABS + 20)
            .map(|index| SessionTab {
                url: format!("https://{index}.test/"),
                title: String::new(),
            })
            .collect();
        let snapshot = SessionSnapshot { tabs, active: 120 }.sanitized();
        assert_eq!(snapshot.tabs.len(), MAX_SESSION_TABS);
        assert_eq!(snapshot.active, MAX_SESSION_TABS - 1);
    }

    #[test]
    fn sanitize_keeps_new_tab_pages() {
        let snapshot = SessionSnapshot {
            tabs: vec![SessionTab {
                url: String::new(),
                title: "New Tab".into(),
            }],
            active: 0,
        }
        .sanitized();
        assert_eq!(snapshot.tabs.len(), 1);
    }

    #[test]
    fn round_trips_through_json() {
        let snapshot = SessionSnapshot {
            tabs: vec![
                SessionTab {
                    url: "https://example.com/".into(),
                    title: "Example".into(),
                },
                SessionTab {
                    url: String::new(),
                    title: "New Tab".into(),
                },
            ],
            active: 1,
        };
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let decoded: SessionSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(snapshot, decoded);
    }

    #[test]
    fn absent_session_is_empty() {
        assert!(SessionSnapshot::default().is_empty());
        assert_eq!(SessionSnapshot::default().sanitized().active, 0);
    }
}
