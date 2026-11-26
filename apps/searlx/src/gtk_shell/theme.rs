//! Search-faithful palette, layout metrics, theme resolution, and GTK3 CSS (`FEATURES.md` §1-§2, §6.1, §21).

use std::sync::OnceLock;

use gtk::prelude::{ObjectExt, WidgetExt};
use searlx_core::{
    LookPreference, Settings, SpaceColor, SpaceGradientPreset, TabLayoutMode, WashStrength,
};

static INITIAL_SYSTEM_DARK: OnceLock<bool> = OnceLock::new();

/// Show `widget` and all of its children even when `widget` has `no_show_all` set.
/// Inner children that have their own `no_show_all` flag still keep their guard.
pub fn reveal_widget(widget: &impl WidgetExt) {
    let guarded = widget.is_no_show_all();
    if guarded {
        widget.set_no_show_all(false);
    }
    widget.show_all();
    if guarded {
        widget.set_no_show_all(true);
    }
}

/// Default window width in logical points (`FEATURES.md` §2.1).
pub const WINDOW_DEFAULT_WIDTH: i32 = 1180;
/// Default window height in logical points (`FEATURES.md` §2.1).
pub const WINDOW_DEFAULT_HEIGHT: i32 = 780;
/// Minimum window width in logical points (`FEATURES.md` §2.1).
pub const WINDOW_MIN_WIDTH: i32 = 640;
/// Minimum window height in logical points (`FEATURES.md` §2.1).
pub const WINDOW_MIN_HEIGHT: i32 = 420;

/// Resizable sidebar default width in points (`FEATURES.md` §2.2).
pub const SIDEBAR_DEFAULT: i32 = 232;
/// Resizable sidebar minimum width in points (`FEATURES.md` §2.2).
pub const SIDEBAR_MIN: i32 = 176;
/// Resizable sidebar maximum width in points (`FEATURES.md` §2.2).
pub const SIDEBAR_MAX: i32 = 440;
/// Icons-only column width in points (`FEATURES.md` §2.2).
pub const SIDEBAR_ICONS: i32 = 48;

/// Edge-peek trigger zone width when sidebar is folded (`FEATURES.md` §2.2).
pub const FOLDED_EDGE_ZONE_PX: i32 = 6;
/// Hover dwell before a permanently hidden sidebar peeks out (`FEATURES.md` §2.2).
pub const FOLDED_DWELL_MS: u64 = 150;
/// Grace period before folded sidebar retracts (`FEATURES.md` §2.2).
pub const FOLDED_GRACE_MS: u64 = 65;

/// Inset around the page card (`FEATURES.md` §2.1).
pub const PAGE_GAP: i32 = 8;
/// Corner radius of the page card (`FEATURES.md` §2.1).
pub const PAGE_RADIUS: i32 = 10;

/// Floating overlay address bar width in points.
pub const FIELD_WIDTH: i32 = 620;
/// Compact inline address bar width when `address.width = Small` (`FEATURES.md` §2.1).
pub const ADDRESS_WIDTH_SMALL: i32 = 220;
/// Compact inline address bar width when `address.width = Medium` (`FEATURES.md` §2.1).
pub const ADDRESS_WIDTH_MEDIUM: i32 = 340;

/// Gutter between split panes (`FEATURES.md` §3.7).
pub const SPLIT_GUTTER: i32 = 7;

/// Side panel minimum width (`FEATURES.md` §3.8).
pub const SIDE_PANEL_MIN: i32 = 300;
/// Side panel maximum width (`FEATURES.md` §3.8).
pub const SIDE_PANEL_MAX: i32 = 640;

