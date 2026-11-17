//! Browser keyboard shortcuts that must work while the web view is focused.
//!
//! WebKit's page view is the window's first responder, so key events never reach
//! GPUI's view. A local `NSEvent` monitor sees key-down events before dispatch,
//! translates the browser commands and swallows only those it handles. Everything
//! else (typing, page shortcuts, clipboard) is passed through unchanged.

use async_channel::Sender;

/// Browser commands reachable from the keyboard while a page has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserShortcut {
    FocusLocation,
    Back,
    Forward,
    Reload,
    HardReload,
    Stop,
    NewTab,
    NewPrivateWindow,
    CloseTab,
    ReopenClosedTab,
    NextTab,
    PreviousTab,
    FindInPage,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    OpenSettings,
    ToggleBookmark,
    ToggleBookmarks,
    FindNext,
    FindPrevious,
    ToggleDownloads,
    CopyUrl,
    CommandPalette,
    Quit,
}

/// Installs the shortcut monitor. Dropping it removes the monitor.
pub struct ShortcutMonitor {
    #[cfg(target_os = "macos")]
    inner: Option<crate::macos::EventMonitor>,
}

impl ShortcutMonitor {
    /// Installs a local key-down monitor that forwards browser shortcuts.
    ///
    /// Must be called on the main thread.
    pub fn install(sender: Sender<BrowserShortcut>) -> Result<Self, crate::PlatformError> {
        #[cfg(target_os = "macos")]
        {
            let inner = crate::macos::install_shortcut_monitor(sender)?;
            Ok(Self { inner: Some(inner) })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = sender;
            Err(crate::PlatformError::NotAppKit)
        }
    }
}

impl Drop for ShortcutMonitor {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        {
            self.inner.take();
        }
    }
}
