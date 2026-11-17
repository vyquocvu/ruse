//! Turns address-bar text into a navigation target.
//!
//! The rules deliberately mirror what users expect from a modern browser:
//!
//! - explicit schemes navigate as given (`https:`, `about:`, `file:`, ...),
//! - text that looks like a host navigates over HTTPS,
//! - everything else becomes a search.

use url::Url;

use crate::search::SearchEngine;

/// What the address bar decided to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationTarget {
    /// Navigate to this absolute URL.
    Url(String),
    /// Run a search for this query.
    Search { query: String, url: String },
}

impl NavigationTarget {
    /// The URL the engine should load for this target.
    pub fn url(&self) -> &str {
        match self {
            Self::Url(url) => url,
            Self::Search { url, .. } => url,
        }
    }
}

/// Resolves address-bar text using the configured search engine.
///
/// Returns `None` for empty or whitespace-only input.
pub fn resolve_input(input: &str, search_engine: &SearchEngine) -> Option<NavigationTarget> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(url) = as_explicit_url(trimmed) {
        return Some(NavigationTarget::Url(url));
    }

    if !contains_whitespace(trimmed) && looks_like_host(trimmed) {
        return Some(NavigationTarget::Url(format!("https://{trimmed}")));
    }

    Some(NavigationTarget::Search {
        query: trimmed.to_owned(),
        url: search_engine.search_url(trimmed),
    })
}

fn contains_whitespace(value: &str) -> bool {
    value.chars().any(char::is_whitespace)
}

/// Recognises inputs with a scheme we are willing to navigate to directly.
fn as_explicit_url(input: &str) -> Option<String> {
    let parsed = Url::parse(input).ok()?;
    let scheme = parsed.scheme();
    // Browsers accept a small set of schemes typed into the URL bar. `javascript:`
    // is explicitly rejected: it would execute arbitrary script in the page
    // context from privileged chrome text.
    let allowed = matches!(
        scheme,
        "http" | "https" | "file" | "about" | "data" | "blob" | "view-source"
    );
    allowed.then(|| input.to_owned())
}

fn looks_like_host(input: &str) -> bool {
    if input.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // Bracketed IPv6 literal, optionally with a port: [::1]:8080
    if input.starts_with('[') {
        return input.contains(']');
    }

    let host_and_port = match input.split_once('/') {
        Some((authority, _path)) => authority,
        None => input,
    };
    let host = match host_and_port.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => host,
        // A colon without a numeric port is not a host (e.g. `foo:bar`).
        Some(_) => return false,
        None => host_and_port,
    };
    if host.is_empty() {
        return false;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    // A dot is required for domains/IPv4; localhost is handled above.
    host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> SearchEngine {
        SearchEngine::duckduckgo()
    }

    fn resolve(input: &str) -> NavigationTarget {
        resolve_input(input, &engine()).expect("input should resolve")
    }

    #[test]
    fn empty_input_is_ignored() {
        assert_eq!(resolve_input("   ", &engine()), None);
        assert_eq!(resolve_input("", &engine()), None);
    }

    #[test]
    fn explicit_https_urls_pass_through() {
        assert_eq!(
            resolve("https://example.com/path?q=1"),
            NavigationTarget::Url("https://example.com/path?q=1".into())
        );
        assert_eq!(
            resolve("http://localhost:8080/"),
            NavigationTarget::Url("http://localhost:8080/".into())
        );
    }

    #[test]
    fn about_and_file_urls_are_accepted() {
        assert_eq!(
            resolve("about:blank"),
            NavigationTarget::Url("about:blank".into())
        );
        assert_eq!(
            resolve("file:///tmp/test.html"),
            NavigationTarget::Url("file:///tmp/test.html".into())
        );
    }

    #[test]
    fn javascript_scheme_is_never_navigated() {
        // It must become a search instead of executing in the page.
        match resolve("javascript:alert(1)") {
            NavigationTarget::Search { query, .. } => assert_eq!(query, "javascript:alert(1)"),
            NavigationTarget::Url(url) => panic!("dangerous scheme navigated: {url}"),
        }
    }

    #[test]
    fn bare_hosts_become_https() {
        assert_eq!(
            resolve("example.com"),
            NavigationTarget::Url("https://example.com".into())
        );
        assert_eq!(
            resolve("localhost:3000"),
            NavigationTarget::Url("https://localhost:3000".into())
        );
        assert_eq!(
            resolve("news.ycombinator.com/item?id=1"),
            NavigationTarget::Url("https://news.ycombinator.com/item?id=1".into())
        );
    }

    #[test]
    fn words_become_searches() {
        assert_eq!(
            resolve("rust web browser"),
            NavigationTarget::Search {
                query: "rust web browser".into(),
                url: "https://duckduckgo.com/?q=rust+web+browser".into()
            }
        );
        assert_eq!(
            resolve("gpui"),
            NavigationTarget::Search {
                query: "gpui".into(),
                url: "https://duckduckgo.com/?q=gpui".into()
            }
        );
    }

    #[test]
    fn trailing_dot_is_not_a_host() {
        assert!(matches!(
            resolve("example."),
            NavigationTarget::Search { .. }
        ));
    }

    #[test]
    fn scheme_without_allowed_form_is_search() {
        assert!(matches!(
            resolve("mailto:someone@example.com"),
            NavigationTarget::Search { .. }
        ));
    }
}