/// Resolved semantic color palette for GTK3 chrome and `search://` pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellPalette {
    pub dark: bool,
    pub high_contrast: bool,
    pub ground: &'static str,
    pub chrome: &'static str,
    pub hover: &'static str,
    pub selected: &'static str,
    pub text: &'static str,
    pub muted: &'static str,
    pub line: &'static str,
    pub page: &'static str,
    pub accent: &'static str,
    pub accent_soft: &'static str,
    pub danger: &'static str,
    pub warning: &'static str,
    pub secure: &'static str,
    pub card_shadow: &'static str,
}

impl ShellPalette {
    /// Resolve palette from `Settings` and live OS dark preference.
    #[must_use]
    pub fn resolve(settings: &Settings, system_dark: bool) -> Self {
        let dark = match settings.look {
            LookPreference::Light => false,
            LookPreference::Dark => true,
            LookPreference::System => system_dark,
        };
        let high_contrast = settings.tabs_high_contrast;
        match (dark, high_contrast) {
            (false, false) => Self {
                dark: false,
                high_contrast: false,
                ground: "#E9E8E6",
                chrome: "#F4F3F1",
                hover: "#E4E2DF",
                selected: "#FFFFFF",
                text: "#1C1B1A",
                muted: "#74716D",
                line: "#D5D2CE",
                page: "#FFFFFF",
                accent: "#3B74F2",
                accent_soft: "rgba(59, 116, 242, 0.14)",
                danger: "#C0392B",
                warning: "#B26B00",
                secure: "#1E8E5A",
                card_shadow: "rgba(0, 0, 0, 0.14)",
            },
            (false, true) => Self {
                dark: false,
                high_contrast: true,
                ground: "#DFDDD9",
                chrome: "#F4F3F1",
                hover: "#D8D5D0",
                selected: "#FFFFFF",
                text: "#000000",
                muted: "#3D3A37",
                line: "#8C8883",
                page: "#FFFFFF",
                accent: "#1F55D8",
                accent_soft: "rgba(31, 85, 216, 0.18)",
                danger: "#9E1F13",
                warning: "#8C5200",
                secure: "#116B41",
                card_shadow: "rgba(0, 0, 0, 0.24)",
            },
            (true, false) => Self {
                dark: true,
                high_contrast: false,
                ground: "#171717",
                chrome: "#202020",
                hover: "#2A2A2A",
                selected: "#323232",
                text: "#EDEBE8",
                muted: "#9A9691",
                line: "#3A3836",
                page: "#121212",
                accent: "#6C9BFF",
                accent_soft: "rgba(108, 155, 255, 0.18)",
                danger: "#E06C5F",
                warning: "#E5A93C",
                secure: "#48C78E",
                card_shadow: "rgba(0, 0, 0, 0.45)",
            },
            (true, true) => Self {
                dark: true,
                high_contrast: true,
                ground: "#101010",
                chrome: "#1A1A1A",
                hover: "#2C2C2C",
                selected: "#363636",
                text: "#FFFFFF",
                muted: "#D4D0CB",
                line: "#605C58",
                page: "#0D0D0D",
                accent: "#88B1FF",
                accent_soft: "rgba(136, 177, 255, 0.24)",
                danger: "#FF8578",
                warning: "#FFC454",
                secure: "#63E2A7",
                card_shadow: "rgba(0, 0, 0, 0.65)",
            },
        }
    }
}

/// Space chrome tint descriptor passed to CSS generation (never applied to the page card).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpaceChromeTint {
    pub color: SpaceColor,
    pub gradient: Option<SpaceGradientPreset>,
    pub strength: WashStrength,
}

