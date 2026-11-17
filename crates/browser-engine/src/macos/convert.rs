//! Small conversions between Rust values and WebKit/Foundation objects.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::WKWebView;

use crate::error::EngineError;
use crate::event::Viewport;

/// Creates a Foundation string.
pub fn ns_string(value: &str) -> Retained<NSString> {
    NSString::from_str(value)
}

/// Converts a Foundation string to a Rust `String`.
pub fn string_from_ns(value: &NSString) -> String {
    value.to_string()
}

/// Returns the current URL of a web view as a string.
pub fn webview_url(webview: &WKWebView) -> Option<String> {
    // SAFETY: called on the main thread from engine or delegate code.
    unsafe {
        webview
            .URL()
            .and_then(|url| url.absoluteString())
            .map(|url| url.to_string())
    }
}

/// Returns the current title of a web view as a string.
pub fn webview_title(webview: &WKWebView) -> Option<String> {
    // SAFETY: called on the main thread from engine or delegate code.
    unsafe { webview.title().map(|title| title.to_string()) }
}

/// Parses an absolute URL string for loading.
pub fn parse_load_url(url: &str) -> Result<Retained<NSURL>, EngineError> {
    let ns_url = ns_string(url);
    NSURL::URLWithString(&ns_url).ok_or_else(|| EngineError::InvalidUrl(url.to_owned()))
}

/// Formats an `NSError` into a human-readable message.
pub fn ns_error_message(error: &objc2_foundation::NSError) -> String {
    error.localizedDescription().to_string()
}

/// Returns the `NSURLErrorDomain` code for a cancelled load (`NSURLErrorCancelled`).
pub fn is_cancelled_error(error: &objc2_foundation::NSError) -> bool {
    const NSURL_ERROR_CANCELLED: isize = -999;
    error.domain().to_string() == "NSURLErrorDomain" && error.code() == NSURL_ERROR_CANCELLED
}

/// Converts a top-left-origin viewport into an AppKit frame for a subview of
/// `parent`, whose coordinate system has a bottom-left origin.
pub fn viewport_to_frame(viewport: Viewport, parent_height: f64) -> NSRect {
    let y = parent_height - viewport.y - viewport.height;
    NSRect {
        origin: NSPoint { x: viewport.x, y },
        size: NSSize {
            width: viewport.width,
            height: viewport.height,
        },
    }
}

/// Convenience constructor for `MainThreadMarker`-bound APIs.
pub fn main_thread_marker() -> Result<MainThreadMarker, EngineError> {
    MainThreadMarker::new().ok_or(EngineError::NotMainThread)
}
