//! Browsing history domain model.
//!
//! Storage keeps one row per URL (last visit wins, with a visit count) so the
//! history list stays useful instead of repeating a page on every reload.

use serde::{Deserialize, Serialize};

/// One visited URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub url: String,
    pub title: String,
    /// Unix milliseconds of the most recent visit.
    pub last_visited_at: i64,
    pub visit_count: u32,
}

impl HistoryEntry {
    /// Title to display, falling back to the URL host.
    pub fn display_title(&self) -> String {
        if !self.title.trim().is_empty() {
            return self.title.clone();
        }
        url::Url::parse(&self.url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| self.url.clone())
    }
}

/// Current time as Unix milliseconds.
pub fn unix_millis_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Formats a timestamp relative to `now_ms` for the history list.
pub fn relative_time(visited_at_ms: i64, now_ms: i64) -> String {
    let delta_seconds = ((now_ms - visited_at_ms) / 1000).max(0);
    match delta_seconds {
        0..=45 => "just now".to_owned(),
        46..=90 => "1 minute ago".to_owned(),
        91..=3570 => format!("{} minutes ago", (delta_seconds + 30) / 60),
        3571..=5400 => "1 hour ago".to_owned(),
        5401..=82_800 => format!("{} hours ago", (delta_seconds + 1800) / 3600),
        82_801..=129_600 => "yesterday".to_owned(),
        _ => format!("{} days ago", (delta_seconds + 43_200) / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, url: &str) -> HistoryEntry {
        HistoryEntry {
            id: 1,
            url: url.into(),
            title: title.into(),
            last_visited_at: 0,
            visit_count: 1,
        }
    }

    #[test]
    fn display_title_falls_back_to_host() {
        assert_eq!(
            entry("", "https://news.ycombinator.com/item?id=1").display_title(),
            "news.ycombinator.com"
        );
        assert_eq!(
            entry("Hacker News", "https://news.ycombinator.com/").display_title(),
            "Hacker News"
        );
    }

    #[test]
    fn relative_times_read_naturally() {
        let now = 1_000_000_000_000;
        assert_eq!(relative_time(now, now), "just now");
        assert_eq!(relative_time(now - 60_000, now), "1 minute ago");
        assert_eq!(relative_time(now - 5 * 60_000, now), "5 minutes ago");
        assert_eq!(relative_time(now - 100 * 60_000, now), "2 hours ago");
        assert_eq!(relative_time(now - 26 * 3_600_000, now), "yesterday");
        assert_eq!(relative_time(now - 3 * 86_400_000, now), "3 days ago");
    }
}
