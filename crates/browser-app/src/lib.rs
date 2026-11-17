//! GPUI Browser application composition.
//!
//! The binary entry point is intentionally thin: it initialises logging and
//! calls [`launch`]. Keeping composition in a library lets end-to-end probe
//! binaries drive the real browser window without duplicating setup.

pub mod actions;
pub mod browser_window;
pub mod favicon;

use browser_core::ThemeMode;
use browser_storage::{SettingsStore, Storage};
use browser_ui::{TextInput, Theme, bind_text_input_keys};
use gpui::{
    App, AppContext, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowHandle,
    WindowOptions, point, px, size,
};

use crate::browser_window::BrowserWindow;

/// Initialises structured logging from `RUST_LOG` (defaults to browser info).
pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "browser_app=info,browser_ui=info,browser_engine=info,browser_platform=info,warn"
                    .into()
            }),
        )
        .with_target(false)
        .init();
}

/// Opens the main browser window and returns a handle for programmatic control.
pub fn launch(cx: &mut App) -> gpui::Result<WindowHandle<BrowserWindow>> {
    Theme::install_default(cx);
    bind_text_input_keys(cx);
    actions::bind_browser_keys(cx);

    let storage = match Storage::open_default() {
        Ok(storage) => Some(storage),
        Err(error) => {
            tracing::warn!(%error, "persistence is disabled");
            None
        }
    };
    let settings_store = storage
        .as_ref()
        .map(|storage| SettingsStore::in_directory(storage.root()));
    let settings = settings_store
        .as_ref()
        .and_then(|store| match store.load() {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(%error, "ignoring unreadable settings file");
                None
            }
        })
        .unwrap_or_default();

    let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from("GPUI Browser")),
                appears_transparent: true,
                traffic_light_position: Some(point(px(14.0), px(10.0))),
            }),
            ..Default::default()
        },
        move |window, cx| {
            let theme = match settings.theme {
                ThemeMode::System => Theme::for_appearance(window.appearance()),
                ThemeMode::Light => Theme::light(),
                ThemeMode::Dark => Theme::dark(),
            };
            Theme::install(cx, theme);
            let address = cx.new(|cx| TextInput::new(cx, "Search or enter address", px(13.0)));
            let browser =
                cx.new(|cx| BrowserWindow::new(cx, address, settings, storage, settings_store));
            browser.update(cx, |this, cx| this.attach(window, cx));
            browser
        },
    )?;

    cx.activate(true);
    Ok(handle)
}
