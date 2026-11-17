//! GPUI rendering for the browser window.
//!
//! Kept separate from state/actions so each file stays reviewable. These impls
//! are on the same type and may access the window's private state.

use browser_core::{NavigationEvent, TabId, relative_time, unix_millis_now};
use browser_ui::{
    TAB_STRIP_HEIGHT, Theme, icon_button, loading_bar, tab_chip, text_button, web_content_host,
};
use gpui::{
    AnyElement, Context, IntoElement, Render, SharedString, Window, WindowControlArea, div, img,
    prelude::*, px,
};

use super::{BrowserWindow, Overlay, Tab};

/// Drag payload for reordering tabs.
#[derive(Clone)]
struct TabDrag {
    id: TabId,
    title: SharedString,
}

struct TabDragPreview {
    title: SharedString,
}

impl Render for TabDragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        div()
            .w(px(160.0))
            .h(px(28.0))
            .px_3()
            .flex()
            .items_center()
            .rounded_md()
            .bg(theme.surface)
            .border_1()
            .border_color(theme.accent)
            .shadow_md()
            .text_size(px(12.0))
            .text_color(theme.text)
            .child(self.title.clone())
    }
}

impl BrowserWindow {
    // ----- Rendering ------------------------------------------------------

    fn render_tab_strip(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let active_id = self.tabs.active_id();
        let tabs: Vec<Tab> = self.tabs.tabs().to_vec();
        div()
            .h(px(TAB_STRIP_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .pl(px(80.0))
            .pr_2()
            .bg(theme.chrome)
            .border_b_1()
            .border_color(theme.border)
            .window_control_area(WindowControlArea::Drag)
            .children(tabs.into_iter().enumerate().map(|(index, tab)| {
                let id = tab.id;
                let entity = cx.entity();
                let title = tab.navigation.display_title();
                // While a tab is loading, a small accent dot takes the
                // favicon slot so each tab shows its own progress state.
                let icon: Option<AnyElement> = if tab.navigation.is_loading() {
                    Some(
                        div()
                            .flex_none()
                            .size(px(14.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(6.0)).rounded_full().bg(theme.accent))
                            .into_any_element(),
                    )
                } else {
                    self.tab_favicons.get(&id).map(|image| {
                        div()
                            .flex_none()
                            .size(px(14.0))
                            .child(img(image.clone()).size(px(14.0)).rounded_sm())
                            .into_any_element()
                    })
                };
                tab_chip(
                    theme,
                    index,
                    title.clone(),
                    icon,
                    Some(id) == active_id,
                    cx.listener(move |this, _event, window, cx| this.activate_tab(id, window, cx)),
                    cx.listener(move |this, _event, window, cx| this.close_tab(id, window, cx)),
                )
                .on_drag(
                    TabDrag {
                        id,
                        title: title.into(),
                    },
                    |drag: &TabDrag, _position, _window, cx| {
                        cx.new(|_| TabDragPreview {
                            title: drag.title.clone(),
                        })
                    },
                )
                .on_drop(move |drag: &TabDrag, _window, cx| {
                    entity.update(cx, |this, cx| this.reorder_tab(drag.id, index, cx));
                })
            }))
            .child(icon_button(
                theme,
                "new-tab",
                "+",
                true,
                cx.listener(|this, _event, window, cx| this.new_tab(window, cx)),
            ))
    }

    fn render_new_tab_page(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let recents = self.new_tab_recents.clone();
        let now_millis = unix_millis_now();
        let recents_row =
            div()
                .w(px(680.0))
                .flex()
                .flex_col()
                .gap_1()
                .children(recents.into_iter().map(|entry| {
                    let url = entry.url.clone();
                    let hover = theme.surface_hover;
                    div()
                        .id(gpui::ElementId::named_usize(
                            "newtab-recent",
                            entry.id as usize,
                        ))
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(move |row| row.bg(hover))
                        .on_click(cx.listener(move |this, _event, window, cx| {
                            this.navigate(&url, window, cx)
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(13.0))
                                .text_color(theme.text)
                                .child(entry.display_title()),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.text_muted)
                                .child(relative_time(entry.last_visited_at, now_millis)),
                        )
                }));
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(90.0))
            .gap_4()
            .bg(theme.background)
            .child(
                div()
                    .text_size(px(26.0))
                    .text_color(theme.text)
                    .child("GPUI Browser"),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(theme.text_muted)
                    .child("Press ⌘L to search or enter an address"),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .pt_2()
                    .child(text_button(
                        theme,
                        "newtab-history",
                        "History",
                        cx.listener(|this, _event, window, cx| this.open_history(window, cx)),
                    ))
                    .child(text_button(
                        theme,
                        "newtab-bookmarks",
                        "Bookmarks",
                        cx.listener(|this, _event, window, cx| this.open_bookmarks(window, cx)),
                    ))
                    .child(text_button(
                        theme,
                        "newtab-downloads",
                        "Downloads",
                        cx.listener(|this, _event, window, cx| this.open_downloads(window, cx)),
                    ))
                    .child(text_button(
                        theme,
                        "newtab-settings",
                        "Settings",
                        cx.listener(|this, _event, window, cx| this.open_settings(window, cx)),
                    )),
            )
            .child(
                div()
                    .pt_4()
                    .text_size(px(11.0))
                    .text_color(theme.text_muted)
                    .child(if self.new_tab_recents.is_empty() {
                        "Pages you visit will appear here."
                    } else {
                        "Recent"
                    }),
            )
            .child(recents_row)
            .into_any_element()
    }

    fn render_bookmark_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let entries: Vec<_> = self.bookmarks.in_folder(None).cloned().collect();
        div()
            .h(px(30.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .bg(theme.chrome)
            .border_b_1()
            .border_color(theme.border)
            .overflow_hidden()
            .children(entries.into_iter().map(|bookmark| {
                let url = bookmark.url.clone();
                let hover = theme.surface_hover;
                div()
                    .id(gpui::ElementId::named_usize(
                        "bookmark-bar-item",
                        bookmark.id as usize,
                    ))
                    .flex()
                    .items_center()
                    .h(px(22.0))
                    .max_w(px(180.0))
                    .px_2()
                    .rounded_sm()
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(theme.text)
                    .cursor_pointer()
                    .hover(move |item| item.bg(hover))
                    .on_click(
                        cx.listener(move |this, _event, window, cx| {
                            this.navigate(&url, window, cx)
                        }),
                    )
                    .child(bookmark.display_title())
            }))
            .child(div().flex_1())
            .child(icon_button(
                theme,
                "bookmark-bar-manage",
                "▤",
                true,
                cx.listener(|this, _event, window, cx| this.open_bookmarks(window, cx)),
            ))
            .into_any_element()
    }

    fn render_find_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let input = self.find_input.clone();
        let status = match self.find_result {
            Some(true) => Some(("Match found", theme.text_muted)),
            Some(false) => Some(("No matches", theme.danger)),
            None => None,
        };
        let input_element: AnyElement = match input {
            Some(input) => div()
                .flex_1()
                .max_w(px(320.0))
                .h(px(26.0))
                .rounded_md()
                .bg(theme.address_background)
                .border_1()
                .border_color(theme.border)
                .overflow_hidden()
                .child(input)
                .into_any_element(),
            None => div().flex_1().into_any_element(),
        };
        div()
            .h(px(36.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .bg(theme.chrome)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child("Find"),
            )
            .child(input_element)
            .children(status.map(|(text, color)| {
                div()
                    .text_size(px(11.0))
                    .text_color(color)
                    .child(text)
                    .into_any_element()
            }))
            .child(div().flex_1())
            .child(icon_button(
                theme,
                "find-previous",
                "↑",
                true,
                cx.listener(|this, _event, window, cx| this.find_previous(window, cx)),
            ))
            .child(icon_button(
                theme,
                "find-next",
                "↓",
                true,
                cx.listener(|this, _event, window, cx| this.find_next(window, cx)),
            ))
            .child(icon_button(
                theme,
                "find-close",
                "✕",
                true,
                cx.listener(|this, _event, window, cx| this.close_find(window, cx)),
            ))
            .into_any_element()
    }

    fn render_history(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let now = unix_millis_now();
        let entries = self.history_entries.clone();
        let count = entries.len();
        let loading = self.history_loading;
        let error = self.history_error.clone();

        let header = div()
            .h(px(58.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_6()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.chrome)
            .child(
                div()
                    .text_size(px(16.0))
                    .text_color(theme.text)
                    .child("History"),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(if loading {
                        "Loading…".to_owned()
                    } else {
                        format!("{count} {}", if count == 1 { "entry" } else { "entries" })
                    }),
            )
            .child(div().flex_1())
            .child(text_button(
                theme,
                "clear-site-data",
                "Clear site data",
                cx.listener(|this, _event, _window, cx| this.clear_browsing_data(cx)),
            ))
            .child(text_button(
                theme,
                "clear-history",
                "Clear All",
                cx.listener(|this, _event, _window, cx| this.clear_history(cx)),
            ))
            .child(icon_button(
                theme,
                "close-history",
                "✕",
                true,
                cx.listener(|this, _event, window, cx| this.close_overlay(window, cx)),
            ));

        let body: AnyElement = if let Some(error) = error {
            div()
                .flex_1()
                .p_6()
                .text_size(px(12.0))
                .text_color(theme.danger)
                .child(format!("Could not load history: {error}"))
                .into_any_element()
        } else if loading && entries.is_empty() {
            div()
                .flex_1()
                .p_6()
                .text_size(px(12.0))
                .text_color(theme.text_muted)
                .child("Loading history…")
                .into_any_element()
        } else if entries.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_1()
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(theme.text)
                        .child("No browsing history yet"),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text_muted)
                        .child("Pages you visit will appear here."),
                )
                .into_any_element()
        } else {
            div()
                .id("history-list")
                .flex_1()
                .overflow_y_scroll()
                .px_4()
                .py_2()
                .flex()
                .flex_col()
                .children(entries.into_iter().map(move |entry| {
                    let url = entry.url.clone();
                    let id = entry.id;
                    let row_hover = theme.surface_hover;
                    div()
                        .id(gpui::ElementId::named_usize("history-row", id as usize))
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .hover(move |row| row.bg(row_hover))
                        .child(
                            div()
                                .id(gpui::ElementId::named_usize("history-open", id as usize))
                                .flex()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex_col()
                                .gap_1()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _event, window, cx| {
                                    this.open_history_entry(&url, window, cx)
                                }))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(13.0))
                                        .text_color(theme.text)
                                        .child(entry.display_title()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(11.0))
                                        .text_color(theme.text_muted)
                                        .child(entry.url.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .w(px(110.0))
                                .text_size(px(11.0))
                                .text_color(theme.text_muted)
                                .child(relative_time(entry.last_visited_at, now)),
                        )
                        .child(
                            div()
                                .id(gpui::ElementId::named_usize("history-delete", id as usize))
                                .flex_none()
                                .size(px(22.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .text_size(px(11.0))
                                .text_color(theme.text_muted)
                                .cursor_pointer()
                                .hover(move |button| button.bg(theme.surface_active))
                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                    this.delete_history_entry(id, cx)
                                }))
                                .child("✕"),
                        )
                }))
                .into_any_element()
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .bg(theme.background)
            .child(header)
            .child(body)
    }

    fn render_toolbar(
        &self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let engine = self.active_engine();
        let back_enabled = engine.as_ref().is_some_and(|engine| engine.can_go_back());
        let forward_enabled = engine
            .as_ref()
            .is_some_and(|engine| engine.can_go_forward());
        let loading = self
            .tabs
            .active_tab()
            .is_some_and(|tab| tab.navigation.is_loading());
        let has_page = self
            .tabs
            .active_tab()
            .is_some_and(|tab| !tab.navigation.url.is_empty());
        let bookmarked = self
            .tabs
            .active_tab()
            .is_some_and(|tab| self.bookmarks.is_bookmarked(&tab.navigation.url));
        let address_focused = self.address.read(cx).is_focused(window);
        let status = self.status.clone();

        div()
            .h(px(44.0))
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .bg(theme.chrome)
            .child(icon_button(
                theme,
                "back",
                "←",
                back_enabled,
                cx.listener(|this, _event, _window, cx| this.go_back(cx)),
            ))
            .child(icon_button(
                theme,
                "forward",
                "→",
                forward_enabled,
                cx.listener(|this, _event, _window, cx| this.go_forward(cx)),
            ))
            .child(icon_button(
                theme,
                "reload-stop",
                if loading { "✕" } else { "↻" },
                has_page,
                cx.listener(|this, _event, _window, cx| this.reload_or_stop(cx)),
            ))
            .child(icon_button(
                theme,
                "bookmark",
                if bookmarked { "★" } else { "☆" },
                has_page,
                cx.listener(|this, _event, window, cx| this.toggle_bookmark(window, cx)),
            ))
            .child(icon_button(
                theme,
                "bookmarks",
                "▤",
                true,
                cx.listener(|this, _event, window, cx| this.toggle_bookmarks(window, cx)),
            ))
            .child(icon_button(
                theme,
                "downloads",
                "⇩",
                true,
                cx.listener(|this, _event, window, cx| this.toggle_downloads(window, cx)),
            ))
            .child(icon_button(
                theme,
                "history",
                "◷",
                true,
                cx.listener(|this, _event, window, cx| this.toggle_history(window, cx)),
            ))
            .child(icon_button(
                theme,
                "settings",
                "⚙",
                true,
                cx.listener(|this, _event, window, cx| this.open_settings(window, cx)),
            ))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .h(px(30.0))
                    .mx_1()
                    .rounded_md()
                    .bg(theme.address_background)
                    .border_1()
                    .border_color(if address_focused {
                        theme.accent
                    } else {
                        theme.border
                    })
                    .overflow_hidden()
                    .child(self.address.clone()),
            )
            .children(status.map(|status| {
                div()
                    .max_w(px(280.0))
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(theme.text_muted)
                    .child(status)
            }))
    }

    fn render_content(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let active_id = self.tabs.active_id();
        let engine = self.active_engine();
        let tab = self.tabs.active_tab().cloned();
        let show_page = active_id
            .map(|id| self.page_should_show(id, active_id))
            .unwrap_or(false);

        let mut content = div().flex_1().overflow_hidden().bg(theme.background);
        if let Some(engine) = engine {
            content = content.child(web_content_host(engine, show_page));
        }

        if self.overlay == Overlay::History {
            return content.child(self.render_history(theme, cx));
        }
        if self.overlay == Overlay::Bookmarks {
            return content.child(self.render_bookmarks_page(theme, cx));
        }
        if self.overlay == Overlay::Downloads {
            return content.child(self.render_downloads_page(theme, cx));
        }
        if self.overlay == Overlay::Settings {
            return content.child(self.render_settings_page(theme, cx));
        }
        if self.overlay == Overlay::Palette {
            return content.child(self.render_palette(theme, cx));
        }

        if show_page {
            return content;
        }

        match tab {
            Some(tab) if tab.navigation.error.is_some() => {
                let Some(error) = tab.navigation.error.clone() else {
                    return content;
                };
                content = content.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .bg(theme.background)
                        .child(
                            div()
                                .text_size(px(18.0))
                                .text_color(theme.text)
                                .child("This page couldn't load"),
                        )
                        .child(
                            div()
                                .max_w(px(520.0))
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(theme.text_muted)
                                .child(error.url),
                        )
                        .child(
                            div()
                                .max_w(px(520.0))
                                .text_size(px(12.0))
                                .text_color(theme.text_muted)
                                .child(error.message),
                        )
                        .child(text_button(
                            theme,
                            "retry",
                            "Try again",
                            cx.listener(|this, _event, _window, cx| {
                                if let Some(engine) = this.active_engine() {
                                    if let Some(url) = engine.current_url()
                                        && let Some(tab) = this.tabs.active_tab_mut()
                                    {
                                        tab.navigation.apply(NavigationEvent::Started { url });
                                    }
                                    engine.reload();
                                }
                                cx.notify();
                            }),
                        )),
                );
            }
            Some(tab) if tab.navigation.url.is_empty() => {
                content = content.child(self.render_new_tab_page(theme, cx));
            }
            _ => {}
        }

        content
    }
}

