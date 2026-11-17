//! M0 feasibility probe.
//!
//! This binary is a test harness, not the browser UI. It opens a real GPUI
//! window, embeds real `WKWebView`s as native subviews, and asserts the
//! behaviour the M0 milestone depends on:
//!
//! 1. a native GPUI window with a real web view composited inside it,
//! 2. a real HTTPS site loading with a title,
//! 3. JavaScript execution and CSS application on a deterministic local page,
//! 4. responsive resize (the native view follows GPUI layout, page width
//!    follows the window),
//! 5. two independent web views with separate JavaScript contexts,
//! 6. page snapshots written to disk as visual evidence.
//!
//! Results are written to `target/m0-report.json`; exit status is non-zero
//! when any check fails. Memory is measured by `scripts/m0-probe.sh`, which
//! samples RSS while the probe runs.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use browser_engine::{
    create_engine, Engine, EngineEvent, EngineOptions, NativeParent, Viewport,
};
use browser_testserver::TestServer;
use gpui::{
    canvas, div, prelude::*, px, size, App, Application, AsyncWindowContext, Bounds, Context,
    IntoElement, Render, SharedString, TitlebarOptions, Timer, WeakEntity, Window, WindowBounds,
    WindowOptions,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 800.0;
const CHROME_HEIGHT: f32 = 64.0;
const RESIZE_WIDTH: f32 = 900.0;
const RESIZE_HEIGHT: f32 = 650.0;
const SNAPSHOT_PATH: &str = "target/m0-snapshot.png";

fn main() {
    let server = TestServer::start().expect("start local fixture server");
    let fixture_origin = server.origin();
    let fixture_url = server.url("/");
    let page_two_url = server.url("/page2");

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("GPUI Browser — M0 probe")),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let parent = match native_parent(window) {
                    Ok(parent) => parent,
                    Err(error) => {
                        exit_with_report(failure("webview_parent", error));
                    }
                };

                let instance = match create_engine(EngineOptions {
                    parent,
                    viewport: Viewport::new(0.0, CHROME_HEIGHT as f64, 0.0, 0.0),
                    initial_url: None,
                    private: false,
                }) {
                    Ok(instance) => instance,
                    Err(error) => {
                        exit_with_report(failure("engine_create", error.to_string()));
                    }
                };
                let (engine, events) = instance.into_parts();
                if let Err(error) = engine.navigate("https://example.com") {
                    exit_with_report(failure("navigate_example", error.to_string()));
                }

                let probe = cx.new(|_| Probe {
                    engine: engine.clone(),
                    second_engine: None,
                    multi_view: Rc::new(Cell::new(false)),
                });
                let probe_for_task = probe.downgrade();

                window.spawn(cx, async move |cx| {
                    let outcome = run_probe(
                        &probe_for_task,
                        cx,
                        engine,
                        events,
                        parent,
                        fixture_url,
                        fixture_origin,
                        page_two_url,
                    )
                    .await;
                    match outcome {
                        Ok(report) => exit_with_report(report),
                        Err(error) => exit_with_report(failure("sequence", error)),
                    }
                })
                .detach();

                probe
            },
        )
        .expect("open window");
        cx.activate(true);
    });

    // The GPUI run loop returns only when the app quits; the probe calls
    // process::exit itself.
    drop(server);
}

fn native_parent(window: &mut Window) -> Result<NativeParent, String> {
    let handle = window
        .window_handle()
        .map_err(|error| format!("no native window handle: {error}"))?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(appkit) => {
            Ok(unsafe { NativeParent::from_ns_view(appkit.ns_view.as_ptr()) })
        }
        other => Err(format!("unexpected window handle: {other:?}")),
    }
}

struct Probe {
    engine: Rc<dyn Engine>,
    second_engine: Option<Rc<dyn Engine>>,
    multi_view: Rc<Cell<bool>>,
}

