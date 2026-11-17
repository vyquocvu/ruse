//! Performance probe: startup, tab memory, navigation latency and idle CPU.
//!
//! Uses a local fixture homepage and a fixed port so runs are comparable. The
//! probe writes `target/perf-report.json`; `scripts/perf.sh` runs it for 1/5/10
//! tabs in a release build and summarizes the numbers.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use browser_app::browser_window::{BrowserSnapshot, BrowserWindow};
use browser_core::{BrowserSettings, SearchEngine};
use browser_storage::SettingsStore;
use browser_testserver::TestServer;
use gpui::{App, Application, AsyncApp, Timer, WindowHandle};

const DATA_DIR: &str = "target/perf-data";
const PORT: u16 = 47901;
const REPORT_PATH: &str = "target/perf-report.json";

fn main() {
    let process_start = Instant::now();
    browser_app::init_logging();
    let tabs = std::env::args()
        .position(|arg| arg == "--tabs")
        .and_then(|index| std::env::args().nth(index + 1))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    let data_dir = std::path::PathBuf::from(DATA_DIR);
    let downloads = std::path::PathBuf::from("target/perf-downloads");
    let _ = std::fs::remove_dir_all(&downloads);
    // Safety: configured before the app starts.
    unsafe {
        std::env::set_var(browser_storage::DATA_DIR_ENV, &data_dir);
        std::env::set_var("GPUI_BROWSER_DOWNLOADS_DIR", &downloads);
    }
    let server = TestServer::start_on(PORT).expect("start fixture server");
    let origin = server.origin();
    // Deterministic local homepage (and no restored session).
    let _ = std::fs::remove_file(data_dir.join("session.json"));
    let settings = BrowserSettings {
        homepage: format!("{origin}/"),
        search_engine: SearchEngine::new("fixture", format!("{origin}/search?q={{query}}")),
        theme: browser_core::ThemeMode::System,
    };
    SettingsStore::in_directory(&data_dir)
        .save(&settings)
        .expect("seed settings");

    let baseline = webkit_pids();
    let app_started = Instant::now();

    Application::new().run(move |cx: &mut App| {
        let handle = match browser_app::launch(cx) {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!("[perf] launch failed: {error}");
                std::process::exit(1);
            }
        };
        let baseline_for_task = baseline.clone();
        cx.spawn(async move |cx| {
            let report = run(
                &handle,
                cx,
                &origin,
                tabs,
                &baseline_for_task,
                process_start,
                app_started,
            )
            .await;
            match report {
                Ok(report) => finish(report),
                Err(error) => {
                    eprintln!("[perf] {error}");
                    std::process::exit(1);
                }
            }
        })
        .detach();
    });
    drop(server);
}

