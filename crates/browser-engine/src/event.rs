//! Engine events and viewport geometry.

use std::path::PathBuf;

/// A rectangle in AppKit points, with a top-left origin relative to the
/// window's content area.
///
/// GPUI lays out its elements in logical points (see `gpui::Pixels`), which is
/// the same unit AppKit uses for `NSView::frame`, so no scale conversion is
/// required when handing a viewport to the native engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Viewport {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }
}

/// Messages emitted by a web engine instance.
///
/// Events are delivered on the main thread through an unbounded channel. The
/// UI layer drains the channel and folds events into browser state; the engine
/// itself never touches UI state.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// A main-frame navigation started (before any content is committed).
    NavigationStarted { url: String },
    /// Content for a main-frame navigation arrived and the URL is committed.
    NavigationCommitted { url: String },
    /// A main-frame navigation finished loading.
    LoadFinished {
        url: String,
        title: Option<String>,
        can_go_back: bool,
        can_go_forward: bool,
    },
    /// A main-frame navigation failed. `is_provisional` is true when the
    /// failure happened before content was committed.
    LoadFailed {
        url: String,
        message: String,
        is_provisional: bool,
    },
    /// The page title changed (fires on load completion and on same-document
    /// title updates reported by the engine).
    TitleChanged { title: String },
    /// The visible URL changed without a full navigation (history API, hash).
    UrlChanged { url: String },
    /// The page requested a new window/tab (for example `target="_blank"`).
    NewWindowRequested { url: String },
    /// A navigation to a non-web scheme that must be handled by the OS.
    ExternalSchemeRequested { url: String },
    /// A download started and a destination file was chosen.
    DownloadStarted {
        url: String,
        filename: String,
        path: PathBuf,
        expected_bytes: i64,
    },
    /// Website data (cookies, caches, storage) was cleared.
    BrowsingDataCleared,
    /// A download finished or failed.
    DownloadFinished {
        path: PathBuf,
        success: bool,
        message: Option<String>,
    },
    /// The web content process crashed or was killed by the system.
    ProcessTerminated,
    /// Result of a find-in-page request.
    FindResult { found: bool },
    /// Result of [`crate::Engine::evaluate_script_json`].
    ScriptResult { id: u64, value: serde_json::Value },
    /// Failure of [`crate::Engine::evaluate_script_json`].
    ScriptError { id: u64, message: String },
    /// A page snapshot was written to disk.
    SnapshotSaved {
        path: PathBuf,
        width: u32,
        height: u32,
    },
    /// A page snapshot could not be produced.
    SnapshotFailed { message: String },
}
