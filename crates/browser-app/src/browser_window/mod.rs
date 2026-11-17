//! The browser window: tabs, chrome, engine lifetimes and event pumps.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use browser_core::{
    BrowserSettings, NavigationEvent, SessionSnapshot, Tab, TabId, TabSet, resolve_input,
};
use browser_engine::{Engine, EngineEvent, EngineOptions, Viewport, create_engine};
use browser_platform::{BrowserShortcut, ShortcutMonitor, native_parent, open_external_url};
use browser_storage::Storage;
use browser_ui::{TextInput, TextInputEvent, Theme};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, RenderImage, SharedString, Subscription, Timer,
    WeakEntity, Window,
};

use crate::favicon;

mod render;

use crate::actions::{
    Back, CloseTab, DuplicateTab, FindInPage, FocusLocation, Forward, HardReload, NewTab, NextTab,
    OpenSettings, PreviousTab, Quit, Reload, ReopenClosedTab, Stop, ZoomIn, ZoomOut, ZoomReset,
};

pub struct BrowserWindow {
    tabs: TabSet,
    runtimes: HashMap<TabId, Rc<dyn Engine>>,
    address: Entity<TextInput>,
    focus_handle: FocusHandle,
    settings: BrowserSettings,
    storage: Option<Storage>,
    /// Serialises session writes; see `start_session_writer`.
    session_sender: Option<async_channel::Sender<SessionSnapshot>>,
    /// Origin -> icon. Successful icons are reused by every page on the origin.
    favicon_cache: HashMap<String, Arc<RenderImage>>,
    tab_favicons: HashMap<TabId, Arc<RenderImage>>,
    /// Pages that produced no usable icon, so they are not retried every load.
    favicon_misses: HashSet<String>,
    /// Page URLs whose icon is currently being fetched.
    favicon_requests: HashSet<String>,
    /// JavaScript evaluation id -> (tab, origin, page URL) for icon discovery.
    pending_favicons: HashMap<u64, (TabId, String, String)>,
    shortcuts: Option<ShortcutMonitor>,
    subscriptions: Vec<Subscription>,
    status: Option<SharedString>,
}

impl Focusable for BrowserWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BrowserWindow {
    pub fn new(
        cx: &mut Context<Self>,
        address: Entity<TextInput>,
        settings: BrowserSettings,
        storage: Option<Storage>,
    ) -> Self {
        Self {
            tabs: TabSet::new(),
            runtimes: HashMap::new(),
            address,
            focus_handle: cx.focus_handle(),
            settings,
            storage,
            session_sender: None,
            favicon_cache: HashMap::new(),
            tab_favicons: HashMap::new(),
            favicon_misses: HashSet::new(),
            favicon_requests: HashSet::new(),
            pending_favicons: HashMap::new(),
            shortcuts: None,
            subscriptions: Vec::new(),
            status: None,
        }
    }