impl SpaceChromeTint {
    /// Build GTK3 CSS `background-color` and `background-image` properties for chrome only.
    #[must_use]
    pub fn chrome_background_css(self, palette: &ShellPalette) -> String {
        let (light_alpha, dark_alpha) = self.strength.opacity_pair();
        let alpha = if palette.dark {
            dark_alpha
        } else {
            light_alpha
        };
        if let Some(grad) = self.gradient {
            let (c1, c2) = gradient_rgb(grad, palette.dark);
            return format!(
                "background-color: {}; background-image: linear-gradient(165deg, rgba({},{},{},{alpha:.2}), rgba({},{},{},{alpha:.2}));",
                palette.chrome, c1.0, c1.1, c1.2, c2.0, c2.1, c2.2
            );
        }
        match space_color_rgb(self.color, palette.dark) {
            Some((r, g, b)) => format!(
                "background-color: {}; background-image: linear-gradient(180deg, rgba({r},{g},{b},{alpha:.2}), rgba({r},{g},{b},{alpha:.2}));",
                palette.chrome
            ),
            None => format!("background-color: {};", palette.chrome),
        }
    }
}

const fn space_color_rgb(color: SpaceColor, dark: bool) -> Option<(u8, u8, u8)> {
    match (color, dark) {
        (SpaceColor::None, _) => None,
        (SpaceColor::Blue, false) => Some((0x3B, 0x74, 0xF2)),
        (SpaceColor::Blue, true) => Some((0x4D, 0x86, 0xFF)),
        (SpaceColor::Green, false) => Some((0x27, 0x99, 0x5B)),
        (SpaceColor::Green, true) => Some((0x3A, 0xB8, 0x74)),
        (SpaceColor::Orange, false) => Some((0xD9, 0x77, 0x24)),
        (SpaceColor::Orange, true) => Some((0xED, 0x93, 0x3D)),
        (SpaceColor::Red, false) => Some((0xC8, 0x43, 0x3B)),
        (SpaceColor::Red, true) => Some((0xE5, 0x67, 0x5F)),
        (SpaceColor::Violet, false) => Some((0x7D, 0x52, 0xD6)),
        (SpaceColor::Violet, true) => Some((0x9C, 0x76, 0xF2)),
    }
}

const fn gradient_rgb(preset: SpaceGradientPreset, dark: bool) -> ((u8, u8, u8), (u8, u8, u8)) {
    match (preset, dark) {
        (SpaceGradientPreset::Sunrise, false) => ((245, 162, 114), (235, 118, 148)),
        (SpaceGradientPreset::Sunrise, true) => ((180, 96, 62), (168, 66, 98)),
        (SpaceGradientPreset::Lagoon, false) => ((74, 173, 212), (76, 122, 232)),
        (SpaceGradientPreset::Lagoon, true) => ((44, 126, 166), (48, 86, 184)),
        (SpaceGradientPreset::Orchard, false) => ((118, 186, 124), (206, 178, 92)),
        (SpaceGradientPreset::Orchard, true) => ((64, 134, 78), (154, 124, 52)),
        (SpaceGradientPreset::Dusk, false) => ((120, 108, 224), (204, 114, 184)),
        (SpaceGradientPreset::Dusk, true) => ((84, 72, 168), (144, 66, 128)),
        (SpaceGradientPreset::Glacier, false) => ((126, 198, 235), (164, 184, 240)),
        (SpaceGradientPreset::Glacier, true) => ((64, 124, 168), (88, 108, 172)),
        (SpaceGradientPreset::Ember, false) => ((224, 96, 84), (232, 158, 82)),
        (SpaceGradientPreset::Ember, true) => ((168, 56, 48), (176, 108, 44)),
    }
}

/// Resolve motion duration in milliseconds, collapsing to `0` when `reduce_motion` is active.
#[must_use]
pub const fn motion_duration_ms(base_ms: u64, reduce_motion: bool) -> u64 {
    if reduce_motion { 0 } else { base_ms }
}

/// Scale a point font size using `Settings::text_scale` (`0.75..=2.5`).
#[must_use]
pub fn scaled_font_px(base_pt: f32, text_scale: f32) -> i32 {
    let clamped = if text_scale.is_finite() {
        text_scale.clamp(0.75, 2.5)
    } else {
        1.0
    };
    (base_pt * clamped).round().max(9.0) as i32
}

