//! Mouse & trackpad gestures (`P3-12`) and Tab lifecycle / sleep / auto-close / twins (`P3-13`).
//!
//! Behavioural specifications:
//! - `FEATURES.md` §2.2, §3.6, §4.5, §23 — Swipe-back/forward + 0.45 s hold history, space swipe, autoscroll, tab give, folded sidebar edge dwell
//! - `FEATURES.md` §3.5 — Tab sleep (`10/30/120 min`, Low Power `10 min`, memory pressure `5/0 min`), auto-close (`12h/1d/1w/1m`), exceptions, archive, and window twins (`tabs.mirror`)

use crate::history::HistoryManager;
use crate::ids::{ProfileId, SpaceId, TabId, WindowId};
use crate::url::PageUrl;

// ============================================================================
// P3-12 — Gestures (`FEATURES.md` §2.2, §3.6, §4.5, §23)
// ============================================================================

/// Distance in points to arm a two-finger back/forward swipe disc (`70 pt`, `FEATURES.md` §4.5).
pub const SWIPE_ARM_PT: f64 = 70.0;
/// Distance in points for a quick flick to trigger navigation (`30 pt`, `FEATURES.md` §4.5).
pub const SWIPE_FLICK_PT: f64 = 30.0;
/// Maximum duration in milliseconds for a quick flick (`250 ms`, `FEATURES.md` §4.5).
pub const SWIPE_FLICK_MAX_MS: u64 = 250;
/// Hold duration while armed to expand the disc into an 8-item history list (`450 ms`, `FEATURES.md` §4.5).
pub const SWIPE_HOLD_MS: u64 = 450;
/// Maximum history rows shown in the swipe-hold picker (`8`, `FEATURES.md` §4.5).
pub const SWIPE_HISTORY_MAX_ITEMS: usize = 8;
/// Vertical pointer travel in points per row in the swipe-hold picker (`22 pt`, `FEATURES.md` §4.5).
pub const SWIPE_ROW_STEP_PT: f64 = 22.0;

/// Direction of a page back/forward swipe gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDirection {
    /// Swipe right / pull from left edge to go Back.
    Back,
    /// Swipe left / pull from right edge to go Forward.
    Forward,
}

/// Resolution of a two-finger horizontal page swipe (`FEATURES.md` §4.5, P3-12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeHistoryOutcome {
    /// The page itself scrolled horizontally (e.g., a carousel) and owns the gesture.
    YieldedToPageCarousel,
    /// Released before arming; the disc slips away with no navigation.
    Cancelled,
    /// Navigated by `steps_In_direction` (`1` for immediate arm/flick, or `1..=8` from the hold list).
    Navigate {
        /// Back or Forward.
        direction: SwipeDirection,
        /// Number of history entries to traverse (`1..=8`).
        steps: usize,
    },
}

/// State machine for the two-finger swipe-back/forward and 0.45 s hold-history gesture (`FEATURES.md` §4.5, P3-12).
#[derive(Clone, Debug, PartialEq)]
pub struct SwipeHistoryGesture {
    /// Swipe direction.
    pub direction: SwipeDirection,
    /// Available history items in `direction`, capped at 8 (`SWIPE_HISTORY_MAX_ITEMS`).
    pub available_items: usize,
    /// Cumulative horizontal pull distance in points (`>= 0.0`).
    pub pull_pt: f64,
    /// Cumulative vertical displacement in points after the hold list opens.
    pub vertical_offset_pt: f64,
    /// Total elapsed gesture duration in milliseconds.
    pub elapsed_ms: u64,
    /// Time spent continuously armed (`pull_pt >= 70 pt`) in milliseconds.
    pub armed_hold_ms: u64,
    /// Whether the 8-item history list is currently expanded.
    pub list_expanded: bool,
    /// Currently highlighted row index (`0..available_items`).
    pub picked_index: usize,
    /// Whether the page has claimed horizontal scroll ownership for any event in this gesture.
    pub page_claimed_scroll: bool,
    /// Whether optional haptics (`haptics_enabled`) fired on the latest step.
    pub haptic_pulsed: bool,
}

