//! M1 end-to-end probe: drives the real browser window.
//!
//! Unlike the M0 probe (which targets the engine directly), this harness uses
//! the same composition the app ships: GPUI chrome, the address bar entity,
//! tab lifecycle and engine event pumps. It asserts navigation, tab creation
//! and closure, back/forward, error handling and address sync against
//! deterministic local fixtures plus one real HTTPS site.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use browser_app::browser_window::{BrowserSnapshot, BrowserWindow};
use browser_testserver::TestServer;
use gpui::{App, Application, AsyncApp, Timer, WindowHandle, px, size};

const REPORT_PATH: &str = "target/m1-report.json";

fn main() {
    browser_app::init_logging();
    let data_dir = PathBuf::from("target/m1-data");
    let _ = std::fs::remove_dir_all(&data_dir);
    // Safety: single-threaded setup before the app launches.
    unsafe { std::env::set_var(browser_storage::DATA_DIR_ENV, &data_dir) };
    let server = TestServer::start().expect("start fixture server");
    let fixture_url = server.url("/");
    let page_two_url = server.url("/page2");
    let favicon_url = server.url("/favicon-test");
    let fixture_origin = server.origin();
    let keyboard_url = server.url("/keyboard");

    Application::new().run(move |cx: &mut App| {
        let handle = match browser_app::launch(cx) {
            Ok(handle) => handle,
            Err(error) => {
                finish(failure_report("launch", error.to_string()));
            }
        };
        cx.spawn(async move |cx| {
            let report = run_probe(
                &handle,
                cx,
                &fixture_url,
                &page_two_url,
                &keyboard_url,
                &favicon_url,
                &fixture_origin,
            )
            .await;
            match report {
                Ok(report) => finish(report),
                Err(error) => finish(failure_report("sequence", error)),
            }
        })
        .detach();
    });
    drop(server);
}

