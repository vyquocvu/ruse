//! Browser-wide actions and their default key bindings.

use gpui::{App, KeyBinding, Menu, MenuItem, SystemMenuType, actions};

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
        NewPrivateWindow,
        CloseTab,
        ReopenClosedTab,
        DuplicateTab,
        ToggleHistory,
        ToggleBookmark,
        ToggleBookmarks,
        FindNext,
        FindPrevious,
        ToggleDownloads,
        CopyUrl,
        CommandPalette,
        PaletteNext,
        PalettePrevious,
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

/// Installs the native application menu bar.
///
/// Shortcut labels come from the key bindings registered with `bind_browser_keys`.
pub fn install_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "GPUI Browser".into(),
            items: vec![
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Quit GPUI Browser", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("New Tab", NewTab),
                MenuItem::action("New Private Window", NewPrivateWindow),
                MenuItem::separator(),
                MenuItem::action("Close Tab", CloseTab),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Reload", Reload),
                MenuItem::action("Reload From Origin", HardReload),
                MenuItem::separator(),
                MenuItem::action("Zoom In", ZoomIn),
                MenuItem::action("Zoom Out", ZoomOut),
                MenuItem::action("Actual Size", ZoomReset),
                MenuItem::separator(),
                MenuItem::action("Find in Page", FindInPage),
            ],
        },
        Menu {
            name: "History".into(),
            items: vec![
                MenuItem::action("Show History", ToggleHistory),
                MenuItem::action("Show Downloads", ToggleDownloads),
            ],
        },
        Menu {
            name: "Bookmarks".into(),
            items: vec![
                MenuItem::action("Bookmark This Page", ToggleBookmark),
                MenuItem::action("Show Bookmarks", ToggleBookmarks),
            ],
        },
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

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
        KeyBinding::new("cmd-shift-n", NewPrivateWindow, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-t", ReopenClosedTab, None),
        KeyBinding::new("cmd-shift-d", DuplicateTab, None),
        KeyBinding::new("cmd-y", ToggleHistory, None),
        KeyBinding::new("cmd-d", ToggleBookmark, None),
        KeyBinding::new("cmd-shift-b", ToggleBookmarks, None),
        KeyBinding::new("cmd-g", FindNext, None),
        KeyBinding::new("cmd-shift-g", FindPrevious, None),
        KeyBinding::new("cmd-shift-j", ToggleDownloads, None),
        KeyBinding::new("cmd-shift-c", CopyUrl, None),
        KeyBinding::new("cmd-k", CommandPalette, None),
        KeyBinding::new("up", PaletteNext, Some("CommandPalette")),
        KeyBinding::new("down", PalettePrevious, Some("CommandPalette")),
        KeyBinding::new("ctrl-p", PaletteNext, Some("CommandPalette")),
        KeyBinding::new("ctrl-n", PalettePrevious, Some("CommandPalette")),
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
