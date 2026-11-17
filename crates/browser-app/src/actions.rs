//! Browser-wide actions and their default key bindings.

use gpui::{App, KeyBinding, actions};

actions!(
    browser,
    [
        FocusLocation,
        Back,
        Forward,
        Reload,
        HardReload,
        Stop,
        NewTab,
        CloseTab,
        ReopenClosedTab,
        NextTab,
        PreviousTab,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        FindInPage,
        OpenSettings,
        Quit,
    ]
);

/// Registers browser shortcuts.
///
/// While the web view is focused these never reach GPUI; `browser-platform`'s
/// local NSEvent monitor forwards them instead. These bindings cover chrome
/// focus.
pub fn bind_browser_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-l", FocusLocation, None),
        KeyBinding::new("cmd-[", Back, None),
        KeyBinding::new("cmd-]", Forward, None),
        KeyBinding::new("cmd-r", Reload, None),
        KeyBinding::new("cmd-shift-r", HardReload, None),
        KeyBinding::new("escape", Stop, None),
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-t", ReopenClosedTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
        KeyBinding::new("cmd-=", ZoomIn, None),
        KeyBinding::new("cmd--", ZoomOut, None),
        KeyBinding::new("cmd-0", ZoomReset, None),
        KeyBinding::new("cmd-f", FindInPage, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
}
