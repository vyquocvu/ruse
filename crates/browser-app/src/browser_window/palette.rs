//! Keyboard command palette (Cmd+K).
//!
//! Items combine static browser commands, bookmarks, recent history and a
//! context-sensitive "Go to / Search" entry for the typed text. Up/Down move the
//! selection; Enter runs it; Esc closes.

use browser_core::{HistoryEntry, resolve_input};
use browser_ui::{TextInput, Theme};
use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*, px};

use super::{BrowserWindow, Overlay};

const VISIBLE_ITEMS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PaletteAction {
    NewTab,
    OpenUrl(String),
    History,
    Bookmarks,
    Downloads,
    Settings,
    FindInPage,
    Reload,
    ClearSiteData,
}

#[derive(Debug, Clone)]
pub(super) struct PaletteItem {
    pub(super) label: String,
    pub(super) detail: String,
    pub(super) action: PaletteAction,
}

pub(super) struct PaletteState {
    pub(super) input: Option<Entity<TextInput>>,
    pub(super) subscriptions: Vec<Subscription>,
    pub(super) commands: Vec<PaletteItem>,
    pub(super) bookmarks: Vec<PaletteItem>,
    pub(super) history: Vec<PaletteItem>,
    pub(super) selected: usize,
    pub(super) open: bool,
}

impl PaletteState {
    pub(super) fn new() -> Self {
        Self {
            input: None,
            subscriptions: Vec::new(),
            commands: Vec::new(),
            bookmarks: Vec::new(),
            history: Vec::new(),
            selected: 0,
            open: false,
        }
    }
}

