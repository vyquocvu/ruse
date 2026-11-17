//! M2 session-restore probe.
//!
//! Runs as two separate OS processes so the "quit and relaunch" path is real:
//!
//! - `m2-probe --phase1` opens tabs, exercises duplicate/close/reorder, waits
//!   until the session file on disk matches the expected state, then exits.
//! - `m2-probe --phase2` starts the app again with the same data directory and
//!   verifies that tab order, active tab, titles and lazy loading are restored.
//!
//! Runs against `data:` URLs so the test never depends on the network.

use std::path::PathBuf;
use std::time::Duration;

use browser_app::browser_window::{BrowserSnapshot, BrowserWindow};
use browser_core::SessionSnapshot;
use gpui::{App, Application, AsyncApp, Timer, WindowHandle};

const DATA_DIR: &str = "target/m2-data";
const REPORT_PHASE1: &str = "target/m2-report-phase1.json";
const REPORT_PHASE2: &str = "target/m2-report-phase2.json";
const ALPHA: &str = "data:text/html,%3Ctitle%3EAlpha%3C/title%3E%3Ch1%3Ealpha%3C/h1%3E";
const BETA: &str = "data:text/html,%3Ctitle%3EBeta%3C/title%3E%3Ch1%3Ebeta%3C/h1%3E";

fn main() {
    browser_app::init_logging();
    let phase = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "phase1".to_owned())
        .trim_start_matches('-')
        .to_owned();
    // Safety: configured before the app (and any other thread) starts.
    unsafe { std::env::set_var(browser_storage::DATA_DIR_ENV, DATA_DIR) };
    let mut report = Report::default();

    Application::new().run(move |cx: &mut App| {
        let handle = match browser_app::launch(cx) {
            Ok(handle) => handle,
            Err(error) => finish(failure("launch", error.to_string()), phase.clone()),
        };
        let phase_for_task = phase.clone();
        cx.spawn(async move |cx| {
            let result = match phase_for_task.as_str() {
                "phase2" => run_phase2(&handle, cx).await,
                _ => run_phase1(&handle, cx).await,
            };
            match result {
                Ok(checks) => {
                    report.checks = checks;
                    finish(report, phase_for_task)
                }
                Err(error) => finish(failure("sequence", error), phase_for_task),
            }
        })
        .detach();
    });
}

