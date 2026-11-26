//! One palette for every surface.
//!
//! Light, dark, and system themes share the same semantic names. The page is
//! never tinted by a space colour; only the chrome is.

use gpui::{Hsla, Rgba, hsla, rgb};
use searlx_core::{LookPreference, Settings, SpaceColor, WashStrength};

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

    /// Resolve a theme preference with optional high-contrast strengthening
    /// (`tabs.highContrast`, `FEATURES.md` §21, P1-10).
    #[must_use]
    pub fn for_theme_with_contrast(theme: Theme, system_dark: bool, high_contrast: bool) -> Self {
        let mut palette = Self::for_theme(theme, system_dark);
        if high_contrast {
            if theme.is_dark(system_dark) {
                palette.text = rgb(0xFFFFFF).into();
                palette.muted = rgb(0xD4D0CB).into();
                palette.line = rgb(0x605C58).into();
            } else {
                palette.text = rgb(0x000000).into();
                palette.muted = rgb(0x3D3A37).into();
                palette.line = rgb(0x8C8883).into();
            }
        }
        palette
    }

    /// Resolve palette directly from `searlx_core::Settings` (`look` and `tabs.highContrast`).
    #[must_use]
    pub fn for_settings(settings: &Settings, system_dark: bool) -> Self {
        Self::for_theme_with_contrast(
            Theme::from_look(settings.look),
            system_dark,
            settings.tabs_high_contrast,
        )
    }

    /// Whether the effective chrome appearance is dark under `settings`.
    #[must_use]
    pub fn is_dark(settings: &Settings, system_dark: bool) -> bool {
        Theme::from_look(settings.look).is_dark(system_dark)
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
    /// Map `searlx_core::LookPreference` to UI [`Theme`].
    #[must_use]
    pub const fn from_look(look: LookPreference) -> Self {
        match look {
            LookPreference::System => Self::System,
            LookPreference::Light => Self::Light,
            LookPreference::Dark => Self::Dark,
        }
    }

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
    /// Map `searlx_core::SpaceColor` to [`SpaceTint`].
    #[must_use]
    pub const fn from_space_color(color: SpaceColor) -> Self {
        match color {
            SpaceColor::None => Self::None,
            SpaceColor::Blue => Self::Blue,
            SpaceColor::Green => Self::Green,
            SpaceColor::Orange => Self::Orange,
            SpaceColor::Red => Self::Red,
            SpaceColor::Violet => Self::Violet,
        }
    }

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

    /// Accent dot color for spaces and tab-group indicators.
    #[must_use]
    pub fn dot_color(self, dark: bool) -> Option<Rgba> {
        let (light, night) = match self {
            Self::None => return None,
            Self::Blue => (0x3B74F2, 0x6C9BFF),
            Self::Green => (0x2D8F5B, 0x4FC185),
            Self::Orange => (0xD47221, 0xF09B4D),
            Self::Red => (0xC0392B, 0xE06C5F),
            Self::Violet => (0x7A4CE0, 0xA884FF),
        };
        Some(rgb(if dark { night } else { light }))
    }

    /// Blend the space tint over `base` chrome surface using `strength` (`FEATURES.md` §6.1).
    /// The page surface itself is never tinted.
    #[must_use]
    pub fn wash_over(self, base: Hsla, dark: bool, strength: WashStrength) -> Hsla {
        let Some(tint) = self.dot_color(dark) else {
            return base;
        };
        let (light_alpha, dark_alpha) = strength.opacity_pair();
        let alpha = if dark { dark_alpha } else { light_alpha };
        let base_rgba = Rgba::from(base);
        Rgba {
            r: base_rgba.r * (1.0 - alpha) + tint.r * alpha,
            g: base_rgba.g * (1.0 - alpha) + tint.g * alpha,
            b: base_rgba.b * (1.0 - alpha) + tint.b * alpha,
            a: base_rgba.a,
        }
        .into()
    }
}

/// A focus ring colour for a focused control.
#[must_use]
pub fn focus_ring(palette: &Palette) -> Hsla {
    hsla(palette.accent.h, palette.accent.s, palette.accent.l, 0.55)
}

/// Compute a tab's single-letter glyph from its host or title (`FEATURES.md` §3.1, P1-10).
#[must_use]
pub fn tab_glyph_letter(title: &str, host: Option<&str>) -> char {
    host.and_then(|h| h.trim_start_matches("www.").chars().next())
        .or_else(|| title.trim().chars().next())
        .map_or('·', |c| c.to_uppercase().next().unwrap_or(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_contrast_strengthens_text_and_muted_colors() {
        let normal_light = Palette::for_theme_with_contrast(Theme::Light, false, false);
        let hc_light = Palette::for_theme_with_contrast(Theme::Light, false, true);
        assert_ne!(normal_light.text, hc_light.text);
        assert_ne!(normal_light.muted, hc_light.muted);

        let normal_dark = Palette::for_theme_with_contrast(Theme::Dark, true, false);
        let hc_dark = Palette::for_theme_with_contrast(Theme::Dark, true, true);
        assert_ne!(normal_dark.text, hc_dark.text);
        assert_ne!(normal_dark.muted, hc_dark.muted);
    }

    #[test]
    fn settings_drive_theme_contrast_and_space_wash() {
        let mut settings = Settings {
            look: LookPreference::Dark,
            tabs_high_contrast: true,
            ..Settings::default()
        };
        assert!(Palette::is_dark(&settings, false));
        let pal = Palette::for_settings(&settings, false);
        assert_eq!(
            pal,
            Palette::for_theme_with_contrast(Theme::Dark, false, true)
        );

        settings.look = LookPreference::Light;
        settings.tabs_high_contrast = false;
        assert!(!Palette::is_dark(&settings, true));
        assert_eq!(Palette::for_settings(&settings, true), Palette::light());

        let tint = SpaceTint::from_space_color(SpaceColor::Blue);
        assert_eq!(tint, SpaceTint::Blue);
        assert_ne!(
            tint.wash_over(pal.chrome, false, WashStrength::Rich),
            pal.chrome
        );
        assert_eq!(
            SpaceTint::from_space_color(SpaceColor::None).wash_over(
                pal.chrome,
                false,
                WashStrength::Vivid
            ),
            pal.chrome
        );
    }

    #[test]
    fn tab_glyph_prefers_clean_host_initial() {
        assert_eq!(tab_glyph_letter("Example", Some("www.github.com")), 'G');
        assert_eq!(tab_glyph_letter("Settings", None), 'S');
        assert_eq!(tab_glyph_letter("", None), '·');
    }
}