async fn run_probe(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    fixture_url: &str,
    page_two_url: &str,
    keyboard_url: &str,
    favicon_url: &str,
    fixture_origin: &str,
) -> Result<Report, String> {
    let mut report = Report::default();

    // 1. Startup tab loads a real HTTPS site.
    let snapshot = wait_for(handle, cx, Duration::from_secs(25), |snapshot| {
        !snapshot.loading && snapshot.active_url.starts_with("https://example.com")
    })
    .await
    .ok_or("startup page did not finish loading")?;
    if snapshot.tab_count == 1 && !snapshot.active_title.is_empty() {
        report.pass(
            "startup",
            format!("{} ({})", snapshot.active_title, snapshot.active_url),
        );
    } else {
        report.fail("startup", format!("{snapshot:?}"));
    }
    if normalize(&snapshot.address_text) == normalize(&snapshot.active_url) {
        report.pass("address_sync", snapshot.address_text.clone());
    } else {
        report.fail(
            "address_sync",
            format!(
                "address={} url={}",
                snapshot.address_text, snapshot.active_url
            ),
        );
    }

    // 2. Address-bar navigation to a deterministic page.
    update(handle, cx, |browser, window, cx| {
        browser.navigate(fixture_url, window, cx)
    })?;
    let snapshot = wait_for(handle, cx, Duration::from_secs(15), |snapshot| {
        !snapshot.loading && snapshot.active_url.starts_with(fixture_origin)
    })
    .await
    .ok_or("fixture navigation did not finish")?;
    if snapshot.active_title == "Local Fixture" {
        report.pass("navigate", snapshot.active_url.clone());
    } else {
        report.fail("navigate", format!("title={}", snapshot.active_title));
    }

    // 3. Back and forward through real engine history.
    update(handle, cx, |browser, _window, cx| browser.go_back(cx))?;
    let back = wait_for(handle, cx, Duration::from_secs(15), |snapshot| {
        !snapshot.loading && snapshot.active_url.starts_with("https://example.com")
    })
    .await
    .ok_or("back navigation did not return to example.com")?;
    report.pass("back", back.active_url.clone());

    update(handle, cx, |browser, _window, cx| browser.go_forward(cx))?;
    let forward = wait_for(handle, cx, Duration::from_secs(15), |snapshot| {
        !snapshot.loading && snapshot.active_url.starts_with(fixture_origin)
    })
    .await
    .ok_or("forward navigation did not return to the fixture")?;
    report.pass("forward", forward.active_url.clone());

    // 4. New tab starts on the GPUI new-tab page, then navigates.
    update(handle, cx, |browser, window, cx| {
        browser.new_tab(window, cx)
    })?;
    let new_tab = wait_for(handle, cx, Duration::from_secs(5), |snapshot| {
        snapshot.tab_count == 2 && snapshot.active_url.is_empty()
    })
    .await
    .ok_or("new tab did not open an empty tab")?;
    report.pass(
        "new_tab",
        format!("{} tabs, active is the new-tab page", new_tab.tab_count),
    );

    update(handle, cx, |browser, window, cx| {
        browser.navigate(page_two_url, window, cx)
    })?;
    let two = wait_for(handle, cx, Duration::from_secs(15), |snapshot| {
        snapshot.tab_count == 2
            && !snapshot.loading
            && snapshot.active_url.ends_with("/page2")
            && snapshot.active_title == "Second Page"
    })
    .await
    .ok_or("second tab did not load page2 with its title")?;
    if two.active_title == "Second Page" {
        report.pass("tab_navigation", format!("{} tabs", two.tab_count));
    } else {
        report.fail("tab_navigation", format!("title={}", two.active_title));
    }
    report.detail("two_tabs", serde_json::json!(two.tabs));

    // 5. Closing the active tab returns to the previous page.
    update(handle, cx, |browser, window, cx| {
        browser.close_active_tab(window, cx)
    })?;
    let after_close = wait_for(handle, cx, Duration::from_secs(5), |snapshot| {
        snapshot.tab_count == 1 && !snapshot.active_url.ends_with("/page2")
    })
    .await
    .ok_or("closing the active tab did not restore the first tab")?;
    if after_close.active_url.starts_with(fixture_origin) {
        report.pass("close_tab", after_close.active_url.clone());
    } else {
        report.fail("close_tab", after_close.active_url.clone());
    }

    // 6. Failed navigation surfaces an error instead of a blank page.
    update(handle, cx, |browser, window, cx| {
        browser.navigate("http://127.0.0.1:1/unreachable", window, cx)
    })?;
    let failed = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        snapshot.error.is_some()
    })
    .await
    .ok_or("connection failure did not surface an error")?;
    report.pass("error_surface", failed.error.clone().unwrap_or_default());

    // 7. Recovering from the error restores a real page.
    update(handle, cx, |browser, window, cx| {
        browser.navigate(fixture_url, window, cx)
    })?;
    let recovered = wait_for(handle, cx, Duration::from_secs(15), |snapshot| {
        !snapshot.loading
            && snapshot.error.is_none()
            && snapshot.active_url.starts_with(fixture_origin)
    })
    .await
    .ok_or("navigating away from the error did not recover")?;
    report.pass("error_recovery", recovered.active_url.clone());

    // 8. Real keyboard input into the address bar, then Enter.
    let keys = handle
        .update(cx, |_browser, window, _cx| {
            browser_platform::KeySender::new(window)
        })
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    update(handle, cx, |browser, window, cx| {
        browser.focus_address(window, cx)
    })?;
    Timer::after(Duration::from_millis(200)).await;
    keys.type_text("example.com")
        .map_err(|error| error.to_string())?;
    Timer::after(Duration::from_millis(200)).await;
    let typed = read_snapshot(handle, cx)?;
    if typed.address_text == "example.com" {
        report.pass("address_typing", typed.address_text.clone());
    } else {
        report.fail("address_typing", format!("address={}", typed.address_text));
    }
    keys.press_enter().map_err(|error| error.to_string())?;
    let typed_nav = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_url.starts_with("https://example.com")
    })
    .await
    .ok_or("Enter in the address bar did not navigate")?;
    report.pass("enter_navigates", typed_nav.active_url.clone());

    // 9. Real keyboard input reaches page content (the fixture retitles the
    // document on every keydown, which the browser polls).
    update(handle, cx, |browser, window, cx| {
        browser.navigate(keyboard_url, window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(15), |snapshot| {
        !snapshot.loading && snapshot.active_title == "Keyboard Fixture"
    })
    .await
    .ok_or("keyboard fixture did not load")?;
    update(handle, cx, |browser, _window, _cx| browser.focus_page())?;
    Timer::after(Duration::from_millis(200)).await;
    keys.type_text("ab").map_err(|error| error.to_string())?;
    let page_keys = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.active_title == "key-2"
    })
    .await
    .ok_or("both keys did not reach page content")?;
    report.pass("page_keyboard", page_keys.active_title.clone());

    // 10. Favicon discovery + fetch from the page's declared icon.
    update(handle, cx, |browser, window, cx| {
        browser.navigate(favicon_url, window, cx)
    })?;
    let favicon = wait_for(handle, cx, Duration::from_secs(15), |snapshot| {
        !snapshot.loading
            && snapshot.active_title == "Favicon Fixture"
            && snapshot.tabs.first().is_some_and(|tab| tab.has_favicon)
    })
    .await
    .ok_or("favicon was not discovered or decoded")?;
    report.pass(
        "favicon",
        format!(
            "decoded favicon for {}",
            favicon
                .tabs
                .first()
                .map(|tab| tab.url.as_str())
                .unwrap_or("")
        ),
    );

    // 11. Resize the real window; browser state must remain consistent.
    let before_resize = read_snapshot(handle, cx)?;
    handle
        .update(cx, |_browser, window, _cx| {
            window.resize(size(px(900.0), px(680.0)));
        })
        .map_err(|error| error.to_string())?;
    Timer::after(Duration::from_millis(600)).await;
    let resized = read_snapshot(handle, cx)?;
    if resized.active_url == before_resize.active_url && resized.error.is_none() {
        report.pass(
            "resize",
            format!("window resized; page still {}", resized.active_url),
        );
    } else {
        report.fail("resize", format!("{resized:?}"));
    }

    report.detail("pid", serde_json::json!(std::process::id()));
    Ok(report)
}

