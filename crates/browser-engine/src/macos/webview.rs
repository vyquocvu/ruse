//! `WKWebView`-backed implementation of [`Engine`].

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSImage, NSView,
};
use objc2_foundation::{NSDictionary, NSError, NSString};
use objc2_web_kit::{
    WKSnapshotConfiguration, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};

use crate::engine::{Engine, EngineInstance, EngineOptions};
use crate::error::EngineError;
use crate::event::{EngineEvent, Viewport};

use super::convert::{
    main_thread_marker, ns_error_message, ns_string, parse_load_url, viewport_to_frame,
    webview_title, webview_url,
};
use super::delegates::{EventSink, NavigationDelegate, UiDelegate};

/// A `WKWebView` driven by the browser.
pub(super) struct WkEngine {
    webview: Retained<WKWebView>,
    navigation_delegate: Retained<NavigationDelegate>,
    ui_delegate: Retained<UiDelegate>,
    /// Keeps AppKit's parent view alive for as long as the web view is a
    /// subview of it.
    parent_view: Retained<NSView>,
    sink: Rc<EventSink>,
    viewport: Cell<Viewport>,
    visible: Cell<bool>,
    closed: Cell<bool>,
    script_sequence: AtomicU64,
}

impl WkEngine {
    pub(super) fn create(options: EngineOptions) -> Result<EngineInstance, EngineError> {
        let mtm = main_thread_marker()?;
        let parent_ptr = options.parent.as_ptr();
        if parent_ptr.is_null() {
            return Err(EngineError::InvalidParentView);
        }
        // SAFETY: the caller guarantees the view pointer is valid for the
        // lifetime of the engine; retaining it makes teardown ordering safe.
        let parent_view: Retained<NSView> =
            unsafe { Retained::retain(parent_ptr.cast::<NSView>()) }
                .ok_or(EngineError::InvalidParentView)?;

        let (sender, receiver) = async_channel::unbounded();
        let sink = EventSink::new(sender);

        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        if options.private {
            let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
            unsafe { configuration.setWebsiteDataStore(&store) };
        }
        // Let WebKit surface window.open() to the UI delegate; the browser
        // decides whether a new tab opens, instead of WebKit doing it.
        let preferences = unsafe { configuration.preferences() };
        unsafe { preferences.setJavaScriptCanOpenWindowsAutomatically(true) };

        let parent_height = ns_view_height(&parent_view);
        let frame = viewport_to_frame(options.viewport, parent_height);
        // SAFETY: initialized on the main thread with a valid configuration.
        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
        };

        let navigation_delegate = NavigationDelegate::new(mtm, sink.clone());
        let ui_delegate = UiDelegate::new(mtm, sink.clone());
        // SAFETY: delegates are retained by this struct and outlive the web view.
        unsafe {
            webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation_delegate)));
            webview.setUIDelegate(Some(ProtocolObject::from_ref(&*ui_delegate)));
            webview.setAllowsBackForwardNavigationGestures(true);
            webview.setInspectable(cfg!(debug_assertions));
        }
        parent_view.addSubview(&webview);

        let engine = Self {
            webview,
            navigation_delegate,
            ui_delegate,
            parent_view,
            sink,
            viewport: Cell::new(options.viewport),
            visible: Cell::new(true),
            closed: Cell::new(false),
            script_sequence: AtomicU64::new(1),
        };

        if let Some(url) = options.initial_url {
            engine.navigate(&url)?;
        }

        Ok(EngineInstance::new(Rc::new(engine), receiver))
    }

    fn apply_viewport(&self) {
        let parent_height = ns_view_height(&self.parent_view);
        let frame = viewport_to_frame(self.viewport.get(), parent_height);
        self.webview.setFrame(frame);
    }
}

fn ns_view_height(view: &NSView) -> f64 {
    view.bounds().size.height
}

impl Engine for WkEngine {
    fn navigate(&self, url: &str) -> Result<(), EngineError> {
        if self.closed.get() {
            return Err(EngineError::Closed);
        }
        let ns_url = parse_load_url(url)?;
        let request = objc2_foundation::NSURLRequest::requestWithURL(&ns_url);
        // SAFETY: main-thread WebKit call with a valid request.
        unsafe {
            self.webview.loadRequest(&request);
        }
        Ok(())
    }

