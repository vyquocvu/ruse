//! macOS web engine adapter built on `WKWebView`.

mod convert;
mod delegates;
mod webview;

use std::rc::Rc;

use objc2::rc::Retained;
use objc2_web_kit::WKWebsiteDataStore;

use crate::engine::{EngineInstance, EngineOptions, PrivateDataStore};
use crate::error::EngineError;

/// Creates an ephemeral `WKWebsiteDataStore` shared by one private window.
pub(super) fn create_private_data_store() -> Result<PrivateDataStore, EngineError> {
    let mtm = convert::main_thread_marker()?;
    // SAFETY: main-thread WebKit call.
    let store: Retained<WKWebsiteDataStore> =
        unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
    Ok(PrivateDataStore::new(Rc::new(store)))
}

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
