//! GPUI browser chrome.
//!
//! This crate owns presentation: theme tokens, reusable components, the
//! address-bar text input and the native web-view host. It knows about the
//! abstract `browser_engine::Engine` trait, never about WebKit.

pub mod components;
pub mod text_input;
pub mod theme;

pub use components::{
    CHROME_HEIGHT, TAB_STRIP_HEIGHT, TOOLBAR_HEIGHT, icon_button, loading_bar, tab_chip,
    text_button, web_content_host,
};
pub use text_input::{TextInput, TextInputEvent, bind_text_input_keys};
pub use theme::Theme;
