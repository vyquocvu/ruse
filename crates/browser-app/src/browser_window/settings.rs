//! Settings state and GPUI settings page.
//!
//! Inputs are created lazily when the page opens and re-filled from the current
//! settings, so cancelling discards drafts. Saving validates the search
//! template, applies the theme immediately and persists asynchronously.

use browser_core::{BrowserSettings, DEFAULT_HOMEPAGE, ThemeMode};
use browser_storage::SettingsStore;
use browser_ui::{TextInput, Theme, icon_button, text_button};
use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*, px};

use super::{BrowserWindow, Overlay};

pub(super) struct SettingsState {
    pub(super) store: Option<SettingsStore>,
    /// Serialises settings writes; see `start_settings_writer`.
    pub(super) sender: Option<async_channel::Sender<BrowserSettings>>,
    pub(super) homepage_input: Option<Entity<TextInput>>,
    pub(super) search_input: Option<Entity<TextInput>>,
    pub(super) subscriptions: Vec<Subscription>,
    pub(super) save_error: Option<String>,
}

impl SettingsState {
    pub(super) fn new(store: Option<SettingsStore>) -> Self {
        Self {
            store,
            sender: None,
            homepage_input: None,
            search_input: None,
            subscriptions: Vec::new(),
            save_error: None,
        }
    }
}

impl BrowserWindow {
    /// Applies the configured theme, falling back to the OS appearance.
    pub(super) fn apply_theme(&self, window: &Window, cx: &mut Context<Self>) {
        let theme = match self.settings.theme {
            ThemeMode::System => Theme::for_appearance(window.appearance()),
            ThemeMode::Light => Theme::light(),
            ThemeMode::Dark => Theme::dark(),
        };
        Theme::install(cx, theme);
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::Settings;
        self.update_engine_visibility();
        self.settings_page.save_error = None;
        if self.settings_page.homepage_input.is_none() {
            let homepage = cx.new(|cx| TextInput::new(cx, "Homepage URL", px(13.0)));
            let search =
                cx.new(|cx| TextInput::new(cx, "https://duckduckgo.com/?q={query}", px(13.0)));
            let subscriptions = vec![
                cx.subscribe_in(
                    &homepage,
                    window,
                    |this, _input, event: &browser_ui::TextInputEvent, window, cx| match event {
                        browser_ui::TextInputEvent::Submitted(_) => this.save_settings(window, cx),
                        browser_ui::TextInputEvent::Cancelled => this.close_overlay(window, cx),
                        browser_ui::TextInputEvent::Changed(_) => {}
                    },
                ),
                cx.subscribe_in(
                    &search,
                    window,
                    |this, _input, event: &browser_ui::TextInputEvent, window, cx| match event {
                        browser_ui::TextInputEvent::Submitted(_) => this.save_settings(window, cx),
                        browser_ui::TextInputEvent::Cancelled => this.close_overlay(window, cx),
                        browser_ui::TextInputEvent::Changed(_) => {}
                    },
                ),
            ];
            self.settings_page.subscriptions.extend(subscriptions);
            self.settings_page.homepage_input = Some(homepage);
            self.settings_page.search_input = Some(search);
        }
        let settings = self.settings.clone();
        if let Some(input) = self.settings_page.homepage_input.clone() {
            input.update(cx, |input, cx| {
                input.set_text(settings.homepage.clone(), cx)
            });
        }
        if let Some(input) = self.settings_page.search_input.clone() {
            input.update(cx, |input, cx| {
                input.set_text(settings.search_engine.url_template.clone(), cx)
            });
        }
        if let Err(error) = browser_platform::focus_chrome(window) {
            tracing::warn!(%error, "could not focus the settings page");
        }
        cx.notify();
    }

