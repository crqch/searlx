//! One palette for every surface.
//!
//! Light, dark, and system themes share the same semantic names. The page is
//! never tinted by a space colour; only the chrome is.

use gpui::{Hsla, Rgba, hsla, rgb};

/// Semantic colours used by the chrome.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Window background around the page card.
    pub ground: Hsla,
    /// Tab column / strip surface.
    pub chrome: Hsla,
    /// Hovered chrome.
    pub hover: Hsla,
    /// Selected tab surface.
    pub selected: Hsla,
    /// Primary text.
    pub text: Hsla,
    /// Secondary text and hints.
    pub muted: Hsla,
    /// Hairlines.
    pub line: Hsla,
    /// The page placeholder surface.
    pub page: Hsla,
    /// Accent (focus rings, live pill).
    pub accent: Hsla,
    /// Destructive actions.
    pub danger: Hsla,
}

impl Palette {
    /// The light palette.
    #[must_use]
    pub fn light() -> Self {
        Self {
            ground: rgb(0xE9E8E6).into(),
            chrome: rgb(0xF4F3F1).into(),
            hover: rgb(0xE4E2DF).into(),
            selected: rgb(0xFFFFFF).into(),
            text: rgb(0x1C1B1A).into(),
            muted: rgb(0x74716D).into(),
            line: rgb(0xD5D2CE).into(),
            page: rgb(0xFFFFFF).into(),
            accent: rgb(0x3B74F2).into(),
            danger: rgb(0xC0392B).into(),
        }
    }

    /// The dark palette.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            ground: rgb(0x171717).into(),
            chrome: rgb(0x202020).into(),
            hover: rgb(0x2A2A2A).into(),
            selected: rgb(0x323232).into(),
            text: rgb(0xEDEBE8).into(),
            muted: rgb(0x9A9691).into(),
            line: rgb(0x3A3836).into(),
            page: rgb(0x121212).into(),
            accent: rgb(0x6C9BFF).into(),
            danger: rgb(0xE06C5F).into(),
        }
    }

    /// Resolve a theme preference against the OS appearance.
    #[must_use]
    pub fn for_theme(theme: Theme, system_dark: bool) -> Self {
        match theme {
            Theme::Light => Self::light(),
            Theme::Dark => Self::dark(),
            Theme::System if system_dark => Self::dark(),
            Theme::System => Self::light(),
        }
    }
}

/// The user's appearance preference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    /// Follow the operating system.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

impl Theme {
    /// Whether this preference means dark for the given system appearance.
    #[must_use]
    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Self::Light => false,
            Self::Dark => true,
            Self::System => system_dark,
        }
    }
}

/// A space tint, kept away from the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SpaceTint {
    /// No tint.
    #[default]
    None,
    /// Blue.
    Blue,
    /// Green.
    Green,
    /// Orange.
    Orange,
    /// Red.
    Red,
    /// Violet.
    Violet,
}

impl SpaceTint {
    /// A low-opacity wash for the chrome.
    #[must_use]
    pub fn wash(self, dark: bool) -> Option<Rgba> {
        let (light, night) = match self {
            Self::None => return None,
            Self::Blue => (0xDCE8FF, 0x1B2A44),
            Self::Green => (0xDDF2E2, 0x183325),
            Self::Orange => (0xFBE9D6, 0x3B2A16),
            Self::Red => (0xFADDD9, 0x3C1D1B),
            Self::Violet => (0xEADFFA, 0x2D1F3E),
        };
        Some(rgb(if dark { night } else { light }))
    }
}

/// A focus ring colour for a focused control.
#[must_use]
pub fn focus_ring(palette: &Palette) -> Hsla {
    hsla(palette.accent.h, palette.accent.s, palette.accent.l, 0.55)
}