async fn run(
    handle: &WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    origin: &str,
    tabs: usize,
    baseline: &HashSet<u32>,
    process_start: Instant,
    app_started: Instant,
) -> Result<Report, String> {
    let first = wait_for(handle, cx, Duration::from_secs(30), |snapshot| {
        !snapshot.loading && snapshot.active_url.starts_with(origin)
    })
    .await
    .ok_or("startup page did not load")?;
    let startup_ms = app_started.elapsed().as_secs_f64() * 1000.0;
    let process_start_ms = process_start.elapsed().as_secs_f64() * 1000.0;

    let tab_open_started = Instant::now();
    for index in 1..tabs {
        update(handle, cx, |browser, window, cx| {
            browser.new_tab(window, cx)
        })?;
        let url = if index % 2 == 0 {
            format!("{origin}/page2")
        } else {
            format!("{origin}/")
        };
        update(handle, cx, |browser, window, cx| {
            browser.navigate(&url, window, cx)
        })?;
        wait_for(handle, cx, Duration::from_secs(20), |snapshot| {
            !snapshot.loading && snapshot.active_url.starts_with(origin)
        })
        .await
        .ok_or_else(|| format!("tab {index} did not load"))?;
    }
    let tab_open_ms = tab_open_started.elapsed().as_secs_f64() * 1000.0;

    // All tabs settled.
    wait_for(handle, cx, Duration::from_secs(30), |snapshot| {
        snapshot.tab_count == tabs
            && snapshot
                .tabs
                .iter()
                .all(|tab| !tab.loading && !tab.url.is_empty())
    })
    .await
    .ok_or("tabs did not settle")?;

    // Warm navigation latency on the active tab.
    let mut latencies = Vec::new();
    for index in 0..5 {
        let url = if index % 2 == 0 {
            format!("{origin}/page2")
        } else {
            format!("{origin}/")
        };
        let started = Instant::now();
        update(handle, cx, |browser, window, cx| {
            browser.navigate(&url, window, cx)
        })?;
        wait_for(handle, cx, Duration::from_secs(10), |snapshot| {
            !snapshot.loading && snapshot.active_url == url
        })
        .await
        .ok_or("navigation did not settle")?;
        latencies.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let navigation_ms = latencies.iter().sum::<f64>() / latencies.len() as f64;

    // Idle CPU over three seconds after settling.
    Timer::after(Duration::from_millis(1500)).await;
    let cpu_before = cpu_seconds(std::process::id());
    Timer::after(Duration::from_secs(3)).await;
    let cpu_after = cpu_seconds(std::process::id());
    let idle_cpu_percent = match (cpu_before, cpu_after) {
        (Some(before), Some(after)) => (after - before) / 3.0 * 100.0,
        _ => -1.0,
    };

    let host_rss_kb = rss_kb(std::process::id()).unwrap_or(0);
    let helpers = webkit_helper_rss_kb(baseline);
    let _ = first;

    Ok(Report {
        tabs,
        startup_ms,
        process_start_ms,
        tab_open_ms,
        navigation_ms,
        idle_cpu_percent,
        host_rss_mb: host_rss_kb as f64 / 1024.0,
        webkit_rss_mb: helpers.total_kb as f64 / 1024.0,
        webkit_processes: helpers.count,
        pid: std::process::id(),
    })
}

struct Report {
    tabs: usize,
    startup_ms: f64,
    process_start_ms: f64,
    tab_open_ms: f64,
    navigation_ms: f64,
    idle_cpu_percent: f64,
    host_rss_mb: f64,
    webkit_rss_mb: f64,
    webkit_processes: usize,
    pid: u32,
}

fn finish(report: Report) -> ! {
    let value = serde_json::json!({
        "tabs": report.tabs,
        "startup_ms": report.startup_ms,
        "process_start_ms": report.process_start_ms,
        "tab_open_ms": report.tab_open_ms,
        "navigation_ms": report.navigation_ms,
        "idle_cpu_percent": report.idle_cpu_percent,
        "host_rss_mb": report.host_rss_mb,
        "webkit_rss_mb": report.webkit_rss_mb,
        "webkit_processes": report.webkit_processes,
        "total_rss_mb": report.host_rss_mb + report.webkit_rss_mb,
        "pid": report.pid,
    });
    println!(
        "[perf] {}",
        serde_json::to_string(&value).unwrap_or_default()
    );
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(REPORT_PATH, text);
    }
    std::process::exit(0);
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
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(snapshot) = handle.update(cx, |browser, _window, cx| browser.snapshot(cx))
            && predicate(&snapshot)
        {
            return Some(snapshot);
        }
        if Instant::now() >= deadline {
            return None;
        }
        Timer::after(Duration::from_millis(30)).await;
    }
}

fn ps_rows() -> Vec<(u32, u64, String)> {
    let output = std::process::Command::new("ps")
        .args(["-eo", "pid=,rss=,comm="])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let rss = parts.next()?.parse().ok()?;
            let comm = parts.collect::<Vec<_>>().join(" ");
            Some((pid, rss, comm))
        })
        .collect()
}

fn rss_kb(pid: u32) -> Option<u64> {
    ps_rows()
        .into_iter()
        .find(|(row_pid, _, _)| *row_pid == pid)
        .map(|(_, rss, _)| rss)
}

fn webkit_pids() -> HashSet<u32> {
    ps_rows()
        .into_iter()
        .filter(|(_, _, comm)| comm.contains("WebKit"))
        .map(|(pid, _, _)| pid)
        .collect()
}

struct HelperRss {
    total_kb: u64,
    count: usize,
}

fn webkit_helper_rss_kb(baseline: &HashSet<u32>) -> HelperRss {
    let rows = ps_rows()
        .into_iter()
        .filter(|(pid, _, comm)| comm.contains("WebKit") && !baseline.contains(pid))
        .collect::<Vec<_>>();
    HelperRss {
        total_kb: rows.iter().map(|(_, rss, _)| rss).sum(),
        count: rows.len(),
    }
}

fn cpu_seconds(pid: u32) -> Option<f64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    let mut parts = text.split(':').rev();
    let seconds: f64 = parts.next()?.trim().parse().ok()?;
    let minutes: f64 = parts
        .next()
        .and_then(|m| m.trim().parse().ok())
        .unwrap_or(0.0);
    let hours: f64 = parts
        .next()
        .and_then(|h| h.trim().parse().ok())
        .unwrap_or(0.0);
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}
