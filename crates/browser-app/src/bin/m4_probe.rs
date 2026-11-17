//! M4 settings probe across two OS processes.
//!
//! Phase 1 edits homepage, search engine and theme through the same public
//! methods the settings page uses, navigates a search query, and waits for the
//! settings file. Phase 2 relaunches with the same data directory and a server
//! on the same port, then verifies that startup used the saved homepage and
//! that search queries still use the saved engine.

use std::collections::BTreeMap;
use std::time::Duration;

use browser_app::browser_window::{BrowserSnapshot, BrowserWindow};
use browser_core::ThemeMode;
use browser_testserver::TestServer;
use gpui::{App, Application, AsyncApp, Timer, WindowHandle};

const DATA_DIR: &str = "target/m4-data";
const PORT: u16 = 47821;
const REPORT_PHASE1: &str = "target/m4-report-phase1.json";
const REPORT_PHASE2: &str = "target/m4-report-phase2.json";

fn main() {
    browser_app::init_logging();
    let phase = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "phase1".to_owned())
        .trim_start_matches('-')
        .to_owned();
    // Safety: configured before the app starts.
    unsafe { std::env::set_var(browser_storage::DATA_DIR_ENV, DATA_DIR) };
    let server = TestServer::start_on(PORT).expect("start fixture server");
    let origin = server.origin();
    let search_template = format!("{origin}/search?q={{query}}");
    let homepage = format!("{origin}/");

    Application::new().run(move |cx: &mut App| {
        let handle = match browser_app::launch(cx) {
            Ok(handle) => handle,
            Err(error) => finish(failure("launch", error.to_string()), phase.clone()),
        };
        let phase_for_task = phase.clone();
        let origin = origin.clone();
        let search_template = search_template.clone();
        let homepage = homepage.clone();
        cx.spawn(async move |cx| {
            let result = match phase_for_task.as_str() {
                "phase2" => run_phase2(&handle, cx, &origin, &search_template, &homepage).await,
                _ => run_phase1(&handle, cx, &origin, &search_template, &homepage).await,
            };
            match result {
                Ok(checks) => finish(Report { checks }, phase_for_task),
                Err(error) => finish(failure("sequence", error), phase_for_task),
            }
        })
        .detach();
    });
    drop(server);
}

async fn run_phase1(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    origin: &str,
    search_template: &str,
    homepage: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut checks: BTreeMap<String, String> = BTreeMap::new();
    // Let the initial homepage load settle before editing settings.
    wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && !snapshot.active_url.is_empty()
    })
    .await
    .ok_or("startup page did not load")?;

    update(handle, cx, |browser, window, cx| {
        browser.set_homepage(homepage, window, cx);
        browser.set_search_engine_template(search_template, window, cx);
        browser.set_theme(ThemeMode::Dark, window, cx);
    })?;
    let edited = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.settings_homepage == homepage
            && snapshot.settings_search_template == search_template
            && snapshot.settings_theme == "dark"
    })
    .await
    .ok_or("settings did not update in memory")?;
    checks.insert(
        "edited".to_owned(),
        format!("homepage={}", edited.settings_homepage),
    );

    // The settings file must contain the same values.
    let persisted = wait_settings_file(Duration::from_secs(10), |value| {
        value["homepage"].as_str() == Some(homepage)
            && value["search_engine"]["url_template"].as_str() == Some(search_template)
            && value["theme"].as_str() == Some("dark")
    })
    .await
    .ok_or("settings.json did not persist the edits")?;
    checks.insert(
        "settings_file".into(),
        persisted["theme"].as_str().unwrap_or("").to_owned(),
    );

    // Search through the address bar uses the configured engine.
    update(handle, cx, |browser, window, cx| {
        browser.navigate("hello world", window, cx)
    })?;
    let search = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading
            && snapshot.active_url == format!("{origin}/search?q=hello+world")
            && snapshot.active_title == "search-hello world"
    })
    .await
    .ok_or("search did not use the configured engine")?;
    checks.insert("search".into(), search.active_url.clone());

    update(handle, cx, |browser, window, cx| {
        browser.open_settings(window, cx)
    })?;
    let page = wait_for(handle, cx, Duration::from_secs(5), |snapshot| {
        snapshot.overlay == "settings"
    })
    .await
    .ok_or("settings page did not open")?;
    checks.insert("settings_page".into(), page.overlay.clone());
    update(handle, cx, |browser, window, cx| {
        browser.close_overlay(window, cx)
    })?;

    // New-tab page lists recent sites and the bookmark bar reflects bookmarks.
    update(handle, cx, |browser, window, cx| {
        browser.toggle_bookmark(window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.bookmark_bar_count >= 1
    })
    .await
    .ok_or("bookmark bar did not receive the bookmark")?;
    update(handle, cx, |browser, window, cx| {
        browser.new_tab(window, cx)
    })?;
    let new_tab = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.active_url.is_empty() && !snapshot.new_tab_recents.is_empty()
    })
    .await
    .ok_or("new tab page did not list recent sites")?;
    checks.insert(
        "new_tab".to_owned(),
        format!(
            "{} recent site(s), {} bookmark(s) in bar",
            new_tab.new_tab_recents.len(),
            new_tab.bookmark_bar_count
        ),
    );
    update(handle, cx, |browser, window, cx| {
        browser.close_active_tab(window, cx)
    })?;

    // Command palette: command execution, URL execution and selection.
    let before_palette = read_snapshot(handle, cx).ok_or("snapshot unavailable")?;
    update(handle, cx, |browser, window, cx| {
        browser.open_palette(window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.palette_open && !snapshot.palette_items.is_empty()
    })
    .await
    .ok_or("command palette did not open with items")?;
    update(handle, cx, |browser, window, cx| {
        browser.palette_set_query("new tab", window, cx)
    })?;
    let command_list = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot
            .palette_items
            .iter()
            .any(|label| label == "New Tab")
    })
    .await
    .ok_or("palette query did not surface the New Tab command")?;
    let new_tab_index = command_list
        .palette_items
        .iter()
        .position(|label| label == "New Tab")
        .ok_or("New Tab command missing")?;
    update(handle, cx, |browser, _window, cx| {
        browser.palette_select_index(new_tab_index, cx)
    })?;
    update(handle, cx, |browser, window, cx| {
        browser.run_palette_selection(window, cx)
    })?;
    let after_command = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.tab_count == before_palette.tab_count + 1 && snapshot.active_url.is_empty()
    })
    .await
    .ok_or("palette New Tab command did not open a tab")?;
    checks.insert(
        "palette_command".to_owned(),
        format!("{} tabs", after_command.tab_count),
    );
    update(handle, cx, |browser, window, cx| {
        browser.close_active_tab(window, cx)
    })?;

    let page_two = format!("{origin}/page2");
    update(handle, cx, |browser, window, cx| {
        browser.open_palette(window, cx)
    })?;
    update(handle, cx, |browser, window, cx| {
        browser.palette_set_query(&page_two, window, cx)
    })?;
    update(handle, cx, |browser, window, cx| {
        browser.run_palette_selection(window, cx)
    })?;
    let palette_url = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_url == page_two
    })
    .await
    .ok_or("palette did not navigate to the typed URL")?;
    checks.insert("palette_url".to_owned(), palette_url.active_url.clone());

    update(handle, cx, |browser, window, cx| {
        browser.open_palette(window, cx)
    })?;
    update(handle, cx, |browser, window, cx| {
        browser.palette_set_query("history", window, cx)
    })?;
    update(handle, cx, |browser, _window, cx| {
        browser.palette_select_next(cx)
    })?;
    let selected = read_snapshot(handle, cx).ok_or("snapshot unavailable")?;
    update(handle, cx, |browser, _window, cx| {
        browser.palette_select_previous(cx)
    })?;
    let back = read_snapshot(handle, cx).ok_or("snapshot unavailable")?;
    if selected.palette_selected == 0 || back.palette_selected != 0 {
        return Err(format!(
            "palette selection did not move: next={} previous={}",
            selected.palette_selected, back.palette_selected
        ));
    }
    checks.insert(
        "palette_keys".to_owned(),
        "up/down selection works".to_owned(),
    );
    update(handle, cx, |browser, window, cx| {
        browser.close_palette(window, cx)
    })?;

    // Start phase 2 without a restored session so startup uses the homepage.
    Timer::after(Duration::from_millis(400)).await;
    let _ = std::fs::remove_file(std::path::Path::new(DATA_DIR).join("session.json"));
    Timer::after(Duration::from_millis(300)).await;
    Ok(checks)
}

