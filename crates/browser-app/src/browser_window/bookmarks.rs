//! Bookmarks state and UI.
//!
//! The store is loaded synchronously once at startup (bookmarks are small).
//! Mutations update memory immediately for the star state, persist through the
//! background executor, and then reload the in-memory lists so ids stay in
//! sync with SQLite.

use std::collections::HashSet;

use browser_core::{Bookmark, BookmarkFolder, unix_millis_now};
use browser_storage::BookmarkStore;
use browser_ui::{TextInput, Theme, icon_button, text_button};
use gpui::{AnyElement, Context, Entity, Window, div, prelude::*, px};

use super::{BrowserWindow, Overlay};

/// In-memory view of bookmarks and folders.
pub(super) struct BookmarksState {
    pub(super) store: Option<BookmarkStore>,
    pub(super) folders: Vec<BookmarkFolder>,
    pub(super) bookmarks: Vec<Bookmark>,
    pub(super) bookmarked_urls: HashSet<String>,
    pub(super) folder_prompt: Option<Entity<TextInput>>,
    pub(super) error: Option<String>,
}

impl BookmarksState {
    pub(super) fn new(store: Option<BookmarkStore>) -> Self {
        let mut state = Self {
            store,
            folders: Vec::new(),
            bookmarks: Vec::new(),
            bookmarked_urls: HashSet::new(),
            folder_prompt: None,
            error: None,
        };
        state.load_blocking();
        state
    }

    fn load_blocking(&mut self) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        match (store.folders(), store.list()) {
            (Ok(folders), Ok(bookmarks)) => {
                self.bookmarked_urls = bookmarks.iter().map(|b| b.url.clone()).collect();
                self.folders = folders;
                self.bookmarks = bookmarks;
                self.error = None;
            }
            (Err(error), _) | (_, Err(error)) => {
                self.error = Some(error.to_string());
            }
        }
    }

    pub(super) fn count_unfiled(&self) -> usize {
        self.bookmarks
            .iter()
            .filter(|bookmark| bookmark.folder_id.is_none())
            .count()
    }

    pub(super) fn is_bookmarked(&self, url: &str) -> bool {
        self.bookmarked_urls.contains(url)
    }

    pub(super) fn in_folder(&self, folder_id: Option<i64>) -> impl Iterator<Item = &Bookmark> {
        self.bookmarks
            .iter()
            .filter(move |bookmark| bookmark.folder_id == folder_id)
    }
}