impl SwipeHistoryGesture {
    /// Begin a two-finger horizontal page swipe.
    #[must_use]
    pub fn begin(direction: SwipeDirection, available_items: usize) -> Self {
        Self {
            direction,
            available_items: available_items.min(SWIPE_HISTORY_MAX_ITEMS),
            pull_pt: 0.0,
            vertical_offset_pt: 0.0,
            elapsed_ms: 0,
            armed_hold_ms: 0,
            list_expanded: false,
            picked_index: 0,
            page_claimed_scroll: false,
            haptic_pulsed: false,
        }
    }

    /// Whether the swipe disc is currently armed (`>= 70 pt`).
    #[must_use]
    pub fn is_armed(&self) -> bool {
        !self.page_claimed_scroll && self.available_items > 0 && self.pull_pt >= SWIPE_ARM_PT
    }

    /// Feed a gesture sample (`delta_pull_pt`, `delta_y_pt`, `dt_ms`, `page_claims_horizontal_scroll`).
    pub fn update(
        &mut self,
        delta_pull_pt: f64,
        delta_y_pt: f64,
        dt_ms: u64,
        page_claims_horizontal_scroll: bool,
        swipe_history_enabled: bool,
        haptics_enabled: bool,
    ) {
        self.haptic_pulsed = false;
        if page_claims_horizontal_scroll {
            self.page_claimed_scroll = true;
            self.list_expanded = false;
            return;
        }
        if self.page_claimed_scroll || self.available_items == 0 {
            return;
        }

        let was_armed = self.is_armed();
        self.pull_pt = (self.pull_pt + delta_pull_pt).max(0.0);
        self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);

        if self.is_armed() {
            if !was_armed && haptics_enabled {
                self.haptic_pulsed = true;
            }
            self.armed_hold_ms = self.armed_hold_ms.saturating_add(dt_ms);
            if swipe_history_enabled && self.armed_hold_ms >= SWIPE_HOLD_MS {
                self.list_expanded = true;
            }
        } else {
            self.armed_hold_ms = 0;
        }

        if self.list_expanded {
            self.vertical_offset_pt += delta_y_pt;
            let raw_step = (self.vertical_offset_pt.abs() / SWIPE_ROW_STEP_PT).floor() as usize;
            let next_pick = raw_step.min(self.available_items.saturating_sub(1));
            if next_pick != self.picked_index {
                self.picked_index = next_pick;
                if haptics_enabled {
                    self.haptic_pulsed = true;
                }
            }
        }
    }

    /// Finish the gesture when fingers lift (`FEATURES.md` §4.5).
    #[must_use]
    pub fn finish(&self, page_claimed_on_release: bool) -> SwipeHistoryOutcome {
        if self.page_claimed_scroll || page_claimed_on_release {
            return SwipeHistoryOutcome::YieldedToPageCarousel;
        }
        if self.available_items == 0 {
            return SwipeHistoryOutcome::Cancelled;
        }
        if self.list_expanded {
            return SwipeHistoryOutcome::Navigate {
                direction: self.direction,
                steps: self.picked_index + 1,
            };
        }
        let is_quick_flick = self.pull_pt >= SWIPE_FLICK_PT
            && self.elapsed_ms > 0
            && self.elapsed_ms <= SWIPE_FLICK_MAX_MS;
        if self.is_armed() || is_quick_flick {
            SwipeHistoryOutcome::Navigate {
                direction: self.direction,
                steps: 1,
            }
        } else {
            SwipeHistoryOutcome::Cancelled
        }
    }
}

