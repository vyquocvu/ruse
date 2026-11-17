//! Operating-system integration for the browser.
//!
//! Everything AppKit-specific that the app needs but the engine does not own:
//! turning a GPUI window into a native parent view, a local key monitor for
//! browser shortcuts while WebKit is focused, and opening external URLs.

use thiserror::Error;

#[cfg(target_os = "macos")]
mod macos;

pub mod shortcuts;

#[cfg(target_os = "macos")]
pub use macos::testing::KeySender;

pub use shortcuts::{BrowserShortcut, ShortcutMonitor};

/// Platform integration failures.
#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("the window does not expose an AppKit view")]
    NotAppKit,
    #[error("platform integration must run on the main thread")]
    NotMainThread,
    #[error("macOS rejected the shortcut monitor")]
    MonitorUnavailable,
    #[error("could not open the URL: {0}")]
    OpenUrl(String),
}

/// Returns the AppKit view that hosts GPUI content for `window`.
pub fn native_parent(
    window: &mut gpui::Window,
) -> Result<browser_engine::NativeParent, PlatformError> {
    #[cfg(target_os = "macos")]
    {
        macos::native_parent(window)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err(PlatformError::NotAppKit)
    }
}

/// Returns keyboard focus to GPUI's content view (browser chrome).
///
/// The native web view is a separate first responder; after the page had
/// focus, the browser must explicitly hand focus back before typing into the
/// address bar.
pub fn focus_chrome(window: &gpui::Window) -> Result<(), PlatformError> {
    #[cfg(target_os = "macos")]
    {
        macos::focus_content_view(window)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err(PlatformError::NotAppKit)
    }
}

/// Directory where the browser saves downloads.
///
/// `GPUI_BROWSER_DOWNLOADS_DIR` overrides `~/Downloads` for tests and power
/// users.
pub fn downloads_directory() -> std::path::PathBuf {
    if let Some(directory) = std::env::var_os("GPUI_BROWSER_DOWNLOADS_DIR") {
        return std::path::PathBuf::from(directory);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(home).join("Downloads");
    }
    std::env::temp_dir().join("gpui-browser-downloads")
}

/// Opens a local file with the default application.
pub fn open_path(path: &std::path::Path) -> Result<(), PlatformError> {
    #[cfg(target_os = "macos")]
    {
        macos::open_path(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err(PlatformError::NotAppKit)
    }
}

/// Reveals a local file in Finder.
pub fn reveal_path(path: &std::path::Path) -> Result<(), PlatformError> {
    #[cfg(target_os = "macos")]
    {
        macos::reveal_path(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err(PlatformError::NotAppKit)
    }
}

/// Hands a URL to the operating system (mail client, other apps).
pub fn open_external_url(url: &str) -> Result<(), PlatformError> {
    #[cfg(target_os = "macos")]
    {
        macos::open_external_url(url)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err(PlatformError::NotAppKit)
    }
}
