//! M3 history probe: records real visits, opens the GPUI history page, then
//! deletes, clears and reopens entries to prove the SQLite round-trip and the
//! overlay/page-view handoff.

use std::collections::BTreeMap;
use std::time::Duration;

use browser_app::browser_window::{BrowserSnapshot, BrowserWindow};
use browser_testserver::TestServer;
use gpui::{App, Application, AsyncApp, Timer, WindowHandle};

const DATA_DIR: &str = "target/m3-data";
const REPORT_PATH: &str = "target/m3-report.json";

fn main() {
    browser_app::init_logging();
    let data_dir = std::path::PathBuf::from(DATA_DIR);
    let _ = std::fs::remove_dir_all(&data_dir);
    // Safety: configured before the app starts.
    unsafe { std::env::set_var(browser_storage::DATA_DIR_ENV, &data_dir) };
    let server = TestServer::start().expect("start fixture server");
    let fixture_url = server.url("/");
    let page_two_url = server.url("/page2");
    let fixture_origin = server.origin();

    Application::new().run(move |cx: &mut App| {
        let handle = match browser_app::launch(cx) {
            Ok(handle) => handle,
            Err(error) => finish(failure("launch", error.to_string())),
        };
        cx.spawn(async move |cx| {
            let result = run(&handle, cx, &fixture_url, &page_two_url, &fixture_origin).await;
            match result {
                Ok(checks) => finish(Report { checks }),
                Err(error) => finish(failure("sequence", error)),
            }
        })
        .detach();
    });
    drop(server);
}

async fn run(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    fixture_url: &str,
    page_two_url: &str,
    fixture_origin: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut checks = BTreeMap::new();

    update(handle, cx, |browser, window, cx| {
        browser.navigate(fixture_url, window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "Local Fixture"
    })
    .await
    .ok_or("fixture page did not load")?;

    update(handle, cx, |browser, window, cx| {
        browser.navigate(page_two_url, window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "Second Page"
    })
    .await
    .ok_or("page2 did not load")?;

    // History writes happen on a background thread; poll with refreshes.
    update(handle, cx, |browser, window, cx| {
        browser.open_history(window, cx)
    })?;
    let history = wait_history(handle, cx, Duration::from_secs(20), |snapshot| {
        snapshot.overlay == "history"
            && !snapshot.history_loading
            && snapshot
                .history_urls
                .iter()
                .any(|url| url.starts_with(fixture_origin) && url.ends_with("/page2"))
            && snapshot
                .history_urls
                .iter()
                .any(|url| url.starts_with(fixture_origin) && url == fixture_url)
    })
    .await
    .ok_or("history did not contain both fixture visits")?;
    checks.insert(
        "recorded".into(),
        format!("{} entries including both visits", history.history_entries),
    );

    // Opening a row closes the overlay and navigates.
    update(handle, cx, |browser, window, cx| {
        browser.open_history_entry(fixture_url, window, cx)
    })?;
    let opened = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        snapshot.overlay == "none" && !snapshot.loading && snapshot.active_title == "Local Fixture"
    })
    .await
    .ok_or("opening a history entry did not navigate")?;
    checks.insert("open_entry".into(), opened.active_url);

    // Delete one entry: it must disappear after reloading from SQLite.
    update(handle, cx, |browser, window, cx| {
        browser.open_history(window, cx)
    })?;
    let listed = wait_history(handle, cx, Duration::from_secs(15), |snapshot| {
        !snapshot.history_loading && !snapshot.history_ids.is_empty()
    })
    .await
    .ok_or("history list did not load")?;
    let doomed_id = listed.history_ids[0];
    update(handle, cx, |browser, _window, cx| {
        browser.delete_history_entry(doomed_id, cx)
    })?;
    update(handle, cx, |browser, window, cx| {
        browser.close_overlay(window, cx);
        browser.open_history(window, cx);
    })?;
    let after_delete = wait_history(handle, cx, Duration::from_secs(15), |snapshot| {
        !snapshot.history_loading && !snapshot.history_ids.contains(&doomed_id)
    })
    .await
    .ok_or("deleted history entry is still present after reload")?;
    checks.insert(
        "delete".into(),
        format!(
            "{} entries remain after reload",
            after_delete.history_entries
        ),
    );

    // Clear all and prove the database is empty on reload.
    update(handle, cx, |browser, _window, cx| browser.clear_history(cx))?;
    update(handle, cx, |browser, window, cx| {
        browser.close_overlay(window, cx);
        browser.open_history(window, cx);
    })?;
    let cleared = wait_history(handle, cx, Duration::from_secs(15), |snapshot| {
        !snapshot.history_loading && snapshot.history_ids.is_empty()
    })
    .await
    .ok_or("history was not empty after clear + reload")?;
    checks.insert(
        "clear".into(),
        format!(
            "history empty after reload ({} entries)",
            cleared.history_entries
        ),
    );

    // Closing the overlay restores the page view.
    update(handle, cx, |browser, window, cx| {
        browser.close_overlay(window, cx)
    })?;
    let closed = wait_for(handle, cx, Duration::from_secs(5), |snapshot| {
        snapshot.overlay == "none"
    })
    .await
    .ok_or("overlay did not close")?;
    checks.insert("overlay_close".into(), closed.active_title);
    Ok(checks)
}

fn update<R>(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    f: impl FnOnce(&mut BrowserWindow, &mut gpui::Window, &mut gpui::Context<BrowserWindow>) -> R,
) -> Result<R, String> {
    handle
        .update(cx, |browser, window, cx| f(browser, window, cx))
        .map_err(|error| error.to_string())
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

/// Polls the history overlay, refreshing it from SQLite periodically so
/// background recording is observed.
async fn wait_history(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    timeout: Duration,
    mut predicate: impl FnMut(&BrowserSnapshot) -> bool,
) -> Option<BrowserSnapshot> {
    let deadline = std::time::Instant::now() + timeout;
    let mut next_refresh = std::time::Instant::now();
    loop {
        if let Ok(snapshot) = handle.update(cx, |browser, _window, cx| browser.snapshot(cx))
            && predicate(&snapshot)
        {
            return Some(snapshot);
        }
        if std::time::Instant::now() >= next_refresh {
            let _ = handle.update(cx, |browser, _window, cx| browser.refresh_history(cx));
            next_refresh = std::time::Instant::now() + Duration::from_millis(600);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        Timer::after(Duration::from_millis(80)).await;
    }
}

struct Report {
    checks: BTreeMap<String, String>,
}

fn failure(name: &str, detail: impl Into<String>) -> Report {
    let mut checks = BTreeMap::new();
    checks.insert(name.to_owned(), detail.into());
    Report { checks }
}

fn finish(report: Report) -> ! {
    let failed = report
        .checks
        .keys()
        .any(|key| key == "sequence" || key == "launch");
    for (name, detail) in &report.checks {
        println!("[m3] {name}: {detail}");
    }
    println!(
        "[m3] result: {}",
        if failed || report.checks.is_empty() {
            "FAIL"
        } else {
            "PASS"
        }
    );
    let value = serde_json::json!({
        "passed": !failed && !report.checks.is_empty(),
        "checks": report.checks,
    });
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(REPORT_PATH, text);
    }
    std::process::exit(if failed || report.checks.is_empty() {
        1
    } else {
        0
    });
}