    fn go_back(&self) -> bool {
        // SAFETY: main-thread WebKit call.
        unsafe {
            if self.webview.canGoBack() {
                self.webview.goBack();
                true
            } else {
                false
            }
        }
    }

    fn go_forward(&self) -> bool {
        // SAFETY: main-thread WebKit call.
        unsafe {
            if self.webview.canGoForward() {
                self.webview.goForward();
                true
            } else {
                false
            }
        }
    }

    fn reload(&self) {
        // SAFETY: main-thread WebKit call.
        unsafe {
            self.webview.reload();
        }
    }

    fn reload_bypassing_cache(&self) {
        // SAFETY: main-thread WebKit call.
        unsafe {
            self.webview.reloadFromOrigin();
        }
    }

    fn stop(&self) {
        // SAFETY: main-thread WebKit call.
        unsafe {
            self.webview.stopLoading();
        }
    }

    fn evaluate_script(&self, script: &str) {
        if self.closed.get() {
            return;
        }
        let script = ns_string(script);
        // SAFETY: main-thread WebKit call.
        unsafe {
            self.webview
                .evaluateJavaScript_completionHandler(&script, None);
        }
    }

    fn evaluate_script_json(&self, expression: &str) -> u64 {
        let id = self.script_sequence.fetch_add(1, Ordering::Relaxed);
        if self.closed.get() {
            self.sink.send(EngineEvent::ScriptError {
                id,
                message: "engine closed".to_owned(),
            });
            return id;
        }

        // Always return a JSON string so the completion handler receives a
        // bridged NSString, and so JavaScript exceptions are reported to the
        // browser instead of silently failing.
        let source = format!(
            "(() => {{ try {{ return JSON.stringify({{ ok: true, value: ({expression}) }}); }}              catch (error) {{ return JSON.stringify({{ ok: false, error: String((error && error.message) || error) }}); }} }})()"
        );
        let script = ns_string(&source);
        let sink = self.sink.clone();
        let completion = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
            handle_script_completion(&sink, id, value, error);
        });
        // SAFETY: main-thread WebKit call.
        unsafe {
            self.webview
                .evaluateJavaScript_completionHandler(&script, Some(&completion));
        }
        id
    }

    fn set_viewport(&self, viewport: Viewport) {
        let clamped = Viewport::new(
            viewport.x,
            viewport.y,
            viewport.width.max(0.0),
            viewport.height.max(0.0),
        );
        self.viewport.set(clamped);
        if !self.closed.get() {
            self.apply_viewport();
        }
    }

    fn set_visible(&self, visible: bool) {
        if self.visible.get() == visible || self.closed.get() {
            return;
        }
        self.visible.set(visible);
        self.webview.setHidden(!visible);
    }

    fn set_page_zoom(&self, zoom: f64) {
        // SAFETY: main-thread WebKit call. Clamp to a sane browser range.
        unsafe { self.webview.setPageZoom(zoom.clamp(0.25, 5.0)) };
    }

    fn page_zoom(&self) -> f64 {
        // SAFETY: main-thread WebKit call.
        unsafe { self.webview.pageZoom() }
    }

    fn current_url(&self) -> Option<String> {
        webview_url(&self.webview)
    }

    fn title(&self) -> Option<String> {
        webview_title(&self.webview)
    }

    fn is_loading(&self) -> bool {
        // SAFETY: main-thread WebKit call.
        unsafe { self.webview.isLoading() }
    }

    fn can_go_back(&self) -> bool {
        // SAFETY: main-thread WebKit call.
        unsafe { self.webview.canGoBack() }
    }

    fn can_go_forward(&self) -> bool {
        // SAFETY: main-thread WebKit call.
        unsafe { self.webview.canGoForward() }
    }

    fn estimated_progress(&self) -> f64 {
        // SAFETY: main-thread WebKit call.
        unsafe { self.webview.estimatedProgress() }
    }

    fn focus_page(&self) {
        let window = self.webview.window();
        if let Some(window) = window {
            window.makeFirstResponder(Some(&self.webview));
        }
    }

    fn take_snapshot(&self, path: PathBuf) {
        let mtm = match main_thread_marker() {
            Ok(mtm) => mtm,
            Err(_) => return,
        };
        let sink = self.sink.clone();
        let event_path = path.clone();
        let completion = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            if !error.is_null() {
                // SAFETY: WebKit passes a valid NSError when non-null.
                let message = unsafe { ns_error_message(&*error) };
                sink.send(EngineEvent::SnapshotFailed { message });
                return;
            }
            if image.is_null() {
                sink.send(EngineEvent::SnapshotFailed {
                    message: "no image returned".to_owned(),
                });
                return;
            }
            // SAFETY: non-null NSImage produced by WebKit.
            let image = unsafe { &*image };
            match write_png(image, &path) {
                Ok((width, height)) => sink.send(EngineEvent::SnapshotSaved {
                    path: event_path.clone(),
                    width,
                    height,
                }),
                Err(message) => sink.send(EngineEvent::SnapshotFailed { message }),
            }
        });
        let configuration = unsafe { WKSnapshotConfiguration::new(mtm) };
        // SAFETY: main-thread WebKit call with retained completion handler.
        unsafe {
            self.webview
                .takeSnapshotWithConfiguration_completionHandler(Some(&configuration), &completion);
        }
    }

    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        if MainThreadMarker::new().is_none() {
            tracing::error!("web engine dropped off the main thread; skipping AppKit teardown");
            return;
        }
        // SAFETY: main-thread AppKit/WebKit teardown.
        unsafe {
            self.webview.stopLoading();
            self.webview.setNavigationDelegate(None);
            self.webview.setUIDelegate(None);
            self.webview.removeFromSuperview();
        }
        let _ = &self.navigation_delegate;
        let _ = &self.ui_delegate;
    }
}