impl BrowserWindow {
    /// Runs a bookmark write on the background executor, then reloads the
    /// in-memory lists from SQLite. Reloading only after the write completes
    /// avoids the optimistic-state overwrite race.
    fn run_bookmark_write<R, F>(&self, cx: &mut Context<Self>, write: F)
    where
        R: Send + 'static,
        F: FnOnce(BookmarkStore) -> R + Send + 'static,
    {
        let Some(store) = self.bookmarks.store.clone() else {
            return;
        };
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.spawn(async move { write(store) }).await;
            let _ = this.update(cx, |this, cx| this.refresh_bookmarks(cx));
        })
        .detach();
    }

    /// Adds or removes the active page from bookmarks (Cmd+D).
    pub fn toggle_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.active_tab() else {
            return;
        };
        let url = tab.navigation.url.clone();
        let title = tab.navigation.display_title();
        if url.is_empty() {
            return;
        }
        if self.bookmarks.is_bookmarked(&url) {
            self.bookmarks.bookmarked_urls.remove(&url);
            self.bookmarks
                .bookmarks
                .retain(|bookmark| bookmark.url != url);
            let url_for_task = url.clone();
            self.run_bookmark_write(cx, move |store| {
                if let Err(error) = store.remove_url(&url_for_task) {
                    tracing::warn!(%error, "could not remove bookmark");
                }
            });
            self.status = Some("Bookmark removed".into());
        } else {
            self.bookmarks.bookmarked_urls.insert(url.clone());
            let url_for_task = url.clone();
            let title_for_task = title.clone();
            let created_at = unix_millis_now();
            self.run_bookmark_write(cx, move |store| {
                if let Err(error) = store.add(&url_for_task, &title_for_task, None, created_at) {
                    tracing::warn!(%error, "could not add bookmark");
                }
            });
            self.status = Some("Bookmark added".into());
        }
        cx.notify();
        let _ = window;
    }

    /// Opens or closes the bookmarks manager.
    pub fn toggle_bookmarks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay == Overlay::Bookmarks {
            self.close_overlay(window, cx);
        } else {
            self.open_bookmarks(window, cx);
        }
    }

    pub fn open_bookmarks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::Bookmarks;
        self.update_engine_visibility();
        if let Err(error) = browser_platform::focus_chrome(window) {
            tracing::warn!(%error, "could not focus the bookmarks page");
        }
        self.refresh_bookmarks(cx);
        cx.notify();
    }

    /// Reloads bookmarks and folders from SQLite on a background thread.
    pub fn refresh_bookmarks(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.bookmarks.store.clone() else {
            return;
        };
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let loaded = executor
                .spawn(async move {
                    let folders = store.folders();
                    let bookmarks = store.list();
                    (folders, bookmarks)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    (Ok(folders), Ok(bookmarks)) => {
                        this.bookmarks.bookmarked_urls =
                            bookmarks.iter().map(|b| b.url.clone()).collect();
                        this.bookmarks.folders = folders;
                        this.bookmarks.bookmarks = bookmarks;
                        this.bookmarks.error = None;
                    }
                    (Err(error), _) | (_, Err(error)) => {
                        this.bookmarks.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn delete_bookmark(&mut self, id: i64, cx: &mut Context<Self>) {
        self.bookmarks
            .bookmarks
            .retain(|bookmark| bookmark.id != id);
        self.run_bookmark_write(cx, move |store| {
            if let Err(error) = store.remove(id) {
                tracing::warn!(%error, "could not delete bookmark");
            }
        });
        cx.notify();
    }

    /// Saves the active page into a folder (or moves it there).
    pub fn bookmark_active_in_folder(
        &mut self,
        folder_id: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.active_tab() else {
            return;
        };
        let url = tab.navigation.url.clone();
        let title = tab.navigation.display_title();
        if url.is_empty() {
            return;
        }
        self.bookmarks.bookmarked_urls.insert(url.clone());
        let created_at = unix_millis_now();
        self.run_bookmark_write(cx, move |store| {
            if let Err(error) = store.add(&url, &title, folder_id, created_at) {
                tracing::warn!(%error, "could not save bookmark to folder");
            }
        });
        cx.notify();
        let _ = window;
    }

    pub fn delete_bookmark_folder(&mut self, id: i64, cx: &mut Context<Self>) {
        self.run_bookmark_write(cx, move |store| {
            if let Err(error) = store.delete_folder(id) {
                tracing::warn!(%error, "could not delete folder");
            }
        });
        cx.notify();
    }

    /// Shows the "New Folder" prompt.
    pub fn prompt_new_bookmark_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new(cx, "Folder name", px(13.0)));
        let subscription =
            cx.subscribe_in(
                &input,
                window,
                |this, _input, event, window, cx| match event {
                    browser_ui::TextInputEvent::Submitted(name) => {
                        this.create_bookmark_folder(name, window, cx)
                    }
                    browser_ui::TextInputEvent::Cancelled => this.cancel_folder_prompt(window, cx),
                    browser_ui::TextInputEvent::Changed(_) => {}
                },
            );
        self.subscriptions.push(subscription);
        input.update(cx, |input, cx| input.focus_and_select_all(window, cx));
        let _ = browser_platform::focus_chrome(window);
        self.bookmarks.folder_prompt = Some(input);
        cx.notify();
    }

    pub fn cancel_folder_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bookmarks.folder_prompt = None;
        if let Err(error) = browser_platform::focus_chrome(window) {
            tracing::warn!(%error, "could not restore focus after closing folder prompt");
        }
        cx.notify();
    }

    /// Creates a folder from the prompt (also used by probes).
    pub fn create_bookmark_folder(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = name.trim();
        if name.is_empty() {
            self.cancel_folder_prompt(window, cx);
            return;
        }
        let name = name.to_owned();
        let created_at = unix_millis_now();
        self.run_bookmark_write(cx, move |store| {
            if let Err(error) = store.create_folder(&name, created_at) {
                tracing::warn!(%error, "could not create folder");
            }
        });
        self.bookmarks.folder_prompt = None;
        if let Err(error) = browser_platform::focus_chrome(window) {
            tracing::warn!(%error, "could not restore focus after creating folder");
        }
        cx.notify();
    }

    pub(super) fn render_bookmarks_page(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                    .child("Bookmarks"),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(format!(
                        "{} bookmarks · {} folders",
                        self.bookmarks.bookmarks.len(),
                        self.bookmarks.folders.len()
                    )),
            )
            .child(div().flex_1())
            .child(text_button(
                theme,
                "new-folder",
                "New Folder",
                cx.listener(|this, _event, window, cx| this.prompt_new_bookmark_folder(window, cx)),
            ))
            .child(icon_button(
                theme,
                "close-bookmarks",
                "✕",
                true,
                cx.listener(|this, _event, window, cx| this.close_overlay(window, cx)),
            ));

        let unfiled = self.bookmarks.in_folder(None).cloned().collect::<Vec<_>>();
        let folders: Vec<(BookmarkFolder, Vec<Bookmark>)> = self
            .bookmarks
            .folders
            .iter()
            .cloned()
            .map(|folder| {
                let entries = self
                    .bookmarks
                    .in_folder(Some(folder.id))
                    .cloned()
                    .collect::<Vec<_>>();
                (folder, entries)
            })
            .collect();

        let mut body = div()
            .id("bookmarks-list")
            .flex_1()
            .overflow_y_scroll()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_4();

        if self.bookmarks.error.is_some() {
            body = body.child(
                div()
                    .p_4()
                    .text_size(px(12.0))
                    .text_color(theme.danger)
                    .child(format!(
                        "Could not load bookmarks: {}",
                        self.bookmarks.error.clone().unwrap_or_default()
                    )),
            );
        }

        if self.bookmarks.bookmarks.is_empty() && self.bookmarks.folders.is_empty() {
            body = body.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .py_10()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .text_color(theme.text)
                            .child("No bookmarks yet"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child("Press ⌘D to bookmark the current page."),
                    ),
            );
        }

        body = body.child(self.render_bookmark_section(theme, None, "Unfiled", &unfiled, cx));
        for (folder, entries) in folders {
            body = body.child(self.render_bookmark_section(
                theme,
                Some(folder.id),
                &folder.name,
                &entries,
                cx,
            ));
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

    fn render_bookmark_section(
        &self,
        theme: &Theme,
        folder_id: Option<i64>,
        title: &str,
        entries: &[Bookmark],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .py_1()
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(title.to_owned()),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_muted)
                    .child(format!("({})", entries.len())),
            )
            .child(div().flex_1())
            .child(text_button(
                theme,
                gpui::ElementId::named_usize(
                    "save-here",
                    folder_id.unwrap_or(-1).unsigned_abs() as usize,
                ),
                "Save current page",
                cx.listener(move |this, _event, window, cx| {
                    this.bookmark_active_in_folder(folder_id, window, cx)
                }),
            ))
            .children(folder_id.map(|id| {
                text_button(
                    theme,
                    gpui::ElementId::named_usize("delete-folder", id as usize),
                    "Delete folder",
                    cx.listener(move |this, _event, _window, cx| {
                        this.delete_bookmark_folder(id, cx)
                    }),
                )
                .into_any_element()
            }));

        let rows = entries.iter().map(|bookmark| {
            let id = bookmark.id;
            let url = bookmark.url.clone();
            let row_hover = theme.surface_hover;
            div()
                .id(gpui::ElementId::named_usize("bookmark-row", id as usize))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_md()
                .hover(move |row| row.bg(row_hover))
                .child(
                    div()
                        .id(gpui::ElementId::named_usize("bookmark-open", id as usize))
                        .flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_col()
                        .gap_1()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _event, window, cx| {
                            this.overlay = Overlay::None;
                            this.navigate(&url, window, cx);
                            this.update_engine_visibility();
                        }))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(13.0))
                                .text_color(theme.text)
                                .child(bookmark.display_title()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(theme.text_muted)
                                .child(bookmark.url.clone()),
                        ),
                )
                .child(
                    div()
                        .id(gpui::ElementId::named_usize("bookmark-delete", id as usize))
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
                            this.delete_bookmark(id, cx)
                        }))
                        .child("✕"),
                )
                .into_any_element()
        });

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(header)
            .children(rows)
            .into_any_element()
    }

    pub(super) fn render_folder_prompt(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let input = self.bookmarks.folder_prompt.as_ref()?.clone();
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::rgba(0x00000088))
                .child(
                    div()
                        .w(px(360.0))
                        .p_4()
                        .rounded_lg()
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.border)
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .text_color(theme.text)
                                .child("New bookmark folder"),
                        )
                        .child(
                            div()
                                .h(px(32.0))
                                .rounded_md()
                                .bg(theme.address_background)
                                .border_1()
                                .border_color(theme.border)
                                .child(input),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(text_button(
                                    theme,
                                    "cancel-folder",
                                    "Cancel",
                                    cx.listener(|this, _event, window, cx| {
                                        this.cancel_folder_prompt(window, cx)
                                    }),
                                ))
                                .child(text_button(
                                    theme,
                                    "create-folder",
                                    "Create",
                                    cx.listener(|this, _event, window, cx| {
                                        this.submit_folder_prompt(window, cx)
                                    }),
                                )),
                        ),
                )
                .into_any_element(),
        )
    }

    fn submit_folder_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.bookmarks.folder_prompt.clone() else {
            return;
        };
        let name = input.read(cx).text().to_owned();
        self.create_bookmark_folder(&name, window, cx);
    }
}
