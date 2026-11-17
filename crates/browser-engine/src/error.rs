//! Engine errors.

/// Failures produced while creating or driving a web engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("the web engine is only implemented for macOS so far")]
    UnsupportedPlatform,

    #[error("a web view must be created on the main thread")]
    NotMainThread,

    #[error("the provided parent view was null")]
    InvalidParentView,

    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    #[error("the engine has been closed")]
    Closed,

    #[error("web engine failure: {0}")]
    Platform(String),
}