async fn run_phase1(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
) -> Result<BTreeChecks, String> {
    let mut checks = BTreeChecks::new();

    // Startup tab -> Alpha.
    update(handle, cx, |browser, window, cx| {
        browser.navigate(ALPHA, window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "Alpha"
    })
    .await
    .ok_or("Alpha did not load")?;

    // New tab -> Beta.
    update(handle, cx, |browser, window, cx| {
        browser.new_tab(window, cx)
    })?;
    update(handle, cx, |browser, window, cx| {
        browser.navigate(BETA, window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        snapshot.tab_count == 2 && !snapshot.loading && snapshot.active_title == "Beta"
    })
    .await
    .ok_or("Beta did not load in the second tab")?;
    checks.insert("tabs_created".into(), "2 tabs: Alpha, Beta".into());

    // Duplicate the active tab; the copy is adjacent and active.
    update(handle, cx, |browser, window, cx| {
        browser.duplicate_active_tab(window, cx)
    })?;
    let duplicated = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.tab_count == 3 && snapshot.active_title == "Beta"
    })
    .await
    .ok_or("duplicate did not create a third tab")?;
    checks.insert(
        "duplicate_tab".into(),
        format!("{} tabs after duplicate", duplicated.tab_count),
    );

    // Close the duplicate; Beta becomes active again.
    update(handle, cx, |browser, window, cx| {
        browser.close_active_tab(window, cx)
    })?;
    wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.tab_count == 2 && snapshot.active_title == "Beta"
    })
    .await
    .ok_or("closing the duplicate did not return to Beta")?;

    // Reorder Beta in front of Alpha.
    update(handle, cx, |browser, _window, cx| {
        let beta = browser
            .snapshot(cx)
            .tabs
            .iter()
            .position(|tab| tab.title == "Beta")
            .unwrap_or(1);
        browser.reorder_tab_by_index(beta, 0, cx)
    })?;
    let reordered = wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
        snapshot.tab_count == 2
            && snapshot.tabs.first().is_some_and(|tab| tab.title == "Beta")
            && snapshot.tabs.get(1).is_some_and(|tab| tab.title == "Alpha")
    })
    .await
    .ok_or("reorder did not put Beta first")?;
    checks.insert(
        "reorder".into(),
        format!(
            "order: {}",
            reordered
                .tabs
                .iter()
                .map(|tab| tab.title.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );

    // Wait until the persisted session reflects the final state.
    let expected = wait_for_session(Duration::from_secs(10), |session| {
        session.tabs.len() == 2
            && session.tabs[0].url == BETA
            && session.tabs[1].url == ALPHA
            && session.active == 0
    })
    .await
    .ok_or("session file did not reach the expected state")?;
    checks.insert(
        "session_written".into(),
        format!("{} tabs, active {}", expected.tabs.len(), expected.active),
    );
    Ok(checks)
}

async fn run_phase2(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
) -> Result<BTreeChecks, String> {
    let mut checks = BTreeChecks::new();

    // Restored order, titles and active tab, with the active page loading.
    let restored = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        snapshot.tab_count == 2
            && snapshot.tabs.first().is_some_and(|tab| tab.title == "Beta")
            && snapshot.tabs.get(1).is_some_and(|tab| tab.title == "Alpha")
            && !snapshot.loading
            && snapshot.active_title == "Beta"
    })
    .await
    .ok_or("session was not restored as expected")?;
    checks.insert(
        "session_restored".into(),
        format!(
            "{} tabs, active '{}'",
            restored.tab_count, restored.active_title
        ),
    );

    // Background tabs are lazy: activating Alpha must load it on demand.
    let alpha_index = restored
        .tabs
        .iter()
        .position(|tab| tab.title == "Alpha")
        .ok_or("Alpha tab missing after restore")?;
    update(handle, cx, |browser, window, cx| {
        browser.activate_tab_index(alpha_index, window, cx)
    })?;
    let alpha = wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
        !snapshot.loading && snapshot.active_title == "Alpha"
    })
    .await
    .ok_or("lazy-loaded Alpha tab did not load")?;
    checks.insert(
        "lazy_restore".into(),
        format!("activated tab loaded: {}", alpha.active_url),
    );

    // Activation is persisted for the next restore.
    let expected = wait_for_session(Duration::from_secs(10), |session| {
        session.active == alpha_index && session.tabs.len() == 2
    })
    .await
    .ok_or("active tab change was not persisted")?;
    checks.insert(
        "active_persisted".into(),
        format!("active index {}", expected.active),
    );
    Ok(checks)
}

type BTreeChecks = std::collections::BTreeMap<String, String>;

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

fn read_session() -> Option<SessionSnapshot> {
    let bytes = std::fs::read(PathBuf::from(DATA_DIR).join("session.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

async fn wait_for_session(
    timeout: Duration,
    mut predicate: impl FnMut(&SessionSnapshot) -> bool,
) -> Option<SessionSnapshot> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(session) = read_session()
            && predicate(&session)
        {
            return Some(session);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        Timer::after(Duration::from_millis(80)).await;
    }
}

#[derive(Default)]
struct Report {
    checks: BTreeChecks,
    failed: bool,
}

fn failure(name: &str, detail: impl Into<String>) -> Report {
    let mut report = Report {
        failed: true,
        ..Default::default()
    };
    report.checks.insert(name.to_owned(), detail.into());
    report
}

fn finish(report: Report, phase: String) -> ! {
    for (name, detail) in &report.checks {
        println!("[m2:{phase}] {name}: {detail}");
    }
    let passed = !report.failed && !report.checks.is_empty();
    println!(
        "[m2:{phase}] result: {}",
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
