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
    let downloads_dir = std::path::PathBuf::from("target/m3-downloads");
    let _ = std::fs::remove_dir_all(&data_dir);
    let _ = std::fs::remove_dir_all(&downloads_dir);
    // Safety: configured before the app starts.
    unsafe {
        std::env::set_var(browser_storage::DATA_DIR_ENV, &data_dir);
        std::env::set_var("GPUI_BROWSER_DOWNLOADS_DIR", &downloads_dir);
    }
    let server = TestServer::start().expect("start fixture server");
    let fixture_url = server.url("/");
    let page_two_url = server.url("/page2");
    let download_url = server.url("/download");
    let storage_url = server.url("/storage");
    let storage_set_url = format!("{storage_url}?set=1");
    let fixture_origin = server.origin();

    Application::new().run(move |cx: &mut App| {
        let handle = match browser_app::launch(cx) {
            Ok(handle) => handle,
            Err(error) => finish(failure("launch", error.to_string())),
        };
        cx.spawn(async move |cx| {
            let result = run(
                &handle,
                cx,
                &fixture_url,
                &page_two_url,
                &download_url,
                &storage_url,
                &storage_set_url,
                &fixture_origin,
            )
            .await;
            match result {
                Ok(checks) => finish(Report { checks }),
                Err(error) => finish(failure("sequence", error)),
            }
        })
        .detach();
    });
    drop(server);
}

