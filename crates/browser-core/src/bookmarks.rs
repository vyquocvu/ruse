//! Bookmark domain models.
//!
//! Bookmarks are unique by URL. Folders are a flat list for now; the schema
//! already allows adding nesting later without a migration of existing rows.

use serde::{Deserialize, Serialize};

/// A saved page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: i64,
    pub url: String,
    pub title: String,
    /// `None` means the bookmark is unfiled.
    pub folder_id: Option<i64>,
    /// Unix milliseconds.
    pub created_at: i64,
}

impl Bookmark {
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

/// A user-created bookmark folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkFolder {
    pub id: i64,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_title_falls_back_to_host() {
        let bookmark = Bookmark {
            id: 1,
            url: "https://example.com/page".into(),
            title: String::new(),
            folder_id: None,
            created_at: 0,
        };
        assert_eq!(bookmark.display_title(), "example.com");
    }

    #[test]
    fn display_title_prefers_saved_title() {
        let bookmark = Bookmark {
            id: 1,
            url: "https://example.com/page".into(),
            title: "Example".into(),
            folder_id: Some(2),
            created_at: 0,
        };
        assert_eq!(bookmark.display_title(), "Example");
    }
}