    /// Wires up window-scoped services and opens the startup tab.
    pub fn attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.install_shortcut_monitor(window, cx);
        self.start_session_writer(cx);
        self.spawn_ticker(window, cx);
        self.observe_address(window, cx);
        self.observe_appearance(window, cx);
        if self.restore_session(window, cx) {
            self.focus_address(window, cx);
        } else {
            let homepage = self.settings.homepage_or_default().to_owned();
            self.open_tab(&homepage, window, cx);
            self.focus_address(window, cx);
        }
    }

    fn install_shortcut_monitor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (sender, receiver) = async_channel::unbounded();
        match ShortcutMonitor::install(sender) {
            Ok(monitor) => {
                self.shortcuts = Some(monitor);
                cx.spawn_in(window, async move |weak, cx| {
                    while let Ok(shortcut) = receiver.recv().await {
                        if weak
                            .update_in(cx, |this, window, cx| {
                                this.handle_shortcut(shortcut, window, cx)
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => {
                tracing::warn!(%error, "browser shortcuts while a page is focused are unavailable");
                self.status = Some(format!("Shortcut monitor unavailable: {error}").into());
            }
        }
    }

    fn observe_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let subscription = cx.subscribe_in(
            &self.address,
            window,
            |this, _input, event: &TextInputEvent, window, cx| match event {
                TextInputEvent::Submitted(text) => this.navigate(text, window, cx),
                TextInputEvent::Cancelled => {
                    this.sync_address(window, cx);
                    if let Some(engine) = this.active_engine() {
                        engine.focus_page();
                    }
                    cx.notify();
                }
                TextInputEvent::Changed(_) => {}
            },
        );
        self.subscriptions.push(subscription);
    }

    fn observe_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let subscription = cx.observe_window_appearance(window, |_this, window, cx| {
            Theme::install(cx, Theme::for_appearance(window.appearance()));
            cx.notify();
        });
        self.subscriptions.push(subscription);
    }

    fn spawn_ticker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |weak, cx| {
            loop {
                Timer::after(Duration::from_millis(90)).await;
                let alive = weak.update_in(cx, |this, window, cx| this.tick(window, cx));
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Polls engine state that WebKit does not notify us about cheaply: load
    /// progress, SPA titles and same-document URL changes.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.tabs.active_id() else {
            return;
        };
        let Some(engine) = self.runtimes.get(&id).cloned() else {
            return;
        };
        let mut changed = false;
        if let Some(tab) = self.tabs.tab_mut(id) {
            if tab.navigation.is_loading() {
                let before = tab.navigation.progress_percent;
                tab.navigation.set_progress(engine.estimated_progress());
                changed |= before != tab.navigation.progress_percent;
            }
            if let Some(title) = engine.title().filter(|title| !title.trim().is_empty())
                && title != tab.navigation.title
            {
                tab.navigation
                    .apply(NavigationEvent::TitleChanged { title });
                changed = true;
            }
            if let Some(url) = engine.current_url()
                && url != tab.navigation.url
                && !tab.navigation.url.is_empty()
            {
                tab.navigation.apply(NavigationEvent::UrlChanged { url });
                changed = true;
            }
            if changed {
                set_window_title(window, &tab.navigation);
            }
        }
        if changed {
            self.sync_address(window, cx);
            cx.notify();
        }
    }

    /// Serialises session writes on one background task so an older snapshot
    /// can never overwrite a newer one.
    fn start_session_writer(&mut self, cx: &mut Context<Self>) {
        let Some(storage) = self.storage.clone() else {
            return;
        };
        let (sender, receiver) = async_channel::unbounded();
        self.session_sender = Some(sender);
        cx.background_executor()
            .spawn(async move {
                while let Ok(snapshot) = receiver.recv().await {
                    if let Err(error) = storage.save_session(&snapshot) {
                        tracing::warn!(%error, "could not save the browser session");
                    }
                }
            })
            .detach();
    }

    fn persist_session(&self) {
        let Some(sender) = self.session_sender.as_ref() else {
            return;
        };
        let snapshot = SessionSnapshot::capture(&self.tabs);
        if let Err(error) = sender.try_send(snapshot) {
            tracing::debug!(%error, "session writer unavailable");
        }
    }

    /// Restores the last session, if any. Returns false when there is nothing to
    /// restore and a homepage tab should be opened instead.
    fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(storage) = self.storage.as_ref() else {
            return false;
        };
        let snapshot = match storage.load_session() {
            Ok(Some(snapshot)) if !snapshot.is_empty() => snapshot,
            Ok(_) => return false,
            Err(error) => {
                tracing::warn!(%error, "ignoring unreadable session file");
                self.status = Some("Could not read the previous session".into());
                return false;
            }
        };

        let active = snapshot.active.min(snapshot.tabs.len().saturating_sub(1));
        for (index, saved) in snapshot.tabs.into_iter().enumerate() {
            let id = self.tabs.open(&saved.url);
            if let Some(tab) = self.tabs.tab_mut(id) {
                tab.navigation.url = saved.url.clone();
                tab.navigation.title = saved.title;
                // Background tabs do not hit the network until activated.
                if index != active && !saved.url.is_empty() {
                    tab.navigation.status = browser_core::LoadStatus::Idle;
                }
            }
            let initial = if index == active {
                saved.url.as_str()
            } else {
                ""
            };
            self.attach_engine(id, initial, window, cx);
        }
        self.tabs.activate_index(active);
        self.update_engine_visibility();
        self.sync_address(window, cx);
        self.persist_session();
        cx.notify();
        true
    }

    /// Restores a tab's page when it first becomes visible after session
    /// restore (background tabs are intentionally not loaded at startup).
    fn ensure_tab_loaded(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(engine) = self.runtimes.get(&id).cloned() else {
            return;
        };
        if engine.current_url().is_some() {
            return;
        }
        let Some(url) = self
            .tabs
            .tab(id)
            .map(|tab| tab.navigation.url.clone())
            .filter(|url| !url.is_empty())
        else {
            return;
        };
        if let Some(tab) = self.tabs.tab_mut(id) {
            tab.navigation
                .apply(NavigationEvent::Started { url: url.clone() });
        }
        if let Err(error) = engine.navigate(&url) {
            tracing::error!(%error, %url, "could not restore tab");
            if let Some(tab) = self.tabs.tab_mut(id) {
                tab.navigation.apply(NavigationEvent::Failed {
                    url,
                    message: error.to_string(),
                });
            }
        }
        cx.notify();
    }

    pub fn active_engine(&self) -> Option<Rc<dyn Engine>> {
        let id = self.tabs.active_id()?;
        self.runtimes.get(&id).cloned()
    }

    fn open_tab(
        &mut self,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<TabId> {
        let id = self.tabs.open(url);
        if !url.is_empty()
            && let Some(tab) = self.tabs.tab_mut(id)
        {
            tab.navigation
                .apply(NavigationEvent::Started { url: url.into() });
        }
        if !self.attach_engine(id, url, window, cx)
            && let Some(tab) = self.tabs.tab_mut(id)
        {
            tab.navigation.apply(NavigationEvent::Failed {
                url: url.into(),
                message: "The web engine could not be created for this tab.".into(),
            });
        }
        self.sync_address(window, cx);
        self.persist_session();
        cx.notify();
        Some(id)
    }

    fn attach_engine(
        &mut self,
        id: TabId,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let parent = match native_parent(window) {
            Ok(parent) => parent,
            Err(error) => {
                tracing::error!(%error, "could not obtain the AppKit view for the engine");
                self.status = Some(format!("Engine unavailable: {error}").into());
                return false;
            }
        };
        let initial_url = (!url.is_empty()).then(|| url.to_owned());
        let instance = match create_engine(EngineOptions {
            parent,
            viewport: Viewport::default(),
            initial_url,
            private: false,
        }) {
            Ok(instance) => instance,
            Err(error) => {
                tracing::error!(%error, %url, "could not create the web engine");
                self.status = Some(format!("Engine unavailable: {error}").into());
                return false;
            }
        };
        let (engine, events) = instance.into_parts();
        self.runtimes.insert(id, engine);
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn_in(window, async move |_spawned_weak, cx| {
            while let Ok(event) = events.recv().await {
                if weak
                    .update_in(cx, |this, window, cx| {
                        this.handle_engine_event(id, event, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        true
    }

    fn handle_engine_event(
        &mut self,
        id: TabId,
        event: EngineEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.tab(id).is_none() {
            return;
        }

        // Favicon discovery results come back through the script channel.
        if let EngineEvent::ScriptResult { id, value } = &event
            && let Some((tab_id, origin, page_url)) = self.pending_favicons.remove(id)
        {
            tracing::debug!(?value, "favicon hint result");
            let icon_url = value
                .as_str()
                .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
                .map(str::to_owned);
            self.fetch_favicon(tab_id, origin, page_url, icon_url, cx);
            return;
        }

        let finished_url = match &event {
            EngineEvent::LoadFinished { url, .. } => Some(url.clone()),
            _ => None,
        };

        // Events that affect the whole window rather than just this tab.
        match &event {
            EngineEvent::NewWindowRequested { url } => {
                self.open_tab(url, window, cx);
                return;
            }
            EngineEvent::ExternalSchemeRequested { url } => {
                if let Err(error) = open_external_url(url) {
                    tracing::warn!(%error, %url, "could not open external URL");
                    self.status = Some(format!("Could not open {url}").into());
                }
                cx.notify();
                return;
            }
            EngineEvent::DownloadRequested { url, .. } => {
                // Downloads are implemented in M3; say so instead of failing
                // silently when a site offers a file.
                self.status = Some(format!("Downloads are not available yet ({url})").into());
                cx.notify();
                return;
            }
            _ => {}
        }

        let mut notify = true;
        let is_active = self.tabs.active_id() == Some(id);
        if let Some(tab) = self.tabs.tab_mut(id) {
            match event {
                EngineEvent::NavigationStarted { url } => {
                    tab.navigation.apply(NavigationEvent::Started { url });
                }
                EngineEvent::NavigationCommitted { url } => {
                    tab.navigation.apply(NavigationEvent::Committed { url });
                }
                EngineEvent::LoadFinished {
                    url,
                    title,
                    can_go_back,
                    can_go_forward,
                } => {
                    tab.navigation.apply(NavigationEvent::Finished {
                        url,
                        title,
                        can_go_back,
                        can_go_forward,
                    });
                }
                EngineEvent::LoadFailed { url, message, .. } => {
                    tab.navigation
                        .apply(NavigationEvent::Failed { url, message });
                }
                EngineEvent::TitleChanged { title } => {
                    tab.navigation
                        .apply(NavigationEvent::TitleChanged { title });
                }
                EngineEvent::UrlChanged { url } => {
                    tab.navigation.apply(NavigationEvent::UrlChanged { url });
                }
                EngineEvent::ProcessTerminated => {
                    tab.navigation.apply(NavigationEvent::ProcessTerminated);
                }
                EngineEvent::ScriptResult { .. }
                | EngineEvent::ScriptError { .. }
                | EngineEvent::SnapshotSaved { .. }
                | EngineEvent::SnapshotFailed { .. } => {
                    notify = false;
                }
                EngineEvent::NewWindowRequested { .. }
                | EngineEvent::ExternalSchemeRequested { .. }
                | EngineEvent::DownloadRequested { .. } => unreachable!("handled above"),
            }
            if notify && is_active {
                set_window_title(window, &tab.navigation);
            }
        }
        if let Some(url) = finished_url {
            self.schedule_favicon(id, &url, cx);
        }
        if notify {
            self.sync_address(window, cx);
            self.persist_session();
            cx.notify();
        }
    }

    /// Looks up the page's declared icon and fetches it if this origin has no
    /// cached icon yet.
    fn schedule_favicon(&mut self, tab_id: TabId, page_url: &str, cx: &mut Context<Self>) {
        let Ok(parsed) = url::Url::parse(page_url) else {
            return;
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            return;
        }
        let origin = parsed.origin().ascii_serialization();
        if let Some(image) = self.favicon_cache.get(&origin) {
            self.tab_favicons.insert(tab_id, image.clone());
            cx.notify();
            return;
        }
        let page_key = page_url.to_owned();
        if self.favicon_misses.contains(&page_key) || self.favicon_requests.contains(&page_key) {
            return;
        }
        if let Some(engine) = self.runtimes.get(&tab_id).cloned() {
            tracing::debug!(%origin, tab = %tab_id.get(), "asking page for favicon");
            let id = engine.evaluate_script_json(
                "(() => { const link = document.querySelector('link[rel~=\"icon\"][href]'); return link ? link.href : null; })()",
            );
            self.pending_favicons.insert(id, (tab_id, origin, page_key));
        } else {
            self.fetch_favicon(tab_id, origin, page_key, None, cx);
        }
    }

    fn fetch_favicon(
        &mut self,
        tab_id: TabId,
        origin: String,
        page_key: String,
        declared_url: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.favicon_requests.insert(page_key.clone()) {
            return;
        }
        tracing::debug!(%origin, ?declared_url, "fetching favicon");
        let origin_for_task = origin.clone();
        let declared = declared_url.clone();
        let (bytes_tx, bytes_rx) = async_channel::bounded(1);
        // Icon bytes are fetched off the UI thread; ureq is blocking.
        std::thread::spawn(move || {
            let bytes = favicon::fetch_icon(&origin_for_task, declared.as_deref());
            let _ = bytes_tx.send_blocking(bytes);
        });
        cx.spawn(async move |this, cx| {
            let bytes = bytes_rx.recv().await.ok().flatten();
            let image = bytes.and_then(|bytes| favicon::decode_icon(&bytes));
            let _ = this.update(cx, |this, cx| {
                this.favicon_loaded(origin, page_key, tab_id, image, cx)
            });
        })
        .detach();
    }

    fn favicon_loaded(
        &mut self,
        origin: String,
        page_key: String,
        tab_id: TabId,
        image: Option<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        self.favicon_requests.remove(&page_key);
        tracing::debug!(%origin, found = image.is_some(), "favicon fetch finished");
        match image {
            Some(image) => {
                self.favicon_cache.insert(origin, image.clone());
                if self.tabs.tab(tab_id).is_some() {
                    self.tab_favicons.insert(tab_id, image);
                    cx.notify();
                }
            }
            None => {
                self.favicon_misses.insert(page_key);
            }
        }
    }

    pub fn navigate(&mut self, input: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = resolve_input(input, &self.settings.search_engine) else {
            return;
        };
        let url = target.url().to_owned();
        let id = match self.tabs.active_id() {
            Some(id) => id,
            None => match self.open_tab(&url, window, cx) {
                Some(id) => id,
                None => return,
            },
        };
        let Some(engine) = self.runtimes.get(&id).cloned() else {
            return;
        };
        if let Some(tab) = self.tabs.tab_mut(id) {
            tab.navigation
                .apply(NavigationEvent::Started { url: url.clone() });
        }
        if let Err(error) = engine.navigate(&url) {
            tracing::error!(%error, %url, "navigation failed");
            if let Some(tab) = self.tabs.tab_mut(id) {
                tab.navigation.apply(NavigationEvent::Failed {
                    url: url.clone(),
                    message: error.to_string(),
                });
            }
        }
        engine.focus_page();
        self.address
            .update(cx, |input, cx| input.set_text(url.clone(), cx));
        if let Some(tab) = self.tabs.tab(id) {
            set_window_title(window, &tab.navigation);
        }
        cx.notify();
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(engine) = self.active_engine() {
            engine.go_back();
            engine.focus_page();
        }
        cx.notify();
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(engine) = self.active_engine() {
            engine.go_forward();
            engine.focus_page();
        }
        cx.notify();
    }

    fn reload_or_stop(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.active_engine() else {
            return;
        };
        let loading = self
            .tabs
            .active_tab()
            .is_some_and(|tab| tab.navigation.is_loading());
        if loading {
            engine.stop();
            if let Some(tab) = self.tabs.active_tab_mut() {
                tab.navigation.apply(NavigationEvent::Stopped);
            }
        } else {
            engine.reload();
            if let Some(tab) = self.tabs.active_tab_mut()
                && let Some(url) = engine.current_url()
            {
                tab.navigation.apply(NavigationEvent::Started { url });
            }
        }
        cx.notify();
    }

    fn hard_reload(&mut self, cx: &mut Context<Self>) {
        if let Some(engine) = self.active_engine() {
            engine.reload_bypassing_cache();
        }
        cx.notify();
    }

    fn activate_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs.activate(id) {
            return;
        }
        self.update_engine_visibility();
        self.ensure_tab_loaded(id, cx);
        self.sync_address(window, cx);
        if let Some(engine) = self.active_engine() {
            engine.focus_page();
        }
        self.persist_session();
        cx.notify();
    }

    fn close_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(engine) = self.runtimes.remove(&id) {
            engine.close();
        }
        self.tab_favicons.remove(&id);
        self.tabs.close(id);
        if self.tabs.is_empty() {
            self.shutdown(window, cx);
            return;
        }
        self.update_engine_visibility();
        self.sync_address(window, cx);
        self.persist_session();
        cx.notify();
    }

    pub fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.tabs.active_id() {
            self.close_tab(id, window, cx);
        }
    }

    fn shutdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (_, engine) in self.runtimes.drain() {
            engine.close();
        }
        self.shortcuts = None;
        window.remove_window();
        cx.notify();
    }

    fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.tabs.reopen_closed() else {
            self.status = Some("No recently closed tabs".into());
            cx.notify();
            return;
        };
        let url = self
            .tabs
            .tab(id)
            .map(|tab| tab.navigation.url.clone())
            .unwrap_or_default();
        if !url.is_empty() {
            self.attach_engine(id, &url, window, cx);
        } else {
            self.attach_engine(id, "", window, cx);
        }
        self.sync_address(window, cx);
        cx.notify();
    }

    /// Activates the tab at `index`, used by probes and future Cmd+1..9.
    pub fn activate_tab_index(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.tabs.tabs().get(index).map(|tab| tab.id) {
            self.activate_tab(id, window, cx);
        }
    }

    /// Moves the tab at `from_index` to `to_index` (probe/drag helper).
    pub fn reorder_tab_by_index(
        &mut self,
        from_index: usize,
        to_index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.tabs.tabs().get(from_index).map(|tab| tab.id) {
            self.reorder_tab(id, to_index, cx);
        }
    }

    /// Moves a tab to `to_index`, used by drag-and-drop reordering.
    pub fn reorder_tab(&mut self, from: TabId, to_index: usize, cx: &mut Context<Self>) {
        let Some(from_index) = self.tabs.index_of(from) else {
            return;
        };
        if self.tabs.move_tab(from_index, to_index) {
            self.persist_session();
            cx.notify();
        }
    }

    /// Duplicates the active tab next to itself and loads the same URL.
    pub fn duplicate_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.tabs.active_id() else {
            return;
        };
        let Some(new_id) = self.tabs.duplicate(active) else {
            return;
        };
        let url = self
            .tabs
            .tab(new_id)
            .map(|tab| tab.navigation.url.clone())
            .unwrap_or_default();
        self.attach_engine(new_id, &url, window, cx);
        self.update_engine_visibility();
        self.sync_address(window, cx);
        self.persist_session();
        cx.notify();
    }

    /// Moves keyboard focus into the page content.
    pub fn focus_page(&mut self) {
        if let Some(engine) = self.active_engine() {
            engine.focus_page();
        }
    }

    pub fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tab("", window, cx);
        self.focus_address(window, cx);
        cx.notify();
    }

    fn cycle_tab(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let target = if forward {
            self.tabs.activate_next()
        } else {
            self.tabs.activate_previous()
        };
        if let Some(id) = target {
            self.activate_tab(id, window, cx);
        }
    }

    fn zoom(&mut self, delta: f64, cx: &mut Context<Self>) {
        if let Some(engine) = self.active_engine() {
            let zoom = engine.page_zoom();
            engine.set_page_zoom(zoom + delta);
        }
        cx.notify();
    }

    fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        if let Some(engine) = self.active_engine() {
            engine.set_page_zoom(1.0);
        }
        cx.notify();
    }

    pub fn focus_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self
            .tabs
            .active_tab()
            .map(|tab| tab.navigation.url.clone())
            .unwrap_or_default();
        self.address.update(cx, |input, cx| {
            input.set_text(url, cx);
            input.focus_and_select_all(window, cx);
        });
        // GPUI focus is not AppKit focus: make the chrome view the window's
        // first responder so keystrokes reach the address bar, not the page.
        if let Err(error) = browser_platform::focus_chrome(window) {
            tracing::warn!(%error, "could not move keyboard focus to the browser chrome");
        }
        cx.notify();
    }

    fn sync_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self
            .tabs
            .active_tab()
            .map(|tab| tab.navigation.url.clone())
            .unwrap_or_default();
        {
            let input = self.address.read(cx);
            // Never overwrite text the user is actively editing, but do
            // normalise the URL while the field is focused and untouched
            // (for example `example.com` -> `https://example.com/`).
            if input.is_focused(window) && input.is_edited() {
                return;
            }
        }
        self.address.update(cx, |input, cx| input.set_text(url, cx));
    }

    /// A snapshot of everything the automated probes assert on.
    pub fn snapshot(&self, cx: &App) -> BrowserSnapshot {
        let active_id = self.tabs.active_id();
        let active = self.tabs.active_tab();
        BrowserSnapshot {
            tab_count: self.tabs.len(),
            active_url: active
                .map(|tab| tab.navigation.url.clone())
                .unwrap_or_default(),
            active_title: active
                .map(|tab| tab.navigation.display_title())
                .unwrap_or_default(),
            loading: active.is_some_and(|tab| tab.navigation.is_loading()),
            error: active.and_then(|tab| {
                tab.navigation
                    .error
                    .as_ref()
                    .map(|error| error.message.clone())
            }),
            address_text: self.address.read(cx).text().to_owned(),
            tabs: self
                .tabs
                .tabs()
                .iter()
                .map(|tab| TabSnapshot {
                    title: tab.navigation.display_title(),
                    url: tab.navigation.url.clone(),
                    active: Some(tab.id) == active_id,
                    has_favicon: self.tab_favicons.contains_key(&tab.id),
                })
                .collect(),
        }
    }

    fn update_engine_visibility(&self) {
        let active = self.tabs.active_id();
        for (id, engine) in &self.runtimes {
            engine.set_visible(self.page_should_show(*id, active));
        }
    }

    fn page_should_show(&self, id: TabId, active: Option<TabId>) -> bool {
        if active != Some(id) {
            return false;
        }
        self.tabs
            .tab(id)
            .is_some_and(|tab| !tab.navigation.url.is_empty() && tab.navigation.error.is_none())
    }

    fn handle_shortcut(
        &mut self,
        shortcut: BrowserShortcut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match shortcut {
            BrowserShortcut::FocusLocation => self.focus_address(window, cx),
            BrowserShortcut::Back => self.go_back(cx),
            BrowserShortcut::Forward => self.go_forward(cx),
            BrowserShortcut::Reload => {
                if let Some(engine) = self.active_engine() {
                    engine.reload();
                }
                cx.notify();
            }
            BrowserShortcut::HardReload => self.hard_reload(cx),
            BrowserShortcut::Stop => {
                if let Some(engine) = self.active_engine() {
                    engine.stop();
                }
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.navigation.apply(NavigationEvent::Stopped);
                }
                cx.notify();
            }
            BrowserShortcut::NewTab => self.new_tab(window, cx),
            BrowserShortcut::CloseTab => self.close_active_tab(window, cx),
            BrowserShortcut::ReopenClosedTab => self.reopen_closed_tab(window, cx),
            BrowserShortcut::NextTab => self.cycle_tab(true, window, cx),
            BrowserShortcut::PreviousTab => self.cycle_tab(false, window, cx),
            BrowserShortcut::FindInPage => {
                self.status = Some("Find in page arrives with the essentials milestone".into());
                cx.notify();
            }
            BrowserShortcut::ZoomIn => self.zoom(0.1, cx),
            BrowserShortcut::ZoomOut => self.zoom(-0.1, cx),
            BrowserShortcut::ZoomReset => self.reset_zoom(cx),
            BrowserShortcut::OpenSettings => {
                self.status = Some("Settings arrive with the modern-experience milestone".into());
                cx.notify();
            }
            BrowserShortcut::Quit => cx.quit(),
        }
    }

    // ----- Action handlers ------------------------------------------------

    fn on_focus_location(
        &mut self,
        _: &FocusLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_address(window, cx);
    }

    fn on_back(&mut self, _: &Back, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_back(cx);
    }

    fn on_forward(&mut self, _: &Forward, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_forward(cx);
    }

    fn on_reload(&mut self, _: &Reload, _window: &mut Window, cx: &mut Context<Self>) {
        self.reload_or_stop(cx);
    }

    fn on_hard_reload(&mut self, _: &HardReload, _window: &mut Window, cx: &mut Context<Self>) {
        self.hard_reload(cx);
    }

    fn on_stop(&mut self, _: &Stop, window: &mut Window, cx: &mut Context<Self>) {
        self.handle_shortcut(BrowserShortcut::Stop, window, cx);
    }

    fn on_new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.new_tab(window, cx);
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.close_active_tab(window, cx);
    }

    fn on_duplicate_tab(&mut self, _: &DuplicateTab, window: &mut Window, cx: &mut Context<Self>) {
        self.duplicate_active_tab(window, cx);
    }

    fn on_reopen_closed_tab(
        &mut self,
        _: &ReopenClosedTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reopen_closed_tab(window, cx);
    }

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_tab(true, window, cx);
    }

    fn on_previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_tab(false, window, cx);
    }

    fn on_zoom_in(&mut self, _: &ZoomIn, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoom(0.1, cx);
    }

    fn on_zoom_out(&mut self, _: &ZoomOut, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoom(-0.1, cx);
    }

    fn on_zoom_reset(&mut self, _: &ZoomReset, _window: &mut Window, cx: &mut Context<Self>) {
        self.reset_zoom(cx);
    }

    fn on_find_in_page(&mut self, _: &FindInPage, _window: &mut Window, cx: &mut Context<Self>) {
        self.status = Some("Find in page arrives with the essentials milestone".into());
        cx.notify();
    }

    fn on_open_settings(&mut self, _: &OpenSettings, _window: &mut Window, cx: &mut Context<Self>) {
        self.status = Some("Settings arrive with the modern-experience milestone".into());
        cx.notify();
    }

    fn on_quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }
}

/// Observable browser state used by automated end-to-end probes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BrowserSnapshot {
    pub tab_count: usize,
    pub active_url: String,
    pub active_title: String,
    pub loading: bool,
    pub error: Option<String>,
    pub address_text: String,
    pub tabs: Vec<TabSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TabSnapshot {
    pub title: String,
    pub url: String,
    pub active: bool,
    pub has_favicon: bool,
}

fn set_window_title(window: &mut Window, navigation: &browser_core::NavigationState) {
    let label = navigation.display_title();
    window.set_window_title(&format!("{label} — GPUI Browser"));
}
