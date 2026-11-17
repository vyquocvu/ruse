//! WebKit delegate implementations.
//!
//! WebKit holds delegates weakly, so the engine keeps strong references to the
//! delegate objects for as long as the web view exists. All callbacks run on
//! the main thread.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSObject, NSObjectProtocol, NSString, NSURL, NSURLResponse,
};
use objc2_web_kit::{
    WKDownload, WKDownloadDelegate, WKFrameInfo, WKMediaCaptureType, WKNavigation,
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKNavigationResponse,
    WKNavigationResponsePolicy, WKPermissionDecision, WKSecurityOrigin, WKUIDelegate, WKWebView,
    WKWindowFeatures,
};

use crate::event::EngineEvent;

use super::convert::{
    is_cancelled_error, ns_error_message, ns_string, string_from_ns, webview_title, webview_url,
};

/// Sends engine events without blocking WebKit's callbacks.
pub(super) struct EventSink {
    sender: async_channel::Sender<EngineEvent>,
}

impl EventSink {
    pub(super) fn new(sender: async_channel::Sender<EngineEvent>) -> Rc<Self> {
        Rc::new(Self { sender })
    }

    pub(super) fn send(&self, event: EngineEvent) {
        // The channel is unbounded, so this only fails when the receiver was
        // dropped (the tab is shutting down); dropping late events is correct.
        if let Err(error) = self.sender.try_send(event) {
            tracing::trace!(%error, "engine event receiver gone");
        }
    }
}

pub struct NavigationDelegateIvars {
    sink: Rc<EventSink>,
    downloads_dir: PathBuf,
    downloads: Rc<RefCell<Vec<Retained<DownloadDelegate>>>>,
}

define_class!(
    /// Delegate that translates WebKit navigation lifecycle callbacks into
    /// engine events and applies the browser's navigation policy.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GPUIBrowserNavigationDelegate"]
    #[ivars = NavigationDelegateIvars]
    pub(super) struct NavigationDelegate;

    unsafe impl NSObjectProtocol for NavigationDelegate {}

    unsafe impl WKNavigationDelegate for NavigationDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            web_view: &WKWebView,
            navigation_action: &WKNavigationAction,
            decision_handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let target_frame: Option<Retained<WKFrameInfo>> =
                unsafe { navigation_action.targetFrame() };
            let is_main_frame = target_frame
                .as_ref()
                .map(|frame| unsafe { frame.isMainFrame() })
                .unwrap_or(false);

            let request = unsafe { navigation_action.request() };
            let url = request
                .URL()
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string());

            let Some(url) = url else {
                decision_handler.call((WKNavigationActionPolicy::Cancel,));
                return;
            };

            if target_frame.is_none() && is_web_scheme(&url) {
                // A page asked for a new window/tab (target="_blank" or
                // window.open). The browser decides where it opens.
                self.ivars()
                    .sink
                    .send(EngineEvent::NewWindowRequested { url });
                decision_handler.call((WKNavigationActionPolicy::Cancel,));
                return;
            }

            if unsafe { navigation_action.shouldPerformDownload() } {
                // Let WebKit perform the download and drive our WKDownload
                // delegate; the destination is chosen there.
                decision_handler.call((WKNavigationActionPolicy::Download,));
                return;
            }

            if !is_web_scheme(&url) {
                // mailto:, tel:, custom app schemes and similar must be
                // handed to the OS, never loaded as a page.
                self.ivars()
                    .sink
                    .send(EngineEvent::ExternalSchemeRequested { url });
                decision_handler.call((WKNavigationActionPolicy::Cancel,));
                return;
            }

            if !is_main_frame && !is_web_scheme(&url) {
                decision_handler.call((WKNavigationActionPolicy::Cancel,));
                return;
            }

            decision_handler.call((WKNavigationActionPolicy::Allow,));
            tracing::trace!(%url, is_main_frame, "navigation allowed");
        }

        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn did_start_provisional(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            if let Some(url) = webview_url(web_view) {
                self.ivars()
                    .sink
                    .send(EngineEvent::NavigationStarted { url });
            }
        }

        #[unsafe(method(webView:didReceiveServerRedirectForProvisionalNavigation:))]
        fn did_receive_server_redirect(
            &self,
            web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
        ) {
            if let Some(url) = webview_url(web_view) {
                self.ivars().sink.send(EngineEvent::UrlChanged { url });
            }
        }

        #[unsafe(method(webView:didCommitNavigation:))]
        fn did_commit(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            if let Some(url) = webview_url(web_view) {
                self.ivars()
                    .sink
                    .send(EngineEvent::NavigationCommitted { url });
            }
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            let Some(url) = webview_url(web_view) else {
                return;
            };
            let title = webview_title(web_view);
            self.ivars().sink.send(EngineEvent::LoadFinished {
                url,
                title: title.clone(),
                can_go_back: unsafe { web_view.canGoBack() },
                can_go_forward: unsafe { web_view.canGoForward() },
            });
            if let Some(title) = title {
                self.ivars().sink.send(EngineEvent::TitleChanged { title });
            }
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(
            &self,
            web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &objc2_foundation::NSError,
        ) {
            if is_cancelled_error(error) {
                return;
            }
            let url = webview_url(web_view).unwrap_or_default();
            self.ivars().sink.send(EngineEvent::LoadFailed {
                url,
                message: ns_error_message(error),
                is_provisional: true,
            });
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &objc2_foundation::NSError,
        ) {
            if is_cancelled_error(error) {
                return;
            }
            let url = webview_url(web_view).unwrap_or_default();
            self.ivars().sink.send(EngineEvent::LoadFailed {
                url,
                message: ns_error_message(error),
                is_provisional: false,
            });
        }

        #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
        fn decide_response_policy(
            &self,
            _web_view: &WKWebView,
            navigation_response: &WKNavigationResponse,
            decision_handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
        ) {
            let can_show = unsafe { navigation_response.canShowMIMEType() };
            let mut is_download = !can_show;
            if !is_download {
                let response = unsafe { navigation_response.response() };
                if let Some(disposition) = content_disposition(&response)
                    && disposition.to_ascii_lowercase().contains("attachment")
                {
                    is_download = true;
                }
            }
            decision_handler.call((if is_download {
                WKNavigationResponsePolicy::Download
            } else {
                WKNavigationResponsePolicy::Allow
            },));
        }

        #[unsafe(method(webView:navigationAction:didBecomeDownload:))]
        fn action_became_download(
            &self,
            _web_view: &WKWebView,
            _navigation_action: &WKNavigationAction,
            download: &WKDownload,
        ) {
            self.attach_download(download);
        }

        #[unsafe(method(webView:navigationResponse:didBecomeDownload:))]
        fn response_became_download(
            &self,
            _web_view: &WKWebView,
            _navigation_response: &WKNavigationResponse,
            download: &WKDownload,
        ) {
            self.attach_download(download);
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn process_did_terminate(&self, _web_view: &WKWebView) {
            self.ivars().sink.send(EngineEvent::ProcessTerminated);
        }
    }
);

