//! macOS web engine adapter built on `WKWebView`.

mod convert;
mod delegates;
mod webview;

use crate::engine::{EngineInstance, EngineOptions};
use crate::error::EngineError;

/// Creates a `WKWebView` engine attached to the provided AppKit view.
pub(super) fn create(options: EngineOptions) -> Result<EngineInstance, EngineError> {
    webview::WkEngine::create(options)
}

/// Whether the current WebKit supports the in-app Web Inspector.
///
/// `WKWebView.isInspectable` is available on macOS 13.3 and newer; the browser
/// only offers devtools when this returns true.
pub fn is_web_inspector_available() -> bool {
    true
}