/// Compares URLs allowing the address bar to show the user's typed spelling
/// (for example without a trailing slash) before a navigation settles.
fn normalize(url: &str) -> &str {
    url.trim_end_matches('/')
}

fn read_snapshot(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
) -> Result<BrowserSnapshot, String> {
    handle
        .update(cx, |browser, _window, cx| browser.snapshot(cx))
        .map_err(|error| error.to_string())
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
        if let Ok(snapshot) = read_snapshot(handle, cx)
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
    checks: BTreeMap<String, Check>,
    details: serde_json::Value,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            checks: BTreeMap::new(),
            details: serde_json::json!({}),
        }
    }
}

#[derive(Debug)]
enum Check {
    Pass(String),
    Fail(String),
}

impl Report {
    fn pass(&mut self, name: &str, detail: impl Into<String>) {
        let detail = detail.into();
        println!("[m1] PASS {name}: {detail}");
        self.checks.insert(name.to_owned(), Check::Pass(detail));
    }

    fn fail(&mut self, name: &str, detail: impl Into<String>) {
        let detail = detail.into();
        println!("[m1] FAIL {name}: {detail}");
        self.checks.insert(name.to_owned(), Check::Fail(detail));
    }

    fn detail(&mut self, key: &str, value: serde_json::Value) {
        if let Some(map) = self.details.as_object_mut() {
            map.insert(key.to_owned(), value);
        }
    }

    fn is_pass(&self) -> bool {
        !self.checks.is_empty()
            && self
                .checks
                .values()
                .all(|check| matches!(check, Check::Pass(_)))
    }
}

fn failure_report(name: &str, detail: impl Into<String>) -> Report {
    let mut report = Report::default();
    report.fail(name, detail);
    report
}

fn finish(report: Report) -> ! {
    let checks = report
        .checks
        .iter()
        .map(|(name, check)| match check {
            Check::Pass(detail) => (
                name.clone(),
                serde_json::json!({"ok": true, "detail": detail}),
            ),
            Check::Fail(detail) => (
                name.clone(),
                serde_json::json!({"ok": false, "detail": detail}),
            ),
        })
        .collect::<serde_json::Map<_, _>>();
    let value = serde_json::json!({
        "passed": report.is_pass(),
        "checks": checks,
        "details": report.details,
    });
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(PathBuf::from(REPORT_PATH), text);
    }
    println!(
        "[m1] result: {}",
        if report.is_pass() { "PASS" } else { "FAIL" }
    );
    std::process::exit(if report.is_pass() { 0 } else { 1 });
}
