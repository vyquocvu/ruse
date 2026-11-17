//! GPUI rendering for the browser window.
//!
//! Kept separate from state/actions so each file stays reviewable. These impls
//! are on the same type and may access the window's private state.

use browser_core::{NavigationEvent, TabId};
use browser_ui::{
    TAB_STRIP_HEIGHT, Theme, icon_button, loading_bar, tab_chip, text_button, web_content_host,
};
use gpui::{
    AnyElement, Context, IntoElement, Render, SharedString, Window, WindowControlArea, div, img,
    prelude::*, px,
};

use super::{BrowserWindow, Tab};

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
                content = content.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .bg(theme.background)
                        .child(
                            div()
                                .text_size(px(24.0))
                                .text_color(theme.text)
                                .child("New Tab"),
                        )
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(theme.text_muted)
                                .child("Type a URL or search above. Press ⌘L to jump to the address bar."),
                        ),
                );
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
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_quit))
            .child(self.render_tab_strip(&theme, cx))
            .child(self.render_toolbar(&theme, window, cx))
            .child(loading_bar(&theme, progress, loading))
            .child(self.render_content(&theme, cx))
    }
}