#[allow(clippy::too_many_arguments)]
async fn run(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    fixture_url: &str,
    page_two_url: &str,
    download_url: &str,
    storage_url: &str,
    storage_set_url: &str,
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

    // ----- Bookmarks -------------------------------------------------
    update(handle, cx, |browser, window, cx| {
        browser.navigate(page_two_url, window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "Second Page"
    })
    .await
    .ok_or("page2 did not reload for bookmarks")?;

    update(handle, cx, |browser, window, cx| {
        browser.toggle_bookmark(window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.bookmarked_active && snapshot.bookmarks_count == 1
    })
    .await
    .ok_or("bookmark was not added")?;
    checks.insert("bookmark_add".into(), "current page bookmarked".into());

    update(handle, cx, |browser, window, cx| {
        browser.open_bookmarks(window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.overlay == "bookmarks" && snapshot.bookmarks_count == 1
    })
    .await
    .ok_or("bookmarks page did not open")?;

    update(handle, cx, |browser, window, cx| {
        browser.create_bookmark_folder("Reading", window, cx)
    })?;
    let with_folder = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot
            .bookmark_folders
            .iter()
            .any(|name| name == "Reading")
    })
    .await
    .ok_or("bookmark folder was not created")?;
    let folder_id = with_folder
        .bookmark_folder_ids
        .get(
            with_folder
                .bookmark_folders
                .iter()
                .position(|name| name == "Reading")
                .unwrap_or(0),
        )
        .copied()
        .ok_or("folder id missing")?;

    update(handle, cx, |browser, window, cx| {
        browser.bookmark_active_in_folder(Some(folder_id), window, cx)
    })?;
    update(handle, cx, |browser, window, cx| {
        browser.close_overlay(window, cx);
        browser.open_bookmarks(window, cx);
    })?;
    let filed = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.bookmark_folder_of.first() == Some(&Some(folder_id))
    })
    .await
    .ok_or("bookmark was not moved into the folder")?;
    checks.insert(
        "bookmark_folder".into(),
        format!("{} folder(s) after reload", filed.bookmark_folders.len()),
    );

    update(handle, cx, |browser, _window, cx| {
        browser.delete_bookmark_folder(folder_id, cx)
    })?;
    update(handle, cx, |browser, window, cx| {
        browser.close_overlay(window, cx);
        browser.open_bookmarks(window, cx);
    })?;
    let unfiled = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.bookmark_folders.is_empty()
            && snapshot.bookmarks_count == 1
            && snapshot.bookmark_folder_of.first() == Some(&None)
    })
    .await
    .ok_or("deleting the folder did not move its bookmark to unfiled")?;
    checks.insert(
        "folder_delete".into(),
        format!("{} unfiled bookmark(s)", unfiled.bookmarks_count),
    );

    let bookmark_id = unfiled
        .bookmark_ids
        .first()
        .copied()
        .ok_or("bookmark id missing")?;
    update(handle, cx, |browser, _window, cx| {
        browser.delete_bookmark(bookmark_id, cx)
    })?;
    let removed = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.bookmarks_count == 0 && !snapshot.bookmarked_active
    })
    .await
    .ok_or("bookmark was not deleted")?;
    checks.insert(
        "bookmark_delete".into(),
        format!("{} bookmarks remain", removed.bookmarks_count),
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

    // ----- Find in page ----------------------------------------------
    update(handle, cx, |browser, window, cx| {
        browser.navigate(fixture_url, window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "Local Fixture"
    })
    .await
    .ok_or("fixture did not reload for find-in-page")?;

    update(handle, cx, |browser, window, cx| {
        browser.find_in_page_for_probe("fixture-heading", window, cx)
    })?;
    let found = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.find_open && snapshot.find_found == Some(true)
    })
    .await
    .ok_or("find-in-page did not find existing text")?;
    checks.insert("find_found".into(), format!("open={}", found.find_open));

    update(handle, cx, |browser, window, cx| {
        browser.find_in_page_for_probe("zzz-no-such-text-zzz", window, cx)
    })?;
    let missing = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.find_open && snapshot.find_found == Some(false)
    })
    .await
    .ok_or("find-in-page reported a match for missing text")?;
    checks.insert(
        "find_missing".into(),
        format!("result={:?}", missing.find_found),
    );

    update(handle, cx, |browser, window, cx| {
        browser.close_find(window, cx)
    })?;
    let find_closed = wait_for(handle, cx, Duration::from_secs(5), |snapshot| {
        !snapshot.find_open && snapshot.active_title == "Local Fixture"
    })
    .await
    .ok_or("find bar did not close cleanly")?;
    checks.insert("find_close".into(), find_closed.active_title);

    // ----- Downloads --------------------------------------------------
    update(handle, cx, |browser, window, cx| {
        browser.navigate(download_url, window, cx)
    })?;
    let downloaded = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        snapshot.downloads_completed >= 1
    })
    .await
    .ok_or("download did not complete")?;
    let path = downloaded
        .download_paths
        .first()
        .ok_or("download path missing")?
        .clone();
    let contents = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    if contents != "download-payload" {
        return Err(format!(
            "downloaded file has unexpected contents: {contents:?}"
        ));
    }
    checks.insert(
        "download".into(),
        format!("{} ({} bytes)", path, contents.len()),
    );

    update(handle, cx, |browser, window, cx| {
        browser.open_downloads(window, cx)
    })?;
    let listed = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.overlay == "downloads" && snapshot.downloads_count >= 1
    })
    .await
    .ok_or("downloads page did not list the download")?;
    checks.insert(
        "downloads_page".into(),
        format!("{} listed item(s)", listed.downloads_count),
    );
    update(handle, cx, |browser, window, cx| {
        browser.close_overlay(window, cx)
    })?;

    // ----- Clear browsing data (cookies + localStorage) --------------
    update(handle, cx, |browser, window, cx| {
        browser.navigate(storage_set_url, window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "storage-set-set"
    })
    .await
    .ok_or("storage fixture did not persist a cookie and localStorage value")?;

    // Cookie-only clearing leaves localStorage intact.
    update(handle, cx, |browser, _window, cx| browser.clear_cookies(cx))?;
    Timer::after(Duration::from_millis(800)).await;
    let mut cookie_cleared = None;
    for _ in 0..10 {
        update(handle, cx, |browser, window, cx| {
            browser.navigate(storage_url, window, cx)
        })?;
        if let Some(snapshot) = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
            !snapshot.loading && snapshot.active_title.starts_with("storage-")
        })
        .await
            && snapshot.active_title == "storage-clear-set"
        {
            cookie_cleared = Some(snapshot.active_title);
            break;
        }
        Timer::after(Duration::from_millis(400)).await;
    }
    let cookie_cleared = cookie_cleared.ok_or_else(|| {
        format!(
            "cookie-only clear did not take effect; title {:?}",
            read_snapshot(handle, cx).map(|snapshot| snapshot.active_title)
        )
    })?;
    checks.insert("clear_cookies".into(), cookie_cleared);

    // Restore the cookie, then clear all site data (cookies + localStorage).
    update(handle, cx, |browser, window, cx| {
        browser.navigate(storage_set_url, window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "storage-set-set"
    })
    .await
    .ok_or("storage fixture did not restore its values")?;
    update(handle, cx, |browser, _window, cx| {
        browser.clear_browsing_data(cx)
    })?;
    Timer::after(Duration::from_millis(1200)).await;
    let mut cleared_title = None;
    for _ in 0..10 {
        update(handle, cx, |browser, window, cx| {
            browser.navigate(storage_url, window, cx)
        })?;
        if let Some(snapshot) = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
            !snapshot.loading && snapshot.active_title.starts_with("storage-")
        })
        .await
            && snapshot.active_title == "storage-clear-clear"
        {
            cleared_title = Some(snapshot.active_title);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }
    let cleared_title = cleared_title.ok_or_else(|| {
        format!(
            "storage was not cleared; last title {:?}",
            read_snapshot(handle, cx).map(|snapshot| snapshot.active_title)
        )
    })?;
    checks.insert("clear_site_data".into(), cleared_title);
    Ok(checks)
}

fn read_snapshot(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
) -> Option<BrowserSnapshot> {
    handle
        .update(cx, |browser, _window, cx| browser.snapshot(cx))
        .ok()
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