/// Threshold in points to switch spaces via two-finger swipe over tabs (`50 pt`, `FEATURES.md` §4.5).
pub const SPACE_SWIPE_THRESHOLD_PT: f64 = 50.0;
/// Cooldown rest duration after switching a space (`400 ms`, `FEATURES.md` §4.5).
pub const SPACE_SWIPE_REST_MS: u64 = 400;
/// Anti-back-stroke suppression window (`200 ms`, `FEATURES.md` §4.5).
pub const SPACE_SWIPE_ANTI_BACK_MS: u64 = 200;

/// Outcome of a space-switch gesture over the tab column or top bar (`FEATURES.md` §4.5, §23).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceSwipeOutcome {
    /// No switch yet (below threshold or inside rest / anti-back-stroke window).
    None,
    /// Switched to existing space at `index`.
    SwitchToSpace(usize),
    /// Swiped past the last space; creates a new space in place (`FEATURES.md` §4.5).
    CreateNewSpaceAtEnd,
}

/// Tracker for two-finger space swipes and mouse wheel / side-button switches over tabs (`P3-12`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpaceSwipeGesture {
    last_switch_ms: Option<u64>,
    last_direction: i8,
}

impl SpaceSwipeGesture {
    /// Create a new space-swipe tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluate a swipe displacement `delta_pt` (positive = next space, negative = previous space).
    pub fn evaluate_swipe(
        &mut self,
        current_index: usize,
        space_count: usize,
        delta_pt: f64,
        now_ms: u64,
    ) -> SpaceSwipeOutcome {
        if space_count == 0 || delta_pt.abs() < SPACE_SWIPE_THRESHOLD_PT {
            return SpaceSwipeOutcome::None;
        }
        let dir: i8 = if delta_pt > 0.0 { 1 } else { -1 };
        if let Some(last_ms) = self.last_switch_ms {
            let elapsed = now_ms.saturating_sub(last_ms);
            if elapsed < SPACE_SWIPE_REST_MS {
                return SpaceSwipeOutcome::None;
            }
            if dir != self.last_direction
                && elapsed < SPACE_SWIPE_REST_MS + SPACE_SWIPE_ANTI_BACK_MS
            {
                return SpaceSwipeOutcome::None;
            }
        }

        self.last_switch_ms = Some(now_ms);
        self.last_direction = dir;

        if dir > 0 {
            if current_index + 1 < space_count {
                SpaceSwipeOutcome::SwitchToSpace(current_index + 1)
            } else {
                SpaceSwipeOutcome::CreateNewSpaceAtEnd
            }
        } else if current_index > 0 {
            SpaceSwipeOutcome::SwitchToSpace(current_index - 1)
        } else {
            SpaceSwipeOutcome::None
        }
    }
}

/// Scale factor when pressing a tab with `tabs.give` enabled (`0.97`, `FEATURES.md` §3.6).
pub const TAB_GIVE_PRESSED_SCALE: f64 = 0.97;
/// Pointer travel distance in points that cancels `tabs.give` so drag wins (`5 pt`, `FEATURES.md` §3.6).
pub const TAB_GIVE_CANCEL_PT: f64 = 5.0;

/// Compute the visual scale of a pressed tab (`1.0` or `0.97`, `FEATURES.md` §3.6).
#[must_use]
pub fn tab_give_scale(tab_give_enabled: bool, pressed: bool, pointer_travel_pt: f64) -> f64 {
    if tab_give_enabled && pressed && pointer_travel_pt <= TAB_GIVE_CANCEL_PT {
        TAB_GIVE_PRESSED_SCALE
    } else {
        1.0
    }
}

/// Trigger zone width at the window edge for peeking a folded sidebar (`6 pt`, `FEATURES.md` §2.2, §23).
pub const FOLDED_EDGE_ZONE_PT: f64 = 6.0;
/// Overshoot past the sidebar that dismisses the peeked column (`48 pt`, `FEATURES.md` §2.2, §23).
pub const FOLDED_EDGE_OVERSHOOT_PT: f64 = 48.0;
/// Grace period before hiding a peeked sidebar (`65 ms`, `FEATURES.md` §2.2, §23).
pub const FOLDED_EDGE_GRACE_MS: u64 = 65;
/// Pointer dwell duration in the 6 pt edge zone required to peek the folded sidebar (`150 ms`, `FEATURES.md` §2.2, §23).
pub const FOLDED_EDGE_DWELL_MS: u64 = 150;
/// Top exclusion offset in fullscreen so the macOS menu bar isn't fought (`60 pt`, `FEATURES.md` §2.2).
pub const FOLDED_EDGE_FULLSCREEN_TOP_PT: f64 = 60.0;