/// Detect Linux desktop dark appearance preference (captured once at startup before app dark overrides).
#[must_use]
pub fn detect_system_dark() -> bool {
    *INITIAL_SYSTEM_DARK.get_or_init(|| {
        if let Ok(theme_env) = std::env::var("GTK_THEME")
            && theme_env.to_ascii_lowercase().contains("dark")
        {
            return true;
        }
        if let Some(gtk_settings) = gtk::Settings::default() {
            if ObjectExt::property::<bool>(&gtk_settings, "gtk-application-prefer-dark-theme") {
                return true;
            }
            if let Some(theme_name) =
                ObjectExt::property::<Option<String>>(&gtk_settings, "gtk-theme-name")
                && theme_name.to_ascii_lowercase().contains("dark")
            {
                return true;
            }
        }
        false
    })
}

/// Apply GTK's native light/dark preference so menus, dialogs, combo popups, and scrollbars match.
pub fn apply_gtk_dark_preference(dark: bool) {
    let _ = detect_system_dark();
    if let Some(gtk_settings) = gtk::Settings::default() {
        ObjectExt::set_property(&gtk_settings, "gtk-application-prefer-dark-theme", dark);
    }
}

/// Return CSS hex color for a named tab-group / space color badge.
#[must_use]
pub const fn space_badge_hex(color: SpaceColor, dark: bool) -> &'static str {
    match (color, dark) {
        (SpaceColor::None, false) => "#74716D",
        (SpaceColor::None, true) => "#9A9691",
        (SpaceColor::Blue, false) => "#3B74F2",
        (SpaceColor::Blue, true) => "#6C9BFF",
        (SpaceColor::Green, false) => "#1E8E5A",
        (SpaceColor::Green, true) => "#48C78E",
        (SpaceColor::Orange, false) => "#C76B14",
        (SpaceColor::Orange, true) => "#F09B45",
        (SpaceColor::Red, false) => "#C0392B",
        (SpaceColor::Red, true) => "#E06C5F",
        (SpaceColor::Violet, false) => "#764BC4",
        (SpaceColor::Violet, true) => "#A782EC",
    }
}

