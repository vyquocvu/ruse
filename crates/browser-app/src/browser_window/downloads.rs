//! Downloads state and manager UI.
//!
//! WebKit performs the transfer through `WKDownload`; the engine reports the
//! chosen path when it starts and completion/failure when it ends. Progress is
//! sampled from the destination file because WebKit exposes no progress API.

use std::path::{Path, PathBuf};

use browser_ui::{Theme, icon_button, text_button};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

use super::{BrowserWindow, Overlay};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DownloadStatus {
    Downloading,
    Completed,
    Failed,
}

impl DownloadStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Downloading => "downloading",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct DownloadEntry {
    pub(super) filename: String,
    pub(super) path: PathBuf,
    pub(super) expected_bytes: i64,
    pub(super) received_bytes: u64,
    pub(super) status: DownloadStatus,
    pub(super) message: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct DownloadsState {
    pub(super) entries: Vec<DownloadEntry>,
}

impl BrowserWindow {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_download_started(
        &mut self,
        _url: String,
        filename: String,
        path: PathBuf,
        expected_bytes: i64,
        cx: &mut Context<Self>,
    ) {
        if self
            .downloads
            .entries
            .iter()
            .any(|entry| entry.path == path)
        {
            return;
        }
        self.status = Some(format!("Downloading {filename}").into());
        self.downloads.entries.push(DownloadEntry {
            filename,
            path,
            expected_bytes,
            received_bytes: 0,
            status: DownloadStatus::Downloading,
            message: None,
        });
        cx.notify();
    }

    pub(super) fn handle_download_finished(
        &mut self,
        path: PathBuf,
        success: bool,
        message: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self
            .downloads
            .entries
            .iter_mut()
            .find(|entry| entry.path == path)
        {
            entry.received_bytes = std::fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(entry.received_bytes);
            entry.status = if success {
                DownloadStatus::Completed
            } else {
                DownloadStatus::Failed
            };
            entry.message = message.clone();
            self.status = Some(
                match (&entry.status, message) {
                    (DownloadStatus::Completed, _) => format!("Downloaded {}", entry.filename),
                    (_, Some(error)) => format!("Download failed: {error}"),
                    _ => format!("Download failed: {}", entry.filename),
                }
                .into(),
            );
        }
        cx.notify();
    }

    /// Samples destination sizes for in-flight downloads.
    pub(super) fn update_download_progress(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        for entry in self
            .downloads
            .entries
            .iter_mut()
            .filter(|entry| entry.status == DownloadStatus::Downloading)
        {
            if let Ok(metadata) = std::fs::metadata(&entry.path)
                && metadata.len() != entry.received_bytes
            {
                entry.received_bytes = metadata.len();
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
        changed
    }

    pub fn toggle_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay == Overlay::Downloads {
            self.close_overlay(window, cx);
        } else {
            self.open_downloads(window, cx);
        }
    }

    pub fn open_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::Downloads;
        self.update_engine_visibility();
        if let Err(error) = browser_platform::focus_chrome(window) {
            tracing::warn!(%error, "could not focus the downloads page");
        }
        cx.notify();
    }

    pub(super) fn cancel_download(&mut self, path: &Path, cx: &mut Context<Self>) {
        let mut cancelled = false;
        for engine in self.runtimes.values() {
            if engine.cancel_download(path) {
                cancelled = true;
            }
        }
        if !cancelled
            && let Some(entry) = self
                .downloads
                .entries
                .iter_mut()
                .find(|entry| entry.path == path)
        {
            entry.status = DownloadStatus::Failed;
            entry.message = Some("Cancelled".into());
            cx.notify();
        }
    }

    pub(super) fn open_download(&mut self, path: &Path) {
        if let Err(error) = browser_platform::open_path(path) {
            tracing::warn!(%error, path = %path.display(), "could not open download");
            self.status = Some(format!("Could not open {}", path.display()).into());
        }
    }

    pub(super) fn reveal_download(&mut self, path: &Path) {
        if let Err(error) = browser_platform::reveal_path(path) {
            tracing::warn!(%error, path = %path.display(), "could not reveal download");
        }
    }

    pub(super) fn clear_finished_downloads(&mut self, cx: &mut Context<Self>) {
        self.downloads
            .entries
            .retain(|entry| entry.status == DownloadStatus::Downloading);
        cx.notify();
    }

    pub(super) fn render_downloads_page(
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
                    .child("Downloads"),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(format!("{} items", self.downloads.entries.len())),
            )
            .child(div().flex_1())
            .child(text_button(
                theme,
                "clear-downloads",
                "Clear finished",
                cx.listener(|this, _event, _window, cx| this.clear_finished_downloads(cx)),
            ))
            .child(icon_button(
                theme,
                "close-downloads",
                "✕",
                true,
                cx.listener(|this, _event, window, cx| this.close_overlay(window, cx)),
            ));

        let mut body = div()
            .id("downloads-list")
            .flex_1()
            .overflow_y_scroll()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_1();

        if self.downloads.entries.is_empty() {
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
                            .child("No downloads yet"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child("Files you download will appear here."),
                    ),
            );
        }

        for (index, entry) in self.downloads.entries.iter().cloned().enumerate() {
            let path = entry.path.clone();
            let path_for_cancel = path.clone();
            let path_for_open = path.clone();
            let path_for_reveal = path.clone();
            let status_color = match entry.status {
                DownloadStatus::Completed => theme.text_muted,
                DownloadStatus::Downloading => theme.accent,
                DownloadStatus::Failed => theme.danger,
            };
            let size = format_bytes(entry.received_bytes, entry.expected_bytes);
            body = body.child(
                div()
                    .id(gpui::ElementId::named_usize("download-row", index))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(theme.surface)
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .text_color(theme.text)
                                    .child(entry.filename.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child(path.display().to_string()),
                            ),
                    )
                    .child(
                        div()
                            .w(px(90.0))
                            .text_size(px(11.0))
                            .text_color(status_color)
                            .child(entry.status.as_str()),
                    )
                    .child(
                        div()
                            .w(px(100.0))
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child(size),
                    )
                    .children(if entry.status == DownloadStatus::Downloading {
                        Some(
                            text_button(
                                theme,
                                "cancel-download",
                                "Cancel",
                                cx.listener(move |this, _event, _window, cx| {
                                    this.cancel_download(&path_for_cancel, cx)
                                }),
                            )
                            .into_any_element(),
                        )
                    } else {
                        None
                    })
                    .child(text_button(
                        theme,
                        "open-download",
                        "Open",
                        cx.listener(move |this, _event, _window, _cx| {
                            this.open_download(&path_for_open)
                        }),
                    ))
                    .child(text_button(
                        theme,
                        "reveal-download",
                        "Reveal",
                        cx.listener(move |this, _event, _window, _cx| {
                            this.reveal_download(&path_for_reveal)
                        }),
                    )),
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

fn format_bytes(received: u64, expected: i64) -> String {
    if expected > 0 {
        format!(
            "{} / {}",
            human_bytes(received),
            human_bytes(expected as u64)
        )
    } else if received > 0 {
        human_bytes(received)
    } else {
        "—".to_owned()
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
