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
}

/// Convert a design number in points to gpui pixels.
#[must_use]
pub fn pt(value: f32) -> Pixels {
    px(value)
}