/// State machine for peeking a folded sidebar at the window edge (`FEATURES.md` §2.2, §23, P3-12).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoldedSidebarEdgePeek {
    /// Milliseconds the pointer has stayed inside the 6 pt edge zone.
    pub dwell_ms: u64,
    /// Milliseconds the pointer has been outside the sidebar + 48 pt overshoot.
    pub outside_ms: u64,
    /// Whether the folded sidebar is currently peeked out.
    pub peeked: bool,
}

impl FoldedSidebarEdgePeek {
    /// Update the edge-peek state from pointer position `(x_from_edge_pt, y_pt)` and elapsed time `dt_ms`.
    pub fn update(
        &mut self,
        x_from_edge_pt: f64,
        y_pt: f64,
        sidebar_width_pt: f64,
        is_fullscreen: bool,
        dt_ms: u64,
    ) -> bool {
        if !self.peeked {
            let in_top_exclusion = is_fullscreen && y_pt < FOLDED_EDGE_FULLSCREEN_TOP_PT;
            if !in_top_exclusion && x_from_edge_pt <= FOLDED_EDGE_ZONE_PT {
                self.dwell_ms = self.dwell_ms.saturating_add(dt_ms);
                if self.dwell_ms >= FOLDED_EDGE_DWELL_MS {
                    self.peeked = true;
                    self.outside_ms = 0;
                }
            } else {
                self.dwell_ms = 0;
            }
        } else if x_from_edge_pt > sidebar_width_pt + FOLDED_EDGE_OVERSHOOT_PT {
            self.outside_ms = self.outside_ms.saturating_add(dt_ms);
            if self.outside_ms >= FOLDED_EDGE_GRACE_MS {
                self.peeked = false;
                self.dwell_ms = 0;
            }
        } else {
            self.outside_ms = 0;
        }
        self.peeked
    }
}

/// Middle-click autoscroll state (`autoscroll`, `FEATURES.md` §21, §23).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MiddleClickAutoscroll {
    /// Anchor point `(x, y)` where middle-click engaged autoscroll.
    pub anchor: Option<(f64, f64)>,
}

impl MiddleClickAutoscroll {
    /// Engage autoscroll at `(x, y)` when `autoscroll_enabled` is true.
    pub fn engage(&mut self, autoscroll_enabled: bool, x: f64, y: f64) -> bool {
        if !autoscroll_enabled {
            return false;
        }
        self.anchor = Some((x, y));
        true
    }

    /// Compute per-frame scroll delta `(dx, dy)` from current pointer `(x, y)`.
    #[must_use]
    pub fn scroll_velocity(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (ax, ay) = self.anchor?;
        Some(((x - ax) * 0.25, (y - ay) * 0.25))
    }

    /// Disengage on any click or `Escape`.
    pub fn cancel(&mut self) {
        self.anchor = None;
    }
}

// ============================================================================
// P3-13 — Tab lifecycle: sleep, auto-close, archive, and window twins (`FEATURES.md` §3.5)
// ============================================================================

/// Configured sleep pace (`tabs.sleep.pace`, `FEATURES.md` §3.5, §21).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SleepPace {
    /// Never sleep tabs automatically.
    Never,
    /// Sleep after 10 minutes inactive (`Sooner`).
    Sooner10Min,
    /// Sleep after 30 minutes inactive (`Normal`, default).
    #[default]
    Normal30Min,
    /// Sleep after 120 minutes inactive (`Later`).
    Later120Min,
}

