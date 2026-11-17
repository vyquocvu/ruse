//! Browser theme tokens.
//!
//! The theme is stored as a GPUI global so nested components can read it
//! without threading colors through every constructor.

use gpui::{App, Global, Rgba, WindowAppearance, rgb};

/// Design tokens for the browser chrome.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// Window background behind everything.
    pub background: Rgba,
    /// Tab strip / toolbar background.
    pub chrome: Rgba,
    /// Raised surfaces (menus, dialogs, new-tab cards).
    pub surface: Rgba,
    /// Hovered surface.
    pub surface_hover: Rgba,
    /// Active/pressed surface.
    pub surface_active: Rgba,
    /// Hairline separators.
    pub border: Rgba,
    /// Primary text.
    pub text: Rgba,
    /// Secondary and placeholder text.
    pub text_muted: Rgba,
    /// Accent color for selection, focus rings and active indicators.
    pub accent: Rgba,
    /// Text drawn on the accent color.
    pub accent_text: Rgba,
    /// Destructive actions and errors.
    pub danger: Rgba,
    /// Address bar background.
    pub address_background: Rgba,
    /// Active tab background.
    pub tab_active: Rgba,
    /// Inactive tab background.
    pub tab_inactive: Rgba,
    /// Text selection highlight.
    pub selection: Rgba,
}

impl Global for Theme {}

impl Theme {
    pub fn dark() -> Self {
        Self {
            dark: true,
            background: rgb(0x0d1117),
            chrome: rgb(0x151b22),
            surface: rgb(0x1b232c),
            surface_hover: rgb(0x232d38),
            surface_active: rgb(0x2a3541),
            border: rgb(0x28323d),
            text: rgb(0xe6edf3),
            text_muted: rgb(0x8a97a5),
            accent: rgb(0x5b9cf8),
            accent_text: rgb(0x0b1016),
            danger: rgb(0xf07178),
            address_background: rgb(0x10161d),
            tab_active: rgb(0x1e2731),
            tab_inactive: rgb(0x151b22),
            selection: gpui::rgba(0x5b9cf84d),
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            background: rgb(0xf7f8fa),
            chrome: rgb(0xffffff),
            surface: rgb(0xffffff),
            surface_hover: rgb(0xeef1f5),
            surface_active: rgb(0xe3e8ef),
            border: rgb(0xdfe4ea),
            text: rgb(0x1a2129),
            text_muted: rgb(0x66717e),
            accent: rgb(0x2563eb),
            accent_text: rgb(0xffffff),
            danger: rgb(0xd1434b),
            address_background: rgb(0xf1f3f6),
            tab_active: rgb(0xffffff),
            tab_inactive: rgb(0xe9edf2),
            selection: gpui::rgba(0x2563eb33),
        }
    }

    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    /// Returns the installed theme.
    pub fn get(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }

    /// Installs the default (dark) theme if none is installed.
    pub fn install_default(cx: &mut App) {
        if !cx.has_global::<Theme>() {
            cx.set_global(Self::dark());
        }
    }

    pub fn install(cx: &mut App, theme: Theme) {
        cx.set_global(theme);
    }
}
