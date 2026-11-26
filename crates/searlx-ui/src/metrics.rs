//! Shared spacing, sizing, and motion metrics.
//!
//! UI code must not scatter magic numbers. If a value is used in more than one
//! place, or represents a product decision, it lives here.

use gpui::Pixels;
use gpui::px;

/// Default window width in logical pixels.
pub const WINDOW_DEFAULT_WIDTH: f32 = 1180.0;
/// Default window height in logical pixels.
pub const WINDOW_DEFAULT_HEIGHT: f32 = 780.0;
/// Minimum window width in logical pixels.
pub const WINDOW_MIN_WIDTH: f32 = 640.0;
/// Minimum window height in logical pixels.
pub const WINDOW_MIN_HEIGHT: f32 = 420.0;

/// Sidebar default width.
pub const SIDEBAR_DEFAULT: f32 = 232.0;
/// Sidebar minimum width.
pub const SIDEBAR_MIN: f32 = 176.0;
/// Sidebar maximum width.
pub const SIDEBAR_MAX: f32 = 440.0;
/// Icons-only column width.
pub const SIDEBAR_ICONS: f32 = 48.0;

/// Height of one tab row.
pub const TAB_ROW_HEIGHT: f32 = 30.0;
/// Height of a roomier tab row.
pub const TAB_ROW_HEIGHT_ROOMY: f32 = 38.0;
/// Inset around the page card.
pub const PAGE_GAP: f32 = 8.0;
/// Corner radius of the page card.
pub const PAGE_RADIUS: f32 = 10.0;

/// Height of the address field.
pub const FIELD_HEIGHT: f32 = 30.0;
/// Width of the over-page address field.
pub const FIELD_WIDTH: f32 = 620.0;
/// Gutter between split panes.
pub const SPLIT_GUTTER: f32 = 7.0;
/// Minimum split pane width.
pub const SPLIT_MIN_PANE: f32 = 250.0;
/// Divider hit area.
pub const SPLIT_DIVIDER: f32 = 4.0;

/// Pinned tab square dimension (`FEATURES.md` §3.6).
pub const PIN_SQUARE_SIZE: f32 = 34.0;
/// Maximum pinned squares per grid row (`FEATURES.md` §3.6).
pub const PIN_GRID_COLUMNS: usize = 4;
/// Top horizontal tab strip height (`TabLayoutMode::TopTabs`).
pub const TOP_BAR_HEIGHT: f32 = 40.0;
/// Always-visible address line width (Small, `FEATURES.md` §2.1).
pub const ADDRESS_LINE_SMALL: f32 = 220.0;
/// Always-visible address line width (Medium, `FEATURES.md` §2.1).
pub const ADDRESS_LINE_MEDIUM: f32 = 340.0;
/// Site Card popover width.
pub const SITE_CARD_WIDTH: f32 = 320.0;
/// Floating Find-in-page pill width.
pub const FIND_PILL_WIDTH: f32 = 340.0;
/// Top inset for the over-page floating address overlay.
pub const OVERLAY_TOP_OFFSET: f32 = 72.0;
/// Maximum visible suggestion rows in the address overlay before scrolling.
pub const OVERLAY_MAX_SUGGESTIONS: usize = 8;
/// Duration per invalid-address shake step in milliseconds.
pub const SHAKE_STEP_MS: u64 = 45;
/// Horizontal pixel offsets for the damped invalid-address shake (`FEATURES.md` §3.6).
pub const SHAKE_OFFSETS_PX: [f32; 5] = [8.0, -7.0, 5.0, -3.0, 0.0];

/// Space dot diameter.
pub const SPACE_DOT: f32 = 8.0;
/// Default side panel width.
pub const SIDE_PANEL_DEFAULT: f32 = 380.0;
/// Minimum side panel width.
pub const SIDE_PANEL_MIN: f32 = 300.0;
/// Maximum side panel width.
pub const SIDE_PANEL_MAX: f32 = 640.0;

