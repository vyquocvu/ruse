//! Reusable GPUI chrome components.
//!
//! Components are plain functions taking a [`Theme`] so they stay stateless and
//! cheap to compose. All clicking is handled by the caller.

use std::rc::Rc;

use browser_engine::Engine;
use gpui::{
    App, ClickEvent, ElementId, IntoElement, SharedString, Window, canvas, div, prelude::*, px,
};

use crate::theme::Theme;

/// Height of the tab strip row.
pub const TAB_STRIP_HEIGHT: f32 = 38.0;
/// Height of the navigation toolbar row.
pub const TOOLBAR_HEIGHT: f32 = 44.0;
/// Total chrome height above page content.
pub const CHROME_HEIGHT: f32 = 82.0;

/// A square icon-sized button used by the navigation toolbar.
pub fn icon_button(
    theme: &Theme,
    id: impl Into<ElementId>,
    glyph: &'static str,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let hover = theme.surface_hover;
    let text = theme.text;
    let muted = theme.text_muted;
    div()
        .id(id)
        .flex()
        .flex_none()
        .size(px(30.0))
        .justify_center()
        .items_center()
        .rounded_md()
        .text_size(px(15.0))
        .text_color(if enabled { text } else { muted })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .when(!enabled, |button| button.opacity(0.35))
        .child(glyph)
}

/// A compact text button (for example "New Tab" or dialogs).
pub fn text_button(
    theme: &Theme,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let hover = theme.surface_hover;
    div()
        .id(id)
        .flex()
        .flex_none()
        .h(px(26.0))
        .px_3()
        .justify_center()
        .items_center()
        .rounded_md()
        .text_size(px(12.0))
        .text_color(theme.text)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(label.into())
}

/// One tab-strip entry.
pub fn tab_chip(
    theme: &Theme,
    index: usize,
    title: impl Into<SharedString>,
    icon: Option<gpui::AnyElement>,
    active: bool,
    on_activate: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let background = if active {
        theme.tab_active
    } else {
        theme.tab_inactive
    };
    let hover = theme.surface_hover;
    let close_hover = theme.surface_active;
    let text_color = if active { theme.text } else { theme.text_muted };
    let title: SharedString = title.into();
    let mut chip = div()
        .id(("tab", index))
        .flex()
        .flex_none()
        .items_center()
        .h(px(30.0))
        .w(px(180.0))
        .rounded_md()
        .bg(background)
        .border_1()
        .border_color(if active {
            theme.border
        } else {
            theme.tab_inactive
        })
        .overflow_hidden()
        .child({
            let mut title_row = div()
                .id(("tab-title", index))
                .flex()
                .flex_1()
                .h_full()
                .items_center()
                .gap_2()
                .px_3()
                .overflow_hidden()
                .cursor_pointer()
                .on_click(move |event, window, cx| on_activate(event, window, cx));
            if let Some(icon) = icon {
                title_row = title_row.child(icon);
            }
            title_row.child(
                div()
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(text_color)
                    .child(title),
            )
        })
        .child(
            div()
                .id(("tab-close", index))
                .flex()
                .flex_none()
                .size(px(20.0))
                .mr(px(4.0))
                .justify_center()
                .items_center()
                .rounded_sm()
                .text_size(px(12.0))
                .text_color(theme.text_muted)
                .cursor_pointer()
                .hover(move |style| style.bg(close_hover))
                .on_click(move |event, window, cx| on_close(event, window, cx))
                .child("×"),
        );
    if !active {
        chip = chip.hover(move |style| style.bg(hover));
    }
    chip
}

/// Thin loading indicator drawn under the toolbar.
pub fn loading_bar(theme: &Theme, progress_percent: u8, visible: bool) -> impl IntoElement {
    let accent = theme.accent;
    let width = if visible {
        (progress_percent.max(8) as f32) / 100.0
    } else {
        0.0
    };
    div()
        .h(px(2.0))
        .w_full()
        .bg(theme.chrome)
        .when(visible, |bar| {
            bar.child(
                div()
                    .h_full()
                    .w(gpui::relative(width))
                    .bg(accent)
                    .rounded_full(),
            )
        })
}

/// Hosts a native web view inside the GPUI layout.
///
/// The native view is positioned to the element's bounds on every frame and
/// hidden when `visible` is false (for example while an error page or the
/// settings page is shown). GPUI cannot draw above it, so anything covering the
/// page must hide it instead.
pub fn web_content_host(engine: Rc<dyn Engine>, visible: bool) -> impl IntoElement {
    canvas(
        move |_bounds, _window, _cx| {},
        move |bounds, _state, _window, _cx| {
            engine.set_visible(visible);
            if visible {
                engine.set_viewport(browser_engine::Viewport::new(
                    f64::from(bounds.origin.x),
                    f64::from(bounds.origin.y),
                    f64::from(bounds.size.width),
                    f64::from(bounds.size.height),
                ));
            }
        },
    )
    .size_full()
}