async fn run_phase2(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    origin: &str,
    search_template: &str,
    homepage: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut checks: BTreeMap<String, String> = BTreeMap::new();
    // Fresh session: the saved homepage should be the only tab, loaded.
    let startup = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.tab_count == 1 && snapshot.active_url == homepage
    })
    .await
    .ok_or("startup did not use the saved homepage")?;
    checks.insert("homepage_restored".into(), startup.active_url.clone());

    if startup.settings_search_template != search_template
        || startup.settings_homepage != homepage
        || startup.settings_theme != "dark"
    {
        return Err(format!(
            "settings were not restored: homepage={} search={} theme={}",
            startup.settings_homepage, startup.settings_search_template, startup.settings_theme
        ));
    }
    checks.insert("settings_restored".into(), startup.settings_theme.clone());

    // The loaded template is actually used for new searches.
    update(handle, cx, |browser, window, cx| {
        browser.navigate("again", window, cx)
    })?;
    let search = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_url == format!("{origin}/search?q=again")
    })
    .await
    .ok_or("restored search engine was not used")?;
    checks.insert("search_after_restart".into(), search.active_url.clone());
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

async fn wait_settings_file(
    timeout: Duration,
    mut predicate: impl FnMut(&serde_json::Value) -> bool,
) -> Option<serde_json::Value> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(bytes) = std::fs::read(std::path::Path::new(DATA_DIR).join("settings.json"))
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && predicate(&value)
        {
            return Some(value);
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
    let mut checks: BTreeMap<String, String> = BTreeMap::new();
    checks.insert(name.to_owned(), detail.into());
    Report { checks }
}

fn finish(report: Report, phase: String) -> ! {
    let failed = report
        .checks
        .keys()
        .any(|key| key == "sequence" || key == "launch");
    for (name, detail) in &report.checks {
        println!("[m4:{phase}] {name}: {detail}");
    }
    let passed = !failed && !report.checks.is_empty();
    println!(
        "[m4:{phase}] result: {}",
        if passed { "PASS" } else { "FAIL" }
    );
    let value = serde_json::json!({
        "phase": phase,
        "passed": passed,
        "checks": report.checks,
    });
    let path = if phase == "phase2" {
        REPORT_PHASE2
    } else {
        REPORT_PHASE1
    };
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(path, text);
    }
    std::process::exit(if passed { 0 } else { 1 });
}