/// OS memory pressure level (`FEATURES.md` §3.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryPressureLevel {
    /// Normal system memory.
    #[default]
    Normal,
    /// Warning memory pressure: shortens sleep timer to 5 minutes.
    Warning,
    /// Critical memory pressure: shortens sleep timer to 0 seconds for eligible tabs.
    Critical,
}

/// Configured auto-close threshold (`tabs.close.after`, `FEATURES.md` §3.5, §21).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutoCloseAfter {
    /// Never auto-close tabs (default).
    #[default]
    Never,
    /// Auto-close after 12 hours (`43,200 s`).
    Hours12,
    /// Auto-close after 1 day (`86,400 s`).
    Day1,
    /// Auto-close after 1 week (`604,800 s`).
    Week1,
    /// Auto-close after 30 days (`2,592,000 s`).
    Month1,
}

impl AutoCloseAfter {
    /// Threshold in seconds, or `None` for `Never`.
    #[must_use]
    pub const fn threshold_secs(self) -> Option<u64> {
        match self {
            Self::Never => None,
            Self::Hours12 => Some(12 * 3600),
            Self::Day1 => Some(24 * 3600),
            Self::Week1 => Some(7 * 24 * 3600),
            Self::Month1 => Some(30 * 24 * 3600),
        }
    }
}

/// Live activity and protection flags of a tab for sleep and auto-close decisions (`FEATURES.md` §3.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabLifecycleCandidate {
    /// Tab ID.
    pub tab_id: TabId,
    /// Space owning the tab.
    pub space_id: SpaceId,
    /// Current page URL.
    pub url: PageUrl,
    /// Tab title.
    pub title: String,
    /// Unix timestamp (seconds) when the user last left/interacted with the tab (persisted across quits).
    pub last_active_unix_secs: u64,
    /// Optional split-group identifier so split pairs auto-close whole or not at all (`FEATURES.md` §3.5).
    pub split_group_id: Option<u64>,
    /// Currently visible on screen (active tab or visible split pane).
    pub on_screen: bool,
    /// Pinned tab.
    pub pinned: bool,
    /// Whether the tab lives in the currently active space.
    pub in_current_space: bool,
    /// Whether the tab's origin has notification permission (`Allow`).
    pub site_can_send_notifications: bool,
    /// Custom user-assigned tab name (`Rename` marks a tab "kept").
    pub named: bool,
    /// Member of a tab group.
    pub grouped: bool,
    /// Private tab.
    pub is_private: bool,
    /// Blank new-tab page.
    pub is_blank: bool,
    /// Page is still loading.
    pub is_loading: bool,
    /// Producing audio.
    pub playing_sound: bool,
    /// Active WebRTC call / camera / microphone capture.
    pub on_call_or_recording: bool,
    /// Active file download associated with this tab.
    pub downloading: bool,
    /// Currently floating video.
    pub video_floating: bool,
    /// JavaScript dialog or permission prompt waiting for user input.
    pub question_waiting: bool,
    /// Dirty form / typed text in an `<input>`, `<textarea>`, or `contenteditable`.
    pub holding_typed_text: bool,
    /// Per-site `"Keep This Site Awake"` toggle from the site card (`FEATURES.md` §5.8).
    pub kept_awake_per_site: bool,
}

impl TabLifecycleCandidate {
    /// Whether the tab is in the "kept" state (`FEATURES.md` §3.5: pinned, named, grouped, or kept awake).
    #[must_use]
    pub const fn is_kept(&self) -> bool {
        self.pinned || self.named || self.grouped || self.kept_awake_per_site
    }

