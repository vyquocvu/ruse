//! GPUI Browser entry point.

use gpui::{App, Application};

fn main() {
    browser_app::init_logging();
    Application::new().run(|cx: &mut App| {
        if let Err(error) = browser_app::launch(cx) {
            eprintln!("failed to launch GPUI Browser: {error:#}");
            cx.quit();
        }
    });
}
