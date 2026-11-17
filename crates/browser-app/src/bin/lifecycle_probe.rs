//! Lifecycle probe: verifies that Cmd+Q quits and closing the last tab closes
//! the window and quits the app (no zombie process, no panic).
//!
//! Exits 0 only if the app actually terminates within the timeout.

use std::time::Duration;

use browser_app::browser_window::{BrowserSnapshot, BrowserWindow};
use gpui::{App, Application, AsyncApp, Timer, WindowHandle};

const DATA_DIR: &str = "target/lifecycle-data";

fn main() {
    browser_app::init_logging();
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "quit".to_owned())
        .trim_start_matches('-')
        .to_owned();
    let _ = std::fs::remove_dir_all(DATA_DIR);
    // Safety: configured before the app starts.
    unsafe { std::env::set_var(browser_storage::DATA_DIR_ENV, DATA_DIR) };

    Application::new().run(move |cx: &mut App| {
        let handle = match browser_app::launch(cx) {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!("[lifecycle] launch failed: {error}");
                std::process::exit(3);
            }
        };
        let mode = mode.clone();
        cx.spawn(async move |cx| {
            if wait_for(&handle, cx, Duration::from_secs(20), |snapshot| {
                !snapshot.loading && !snapshot.active_url.is_empty()
            })
            .await
            .is_none()
            {
                eprintln!("[lifecycle] startup did not load");
                std::process::exit(3);
            }
            let keys = match handle.update(cx, |_browser, window, _cx| {
                browser_platform::KeySender::new(window)
            }) {
                Ok(Ok(keys)) => keys,
                _ => {
                    eprintln!("[lifecycle] could not create key sender");
                    std::process::exit(3);
                }
            };
            let result = match mode.as_str() {
                "close-tab" => keys.send_key(13, "w", true, false),
                _ => keys.send_key(12, "q", true, false),
            };
            if let Err(error) = result {
                eprintln!("[lifecycle] could not send key: {error}");
                std::process::exit(3);
            }
            // The app should terminate during or shortly after the event.
            Timer::after(Duration::from_secs(4)).await;
            eprintln!("[lifecycle] app still running after {mode}");
            std::process::exit(4);
        })
        .detach();
    });
}

async fn wait_for(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    timeout: Duration,
    mut predicate: impl FnMut(&BrowserSnapshot) -> bool,
) -> Option<BrowserSnapshot> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(snapshot) = handle.update(cx, |browser, _window, cx| browser.snapshot(cx))
            && predicate(&snapshot)
        {
            return Some(snapshot);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        Timer::after(Duration::from_millis(80)).await;
    }
}