    /// Whether the tab is currently busy with live media, a call, a download, a prompt, or typed text.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        self.playing_sound
            || self.on_call_or_recording
            || self.downloading
            || self.video_floating
            || self.question_waiting
            || self.holding_typed_text
            || self.is_loading
    }

    /// Whether the tab is eligible to be put to sleep once its inactivity timer expires (`FEATURES.md` §3.5).
    #[must_use]
    pub const fn can_sleep(&self) -> bool {
        if self.on_screen || self.is_blank || self.kept_awake_per_site || self.is_busy() {
            return false;
        }
        if self.pinned {
            if self.in_current_space {
                return false;
            }
            // A pin in another space sleeps unless its site may send notifications (`FEATURES.md` §3.5).
            if self.site_can_send_notifications {
                return false;
            }
        }
        true
    }

    /// Whether the tab is eligible for automatic closing (`FEATURES.md` §3.5).
    #[must_use]
    pub const fn can_auto_close(&self) -> bool {
        !(self.on_screen || self.is_kept() || self.is_private || self.is_blank || self.is_busy())
    }
}

/// Compute the effective sleep timeout in seconds given pace, Low Power Mode, and memory pressure (`FEATURES.md` §3.5).
#[must_use]
pub const fn effective_sleep_timeout_secs(
    pace: SleepPace,
    low_power_mode_with_light_motion: bool,
    pressure: MemoryPressureLevel,
) -> Option<u64> {
    let base = match pace {
        SleepPace::Never => return None,
        SleepPace::Sooner10Min => 10 * 60,
        SleepPace::Normal30Min => 30 * 60,
        SleepPace::Later120Min => 120 * 60,
    };
    let after_power = if low_power_mode_with_light_motion && base > 10 * 60 {
        10 * 60
    } else {
        base
    };
    Some(match pressure {
        MemoryPressureLevel::Normal => after_power,
        MemoryPressureLevel::Warning => {
            if after_power > 5 * 60 {
                5 * 60
            } else {
                after_power
            }
        }
        MemoryPressureLevel::Critical => 0,
    })
}

/// Evaluate which tabs should sleep and which tabs should auto-close into the archive (`FEATURES.md` §3.5, P3-13).
pub fn evaluate_tab_lifecycle(
    candidates: &[TabLifecycleCandidate],
    now_unix_secs: u64,
    sleep_pace: SleepPace,
    low_power_with_light_motion: bool,
    pressure: MemoryPressureLevel,
    auto_close: AutoCloseAfter,
    history: &mut HistoryManager,
) -> (Vec<TabId>, Vec<TabId>) {
    let sleep_timeout =
        effective_sleep_timeout_secs(sleep_pace, low_power_with_light_motion, pressure);
    let close_timeout = auto_close.threshold_secs();

    // First determine auto-closed tabs, enforcing that a split pair closes whole or not at all (`FEATURES.md` §3.5).
    let mut to_auto_close = Vec::new();
    if let Some(threshold) = close_timeout {
        for candidate in candidates {
            let individual_ready = candidate.can_auto_close()
                && now_unix_secs.saturating_sub(candidate.last_active_unix_secs) >= threshold;
            if !individual_ready {
                continue;
            }
            let whole_split_ready = match candidate.split_group_id {
                None => true,
                Some(group_id) => candidates
                    .iter()
                    .filter(|c| c.split_group_id == Some(group_id))
                    .all(|c| {
                        c.can_auto_close()
                            && now_unix_secs.saturating_sub(c.last_active_unix_secs) >= threshold
                    }),
            };
            if whole_split_ready {
                to_auto_close.push(candidate.tab_id);
                history.archive_auto_closed_tab(
                    format!("arch-{}", candidate.tab_id),
                    &candidate.url,
                    candidate.title.clone(),
                    now_unix_secs.to_string(),
                    candidate.is_private,
                    Some(candidate.space_id.to_string()),
                );
            }
        }
    }

    let mut to_sleep = Vec::new();
    if let Some(threshold) = sleep_timeout {
        for candidate in candidates {
            if to_auto_close.contains(&candidate.tab_id) {
                continue;
            }
            if candidate.can_sleep()
                && now_unix_secs.saturating_sub(candidate.last_active_unix_secs) >= threshold
            {
                to_sleep.push(candidate.tab_id);
            }
        }
    }

    (to_sleep, to_auto_close)
}

