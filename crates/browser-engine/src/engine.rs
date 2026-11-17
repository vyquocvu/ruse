//! The engine abstraction.
//!
//! The UI layer talks to web content exclusively through [`Engine`]. Concrete
//! adapters (WKWebView today, WebView2/WebKitGTK/CEF later) live behind this
//! boundary so browser chrome never depends on platform engine types.

use std::path::PathBuf;
use std::rc::Rc;

use async_channel::Receiver;

use crate::error::EngineError;
use crate::event::{EngineEvent, Viewport};

/// A handle to an AppKit `NSView` that will host the native web view.
///
/// The pointer is not owned by the engine. Callers must guarantee that the
/// view outlives the engine and that all engine calls happen on the main
/// thread.
#[derive(Debug, Clone, Copy)]
pub struct NativeParent {
    ns_view: *mut std::ffi::c_void,
}

impl NativeParent {
    /// Wrap a raw AppKit `NSView *`.
    ///
    /// # Safety
    ///
    /// `ns_view` must be a valid, non-null `NSView` pointer that stays alive
    /// for as long as the engine exists. The caller must only use the engine
    /// from the main thread.
    pub unsafe fn from_ns_view(ns_view: *mut std::ffi::c_void) -> Self {
        Self { ns_view }
    }

    pub fn as_ptr(&self) -> *mut std::ffi::c_void {
        self.ns_view
    }
}

/// Options used to create an engine instance.
#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub parent: NativeParent,
    pub viewport: Viewport,
    pub initial_url: Option<String>,
    /// When true the engine uses a non-persistent website data store, so
    /// cookies, caches and local storage are discarded with the session.
    pub private: bool,
}

/// Operations the browser can perform on a web page.
///
/// Methods must be called on the main thread. Implementations are not `Send`.
pub trait Engine {
    /// Navigates to an absolute URL.
    fn navigate(&self, url: &str) -> Result<(), EngineError>;

    /// Navigates back in the engine's history. Returns false when there is no
    /// entry to go back to.
    fn go_back(&self) -> bool;

    /// Navigates forward in the engine's history.
    fn go_forward(&self) -> bool;

    /// Reloads the current page.
    fn reload(&self);

    /// Reloads bypassing caches where the engine supports it.
    fn reload_bypassing_cache(&self);

    /// Stops the in-flight load.
    fn stop(&self);

    /// Runs JavaScript in the top frame, discarding the result.
    fn evaluate_script(&self, script: &str);

    /// Evaluates a JavaScript *expression* and returns its JSON-serialized
    /// value through [`EngineEvent::ScriptResult`].
    ///
    /// The expression is wrapped in `JSON.stringify`, so it must produce a
    /// JSON-serializable value.
    fn evaluate_script_json(&self, expression: &str) -> u64;

    /// Positions the native view.
    fn set_viewport(&self, viewport: Viewport);

    /// Shows or hides the native view without destroying the page.
    fn set_visible(&self, visible: bool);

    fn set_page_zoom(&self, zoom: f64);

    fn page_zoom(&self) -> f64;

    fn current_url(&self) -> Option<String>;

    fn title(&self) -> Option<String>;

    fn is_loading(&self) -> bool;

    fn can_go_back(&self) -> bool;

    fn can_go_forward(&self) -> bool;

    fn estimated_progress(&self) -> f64;

    /// Makes the web view the first responder so keyboard input goes to the
    /// page instead of the browser chrome.
    fn focus_page(&self);

    /// Captures the rendered page to a PNG file. The result arrives through
    /// [`EngineEvent::SnapshotSaved`] or [`EngineEvent::SnapshotFailed`].
    fn take_snapshot(&self, path: PathBuf);

    /// Removes the native view and stops loading. Further commands fail.
    fn close(&self);
}

/// An engine plus the event stream it produces.
pub struct EngineInstance {
    engine: Rc<dyn Engine>,
    events: Receiver<EngineEvent>,
}

impl EngineInstance {
    pub fn new(engine: Rc<dyn Engine>, events: Receiver<EngineEvent>) -> Self {
        Self { engine, events }
    }

    pub fn engine(&self) -> &Rc<dyn Engine> {
        &self.engine
    }

    pub fn into_parts(self) -> (Rc<dyn Engine>, Receiver<EngineEvent>) {
        (self.engine, self.events)
    }
}

/// Creates the platform web engine and starts delivering its events.
pub fn create_engine(options: EngineOptions) -> Result<EngineInstance, EngineError> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::create(options)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = options;
        Err(EngineError::UnsupportedPlatform)
    }
}