impl NavigationDelegate {
    pub(super) fn new(
        mtm: MainThreadMarker,
        sink: Rc<EventSink>,
        downloads_dir: PathBuf,
        downloads: Rc<RefCell<Vec<Retained<DownloadDelegate>>>>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavigationDelegateIvars {
            sink,
            downloads_dir,
            downloads,
        });
        // SAFETY: NSObject's init has no extra requirements.
        unsafe { msg_send![super(this), init] }
    }

    fn attach_download(&self, download: &WKDownload) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let delegate = DownloadDelegate::new(
            mtm,
            self.ivars().sink.clone(),
            self.ivars().downloads_dir.clone(),
        );
        // SAFETY: main-thread WebKit call; the delegate is retained by the
        // engine's registry for the lifetime of the web view.
        unsafe {
            download.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        }
        self.ivars().downloads.borrow_mut().push(delegate);
    }
}

/// Drains the Content-Disposition header from an HTTP response, if any.
fn content_disposition(response: &NSURLResponse) -> Option<String> {
    // SAFETY: NSURLResponse is a class cluster; the isKindOfClass check below
    // guarantees the downcast to NSHTTPURLResponse is valid.
    let object: &AnyObject = unsafe { &*(response as *const NSURLResponse as *const AnyObject) };
    let is_http: bool = unsafe { msg_send![object, isKindOfClass: NSHTTPURLResponse::class()] };
    if !is_http {
        return None;
    }
    let http: &NSHTTPURLResponse =
        unsafe { &*(object as *const AnyObject as *const NSHTTPURLResponse) };
    http.valueForHTTPHeaderField(&ns_string("Content-Disposition"))
        .map(|value| value.to_string())
}

pub struct DownloadDelegateIvars {
    sink: Rc<EventSink>,
    downloads_dir: PathBuf,
    path: RefCell<Option<PathBuf>>,
    download: RefCell<Option<Retained<WKDownload>>>,
}

define_class!(
    /// Chooses a destination inside the browser's downloads directory and
    /// reports completion/failure.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GPUIBrowserDownloadDelegate"]
    #[ivars = DownloadDelegateIvars]
    pub(super) struct DownloadDelegate;

    unsafe impl NSObjectProtocol for DownloadDelegate {}

    unsafe impl WKDownloadDelegate for DownloadDelegate {
        #[unsafe(method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))]
        fn decide_destination(
            &self,
            download: &WKDownload,
            response: &NSURLResponse,
            suggested_filename: &NSString,
            completion_handler: &block2::DynBlock<dyn Fn(*mut NSURL)>,
        ) {
            let suggested = string_from_ns(suggested_filename);
            let path =
                browser_security::unique_download_path(&self.ivars().downloads_dir, &suggested);
            if let Some(parent) = path.parent()
                && let Err(error) = std::fs::create_dir_all(parent)
            {
                tracing::warn!(%error, "could not create downloads directory");
            }
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("download")
                .to_owned();
            let url = unsafe { download.originalRequest() }
                .and_then(|request| request.URL())
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string())
                .unwrap_or_default();
            let expected_bytes = response.expectedContentLength();

            // SAFETY: retaining the download keeps it valid for cancellation;
            // main-thread WebKit object.
            unsafe {
                if let Some(retained) =
                    Retained::retain(download as *const WKDownload as *mut WKDownload)
                {
                    *self.ivars().download.borrow_mut() = Some(retained);
                }
            }
            *self.ivars().path.borrow_mut() = Some(path.clone());
            self.ivars().sink.send(EngineEvent::DownloadStarted {
                url,
                filename,
                path: path.clone(),
                expected_bytes,
            });

            let file_url = NSURL::fileURLWithPath(&ns_string(&path.to_string_lossy()));
            completion_handler.call((Retained::as_ptr(&file_url) as *mut NSURL,));
        }

        #[unsafe(method(downloadDidFinish:))]
        fn did_finish(&self, _download: &WKDownload) {
            self.finish(true, None);
        }

        #[unsafe(method(download:didFailWithError:resumeData:))]
        fn did_fail(&self, _download: &WKDownload, error: &NSError, _resume_data: Option<&NSData>) {
            self.finish(false, Some(ns_error_message(error)));
        }
    }
);