impl Render for Probe {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let engine = self.engine.clone();
        let multi_view = self.multi_view.clone();
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(gpui::rgb(0x101418))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .h(px(CHROME_HEIGHT))
                    .px_4()
                    .justify_center()
                    .bg(gpui::rgb(0x161b21))
                    .border_b_1()
                    .border_color(gpui::rgb(0x2a323c))
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(0x8a97a5))
                            .child("GPUI Browser · M0 engine feasibility probe"),
                    ),
            )
            .child(div().flex_1().child(
                canvas(
                    move |_bounds, _window, _cx| {},
                    move |bounds, _state, _window, _cx| {
                        if multi_view.get() {
                            return;
                        }
                        engine.set_viewport(Viewport::new(
                            bounds.origin.x.into(),
                            bounds.origin.y.into(),
                            bounds.size.width.into(),
                            bounds.size.height.into(),
                        ));
                    },
                )
                .size_full(),
            ))
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
        println!("[m0] PASS {name}: {detail}");
        self.checks.insert(name.to_owned(), Check::Pass(detail));
    }

    fn fail(&mut self, name: &str, detail: impl Into<String>) {
        let detail = detail.into();
        println!("[m0] FAIL {name}: {detail}");
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

fn failure(name: &str, detail: impl Into<String>) -> Report {
    let mut report = Report::default();
    report.fail(name, detail);
    report
}

fn exit_with_report(report: Report) -> ! {
    let checks = report
        .checks
        .iter()
        .map(|(name, check)| match check {
            Check::Pass(detail) => (name.clone(), serde_json::json!({"ok": true, "detail": detail})),
            Check::Fail(detail) => (name.clone(), serde_json::json!({"ok": false, "detail": detail})),
        })
        .collect::<serde_json::Map<_, _>>();
    let value = serde_json::json!({
        "passed": report.is_pass(),
        "checks": checks,
        "details": report.details,
        "pid": std::process::id(),
    });
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        if let Err(error) = std::fs::write("target/m0-report.json", text) {
            eprintln!("[m0] could not write report: {error}");
        }
    }
    println!(
        "[m0] result: {}",
        if report.is_pass() { "PASS" } else { "FAIL" }
    );
    std::process::exit(if report.is_pass() { 0 } else { 1 });
}

async fn wait_for(
    events: &async_channel::Receiver<EngineEvent>,
    report: &mut Report,
    timeout: Duration,
    mut predicate: impl FnMut(&EngineEvent) -> bool,
) -> Option<EngineEvent> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            println!("[m0] timed out waiting for an engine event");
            return None;
        }
        let received = async { events.recv().await.ok() };
        let timed_out = async {
            Timer::after(remaining).await;
            None
        };
        match futures_lite::future::or(received, timed_out).await {
            Some(event) => {
                if predicate(&event) {
                    return Some(event);
                }
                println!("[m0] event: {event:?}");
                note_event(report, &event);
            }
            None => return None,
        }
    }
}

fn note_event(report: &mut Report, event: &EngineEvent) {
    match event {
        EngineEvent::LoadFailed { message, url, .. } => {
            report.fail("unexpected_load_failure", format!("{url}: {message}"));
        }
        EngineEvent::ProcessTerminated => {
            report.fail("unexpected_process_termination", "web content process died");
        }
        _ => {}
    }
}

