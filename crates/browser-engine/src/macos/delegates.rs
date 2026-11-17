//! WebKit delegate implementations.
//!
//! WebKit holds delegates weakly, so the engine keeps strong references to the
//! delegate objects for as long as the web view exists. All callbacks run on
//! the main thread.

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use objc2_web_kit::{
    WKFrameInfo, WKMediaCaptureType, WKNavigation, WKNavigationAction,
    WKNavigationActionPolicy, WKNavigationDelegate, WKPermissionDecision, WKSecurityOrigin,
    WKUIDelegate, WKWebView, WKWindowFeatures,
};

use crate::event::EngineEvent;

use super::convert::{is_cancelled_error, ns_error_message, string_from_ns, webview_title, webview_url};

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
            let target_frame: Option<Retained<WKFrameInfo>> = unsafe { navigation_action.targetFrame() };
            let is_main_frame = target_frame
                .as_ref()
                .map(|frame| unsafe { frame.isMainFrame() })
                .unwrap_or(false);

            let request = unsafe { navigation_action.request() };
            let url = request.URL()
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string());

            let Some(url) = url else {
                decision_handler.call((WKNavigationActionPolicy::Cancel,));
                return;
            };

            if target_frame.is_none() && is_web_scheme(&url) {
                // A page asked for a new window/tab (target="_blank" or
                // window.open). The browser decides where it opens.
                self.ivars().sink.send(EngineEvent::NewWindowRequested { url });
                decision_handler.call((WKNavigationActionPolicy::Cancel,));
                return;
            }

            if unsafe { navigation_action.shouldPerformDownload() } {
                self.ivars().sink.send(EngineEvent::DownloadRequested {
                    url,
                    suggested_filename: None,
                });
                decision_handler.call((WKNavigationActionPolicy::Cancel,));
                return;
            }

            if !is_web_scheme(&url) {
                // mailto:, tel:, custom app schemes and similar must be
                // handed to the OS, never loaded as a page.
                self.ivars().sink.send(EngineEvent::ExternalSchemeRequested { url });
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
        fn did_start_provisional(
            &self,
            web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
        ) {
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

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn process_did_terminate(&self, _web_view: &WKWebView) {
            self.ivars().sink.send(EngineEvent::ProcessTerminated);
        }
    }
);

impl NavigationDelegate {
    pub(super) fn new(mtm: MainThreadMarker, sink: Rc<EventSink>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavigationDelegateIvars { sink });
        // SAFETY: NSObject's init has no extra requirements.
        unsafe { msg_send![super(this), init] }
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
            let url = request.URL()
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string());
            if let Some(url) = url {
                if is_web_scheme(&url) {
                    self.ivars().sink.send(EngineEvent::NewWindowRequested { url });
                } else {
                    self.ivars().sink.send(EngineEvent::ExternalSchemeRequested { url });
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