impl Drop for WkEngine {
    fn drop(&mut self) {
        self.close();
        let _ = &self.parent_view;
    }
}

fn handle_script_completion(sink: &EventSink, id: u64, value: *mut AnyObject, error: *mut NSError) {
    if !error.is_null() {
        // SAFETY: WebKit passes a valid NSError when non-null.
        let message = unsafe { ns_error_message(&*error) };
        sink.send(EngineEvent::ScriptError { id, message });
        return;
    }
    if value.is_null() {
        sink.send(EngineEvent::ScriptError {
            id,
            message: "script returned no value".to_owned(),
        });
        return;
    }

    // WebKit bridges JavaScript JSON strings to NSString. Be defensive about
    // other bridges so a type mismatch reports an error instead of crashing.
    // SAFETY: isKindOfClass is a standard NSObject method.
    let is_string: bool = unsafe { msg_send![value, isKindOfClass: NSString::class()] };
    if !is_string {
        sink.send(EngineEvent::ScriptError {
            id,
            message: "script result was not a JSON string".to_owned(),
        });
        return;
    }
    // SAFETY: the object was just checked to be an NSString.
    let text = unsafe { (*(value as *const NSString)).to_string() };

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut object)) => {
            let ok = object
                .remove("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if ok {
                let value = object.remove("value").unwrap_or(serde_json::Value::Null);
                sink.send(EngineEvent::ScriptResult { id, value });
            } else {
                let message = object
                    .remove("error")
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown JavaScript error".to_owned());
                sink.send(EngineEvent::ScriptError { id, message });
            }
        }
        Ok(value) => sink.send(EngineEvent::ScriptResult { id, value }),
        Err(error) => sink.send(EngineEvent::ScriptError {
            id,
            message: format!("could not decode script result: {error}"),
        }),
    }
}

fn write_png(image: &NSImage, path: &std::path::Path) -> Result<(u32, u32), String> {
    let tiff = image
        .TIFFRepresentation()
        .ok_or_else(|| "snapshot has no TIFF representation".to_owned())?;
    let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)
        .ok_or_else(|| "could not decode snapshot bitmap".to_owned())?;
    let properties: Retained<NSDictionary<NSBitmapImageRepPropertyKey, AnyObject>> =
        NSDictionary::new();
    // SAFETY: main-thread AppKit call.
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }
    .ok_or_else(|| "could not encode snapshot as PNG".to_owned())?;
    std::fs::write(path, png.to_vec()).map_err(|error| error.to_string())?;
    let width = bitmap.pixelsWide().max(0) as u32;
    let height = bitmap.pixelsHigh().max(0) as u32;
    Ok((width, height))
}