/// Window twins coordinator (`tabs.mirror`, `FEATURES.md` §3.5, P3-13).
///
/// When `tabs.mirror` is on, every window of a profile mirrors the same open
/// non-private tabs in each space (with independent page instances per window),
/// while private tabs remain strictly local to the window that opened them.
#[derive(Clone, Debug, Default)]
pub struct WindowTwinsCoordinator {
    /// Shared `(ProfileId, SpaceId)` → ordered list of non-private `PageUrl`s.
    shared_space_tabs: Vec<(ProfileId, SpaceId, Vec<PageUrl>)>,
    /// Window-local private tabs `WindowId` → `Vec<PageUrl>`.
    local_private_tabs: Vec<(WindowId, Vec<PageUrl>)>,
}

impl WindowTwinsCoordinator {
    /// Create an empty twins coordinator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a tab open in `window_id`; mirrors across the profile's windows only when non-private.
    pub fn open_tab(
        &mut self,
        profile: ProfileId,
        space: SpaceId,
        window_id: WindowId,
        url: PageUrl,
        is_private: bool,
    ) {
        if is_private {
            if let Some((_, list)) = self
                .local_private_tabs
                .iter_mut()
                .find(|(w, _)| *w == window_id)
            {
                list.push(url);
            } else {
                self.local_private_tabs.push((window_id, vec![url]));
            }
            return;
        }
        if let Some((_, _, list)) = self
            .shared_space_tabs
            .iter_mut()
            .find(|(p, s, _)| *p == profile && *s == space)
        {
            list.push(url);
        } else {
            self.shared_space_tabs.push((profile, space, vec![url]));
        }
    }

    /// Closing a window removes only its local private tabs and leaves shared tabs intact for other windows (`FEATURES.md` §3.5).
    pub fn close_window(&mut self, window_id: WindowId) {
        self.local_private_tabs.retain(|(w, _)| *w != window_id);
    }

    /// Mirrored non-private tabs visible in any window of `profile` for `space`.
    #[must_use]
    pub fn mirrored_tabs(&self, profile: ProfileId, space: SpaceId) -> &[PageUrl] {
        self.shared_space_tabs
            .iter()
            .find(|(p, s, _)| *p == profile && *s == space)
            .map_or(&[], |(_, _, list)| list.as_slice())
    }

