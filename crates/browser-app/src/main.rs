use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size};

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Root),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}

struct Root;

impl Render for Root {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x1e1e1e))
    }
}
