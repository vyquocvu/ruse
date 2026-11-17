//! Navigation state for a tab.
//!
//! The engine adapter feeds [`NavigationEvent`]s; the tab state machine decides
//! what the toolbar, tab strip and address bar show. The UI never inspects
//! engine-specific types.

use serde::{Deserialize, Serialize};

/// Coarse loading status shown in the chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadStatus {
    Idle,
    Loading,
    Loaded,
    Failed,
}

/// A visible navigation failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadError {
    pub url: String,
    pub message: String,
}

/// Events that drive navigation state. Mapped from engine events by the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationEvent {
    Started {
        url: String,
    },
    Committed {
        url: String,
    },
    Finished {
        url: String,
        title: Option<String>,
        can_go_back: bool,
        can_go_forward: bool,
    },
    Failed {
        url: String,
        message: String,
    },
    TitleChanged {
        title: String,
    },
    UrlChanged {
        url: String,
    },
    ProcessTerminated,
    Stopped,
}

/// Everything the chrome needs to know about one tab's page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationState {
    pub url: String,
    pub title: String,
    pub status: LoadStatus,
    /// 0.0..=1.0 for the loading indicator.
    pub progress_percent: u8,
    pub error: Option<LoadError>,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            url: String::new(),
            title: String::new(),
            status: LoadStatus::Idle,
            progress_percent: 0,
            error: None,
            can_go_back: false,
            can_go_forward: false,
        }
    }
}

impl NavigationState {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    /// Applies an engine event to the state.
    pub fn apply(&mut self, event: NavigationEvent) {
        match event {
            NavigationEvent::Started { url } => {
                self.url = url;
                self.status = LoadStatus::Loading;
                self.progress_percent = 5;
                self.error = None;
                self.can_go_back = false;
                self.can_go_forward = false;
            }
            NavigationEvent::Committed { url } => {
                self.url = url;
                self.status = LoadStatus::Loading;
                // Keep progress where it is; commit is not completion.
                if self.progress_percent < 10 {
                    self.progress_percent = 10;
                }
            }
            NavigationEvent::Finished {
                url,
                title,
                can_go_back,
                can_go_forward,
            } => {
                self.url = url;
                if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
                    self.title = title;
                }
                self.status = LoadStatus::Loaded;
                self.progress_percent = 100;
                self.error = None;
                self.can_go_back = can_go_back;
                self.can_go_forward = can_go_forward;
            }
            NavigationEvent::Failed { url, message } => {
                if !url.is_empty() {
                    self.url = url;
                }
                self.status = LoadStatus::Failed;
                self.progress_percent = 0;
                self.error = Some(LoadError {
                    url: self.url.clone(),
                    message,
                });
            }
            NavigationEvent::TitleChanged { title } => {
                // Pages frequently fire an empty title before setting the real
                // one; never erase a useful title with an empty update.
                if !title.trim().is_empty() {
                    self.title = title;
                }
            }
            NavigationEvent::UrlChanged { url } => {
                self.url = url;
            }
            NavigationEvent::ProcessTerminated => {
                self.status = LoadStatus::Failed;
                self.progress_percent = 0;
                self.error = Some(LoadError {
                    url: self.url.clone(),
                    message: "The web content process stopped unexpectedly. Reload to try again."
                        .to_owned(),
                });
            }
            NavigationEvent::Stopped => {
                if self.status == LoadStatus::Loading {
                    self.status = LoadStatus::Idle;
                    self.progress_percent = 0;
                }
            }
        }
    }

    /// Updates the coarse progress shown by the loading indicator.
    pub fn set_progress(&mut self, progress: f64) {
        if self.status == LoadStatus::Loading {
            let percent = (progress.clamp(0.0, 1.0) * 100.0).round() as u8;
            self.progress_percent = percent.max(self.progress_percent.min(95));
        }
    }

    pub fn is_loading(&self) -> bool {
        self.status == LoadStatus::Loading
    }

    /// Title for a tab strip entry, falling back to the host/URL.
    pub fn display_title(&self) -> String {
        if !self.title.trim().is_empty() {
            return self.title.clone();
        }
        if self.url.is_empty() {
            return "New Tab".to_owned();
        }
        host_of(&self.url).unwrap_or_else(|| self.url.clone())
    }

    /// Text the address bar should show when not being edited.
    pub fn display_url(&self) -> &str {
        &self.url
    }
}

