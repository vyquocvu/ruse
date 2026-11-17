//! Browser domain models.
//!
//! This crate is pure state and policy: no GPUI, no AppKit, no WebKit. It is
//! unit-testable without a window and owns the rules the UI follows.

pub mod navigation;
pub mod search;
pub mod session;
pub mod settings;
pub mod tab;
pub mod url_input;

pub use navigation::{LoadStatus, NavigationEvent, NavigationState};
pub use search::SearchEngine;
pub use session::{MAX_SESSION_TABS, SessionSnapshot, SessionTab};
pub use settings::{BrowserSettings, DEFAULT_HOMEPAGE};
pub use tab::{ClosedTab, Tab, TabId, TabSet};
pub use url_input::{NavigationTarget, resolve_input};
