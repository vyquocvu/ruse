//! The browser window: tabs, chrome, engine lifetimes and event pumps.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use browser_core::{
    BrowserSettings, HistoryEntry, NavigationEvent, SessionSnapshot, Tab, TabId, TabSet,
    resolve_input, unix_millis_now,
};
use browser_engine::{Engine, EngineEvent, EngineOptions, Viewport, create_engine};
use browser_platform::{BrowserShortcut, ShortcutMonitor, native_parent, open_external_url};
use browser_storage::{BookmarkStore, HistoryStore, Storage};
use browser_ui::{TextInput, TextInputEvent, Theme};
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, RenderImage, SharedString,
    Subscription, Timer, WeakEntity, Window, px,
};

use crate::favicon;

mod bookmarks;
mod downloads;
mod render;

use crate::actions::{
    Back, CloseTab, CopyUrl, DuplicateTab, FindInPage, FindNext, FindPrevious, FocusLocation,
    Forward, HardReload, NewTab, NextTab, OpenSettings, PreviousTab, Quit, Reload, ReopenClosedTab,
    Stop, ToggleBookmark, ToggleBookmarks, ToggleDownloads, ToggleHistory, ZoomIn, ZoomOut,
    ZoomReset,
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
    history: Option<HistoryStore>,
    bookmarks: bookmarks::BookmarksState,
    overlay: Overlay,
    history_entries: Vec<HistoryEntry>,
    history_loading: bool,
    history_error: Option<String>,
    find_open: bool,
    find_input: Option<Entity<TextInput>>,
    find_result: Option<bool>,
    downloads: downloads::DownloadsState,
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
        let history = storage
            .as_ref()
            .map(|storage| HistoryStore::in_directory(storage.root()));
        let bookmarks = bookmarks::BookmarksState::new(
            storage
                .as_ref()
                .map(|storage| BookmarkStore::in_directory(storage.root())),
        );
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
            history,
            bookmarks,
            overlay: Overlay::None,
            history_entries: Vec::new(),
            history_loading: false,
            history_error: None,
            find_open: false,
            find_input: None,
            find_result: None,
            downloads: downloads::DownloadsState::default(),
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
        self.update_download_progress(cx);
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
            downloads_dir: browser_platform::downloads_directory(),
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

        if let EngineEvent::FindResult { found } = &event {
            self.handle_find_result(*found, cx);
            return;
        }

        let finished_url = match &event {
            EngineEvent::LoadFinished { url, .. } => Some(url.clone()),
            _ => None,
        };

        // Events that affect the whole window rather than just this tab.
        match &event {
            EngineEvent::BrowsingDataCleared => {
                self.status = Some("Browsing data cleared".into());
                cx.notify();
                return;
            }
            EngineEvent::DownloadStarted {
                url,
                filename,
                path,
                expected_bytes,
            } => {
                self.handle_download_started(
                    url.clone(),
                    filename.clone(),
                    path.clone(),
                    *expected_bytes,
                    cx,
                );
                return;
            }
            EngineEvent::DownloadFinished {
                path,
                success,
                message,
            } => {
                self.handle_download_finished(path.clone(), *success, message.clone(), cx);
                return;
            }
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
                EngineEvent::FindResult { .. } => {}
                EngineEvent::BrowsingDataCleared => {}
                EngineEvent::ScriptResult { .. }
                | EngineEvent::ScriptError { .. }
                | EngineEvent::SnapshotSaved { .. }
                | EngineEvent::SnapshotFailed { .. } => {
                    notify = false;
                }
                EngineEvent::NewWindowRequested { .. }
                | EngineEvent::ExternalSchemeRequested { .. }
                | EngineEvent::DownloadStarted { .. }
                | EngineEvent::DownloadFinished { .. } => unreachable!("handled above"),
            }
            if notify && is_active {
                set_window_title(window, &tab.navigation);
            }
        }
        if let Some(url) = finished_url {
            self.schedule_favicon(id, &url, cx);
            let title = self
                .tabs
                .tab(id)
                .map(|tab| tab.navigation.title.clone())
                .unwrap_or_default();
            self.record_history(&url, &title, cx);
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
            overlay: self.overlay_name().to_owned(),
            history_entries: self.history_entries.len(),
            history_urls: self
                .history_entries
                .iter()
                .map(|entry| entry.url.clone())
                .collect(),
            history_ids: self.history_entries.iter().map(|entry| entry.id).collect(),
            history_loading: self.history_loading,
            bookmarks_count: self.bookmarks.bookmarks.len(),
            bookmark_urls: self
                .bookmarks
                .bookmarks
                .iter()
                .map(|bookmark| bookmark.url.clone())
                .collect(),
            bookmark_folders: self
                .bookmarks
                .folders
                .iter()
                .map(|folder| folder.name.clone())
                .collect(),
            bookmark_folder_ids: self
                .bookmarks
                .folders
                .iter()
                .map(|folder| folder.id)
                .collect(),
            bookmark_ids: self
                .bookmarks
                .bookmarks
                .iter()
                .map(|bookmark| bookmark.id)
                .collect(),
            bookmark_folder_of: self
                .bookmarks
                .bookmarks
                .iter()
                .map(|bookmark| bookmark.folder_id)
                .collect(),
            bookmarked_active: self
                .tabs
                .active_tab()
                .is_some_and(|tab| self.bookmarks.is_bookmarked(&tab.navigation.url)),
            find_open: self.find_open,
            find_found: self.find_result,
            downloads_count: self.downloads.entries.len(),
            download_paths: self
                .downloads
                .entries
                .iter()
                .map(|entry| entry.path.display().to_string())
                .collect(),
            download_statuses: self
                .downloads
                .entries
                .iter()
                .map(|entry| entry.status.as_str().to_owned())
                .collect(),
            downloads_completed: self
                .downloads
                .entries
                .iter()
                .filter(|entry| entry.status == downloads::DownloadStatus::Completed)
                .count(),
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
        if active != Some(id) || self.overlay != Overlay::None {
            return false;
        }
        self.tabs
            .tab(id)
            .is_some_and(|tab| !tab.navigation.url.is_empty() && tab.navigation.error.is_none())
    }

    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = true;
        if self.find_input.is_none() {
            let input = cx.new(|cx| TextInput::new(cx, "Find in page", px(13.0)));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |this, _input, event: &TextInputEvent, window, cx| match event {
                    TextInputEvent::Changed(_) => this.search_find(true, window, cx),
                    TextInputEvent::Submitted(_) => this.search_find(true, window, cx),
                    TextInputEvent::Cancelled => this.close_find(window, cx),
                },
            );
            self.subscriptions.push(subscription);
            self.find_input = Some(input);
        }
        if let Some(input) = self.find_input.clone() {
            input.update(cx, |input, cx| input.focus_and_select_all(window, cx));
        }
        if let Err(error) = browser_platform::focus_chrome(window) {
            tracing::warn!(%error, "could not focus the find bar");
        }
        self.search_find(true, window, cx);
        cx.notify();
    }

    pub fn close_find(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.find_open {
            return;
        }
        self.find_open = false;
        if let Some(engine) = self.active_engine() {
            engine.focus_page();
        }
        cx.notify();
    }

    fn find_query(&self, cx: &App) -> Option<String> {
        let input = self.find_input.as_ref()?;
        let text = input.read(cx).text().trim().to_owned();
        (!text.is_empty()).then_some(text)
    }

    fn search_find(&mut self, forward: bool, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(query) = self.find_query(cx) else {
            self.find_result = None;
            cx.notify();
            return;
        };
        if let Some(engine) = self.active_engine() {
            engine.find_in_page(&query, forward);
            self.find_result = None;
            cx.notify();
        }
    }

    fn handle_find_result(&mut self, found: bool, cx: &mut Context<Self>) {
        self.find_result = Some(found);
        cx.notify();
    }

    pub fn find_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.find_open {
            self.open_find(window, cx);
            return;
        }
        self.search_find(true, window, cx);
    }

    pub fn find_previous(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.find_open {
            self.open_find(window, cx);
            return;
        }
        self.search_find(false, window, cx);
    }

    /// Probe helper: run a find for `query` and focus the find bar.
    pub fn find_in_page_for_probe(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_find(window, cx);
        let query = query.to_owned();
        if let Some(input) = self.find_input.clone() {
            input.update(cx, |input, cx| input.set_text(query, cx));
        }
        self.search_find(true, window, cx);
    }

    pub fn find_state(&self) -> (bool, Option<bool>) {
        (self.find_open, self.find_result)
    }

    fn record_history(&self, url: &str, title: &str, cx: &Context<Self>) {
        let Some(history) = self.history.clone() else {
            return;
        };
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return;
        }
        let url = url.to_owned();
        let title = title.to_owned();
        let visited_at = unix_millis_now();
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = history.record(&url, &title, visited_at) {
                    tracing::warn!(%error, "could not record history");
                }
            })
            .detach();
    }

    /// Opens the full-window history page (hides the native page view).
    pub fn open_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::History;
        self.history_entries.clear();
        self.history_error = None;
        self.history_loading = true;
        self.update_engine_visibility();
        if let Err(error) = browser_platform::focus_chrome(window) {
            tracing::warn!(%error, "could not focus the history page");
        }
        self.reload_history(cx);
        cx.notify();
    }

    fn reload_history(&mut self, cx: &mut Context<Self>) {
        let Some(history) = self.history.clone() else {
            self.history_loading = false;
            self.history_error =
                Some("History is unavailable because storage failed to open".into());
            cx.notify();
            return;
        };
        self.history_loading = true;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = executor.spawn(async move { history.recent(300) }).await;
            let _ = this.update(cx, |this, cx| {
                this.history_loading = false;
                match result {
                    Ok(entries) => this.history_entries = entries,
                    Err(error) => this.history_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn toggle_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay == Overlay::History {
            self.close_overlay(window, cx);
        } else {
            self.open_history(window, cx);
        }
    }

    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay == Overlay::None {
            return;
        }
        self.overlay = Overlay::None;
        self.update_engine_visibility();
        if let Some(engine) = self.active_engine() {
            engine.focus_page();
        }
        let _ = window;
        cx.notify();
    }

    pub fn delete_history_entry(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(history) = self.history.clone() else {
            return;
        };
        self.history_entries.retain(|entry| entry.id != id);
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = history.delete(id) {
                    tracing::warn!(%error, "could not delete history entry");
                }
            })
            .detach();
        cx.notify();
    }

    /// Clears cookies, caches and site storage across every tab's data store.
    /// Copies the active page URL to the clipboard.
    pub fn copy_url(&mut self, cx: &mut Context<Self>) {
        let Some(url) = self
            .tabs
            .active_tab()
            .map(|tab| tab.navigation.url.clone())
            .filter(|url| !url.is_empty())
        else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(url));
        self.status = Some("URL copied".into());
        cx.notify();
    }

    pub fn clear_browsing_data(&mut self, cx: &mut Context<Self>) {
        let engines: Vec<Rc<dyn Engine>> = self.runtimes.values().cloned().collect();
        if engines.is_empty() {
            return;
        }
        self.status = Some("Clearing browsing data…".into());
        for engine in engines {
            engine.clear_browsing_data();
        }
        cx.notify();
    }

    pub fn clear_history(&mut self, cx: &mut Context<Self>) {
        let Some(history) = self.history.clone() else {
            return;
        };
        self.history_entries.clear();
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = history.clear() {
                    tracing::warn!(%error, "could not clear history");
                }
            })
            .detach();
        cx.notify();
    }

    pub fn open_history_entry(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        self.navigate(url, window, cx);
        self.update_engine_visibility();
    }

    /// Exposed for probes: which full-window surface is visible, if any.
    pub fn overlay_name(&self) -> &'static str {
        match self.overlay {
            Overlay::None => "none",
            Overlay::History => "history",
            Overlay::Bookmarks => "bookmarks",
            Overlay::Downloads => "downloads",
        }
    }

    pub fn history_entry_count(&self) -> usize {
        self.history_entries.len()
    }

    /// Reloads the history page contents (probe helper).
    pub fn refresh_history(&mut self, cx: &mut Context<Self>) {
        self.reload_history(cx);
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
                if self.overlay != Overlay::None {
                    self.close_overlay(window, cx);
                    return;
                }
                if self.find_open {
                    self.close_find(window, cx);
                    return;
                }
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
            BrowserShortcut::FindInPage => self.open_find(window, cx),
            BrowserShortcut::FindNext => self.find_next(window, cx),
            BrowserShortcut::FindPrevious => self.find_previous(window, cx),
            BrowserShortcut::ZoomIn => self.zoom(0.1, cx),
            BrowserShortcut::ZoomOut => self.zoom(-0.1, cx),
            BrowserShortcut::ZoomReset => self.reset_zoom(cx),
            BrowserShortcut::OpenSettings => {
                self.status = Some("Settings arrive with the modern-experience milestone".into());
                cx.notify();
            }
            BrowserShortcut::ToggleBookmark => self.toggle_bookmark(window, cx),
            BrowserShortcut::ToggleBookmarks => self.toggle_bookmarks(window, cx),
            BrowserShortcut::ToggleDownloads => self.toggle_downloads(window, cx),
            BrowserShortcut::CopyUrl => self.copy_url(cx),
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

    fn on_toggle_history(
        &mut self,
        _: &ToggleHistory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_history(window, cx);
    }

    fn on_toggle_bookmark(
        &mut self,
        _: &ToggleBookmark,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_bookmark(window, cx);
    }

    fn on_toggle_bookmarks(
        &mut self,
        _: &ToggleBookmarks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_bookmarks(window, cx);
    }

    fn on_copy_url(&mut self, _: &CopyUrl, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_url(cx);
    }

    fn on_toggle_downloads(
        &mut self,
        _: &ToggleDownloads,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_downloads(window, cx);
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

    fn on_find_in_page(&mut self, _: &FindInPage, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(window, cx);
    }

    fn on_find_next(&mut self, _: &FindNext, window: &mut Window, cx: &mut Context<Self>) {
        self.find_next(window, cx);
    }

    fn on_find_previous(&mut self, _: &FindPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.find_previous(window, cx);
    }

    fn on_open_settings(&mut self, _: &OpenSettings, _window: &mut Window, cx: &mut Context<Self>) {
        self.status = Some("Settings arrive with the modern-experience milestone".into());
        cx.notify();
    }

    fn on_quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }
}

/// Full-window GPUI surfaces that temporarily replace page content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    History,
    Bookmarks,
    Downloads,
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
    pub overlay: String,
    pub history_entries: usize,
    pub history_urls: Vec<String>,
    pub history_ids: Vec<i64>,
    pub history_loading: bool,
    pub bookmarks_count: usize,
    pub bookmark_urls: Vec<String>,
    pub bookmark_folders: Vec<String>,
    pub bookmark_folder_ids: Vec<i64>,
    pub bookmark_ids: Vec<i64>,
    pub bookmark_folder_of: Vec<Option<i64>>,
    pub bookmarked_active: bool,
    pub find_open: bool,
    pub find_found: Option<bool>,
    pub downloads_count: usize,
    pub download_paths: Vec<String>,
    pub download_statuses: Vec<String>,
    pub downloads_completed: usize,
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