    /// Window-local private tabs for `window_id`.
    #[must_use]
    pub fn private_tabs_for_window(&self, window_id: WindowId) -> &[PageUrl] {
        self.local_private_tabs
            .iter()
            .find(|(w, _)| *w == window_id)
            .map_or(&[], |(_, list)| list.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swipe_history_arms_at_70pt_holds_for_8_items_and_yields_to_horizontal_carousels() {
        // 1. Horizontal carousel takes ownership
        let mut carousel_gesture = SwipeHistoryGesture::begin(SwipeDirection::Back, 5);
        carousel_gesture.update(85.0, 0.0, 100, true, true, true);
        assert_eq!(
            carousel_gesture.finish(false),
            SwipeHistoryOutcome::YieldedToPageCarousel
        );

        // 2. Armed + held for 450 ms expands 8-item history list, 22 pt per row
        let mut hold_gesture = SwipeHistoryGesture::begin(SwipeDirection::Back, 12);
        assert_eq!(hold_gesture.available_items, SWIPE_HISTORY_MAX_ITEMS);
        hold_gesture.update(75.0, 0.0, 100, false, true, true);
        assert!(hold_gesture.is_armed());
        assert!(hold_gesture.haptic_pulsed);
        hold_gesture.update(0.0, 0.0, 450, false, true, true);
        assert!(hold_gesture.list_expanded);
        hold_gesture.update(0.0, 45.0, 50, false, true, true); // 45 pt -> row index 2 (3 steps back)
        assert_eq!(hold_gesture.picked_index, 2);
        assert_eq!(
            hold_gesture.finish(false),
            SwipeHistoryOutcome::Navigate {
                direction: SwipeDirection::Back,
                steps: 3,
            }
        );
    }

    #[test]
    fn space_swipe_fold_edge_and_tab_give_follow_spec_thresholds() {
        let mut space_swipe = SpaceSwipeGesture::new();
        assert_eq!(
            space_swipe.evaluate_swipe(0, 2, 55.0, 1000),
            SpaceSwipeOutcome::SwitchToSpace(1)
        );
        // Inside 400 ms rest period -> ignored
        assert_eq!(
            space_swipe.evaluate_swipe(1, 2, 55.0, 1200),
            SpaceSwipeOutcome::None
        );
        // Past last space after rest -> creates new space
        assert_eq!(
            space_swipe.evaluate_swipe(1, 2, 55.0, 1700),
            SpaceSwipeOutcome::CreateNewSpaceAtEnd
        );

        // Tab give cancels past 5 pt travel
        assert_eq!(tab_give_scale(true, true, 3.0), 0.97);
        assert_eq!(tab_give_scale(true, true, 5.5), 1.0);

        // Folded sidebar edge peek: 6 pt zone, 150 ms dwell, 48 pt overshoot, 65 ms grace
        let mut edge = FoldedSidebarEdgePeek::default();
        assert!(!edge.update(4.0, 100.0, 220.0, false, 100));
        assert!(edge.update(4.0, 100.0, 220.0, false, 60));
        assert!(edge.update(250.0, 100.0, 220.0, false, 40)); // 250 <= 220 + 48
        assert!(!edge.update(280.0, 100.0, 220.0, false, 70)); // > 268 for 70 ms -> hides
    }

    #[test]
    fn tab_lifecycle_sleeps_idle_tabs_protects_exceptions_and_archives_auto_closed_tabs() {
        let mut history = HistoryManager::new();
        let space = SpaceId::new();
        let now = 100_000;
        let idle_tab = TabId::new(1);
        let audible_tab = TabId::new(2);
        let split_a = TabId::new(3);
        let split_b = TabId::new(4);
        let url = PageUrl::parse("https://example.com/page").expect("valid url");

        let base = TabLifecycleCandidate {
            tab_id: idle_tab,
            space_id: space,
            url: url.clone(),
            title: "Idle Page".to_owned(),
            last_active_unix_secs: now - 50_000, // ~13.8 hours idle
            split_group_id: None,
            on_screen: false,
            pinned: false,
            in_current_space: true,
            site_can_send_notifications: false,
            named: false,
            grouped: false,
            is_private: false,
            is_blank: false,
            is_loading: false,
            playing_sound: false,
            on_call_or_recording: false,
            downloading: false,
            video_floating: false,
            question_waiting: false,
            holding_typed_text: false,
            kept_awake_per_site: false,
        };

        let audible = TabLifecycleCandidate {
            tab_id: audible_tab,
            playing_sound: true,
            ..base.clone()
        };
        // Split pair where split_a is old enough to auto-close, but split_b is active on screen -> neither auto-closes!
        let pane_a = TabLifecycleCandidate {
            tab_id: split_a,
            split_group_id: Some(77),
            ..base.clone()
        };
        let pane_b = TabLifecycleCandidate {
            tab_id: split_b,
            split_group_id: Some(77),
            on_screen: true,
            ..base.clone()
        };

        let (to_sleep, to_close) = evaluate_tab_lifecycle(
            &[base, audible, pane_a, pane_b],
            now,
            SleepPace::Normal30Min,
            false,
            MemoryPressureLevel::Normal,
            AutoCloseAfter::Hours12,
            &mut history,
        );

        assert_eq!(to_close, vec![idle_tab]);
        assert_eq!(to_sleep, vec![split_a]);
        assert_eq!(history.archive().len(), 1);
        assert_eq!(history.archive()[0].title, "Idle Page");
    }
}