/// Motion durations, in milliseconds. Reduced motion uses [`MOTION_REDUCED`].
pub mod motion {
    /// Pill slide.
    pub const PILL: u64 = 180;
    /// Page cover fade.
    pub const COVER: u64 = 200;
    /// Split pane add.
    pub const SPLIT_ADD: u64 = 300;
    /// Split pane remove.
    pub const SPLIT_REMOVE: u64 = 320;
    /// Switcher hold before it appears.
    pub const SWITCHER_HOLD: u64 = 150;
    /// Divider hover dwell.
    pub const DIVIDER_HOVER: u64 = 100;
    /// What everything collapses to when Reduce Motion is on.
    pub const MOTION_REDUCED: u64 = 0;

    /// Resolve an animation duration in milliseconds, collapsing to `0` when
    /// Reduce Motion is active (`AGENTS.md` §3.3, P1-10).
    #[must_use]
    pub const fn duration_ms(base_ms: u64, reduce_motion: bool) -> u64 {
        if reduce_motion {
            MOTION_REDUCED
        } else {
            base_ms
        }
    }
}

/// Tab glyph box size in logical points (`1x`).
pub const GLYPH_SIZE_PT: f32 = 18.0;

/// Scale a design font/metric value by the user or platform text scale factor
/// (`0.75..=2.5`, P1-10).
#[must_use]
pub fn scaled_pt(value: f32, scale: f32) -> Pixels {
    let clamped = if scale.is_finite() {
        scale.clamp(0.75, 2.5)
    } else {
        1.0
    };
    px(value * clamped)
}

/// Raster icon pixel size for a logical point size at a display scale factor
/// (`1.0` for 1x, `2.0` for 2x Retina/HiDPI; P1-10).
#[must_use]
pub fn icon_device_px(logical_pt: f32, scale_factor: f32) -> u32 {
    let factor = if scale_factor.is_finite() {
        scale_factor.clamp(1.0, 4.0)
    } else {
        1.0
    };
    (logical_pt * factor).round().max(1.0) as u32
}

/// Convert a design number in points to gpui pixels.
#[must_use]
pub fn pt(value: f32) -> Pixels {
    px(value)
}

/// Tab row height respecting `sidebar.roomy` (`FEATURES.md` §2.2).
#[must_use]
pub const fn tab_row_height(roomy: bool) -> f32 {
    if roomy {
        TAB_ROW_HEIGHT_ROOMY
    } else {
        TAB_ROW_HEIGHT
    }
}

/// Width of the always-visible inline address line (`FEATURES.md` §2.1).
#[must_use]
pub const fn address_line_width(width: searlx_core::AddressBarWidth) -> f32 {
    match width {
        searlx_core::AddressBarWidth::Small => ADDRESS_LINE_SMALL,
        searlx_core::AddressBarWidth::Medium => ADDRESS_LINE_MEDIUM,
    }
}

/// Horizontal pixel offset for an invalid-address shake animation step.
/// Collapses to `0.0` when `reduce_motion` is active (`AGENTS.md` §3.3).
#[must_use]
pub fn shake_offset_px(step: usize, reduce_motion: bool) -> f32 {
    if reduce_motion {
        0.0
    } else {
        SHAKE_OFFSETS_PX.get(step).copied().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_motion_collapses_durations_and_icons_scale_1x_2x() {
        assert_eq!(motion::duration_ms(motion::PILL, false), 180);
        assert_eq!(motion::duration_ms(motion::PILL, true), 0);
        assert_eq!(icon_device_px(GLYPH_SIZE_PT, 1.0), 18);
        assert_eq!(icon_device_px(GLYPH_SIZE_PT, 2.0), 36);
        assert_eq!(scaled_pt(12.0, 1.5), px(18.0));
        assert_eq!(shake_offset_px(0, false), 8.0);
        assert_eq!(shake_offset_px(0, true), 0.0);
        assert_eq!(tab_row_height(false), TAB_ROW_HEIGHT);
        assert_eq!(tab_row_height(true), TAB_ROW_HEIGHT_ROOMY);
        assert_eq!(
            address_line_width(searlx_core::AddressBarWidth::Medium),
            ADDRESS_LINE_MEDIUM
        );
    }
}