impl Render for BrowserWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let loading = self
            .tabs
            .active_tab()
            .is_some_and(|tab| tab.navigation.is_loading());
        let progress = self
            .tabs
            .active_tab()
            .map(|tab| tab.navigation.progress_percent)
            .unwrap_or(0);

        // Keep native visibility in sync even for tabs hidden without a redraw.
        self.update_engine_visibility();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_focus_location))
            .on_action(cx.listener(Self::on_back))
            .on_action(cx.listener(Self::on_forward))
            .on_action(cx.listener(Self::on_reload))
            .on_action(cx.listener(Self::on_hard_reload))
            .on_action(cx.listener(Self::on_stop))
            .on_action(cx.listener(Self::on_new_tab))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_duplicate_tab))
            .on_action(cx.listener(Self::on_reopen_closed_tab))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_previous_tab))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::on_find_in_page))
            .on_action(cx.listener(Self::on_find_next))
            .on_action(cx.listener(Self::on_find_previous))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_toggle_history))
            .on_action(cx.listener(Self::on_toggle_bookmark))
            .on_action(cx.listener(Self::on_toggle_bookmarks))
            .on_action(cx.listener(Self::on_toggle_downloads))
            .on_action(cx.listener(Self::on_copy_url))
            .on_action(cx.listener(Self::on_command_palette))
            .on_action(cx.listener(Self::on_palette_next))
            .on_action(cx.listener(Self::on_palette_previous))
            .on_action(cx.listener(Self::on_quit))
            .child(self.render_tab_strip(&theme, cx))
            .child(self.render_toolbar(&theme, window, cx))
            .children(
                (self.bookmark_bar_count_for_render() > 0 && self.overlay == Overlay::None)
                    .then(|| self.render_bookmark_bar(&theme, cx)),
            )
            .child(loading_bar(&theme, progress, loading))
            .children(self.find_open.then(|| self.render_find_bar(&theme, cx)))
            .child(self.render_content(&theme, cx))
            .children(self.render_folder_prompt(&theme, cx))
    }
}