fn host_of(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    parsed.host_str().map(|host| host.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started() -> NavigationState {
        let mut state = NavigationState::default();
        state.apply(NavigationEvent::Started {
            url: "https://example.com/".into(),
        });
        state
    }

    #[test]
    fn started_sets_loading_and_clears_error() {
        let mut state = NavigationState::default();
        state.apply(NavigationEvent::Failed {
            url: "https://bad.test/".into(),
            message: "offline".into(),
        });
        state.apply(NavigationEvent::Started {
            url: "https://example.com/".into(),
        });
        assert_eq!(state.status, LoadStatus::Loading);
        assert_eq!(state.url, "https://example.com/");
        assert!(state.error.is_none());
        assert!(state.progress_percent > 0);
    }

    #[test]
    fn finished_records_title_and_history_availability() {
        let mut state = started();
        state.apply(NavigationEvent::Finished {
            url: "https://example.com/".into(),
            title: Some("Example".into()),
            can_go_back: true,
            can_go_forward: false,
        });
        assert_eq!(state.status, LoadStatus::Loaded);
        assert_eq!(state.title, "Example");
        assert_eq!(state.progress_percent, 100);
        assert!(state.can_go_back);
        assert!(!state.can_go_forward);
    }

    #[test]
    fn empty_titles_do_not_erase_good_titles() {
        let mut state = started();
        state.apply(NavigationEvent::TitleChanged {
            title: "Good".into(),
        });
        state.apply(NavigationEvent::TitleChanged { title: "  ".into() });
        assert_eq!(state.title, "Good");
    }

    #[test]
    fn failure_keeps_url_and_exposes_error() {
        let mut state = started();
        state.apply(NavigationEvent::Failed {
            url: "https://missing.invalid/".into(),
            message: "A server with the specified hostname could not be found.".into(),
        });
        assert_eq!(state.status, LoadStatus::Failed);
        let error = state.error.expect("error");
        assert!(error.message.contains("hostname"));
        assert_eq!(state.url, "https://missing.invalid/");
    }

    #[test]
    fn stop_only_transitions_from_loading() {
        let mut state = started();
        state.apply(NavigationEvent::Stopped);
        assert_eq!(state.status, LoadStatus::Idle);
        state.apply(NavigationEvent::Finished {
            url: "https://example.com/".into(),
            title: None,
            can_go_back: false,
            can_go_forward: false,
        });
        state.apply(NavigationEvent::Stopped);
        assert_eq!(state.status, LoadStatus::Loaded);
    }

    #[test]
    fn process_termination_is_a_recoverable_failure() {
        let mut state = started();
        state.apply(NavigationEvent::ProcessTerminated);
        assert_eq!(state.status, LoadStatus::Failed);
        assert!(state.error.expect("error").message.contains("Reload"));
    }

    #[test]
    fn progress_never_regresses_while_loading() {
        let mut state = started();
        state.set_progress(0.6);
        let high = state.progress_percent;
        state.set_progress(0.2);
        assert!(state.progress_percent >= high);
        state.set_progress(1.0);
        assert_eq!(state.progress_percent, 100);
    }

    #[test]
    fn display_title_falls_back_to_host() {
        let mut state = NavigationState {
            url: "https://news.ycombinator.com/item?id=1".into(),
            ..Default::default()
        };
        assert_eq!(state.display_title(), "news.ycombinator.com");
        state.title = "Hacker News".into();
        assert_eq!(state.display_title(), "Hacker News");
        assert_eq!(NavigationState::default().display_title(), "New Tab");
    }
}
