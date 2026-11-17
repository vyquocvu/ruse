//! Search engine configuration.

use serde::{Deserialize, Serialize};

/// A search engine: a display name and a URL template containing `{query}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEngine {
    pub name: String,
    pub url_template: String,
}

impl SearchEngine {
    pub fn new(name: impl Into<String>, url_template: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url_template: url_template.into(),
        }
    }

    /// The default engine: DuckDuckGo, which does not profile by default and
    /// needs no account.
    pub fn duckduckgo() -> Self {
        Self::new("DuckDuckGo", "https://duckduckgo.com/?q={query}")
    }

    /// Builds the URL for a search query.
    ///
    /// The query is percent-encoded with `+` for spaces, matching common search
    /// engine expectations. Templates without `{query}` are treated as invalid
    /// configuration and fall back to DuckDuckGo rather than sending the user to
    /// an unrelated page.
    pub fn search_url(&self, query: &str) -> String {
        let encoded = encode_query(query.trim());
        if self.url_template.contains("{query}") {
            self.url_template.replace("{query}", &encoded)
        } else {
            Self::duckduckgo().search_url(query)
        }
    }
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::duckduckgo()
    }
}

/// Percent-encodes a query the way search engines expect (`%20` replaced by `+`).
pub fn encode_query(query: &str) -> String {
    let mut encoded = String::with_capacity(query.len());
    for byte in query.trim().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_search_urls() {
        let engine = SearchEngine::duckduckgo();
        assert_eq!(
            engine.search_url("rust gpui"),
            "https://duckduckgo.com/?q=rust+gpui"
        );
        assert_eq!(
            engine.search_url("a&b=c"),
            "https://duckduckgo.com/?q=a%26b%3Dc"
        );
        assert_eq!(
            engine.search_url("café"),
            "https://duckduckgo.com/?q=caf%C3%A9"
        );
    }

    #[test]
    fn invalid_template_falls_back() {
        let engine = SearchEngine::new("broken", "https://example.com/search");
        assert_eq!(
            engine.search_url("hello"),
            "https://duckduckgo.com/?q=hello"
        );
    }
}