/// Build the complete GTK3 CSS stylesheet for the Search Linux shell.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn build_gtk_css(
    palette: &ShellPalette,
    settings: &Settings,
    space_tint: Option<SpaceChromeTint>,
) -> String {
    let chrome_bg = space_tint.map_or_else(
        || format!("background-color: {};", palette.chrome),
        |t| t.chrome_background_css(palette),
    );
    let body_px = scaled_font_px(
        if settings.sidebar_roomy { 14.0 } else { 13.0 },
        settings.text_scale,
    );
    let small_px = scaled_font_px(11.0, settings.text_scale);
    let title_px = scaled_font_px(15.0, settings.text_scale);
    let row_height = if settings.sidebar_roomy { 38 } else { 30 };
    let transition_ms = motion_duration_ms(140, settings.reduce_motion);
    let card_radius = if settings.layout_cards {
        PAGE_RADIUS
    } else {
        0
    };
    let card_margin = if settings.layout_cards { PAGE_GAP } else { 0 };
    let card_border = if settings.layout_cards {
        format!("1px solid {}", palette.line)
    } else {
        "none".to_owned()
    };
    let top_strip_height = if settings.tabs_compact { 34 } else { 40 };

    let sidebar_border_side = match settings.tab_layout {
        TabLayoutMode::RightColumn => "border-left",
        _ => "border-right",
    };

    format!(
        r#"
window.searlx-window, .searlx-ground {{
    background-color: {ground};
    color: {text};
    font-size: {body_px}px;
}}

.searlx-sidebar {{
    {chrome_bg}
    {sidebar_border_side}: 1px solid {line};
    padding: 8px 8px 8px 8px;
}}

.searlx-sidebar.folded-peek {{
    border-radius: 10px;
    border: 1px solid {line};
    box-shadow: 0 12px 32px {shadow};
    margin: 8px;
}}

.searlx-topstrip {{
    {chrome_bg}
    border-bottom: 1px solid {line};
    min-height: {top_strip_height}px;
    padding: 4px 10px;
}}

.searlx-nav-btn {{
    background-color: transparent;
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: none;
    border-radius: 7px;
    color: {muted};
    min-width: 26px;
    min-height: 26px;
    padding: 2px 6px;
    transition: background-color {transition_ms}ms ease, color {transition_ms}ms ease;
}}
.searlx-nav-btn:hover {{
    background-color: {hover};
    background-image: none;
    color: {text};
}}
.searlx-nav-btn:disabled {{
    opacity: 0.38;
}}

.searlx-site-chip {{
    background-color: {hover};
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: 1px solid {line};
    border-radius: 8px;
    color: {text};
    padding: 4px 10px;
    font-size: {small_px}px;
}}
.searlx-site-chip:hover {{
    background-color: {selected};
    background-image: none;
    border-color: {accent};
}}
.searlx-site-chip.insecure {{
    border-color: {warning};
    color: {warning};
}}

.searlx-inline-address {{
    background-color: {selected};
    background-image: none;
    box-shadow: none;
    color: {text};
    border: 1px solid {line};
    border-radius: 8px;
    padding: 4px 8px;
    min-height: 28px;
    font-size: {small_px}px;
}}
.searlx-inline-address:focus {{
    border-color: {accent};
}}

.searlx-section-label {{
    color: {muted};
    font-size: {small_px}px;
    font-weight: 600;
    padding: 4px 6px 2px 6px;
}}

.searlx-pin-tile {{
    background-color: {hover};
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: 1px solid transparent;
    border-radius: 8px;
    min-height: 34px;
    padding: 4px;
    color: {text};
    font-weight: 600;
    transition: background-color {transition_ms}ms ease, border-color {transition_ms}ms ease;
}}
.searlx-pin-tile:hover {{
    background-color: {selected};
    background-image: none;
    border-color: {line};
}}
.searlx-pin-tile.active {{
    background-color: {selected};
    background-image: none;
    border-color: {accent};
    box-shadow: 0 1px 3px {shadow};
}}
.searlx-pin-tile.sleeping {{
    opacity: 0.58;
}}

.searlx-group-header {{
    background-color: transparent;
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: none;
    border-radius: 7px;
    color: {muted};
    font-size: {small_px}px;
    font-weight: 600;
    padding: 4px 6px;
}}
.searlx-group-header:hover {{
    background-color: {hover};
    background-image: none;
    color: {text};
}}

.searlx-tab-row {{
    background-color: transparent;
    background-image: none;
    box-shadow: none;
    border: 1px solid transparent;
    border-radius: 8px;
    min-height: {row_height}px;
    padding: 2px 6px;
    color: {text};
    transition: background-color {transition_ms}ms ease;
}}
.searlx-tab-row:hover {{
    background-color: {hover};
    background-image: none;
}}
.searlx-tab-row.active {{
    background-color: {selected};
    background-image: none;
    border-color: {line};
    box-shadow: 0 1px 3px {shadow};
}}
.searlx-tab-row.sleeping .searlx-tab-title {{
    color: {muted};
    opacity: 0.68;
}}
.searlx-tab-row.put-away .searlx-tab-title {{
    color: {muted};
    font-style: italic;
}}
.searlx-tab-row.drag-over {{
    border-top: 2px solid {accent};
}}

.searlx-tab-glyph {{
    background-color: {hover};
    border-radius: 5px;
    min-width: 20px;
    min-height: 20px;
    color: {text};
    font-size: {small_px}px;
    font-weight: 700;
    padding: 1px 4px;
}}
.searlx-tab-row.active .searlx-tab-glyph {{
    background-color: {accent_soft};
    color: {accent};
}}

.searlx-tab-title {{
    color: {text};
    font-size: {body_px}px;
}}
.searlx-tab-row.active .searlx-tab-title {{
    font-weight: 600;
}}

.searlx-tab-badge {{
    background-color: {accent_soft};
    color: {accent};
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 4px;
}}
.searlx-private-badge {{
    background-color: rgba(167, 130, 236, 0.22);
    color: {text};
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 5px;
}}
.searlx-crash-badge {{
    background-color: rgba(224, 108, 95, 0.22);
    color: {danger};
    border-radius: 4px;
    font-size: 10px;
    font-weight: 700;
    padding: 1px 5px;
}}

.searlx-tab-close {{
    background-color: transparent;
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: none;
    border-radius: 10px;
    min-width: 18px;
    min-height: 18px;
    padding: 0 3px;
    color: {muted};
    font-size: 11px;
}}
.searlx-tab-close:hover {{
    background-color: {line};
    background-image: none;
    color: {text};
}}

.searlx-reading-bar trough {{
    min-height: 2px;
    background: transparent;
    border: none;
}}
.searlx-reading-bar progress {{
    min-height: 2px;
    background-color: {accent};
    border-radius: 1px;
}}

.searlx-page-card {{
    background-color: {page};
    border-radius: {card_radius}px;
    border: {card_border};
    margin: {card_margin}px;
}}

.searlx-split-pane {{
    border: 2px solid transparent;
    border-radius: 6px;
}}
.searlx-split-pane.focused {{
    border-color: {accent};
}}
.searlx-split-pane-bar {{
    background-color: {chrome};
    border-bottom: 1px solid {line};
    padding: 2px 8px;
    font-size: {small_px}px;
    color: {muted};
}}

.searlx-split-divider {{
    background-color: {ground};
    min-width: {SPLIT_GUTTER}px;
}}
.searlx-split-divider-handle {{
    background-color: {line};
    border-radius: 2px;
    min-width: 4px;
    min-height: 36px;
}}
.searlx-split-divider:hover .searlx-split-divider-handle {{
    background-color: {accent};
}}

.searlx-split-drop-band {{
    background-color: {accent_soft};
    border: 2px dashed {accent};
    border-radius: 10px;
    margin: 12px;
}}
.searlx-split-drop-label {{
    color: {accent};
    font-size: {title_px}px;
    font-weight: 700;
}}

.searlx-bookmarks-bar {{
    background-color: {chrome};
    border-bottom: 1px solid {line};
    padding: 3px 10px;
}}
.searlx-bookmark-chip {{
    background-color: transparent;
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: none;
    border-radius: 6px;
    color: {text};
    font-size: {small_px}px;
    padding: 2px 8px;
}}
.searlx-bookmark-chip:hover {{
    background-color: {hover};
    background-image: none;
}}

.searlx-side-panel {{
    background-color: {chrome};
    border: 1px solid {line};
    border-radius: {card_radius}px;
    margin: {card_margin}px;
}}
.searlx-side-panel-header {{
    border-bottom: 1px solid {line};
    padding: 6px 10px;
    font-size: {small_px}px;
}}

.searlx-overlay-scrim {{
    background-color: rgba(0, 0, 0, 0.30);
}}

.searlx-overlay-card {{
    background-color: {chrome};
    border: 1px solid {line};
    border-radius: 12px;
    padding: 12px 14px;
    box-shadow: 0 18px 46px {shadow};
}}
.searlx-overlay-card.shake {{
    border: 2px solid {danger};
    margin-left: 10px;
}}

.searlx-overlay-entry {{
    background-color: {selected};
    background-image: none;
    box-shadow: none;
    color: {text};
    border: 1px solid {line};
    border-radius: 8px;
    min-height: 38px;
    padding: 4px 12px;
    font-size: {title_px}px;
}}
.searlx-overlay-entry:focus {{
    border-color: {accent};
}}

.searlx-completion-hint {{
    color: {muted};
    font-size: {small_px}px;
    padding: 2px 4px;
}}
.searlx-shake-label {{
    color: {danger};
    font-size: {small_px}px;
    font-weight: 600;
}}

.searlx-suggestion-row {{
    border-radius: 7px;
    padding: 6px 10px;
    color: {text};
}}
.searlx-suggestion-row:selected, .searlx-suggestion-row:hover {{
    background-color: {accent_soft};
}}

popover.searlx-site-card, popover.background.searlx-site-card, .searlx-site-card {{
    background-color: {chrome};
    background-image: none;
    border: 1px solid {line};
    color: {text};
    padding: 10px 12px;
}}
.searlx-site-card label,
.searlx-site-card combobox cellview {{
    color: {text};
}}
.searlx-site-card combobox button,
.searlx-site-card combobox button.combo {{
    background-color: {hover};
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: 1px solid {line};
    border-radius: 7px;
    color: {text};
    padding: 3px 8px;
}}
.searlx-site-card combobox button:hover {{
    background-color: {selected};
    background-image: none;
    border-color: {accent};
}}
.searlx-site-card switch {{
    background-color: {hover};
    background-image: none;
    border: 1px solid {line};
    border-radius: 14px;
    color: {text};
}}
.searlx-site-card switch:checked {{
    background-color: {accent};
    background-image: none;
    border-color: {accent};
}}
.searlx-site-card switch slider {{
    background-color: {selected};
    background-image: none;
    box-shadow: none;
    border: 1px solid {line};
}}
.searlx-site-card-title {{
    font-size: {title_px}px;
    font-weight: 700;
    color: {text};
}}
.searlx-site-card-btn,
.searlx-site-card button.searlx-site-card-btn {{
    background-color: {hover};
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: 1px solid {line};
    border-radius: 7px;
    color: {text};
    padding: 4px 10px;
    font-size: {small_px}px;
}}
.searlx-site-card-btn:hover,
.searlx-site-card button.searlx-site-card-btn:hover {{
    background-color: {selected};
    background-image: none;
    border-color: {accent};
}}
.searlx-site-card-btn:disabled {{
    opacity: 0.45;
}}

.searlx-find-pill {{
    background-color: {chrome};
    background-image: none;
    border: 1px solid {line};
    border-radius: 10px;
    padding: 6px 10px;
    color: {text};
    box-shadow: 0 8px 24px {shadow};
}}
.searlx-find-pill togglebutton,
.searlx-find-pill button {{
    background-color: {hover};
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: 1px solid {line};
    border-radius: 6px;
    color: {text};
    padding: 2px 6px;
}}
.searlx-find-pill togglebutton:checked {{
    background-color: {accent_soft};
    border-color: {accent};
    color: {accent};
}}
.searlx-find-entry {{
    background-color: {selected};
    background-image: none;
    box-shadow: none;
    color: {text};
    border: 1px solid {line};
    border-radius: 6px;
    padding: 2px 8px;
    min-width: 170px;
}}

.searlx-status-line {{
    background-color: {chrome};
    color: {muted};
    border: 1px solid {line};
    border-radius: 7px;
    padding: 3px 10px;
    font-size: {small_px}px;
    box-shadow: 0 4px 14px {shadow};
}}

.searlx-status-banner {{
    background-color: {chrome};
    color: {text};
    border: 1px solid {accent};
    border-radius: 8px;
    padding: 6px 12px;
    box-shadow: 0 8px 24px {shadow};
}}

.searlx-banner-prompt {{
    background-color: {chrome};
    border: 1px solid {line};
    border-radius: 10px;
    padding: 10px 14px;
    box-shadow: 0 10px 28px {shadow};
}}

.searlx-crash-card {{
    background-color: {chrome};
    border: 1px solid {danger};
    border-radius: 12px;
    padding: 20px 24px;
    box-shadow: 0 12px 32px {shadow};
}}

.searlx-space-pill {{
    background-color: transparent;
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    border: 1px solid transparent;
    border-radius: 8px;
    padding: 3px 8px;
    color: {muted};
    font-size: {small_px}px;
}}
.searlx-space-pill:hover {{
    background-color: {hover};
    background-image: none;
    color: {text};
}}
.searlx-space-pill.active {{
    background-color: {selected};
    background-image: none;
    border-color: {line};
    color: {text};
    font-weight: 600;
}}

.searlx-switcher-card {{
    background-color: {chrome};
    border: 1px solid {line};
    border-radius: 12px;
    padding: 14px;
    box-shadow: 0 20px 50px {shadow};
}}
.searlx-switcher-tile {{
    background-color: {hover};
    border: 2px solid transparent;
    border-radius: 8px;
    padding: 10px;
    min-width: 140px;
    min-height: 82px;
}}
.searlx-switcher-tile.selected {{
    background-color: {selected};
    border-color: {accent};
}}

.searlx-primary-btn {{
    background-color: {accent};
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    color: #FFFFFF;
    border: none;
    border-radius: 7px;
    padding: 5px 12px;
    font-weight: 600;
}}
.searlx-danger-btn {{
    background-color: {danger};
    background-image: none;
    box-shadow: none;
    text-shadow: none;
    color: #FFFFFF;
    border: none;
    border-radius: 7px;
    padding: 5px 12px;
    font-weight: 600;
}}
"#,
        ground = palette.ground,
        text = palette.text,
        muted = palette.muted,
        line = palette.line,
        hover = palette.hover,
        selected = palette.selected,
        page = palette.page,
        accent = palette.accent,
        accent_soft = palette.accent_soft,
        danger = palette.danger,
        warning = palette.warning,
        shadow = palette.card_shadow,
        chrome = palette.chrome,
        SPLIT_GUTTER = SPLIT_GUTTER,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_resolves_light_dark_system_and_high_contrast() {
        let mut settings = Settings {
            look: LookPreference::Light,
            ..Settings::default()
        };
        let light = ShellPalette::resolve(&settings, true);
        assert!(!light.dark);
        assert_eq!(light.ground, "#E9E8E6");

        settings.look = LookPreference::Dark;
        let dark = ShellPalette::resolve(&settings, false);
        assert!(dark.dark);
        assert_eq!(dark.ground, "#171717");

        settings.look = LookPreference::System;
        assert!(ShellPalette::resolve(&settings, true).dark);
        assert!(!ShellPalette::resolve(&settings, false).dark);

        settings.tabs_high_contrast = true;
        let hc_dark = ShellPalette::resolve(&settings, true);
        assert!(hc_dark.high_contrast);
        assert_eq!(hc_dark.text, "#FFFFFF");
        assert_ne!(hc_dark.line, dark.line);
    }

    #[test]
    fn css_generation_respects_reduce_motion_and_layout_metrics() {
        let mut settings = Settings {
            look: LookPreference::Dark,
            reduce_motion: true,
            layout_cards: true,
            sidebar_roomy: true,
            ..Settings::default()
        };
        let palette = ShellPalette::resolve(&settings, true);
        let css = build_gtk_css(
            &palette,
            &settings,
            Some(SpaceChromeTint {
                color: SpaceColor::Blue,
                gradient: None,
                strength: WashStrength::Subtle,
            }),
        );
        assert!(css.contains("0ms ease"));
        assert!(css.contains("border-radius: 10px;"));
        assert!(css.contains("margin: 8px;"));
        assert!(css.contains("min-height: 38px;"));
        assert_eq!(motion_duration_ms(180, true), 0);
        assert_eq!(motion_duration_ms(180, false), 180);

        settings.layout_cards = false;
        let flat_css = build_gtk_css(&palette, &settings, None);
        assert!(flat_css.contains("border-radius: 0px;"));
    }
}