async fn evaluate(
    events: &async_channel::Receiver<EngineEvent>,
    report: &mut Report,
    engine: &Rc<dyn Engine>,
    expression: &str,
    timeout: Duration,
) -> Option<serde_json::Value> {
    let id = engine.evaluate_script_json(expression);
    match wait_for(events, report, timeout, |event| {
        matches!(event, EngineEvent::ScriptResult { id: this, .. } if *this == id)
    })
    .await
    {
        Some(EngineEvent::ScriptResult { value, .. }) => Some(value),
        Some(EngineEvent::ScriptError { message, .. }) => {
            report.fail("script_error", message);
            None
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_probe(
    probe: &WeakEntity<Probe>,
    cx: &mut AsyncWindowContext,
    engine: Rc<dyn Engine>,
    events: async_channel::Receiver<EngineEvent>,
    parent: NativeParent,
    fixture_url: String,
    fixture_origin: String,
    page_two_url: String,
) -> Result<Report, String> {
    let mut report = Report::default();

    // ----- Step 1: real HTTPS navigation -------------------------------
    let loaded = wait_for(&events, &mut report, Duration::from_secs(25), |event| {
        matches!(event, EngineEvent::LoadFinished { .. })
    })
    .await
    .ok_or("timed out waiting for https://example.com")?;

    let (https_url, https_title) = match loaded {
        EngineEvent::LoadFinished { url, title, .. } => (url, title),
        _ => unreachable!(),
    };
    if https_url.starts_with("https://") && https_title.as_deref().is_some_and(|t| !t.is_empty()) {
        report.pass(
            "https_navigation",
            format!("{} ({})", https_title.clone().unwrap_or_default(), https_url),
        );
    } else {
        report.fail(
            "https_navigation",
            format!("url={https_url} title={https_title:?}"),
        );
    }

    let site_facts = evaluate(
        &events,
        &mut report,
        &engine,
        "({ title: document.title, bodyText: document.body.innerText.trim().length, ready: document.readyState, hasStyleSheets: document.styleSheets.length > 0 })",
        Duration::from_secs(5),
    )
    .await;
    if let Some(facts) = site_facts {
        report.detail("example_site", facts.clone());
        let body_len = facts.get("bodyText").and_then(|value| value.as_u64());
        if body_len.is_some_and(|length| length > 0)
            && facts.get("ready").and_then(|v| v.as_str()) == Some("complete")
            && facts.get("hasStyleSheets").and_then(|v| v.as_bool()) == Some(true)
        {
            report.pass("https_dom", format!("example.com body text {body_len:?} chars"));
        } else {
            report.fail("https_dom", format!("unexpected DOM facts: {facts}"));
        }
    }

    // ----- Step 2: deterministic local page (JS + CSS) ----------------
    engine
        .navigate(&fixture_url)
        .map_err(|error| error.to_string())?;
    let loaded = wait_for(&events, &mut report, Duration::from_secs(15), |event| {
        matches!(event, EngineEvent::LoadFinished { url, .. } if url.starts_with(&fixture_origin))
    })
    .await
    .ok_or("timed out waiting for fixture page")?;
    let (loaded_url, loaded_title) = match loaded {
        EngineEvent::LoadFinished { url, title, .. } => (url, title),
        _ => unreachable!(),
    };

    let facts = evaluate(
        &events,
        &mut report,
        &engine,
        "({
            title: document.title,
            heading: document.getElementById('heading')?.textContent ?? null,
            js: document.getElementById('js')?.textContent ?? null,
            color: (() => { const h = document.getElementById('heading'); return h ? getComputedStyle(h).color : null; })(),
            sheets: document.styleSheets.length,
            hasForm: !!document.querySelector('form'),
            globalAnswer: window.__fixture?.answer ?? null,
            links: document.querySelectorAll('a').length,
            scrollHeight: document.documentElement.scrollHeight,
            innerWidth: window.innerWidth,
            ready: document.readyState
        })",
        Duration::from_secs(5),
    )
    .await
    .ok_or("fixture page did not answer the DOM probe")?;

    report.detail("fixture", facts.clone());
    report.detail(
        "fixture_page",
        serde_json::json!({ "url": loaded_url, "title": loaded_title }),
    );

    let heading = facts.get("heading").and_then(|v| v.as_str());
    let js_text = facts.get("js").and_then(|v| v.as_str());
    let color = facts.get("color").and_then(|v| v.as_str());
    if heading == Some("fixture-heading") && js_text == Some("js-ok") {
        report.pass(
            "javascript_execution",
            "external script ran and inline script mutated the DOM",
        );
    } else {
        report.fail(
            "javascript_execution",
            format!("heading={heading:?} js={js_text:?}"),
        );
    }
    if color == Some("rgb(12, 120, 90)") {
        report.pass("css_application", format!("computed heading color {color:?}"));
    } else {
        report.fail("css_application", format!("computed heading color {color:?}"));
    }
    if facts.get("globalAnswer").and_then(|v| v.as_i64()) == Some(42) {
        report.pass("page_context", "window.__fixture survives in the page context");
    } else {
        report.fail(
            "page_context",
            format!("globalAnswer={:?}", facts.get("globalAnswer")),
        );
    }
    if facts.get("hasForm").and_then(|v| v.as_bool()) == Some(true) {
        report.pass("form_rendering", "fixture form present in the DOM");
    } else {
        report.fail("form_rendering", "fixture form missing");
    }

    // ----- Step 3: snapshot (visual evidence) -------------------------
    engine.take_snapshot(PathBuf::from(SNAPSHOT_PATH));
    match wait_for(&events, &mut report, Duration::from_secs(10), |event| {
        matches!(
            event,
            EngineEvent::SnapshotSaved { .. } | EngineEvent::SnapshotFailed { .. }
        )
    })
    .await
    {
        Some(EngineEvent::SnapshotSaved {
            path,
            width,
            height,
        }) => {
            if width > 0 && height > 0 && path.exists() {
                report.pass(
                    "page_snapshot",
                    format!("{}x{} at {}", width, height, path.display()),
                );
            } else {
                report.fail(
                    "page_snapshot",
                    format!("unexpected snapshot {} ({width}x{height})", path.display()),
                );
            }
        }
        Some(EngineEvent::SnapshotFailed { message }) => report.fail("page_snapshot", message),
        _ => report.fail("page_snapshot", "timed out"),
    }

    // ----- Step 4: resize responsiveness ------------------------------
    let before_width = facts.get("innerWidth").and_then(|v| v.as_f64());
    probe
        .update_in(cx, |_probe, window, _cx| {
            window.resize(size(px(RESIZE_WIDTH), px(RESIZE_HEIGHT)));
        })
        .map_err(|error| error.to_string())?;
    Timer::after(Duration::from_millis(800)).await;

    let resized = evaluate(
        &events,
        &mut report,
        &engine,
        "({ innerWidth: window.innerWidth, innerHeight: window.innerHeight, dpr: window.devicePixelRatio })",
        Duration::from_secs(5),
    )
    .await
    .ok_or("resize probe did not answer")?;
    let resized_width = resized.get("innerWidth").and_then(|v| v.as_f64());
    if let (Some(before), Some(after)) = (before_width, resized_width) {
        if (after - RESIZE_WIDTH as f64).abs() < 2.0 && after < before {
            report.pass(
                "resize",
                format!("innerWidth {before} -> {after} (expected {RESIZE_WIDTH})"),
            );
        } else {
            report.fail("resize", format!("innerWidth {before} -> {after}"));
        }
    } else {
        report.fail("resize", format!("missing widths: {resized}"));
    }
    report.detail("resized", resized);

    // ----- Step 5: two independent web views --------------------------
    engine.set_viewport(Viewport::new(0.0, CHROME_HEIGHT as f64, 450.0, 520.0));
    probe
        .update_in(cx, |probe, _window, cx| {
            probe.multi_view.set(true);
            cx.notify();
        })
        .map_err(|error| error.to_string())?;

    let second_instance = create_engine(EngineOptions {
        parent,
        viewport: Viewport::new(470.0, CHROME_HEIGHT as f64, 420.0, 520.0),
        initial_url: Some(page_two_url.clone()),
        private: false,
    })
    .map_err(|error| error.to_string())?;
    let (second_engine, second_events) = second_instance.into_parts();
    probe
        .update_in(cx, |probe, _window, cx| {
            probe.second_engine = Some(second_engine.clone());
            cx.notify();
        })
        .map_err(|error| error.to_string())?;

    let second_loaded = wait_for(&second_events, &mut report, Duration::from_secs(15), |event| {
        matches!(event, EngineEvent::LoadFinished { .. })
    })
    .await
    .ok_or("timed out waiting for the second web view")?;
    let (second_url, second_title) = match second_loaded {
        EngineEvent::LoadFinished { url, title, .. } => (url, title),
        _ => unreachable!(),
    };
    let second_dom = evaluate(
        &second_events,
        &mut report,
        &second_engine,
        "({ title: document.title, heading: document.querySelector('h1')?.textContent ?? null })",
        Duration::from_secs(5),
    )
    .await;
    let second_title = second_dom
        .as_ref()
        .and_then(|facts| facts.get("title"))
        .and_then(|title| title.as_str())
        .map(str::to_owned)
        .or(second_title);
    if second_url.starts_with(&fixture_origin) && second_title.as_deref() == Some("Second Page") {
        report.pass(
            "multiple_views",
            format!("second view loaded {second_title:?} at {second_url}"),
        );
    } else {
        report.fail(
            "multiple_views",
            format!("url={second_url} title={second_title:?}"),
        );
    }

    // Each view must have its own JavaScript context.
    let second_global = evaluate(
        &second_events,
        &mut report,
        &second_engine,
        "typeof window.__fixture",
        Duration::from_secs(5),
    )
    .await;
    let first_global = evaluate(
        &events,
        &mut report,
        &engine,
        "window.__fixture?.answer ?? null",
        Duration::from_secs(5),
    )
    .await;
    report.detail(
        "independent_contexts",
        serde_json::json!({ "second_typeof": second_global, "first_answer": first_global }),
    );
    if second_global.as_ref().and_then(|v| v.as_str()) == Some("undefined")
        && first_global.as_ref().and_then(|v| v.as_i64()) == Some(42)
    {
        report.pass(
            "independent_contexts",
            "second view cannot see the first view's globals",
        );
    } else {
        report.fail(
            "independent_contexts",
            format!("second={second_global:?} first={first_global:?}"),
        );
    }

    // Closing one view must not disturb the other.
    second_engine.close();
    probe
        .update_in(cx, |probe, _window, cx| {
            probe.second_engine = None;
            cx.notify();
        })
        .map_err(|error| error.to_string())?;
    Timer::after(Duration::from_millis(200)).await;
    let still_alive = evaluate(
        &events,
        &mut report,
        &engine,
        "({ answer: window.__fixture?.answer ?? null, alive: document.readyState })",
        Duration::from_secs(5),
    )
    .await;
    if still_alive
        .as_ref()
        .and_then(|v| v.get("answer"))
        .and_then(|v| v.as_i64())
        == Some(42)
    {
        report.pass(
            "view_lifecycle",
            "closing the second view leaves the first page alive",
        );
    } else {
        report.fail("view_lifecycle", format!("{still_alive:?}"));
    }

    report.detail("pid", serde_json::json!(std::process::id()));
    Ok(report)
}
