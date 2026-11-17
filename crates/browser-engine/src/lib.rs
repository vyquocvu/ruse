//! Platform-independent web engine abstraction.
//!
//! The macOS implementation wraps a `WKWebView` (WebKit) using typed objc2
//! bindings. See `docs/decisions/0002-wkwebview-engine.md` for the rationale.

pub mod engine;
pub mod error;
pub mod event;

#[cfg(target_os = "macos")]
mod macos;

pub use engine::{
    Engine, EngineInstance, EngineOptions, NativeParent, PrivateDataStore, create_engine,
    create_private_data_store,
};
pub use error::EngineError;
pub use event::{EngineEvent, Viewport};

#[cfg(target_os = "macos")]
pub use macos::is_web_inspector_available;