impl DownloadDelegate {
    fn new(mtm: MainThreadMarker, sink: Rc<EventSink>, downloads_dir: PathBuf) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DownloadDelegateIvars {
            sink,
            downloads_dir,
            path: RefCell::new(None),
            download: RefCell::new(None),
        });
        // SAFETY: NSObject's init has no extra requirements.
        unsafe { msg_send![super(this), init] }
    }

    fn finish(&self, success: bool, message: Option<String>) {
        let path = self.ivars().path.borrow_mut().take();
        if let Some(path) = path {
            self.ivars().sink.send(EngineEvent::DownloadFinished {
                path,
                success,
                message,
            });
        }
        let _ = self.ivars().download.borrow_mut().take();
    }

    pub(super) fn destination(&self) -> Option<PathBuf> {
        self.ivars().path.borrow().clone()
    }

    pub(super) fn cancel(&self) {
        let download = self.ivars().download.borrow();
        if let Some(download) = download.as_ref() {
            let completion = RcBlock::new(|_: *mut NSData| {});
            // SAFETY: main-thread WebKit call on a download we retain.
            unsafe {
                download.cancel(Some(&completion));
            }
        }
    }
}

pub struct UiDelegateIvars {
    sink: Rc<EventSink>,
}

define_class!(
    /// Handles chrome-level requests WebKit asks the embedding application to
    /// resolve: new windows, media capture permissions and JS dialogs.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GPUIBrowserUIDelegate"]
    #[ivars = UiDelegateIvars]
    pub(super) struct UiDelegate;

    unsafe impl NSObjectProtocol for UiDelegate {}

    unsafe impl WKUIDelegate for UiDelegate {
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        unsafe fn create_web_view(
            &self,
            _web_view: &WKWebView,
            _configuration: &objc2_web_kit::WKWebViewConfiguration,
            navigation_action: &WKNavigationAction,
            _window_features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            let request = unsafe { navigation_action.request() };
            let url = request
                .URL()
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string());
            if let Some(url) = url {
                if is_web_scheme(&url) {
                    self.ivars()
                        .sink
                        .send(EngineEvent::NewWindowRequested { url });
                } else {
                    self.ivars()
                        .sink
                        .send(EngineEvent::ExternalSchemeRequested { url });
                }
            }
            // Returning nil cancels WebKit's own window; the browser opens the
            // URL in a tab it controls instead.
            None
        }

        #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
        fn request_media_capture(
            &self,
            _web_view: &WKWebView,
            _origin: &WKSecurityOrigin,
            _frame: &WKFrameInfo,
            capture_type: WKMediaCaptureType,
            decision_handler: &block2::DynBlock<dyn Fn(WKPermissionDecision)>,
        ) {
            // Deny until the permission UI exists; never auto-grant device
            // access to page content.
            tracing::debug!(?capture_type, "denying media capture request");
            decision_handler.call((WKPermissionDecision::Deny,));
        }

        #[unsafe(method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn run_alert(
            &self,
            _web_view: &WKWebView,
            message: &NSString,
            _frame: &WKFrameInfo,
            completion_handler: &block2::DynBlock<dyn Fn()>,
        ) {
            tracing::info!(message = %string_from_ns(message), "javascript alert (auto-dismissed)");
            completion_handler.call(());
        }
    }
);

impl UiDelegate {
    pub(super) fn new(mtm: MainThreadMarker, sink: Rc<EventSink>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(UiDelegateIvars { sink });
        // SAFETY: NSObject's init has no extra requirements.
        unsafe { msg_send![super(this), init] }
    }
}

/// Schemes that the browser itself renders. Everything else is delegated to
/// the operating system after explicit policy checks.
fn is_web_scheme(url: &str) -> bool {
    let scheme_end = url.find(':');
    let Some(scheme_end) = scheme_end else {
        return false;
    };
    matches!(
        url[..scheme_end].to_ascii_lowercase().as_str(),
        "http" | "https" | "about" | "data" | "blob" | "file"
    )
}