impl BrowserWindow {
    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::Palette;
        self.palette.open = true;
        self.palette.selected = 0;
        self.update_engine_visibility();
        if self.palette.input.is_none() {
            let input = cx.new(|cx| TextInput::new(cx, "Type a command or URL", px(14.0)));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |this, _input, event: &browser_ui::TextInputEvent, window, cx| match event {
                    browser_ui::TextInputEvent::Changed(_) => {
                        this.palette.selected = 0;
                        cx.notify();
                    }
                    browser_ui::TextInputEvent::Submitted(_) => {
                        this.run_palette_selection(window, cx)
                    }
                    browser_ui::TextInputEvent::Cancelled => this.close_palette(window, cx),
                },
            );
            self.palette.subscriptions.push(subscription);
            self.palette.input = Some(input);
        }
        if let Some(input) = self.palette.input.clone() {
            input.update(cx, |input, cx| input.set_text("", cx));
            input.update(cx, |input, cx| input.focus_and_select_all(window, cx));
        }
        if let Err(error) = browser_platform::focus_chrome(window) {
            tracing::warn!(%error, "could not focus the command palette");
        }
        self.refresh_palette_items(cx);
        cx.notify();
    }

    fn refresh_palette_items(&mut self, cx: &mut Context<Self>) {
        self.palette.commands = vec![
            command("New Tab", "Open a new tab", PaletteAction::NewTab),
            command("History", "Browse visited pages", PaletteAction::History),
            command("Bookmarks", "Manage bookmarks", PaletteAction::Bookmarks),
            command("Downloads", "Show downloads", PaletteAction::Downloads),
            command(
                "Settings",
                "Homepage, search, theme",
                PaletteAction::Settings,
            ),
            command(
                "Find in Page",
                "Search the current page",
                PaletteAction::FindInPage,
            ),
            command("Reload", "Reload the current page", PaletteAction::Reload),
            command(
                "Clear Site Data",
                "Remove cookies, caches and storage",
                PaletteAction::ClearSiteData,
            ),
        ];
        self.palette.bookmarks = self
            .bookmarks
            .bookmarks
            .iter()
            .map(|bookmark| PaletteItem {
                label: bookmark.display_title(),
                detail: bookmark.url.clone(),
                action: PaletteAction::OpenUrl(bookmark.url.clone()),
            })
            .collect();
        let Some(history) = self.history.clone() else {
            return;
        };
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let recent = executor.spawn(async move { history.recent(200) }).await;
            if let Ok(entries) = recent {
                let _ = this.update(cx, |this, cx| {
                    this.palette.history = entries
                        .into_iter()
                        .map(history_item)
                        .collect::<Vec<PaletteItem>>();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.palette.open {
            return;
        }
        self.palette.open = false;
        self.overlay = Overlay::None;
        self.update_engine_visibility();
        if let Some(engine) = self.active_engine() {
            engine.focus_page();
        }
        let _ = window;
        cx.notify();
    }

    fn query_text(&self, cx: &gpui::App) -> String {
        self.palette
            .input
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_owned())
            .unwrap_or_default()
    }

    /// Filtered items for the current query. The final entry is always a
    /// context-sensitive navigation choice when the user typed something.
    pub(super) fn filtered_palette_items(&self, cx: &gpui::App) -> Vec<PaletteItem> {
        let query = self.query_text(cx);
        let needle = query.to_lowercase();
        let mut items: Vec<PaletteItem> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let candidates = self
            .palette
            .commands
            .iter()
            .chain(self.palette.bookmarks.iter())
            .chain(self.palette.history.iter());
        for item in candidates {
            if needle.is_empty() {
                // Without a query, show commands and bookmarks only.
                let is_history =
                    self.palette.history.iter().any(|history| {
                        history.label == item.label && history.detail == item.detail
                    }) && !self
                        .palette
                        .commands
                        .iter()
                        .any(|command| command.label == item.label)
                        && !self
                            .palette
                            .bookmarks
                            .iter()
                            .any(|bookmark| bookmark.label == item.label);
                if is_history {
                    continue;
                }
            } else if !item.label.to_lowercase().contains(&needle)
                && !item.detail.to_lowercase().contains(&needle)
            {
                continue;
            }
            if seen.insert((item.label.clone(), item.detail.clone())) {
                items.push(item.clone());
            }
        }
        if !query.is_empty()
            && let Some(target) = resolve_input(&query, &self.settings.search_engine)
        {
            let (label, detail) = match &target {
                browser_core::NavigationTarget::Url(url) => (format!("Go to {url}"), url.clone()),
                browser_core::NavigationTarget::Search { url, .. } => {
                    (format!("Search for “{query}”"), url.clone())
                }
            };
            items.insert(
                0,
                PaletteItem {
                    label,
                    detail,
                    action: PaletteAction::OpenUrl(target.url().to_owned()),
                },
            );
        }
        items
    }

    pub fn palette_select_next(&mut self, cx: &mut Context<Self>) {
        let count = self.filtered_palette_items(cx).len();
        if count > 0 {
            self.palette.selected = (self.palette.selected + 1) % count;
            cx.notify();
        }
    }

    pub fn palette_select_previous(&mut self, cx: &mut Context<Self>) {
        let count = self.filtered_palette_items(cx).len();
        if count > 0 {
            self.palette.selected = (self.palette.selected + count - 1) % count;
            cx.notify();
        }
    }

    pub fn run_palette_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.filtered_palette_items(cx);
        let Some(item) = items.get(self.palette.selected).cloned() else {
            return;
        };
        self.palette.open = false;
        self.overlay = Overlay::None;
        match item.action {
            PaletteAction::NewTab => self.new_tab(window, cx),
            PaletteAction::OpenUrl(url) => {
                self.update_engine_visibility();
                self.navigate(&url, window, cx);
            }
            PaletteAction::History => self.open_history(window, cx),
            PaletteAction::Bookmarks => self.open_bookmarks(window, cx),
            PaletteAction::Downloads => self.open_downloads(window, cx),
            PaletteAction::Settings => self.open_settings(window, cx),
            PaletteAction::FindInPage => self.open_find(window, cx),
            PaletteAction::Reload => {
                if let Some(engine) = self.active_engine() {
                    engine.reload();
                }
                self.update_engine_visibility();
                cx.notify();
            }
            PaletteAction::ClearSiteData => {
                self.update_engine_visibility();
                self.clear_browsing_data(cx);
            }
        }
    }

    /// Selects an item by index (clamped); used by keyboard navigation tests.
    pub fn palette_select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let count = self.filtered_palette_items(cx).len();
        if count > 0 {
            self.palette.selected = index.min(count - 1);
            cx.notify();
        }
    }

    /// Probe helper: set the palette query through the real input entity.
    pub fn palette_set_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let query = query.to_owned();
        if let Some(input) = self.palette.input.clone() {
            input.update(cx, |input, cx| {
                input.set_text(query, cx);
                input.focus_and_select_all(window, cx);
            });
            self.palette.selected = 0;
            cx.notify();
        }
    }

    pub fn palette_state(&self, cx: &gpui::App) -> (bool, usize, Vec<String>) {
        (
            self.palette.open,
            self.palette.selected,
            self.filtered_palette_items(cx)
                .into_iter()
                .map(|item| item.label)
                .collect(),
        )
    }

    pub(super) fn render_palette(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let input = self.palette.input.clone();
        let items = self.filtered_palette_items(cx);
        let selected = self.palette.selected.min(items.len().saturating_sub(1));
        let visible: Vec<PaletteItem> = items.into_iter().take(VISIBLE_ITEMS).collect();

        let rows = visible.into_iter().enumerate().map(|(index, item)| {
            let is_selected = index == selected;
            let action_index = index;
            let background = if is_selected {
                theme.surface_active
            } else {
                theme.surface
            };
            div()
                .id(gpui::ElementId::named_usize("palette-item", index))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(background)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _event, window, cx| {
                    this.palette.selected = action_index;
                    this.run_palette_selection(window, cx);
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(13.0))
                        .text_color(theme.text)
                        .child(item.label),
                )
                .child(
                    div()
                        .max_w(px(240.0))
                        .truncate()
                        .text_size(px(11.0))
                        .text_color(theme.text_muted)
                        .child(item.detail),
                )
        });

        let mut input_row = div()
            .h(px(36.0))
            .rounded_md()
            .bg(theme.address_background)
            .border_1()
            .border_color(theme.accent)
            .overflow_hidden();
        if let Some(input) = input {
            input_row = input_row.child(input);
        } else {
            input_row = input_row.child(
                div()
                    .px_3()
                    .text_size(px(13.0))
                    .text_color(theme.text_muted)
                    .child("Type a command or URL"),
            );
        }

        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(120.0))
            .bg(gpui::rgba(0x00000066))
            .key_context("CommandPalette")
            .child(
                div()
                    .w(px(560.0))
                    .p_3()
                    .rounded_lg()
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(input_row)
                    .children(rows)
                    .child(
                        div()
                            .pt_1()
                            .text_size(px(10.0))
                            .text_color(theme.text_muted)
                            .child("↑↓ select · Enter run · Esc close"),
                    ),
            )
            .into_any_element()
    }
}

fn command(label: &str, detail: &str, action: PaletteAction) -> PaletteItem {
    PaletteItem {
        label: label.to_owned(),
        detail: detail.to_owned(),
        action,
    }
}

fn history_item(entry: HistoryEntry) -> PaletteItem {
    PaletteItem {
        label: entry.display_title(),
        detail: entry.url.clone(),
        action: PaletteAction::OpenUrl(entry.url),
    }
}