    /// Reads the page inputs, validates them and persists the result.
    pub fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let homepage = self
            .settings_page
            .homepage_input
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_owned())
            .unwrap_or_else(|| self.settings.homepage.clone());
        let template = self
            .settings_page
            .search_input
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_owned())
            .unwrap_or_else(|| self.settings.search_engine.url_template.clone());

        let homepage = if homepage.is_empty() {
            DEFAULT_HOMEPAGE.to_owned()
        } else {
            homepage
        };
        if !template.contains("{query}") {
            self.settings_page.save_error = Some("Search URL must contain {query}".to_owned());
            cx.notify();
            return;
        }
        let engine_name = url::Url::parse(&template)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "Custom".to_owned());
        self.settings.homepage = homepage;
        self.settings.search_engine.name = engine_name;
        self.settings.search_engine.url_template = template;
        self.settings_page.save_error = None;
        self.persist_settings(cx);
        self.apply_theme(window, cx);
        self.status = Some("Settings saved".into());
        cx.notify();
    }

    /// Starts the single background writer for settings snapshots.
    pub(super) fn start_settings_writer(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.settings_page.store.clone() else {
            return;
        };
        let (sender, receiver) = async_channel::unbounded();
        self.settings_page.sender = Some(sender);
        cx.background_executor()
            .spawn(async move {
                while let Ok(settings) = receiver.recv().await {
                    if let Err(error) = store.save(&settings) {
                        tracing::warn!(%error, "could not save settings");
                    }
                }
            })
            .detach();
    }

    fn persist_settings(&self, cx: &mut Context<Self>) {
        let Some(sender) = self.settings_page.sender.as_ref() else {
            return;
        };
        if let Err(error) = sender.try_send(self.settings.clone()) {
            tracing::debug!(%error, "settings writer unavailable");
        }
        cx.notify();
    }

    /// Updates the homepage and persists (also used by probes).
    pub fn set_homepage(&mut self, homepage: &str, window: &mut Window, cx: &mut Context<Self>) {
        let homepage = homepage.trim();
        if homepage.is_empty() {
            return;
        }
        self.settings.homepage = homepage.to_owned();
        if let Some(input) = self.settings_page.homepage_input.clone() {
            let text = self.settings.homepage.clone();
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
        self.persist_settings(cx);
        self.apply_theme(window, cx);
        cx.notify();
    }

    /// Updates the search engine template (must contain `{query}`).
    pub fn set_search_engine_template(
        &mut self,
        template: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !template.contains("{query}") {
            return false;
        }
        self.settings.search_engine.url_template = template.to_owned();
        self.settings.search_engine.name = url::Url::parse(template)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "Custom".to_owned());
        if let Some(input) = self.settings_page.search_input.clone() {
            let text = self.settings.search_engine.url_template.clone();
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
        self.persist_settings(cx);
        self.apply_theme(window, cx);
        cx.notify();
        true
    }

    pub fn set_theme(&mut self, theme: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.theme = theme;
        self.persist_settings(cx);
        self.apply_theme(window, cx);
        cx.notify();
    }

    pub(super) fn render_settings_page(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let homepage_input = self.settings_page.homepage_input.clone();
        let search_input = self.settings_page.search_input.clone();
        let error = self.settings_page.save_error.clone();
        let current_mode = self.settings.theme;

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
                    .child("Settings"),
            )
            .child(div().flex_1())
            .child(text_button(
                theme,
                "save-settings",
                "Save",
                cx.listener(|this, _event, window, cx| this.save_settings(window, cx)),
            ))
            .child(icon_button(
                theme,
                "close-settings",
                "✕",
                true,
                cx.listener(|this, _event, window, cx| this.close_overlay(window, cx)),
            ));

        let mut body = div()
            .id("settings-scroll")
            .flex_1()
            .overflow_y_scroll()
            .px_8()
            .py_6()
            .flex()
            .flex_col()
            .gap_6();

        let field = |label: &str, hint: &str, input: Option<Entity<TextInput>>, theme: &Theme| {
            let mut column = div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme.text)
                        .child(label.to_owned()),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.text_muted)
                        .child(hint.to_owned()),
                );
            if let Some(input) = input {
                column = column.child(
                    div()
                        .w(px(460.0))
                        .h(px(30.0))
                        .rounded_md()
                        .bg(theme.address_background)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(input),
                );
            }
            column
        };

        body = body
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child("GENERAL"),
                    )
                    .child(field(
                        "Homepage",
                        "Opened when a new window starts.",
                        homepage_input,
                        theme,
                    ))
                    .child(field(
                        "Search engine URL",
                        "Use {query} where the search terms go. Enter saves.",
                        search_input,
                        theme,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child("APPEARANCE"),
                    )
                    .child(
                        div().flex().gap_2().children(
                            [
                                ("System", ThemeMode::System),
                                ("Light", ThemeMode::Light),
                                ("Dark", ThemeMode::Dark),
                            ]
                            .into_iter()
                            .map(|(label, mode)| {
                                choice_button(
                                    theme,
                                    label,
                                    current_mode == mode,
                                    cx.listener(move |this, _event, window, cx| {
                                        this.set_theme(mode, window, cx)
                                    }),
                                )
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child("PRIVACY"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(text_button(
                                theme,
                                "settings-clear-site-data",
                                "Clear site data",
                                cx.listener(|this, _event, _window, cx| {
                                    this.clear_browsing_data(cx)
                                }),
                            ))
                            .child(text_button(
                                theme,
                                "settings-clear-history",
                                "Clear history",
                                cx.listener(|this, _event, _window, cx| this.clear_history(cx)),
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child("Clears cookies, caches, local storage and browsing history."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child("ABOUT"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text)
                            .child("GPUI Browser 0.1 · WebKit engine"),
                    ),
            );

        if let Some(error) = error {
            body = body.child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.danger)
                    .child(error),
            );
        }

        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .bg(theme.background)
            .child(header)
            .child(body)
            .into_any_element()
    }
}

fn choice_button(
    theme: &Theme,
    label: &'static str,
    selected: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let background = if selected {
        theme.accent
    } else {
        theme.surface
    };
    let text_color = if selected {
        theme.accent_text
    } else {
        theme.text
    };
    let hover = theme.surface_hover;
    div()
        .id(label)
        .flex()
        .items_center()
        .justify_center()
        .h(px(28.0))
        .px_4()
        .rounded_md()
        .bg(background)
        .border_1()
        .border_color(theme.border)
        .text_size(px(12.0))
        .text_color(text_color)
        .cursor_pointer()
        .hover(move |button| button.bg(hover))
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(label)
        .into_any_element()
}
