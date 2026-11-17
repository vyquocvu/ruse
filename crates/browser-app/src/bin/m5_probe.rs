//! M5 private-browsing probe.
//!
//! Opens two private windows plus one normal window against the same fixture
//! server and proves:
//! - private tabs do not see cookies from the persistent window,
//! - private windows do not share cookies with each other,
//! - tabs inside one private window do share the window's ephemeral store,
//! - private navigations are absent from history and from the saved session.

use std::collections::BTreeMap;
use std::time::Duration;

use browser_app::browser_window::{BrowserSnapshot, BrowserWindow};
use browser_storage::HistoryStore;
use browser_testserver::TestServer;
use gpui::{App, Application, AsyncApp, Timer, WindowHandle};

const DATA_DIR: &str = "target/m5-data";
const REPORT_PATH: &str = "target/m5-report.json";

fn main() {
    browser_app::init_logging();
    let data_dir = std::path::PathBuf::from(DATA_DIR);
    let downloads = std::path::PathBuf::from("target/m5-downloads");
    let _ = std::fs::remove_dir_all(&data_dir);
    let _ = std::fs::remove_dir_all(&downloads);
    // Safety: configured before the app starts.
    unsafe {
        std::env::set_var(browser_storage::DATA_DIR_ENV, &data_dir);
        std::env::set_var("GPUI_BROWSER_DOWNLOADS_DIR", &downloads);
    }
    let server = TestServer::start().expect("start fixture server");
    let _origin = server.origin();
    let storage_url = server.url("/storage");
    let storage_set_url = format!("{storage_url}?set=1");
    let marker_url = server.url("/private-marker");

    Application::new().run(move |cx: &mut App| {
        let normal = match browser_app::launch(cx) {
            Ok(handle) => handle,
            Err(error) => finish(failure("launch", error.to_string())),
        };
        let private_a = match browser_app::open_browser_window(cx, true) {
            Ok(handle) => handle,
            Err(error) => finish(failure("private_window", error.to_string())),
        };
        let private_b = match browser_app::open_browser_window(cx, true) {
            Ok(handle) => handle,
            Err(error) => finish(failure("private_window_b", error.to_string())),
        };
        cx.spawn(async move |cx| {
            let result = run(
                &normal,
                &private_a,
                &private_b,
                cx,
                &storage_url,
                &storage_set_url,
                &marker_url,
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
    normal: &WindowHandle<BrowserWindow>,
    private_a: &WindowHandle<BrowserWindow>,
    private_b: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    storage_url: &str,
    storage_set_url: &str,
    marker_url: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut checks: BTreeMap<String, String> = BTreeMap::new();

    for (name, handle) in [
        ("normal", normal),
        ("private_a", private_a),
        ("private_b", private_b),
    ] {
        wait_for(handle, cx, Duration::from_secs(25), |snapshot| {
            !snapshot.loading && !snapshot.active_url.is_empty()
        })
        .await
        .ok_or_else(|| format!("{name} startup page did not load"))?;
    }

    let private_snapshot = read_snapshot(private_a, cx).ok_or("private snapshot missing")?;
    let normal_snapshot = read_snapshot(normal, cx).ok_or("normal snapshot missing")?;
    if !private_snapshot.private || normal_snapshot.private {
        return Err("windows have the wrong private flag".into());
    }
    checks.insert(
        "private_flag".into(),
        "private windows flagged, normal window not".into(),
    );

    // Persistent cookie in the normal window.
    update(normal, cx, |browser, window, cx| {
        browser.navigate(storage_set_url, window, cx)
    })?;
    wait_for(normal, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "storage-set-set"
    })
    .await
    .ok_or("normal window did not persist its cookie")?;

    // Private window must not see the persistent cookie.
    update(private_a, cx, |browser, window, cx| {
        browser.navigate(storage_url, window, cx)
    })?;
    let isolated = wait_for(private_a, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "storage-clear-clear"
    })
    .await
    .ok_or("private window saw the normal window's cookie")?;
    checks.insert("store_isolation".into(), isolated.active_title.clone());

    // A marker visit that must never reach history or the session file.
    update(private_a, cx, |browser, window, cx| {
        browser.navigate(marker_url, window, cx)
    })?;
    wait_for(private_a, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "Private Marker"
    })
    .await
    .ok_or("private marker page did not load")?;

    // Tabs within the same private window share cookies.
    update(private_a, cx, |browser, window, cx| {
        browser.navigate(storage_set_url, window, cx)
    })?;
    wait_for(private_a, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "storage-set-set"
    })
    .await
    .ok_or("private window did not accept its own cookie")?;
    update(private_a, cx, |browser, window, cx| {
        browser.new_tab(window, cx)
    })?;
    update(private_a, cx, |browser, window, cx| {
        browser.navigate(storage_url, window, cx)
    })?;
    let shared = wait_for(private_a, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "storage-set-set"
    })
    .await
    .ok_or("private tabs did not share their window's ephemeral store")?;
    checks.insert("private_tab_sharing".into(), shared.active_title.clone());
    update(private_a, cx, |browser, window, cx| {
        browser.close_active_tab(window, cx)
    })?;

    // A second private window is its own ephemeral session.
    update(private_b, cx, |browser, window, cx| {
        browser.navigate(storage_url, window, cx)
    })?;
    let second = wait_for(private_b, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "storage-clear-clear"
    })
    .await
    .ok_or("second private window shared the first private window's cookies")?;
    checks.insert(
        "private_window_isolation".into(),
        second.active_title.clone(),
    );

    // Wait for the normal session writer, then check persistence boundaries.
    Timer::after(Duration::from_millis(1200)).await;
    let session_path = std::path::Path::new(DATA_DIR).join("session.json");
    let session_text = std::fs::read_to_string(&session_path).unwrap_or_default();
    if session_text.contains("private-marker") {
        return Err("private visit leaked into the saved session".into());
    }
    checks.insert(
        "session_excludes_private".into(),
        format!("session size {}", session_text.len()),
    );

    let history = HistoryStore::in_directory(DATA_DIR);
    let entries = history.recent(200).map_err(|error| error.to_string())?;
    if entries
        .iter()
        .any(|entry| entry.url.contains("private-marker"))
    {
        return Err("private visit leaked into history".into());
    }
    if !entries.iter().any(|entry| entry.url.contains("/storage")) {
        return Err("normal visit was not recorded in history".into());
    }
    checks.insert(
        "history_excludes_private".into(),
        format!(
            "{} normal history entries, no private marker",
            entries.len()
        ),
    );

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
        .any(|key| key == "sequence" || key == "launch" || key == "private_window");
    for (name, detail) in &report.checks {
        println!("[m5] {name}: {detail}");
    }
    let passed = !failed && !report.checks.is_empty();
    println!("[m5] result: {}", if passed { "PASS" } else { "FAIL" });
    let value = serde_json::json!({
        "passed": passed,
        "checks": report.checks,
    });
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(REPORT_PATH, text);
    }
    std::process::exit(if passed { 0 } else { 1 });
}
