//! Favicon discovery fetching and decoding.
//!
//! WKWebView exposes no favicon API, so the browser asks the page for its
//! declared icon (via JavaScript), falls back to `/favicon.ico`, downloads it on
//! a worker thread and decodes it for GPUI. Downloads are bounded and only
//! http(s) URLs are accepted.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use gpui::RenderImage;

const MAX_ICON_BYTES: u64 = 512 * 1024;
const MAX_ICON_DIMENSION: u32 = 512;

/// Fetches the first usable icon for `origin`, preferring `declared_url`.
///
/// Blocking: always call from a worker thread.
pub fn fetch_icon(origin: &str, declared_url: Option<&str>) -> Option<Vec<u8>> {
    let fallback = format!("{origin}/favicon.ico");
    let candidates: Vec<&str> = match declared_url {
        Some(url) if url != fallback => vec![url, fallback.as_str()],
        Some(url) => vec![url],
        None => vec![fallback.as_str()],
    };

    let agent: ureq::Agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(6))
        .redirects(4)
        .user_agent("GPUI Browser/0.1 (favicon)")
        .build();

    for url in candidates {
        if !is_http_url(url) {
            continue;
        }
        let Ok(response) = agent.get(url).call() else {
            continue;
        };
        if !(200..300).contains(&response.status()) {
            continue;
        }
        let mut bytes = Vec::new();
        let mut limited = response.into_reader().take(MAX_ICON_BYTES + 1);
        if limited.read_to_end(&mut bytes).is_err() {
            continue;
        }
        if bytes.is_empty() || bytes.len() as u64 > MAX_ICON_BYTES {
            continue;
        }
        if decode_icon(&bytes).is_some() {
            return Some(bytes);
        }
    }
    None
}

pub fn is_http_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Decodes arbitrary image bytes into something GPUI can paint.
pub fn decode_icon(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let image = image::load_from_memory(bytes).ok()?;
    if image.width() == 0
        || image.height() == 0
        || image.width() > MAX_ICON_DIMENSION
        || image.height() > MAX_ICON_DIMENSION
    {
        return None;
    }
    Some(Arc::new(RenderImage::new(smallvec::smallvec![
        image::Frame::new(image.to_rgba8()),
    ])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_schemes() {
        assert!(!is_http_url("file:///etc/passwd"));
        assert!(!is_http_url("data:image/png;base64,AAAA"));
        assert!(!is_http_url("javascript:alert(1)"));
        assert!(is_http_url("https://example.com/favicon.ico"));
    }

    #[test]
    fn decodes_a_png() {
        // 1x1 red PNG.
        let png: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99,
            0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let image = decode_icon(png).expect("decode png");
        assert_eq!(image.size(0).width.0, 1);
        assert_eq!(image.size(0).height.0, 1);
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_icon(b"not an image").is_none());
        assert!(decode_icon(&[]).is_none());
    }
}
