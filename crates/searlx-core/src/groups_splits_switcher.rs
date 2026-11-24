//! Tab groups (`P3-06`), 2–4 pane Split View layout & snapping (`P3-07`), and
//! the `⌃Tab` recent-tab switcher with moons (`P3-08`) (`FEATURES.md` §3.4, §3.7, §8).

use serde::{Deserialize, Serialize};

use crate::domain::{SpaceColor, SplitItem, TabGroupItem};
use crate::error::{CoreError, Result};
use crate::ids::{GroupId, SpaceId, TabId, WindowId};

// ============================================================================
// P3-06 — Tab groups (`FEATURES.md` §8)
// ============================================================================

/// A closed tab group kept on the undo/reopen stack so `⇧⌘T` restores the whole group (`FEATURES.md` §8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosedTabGroup {
    /// Closed group metadata and member tab IDs.
    pub group: TabGroupItem,
    /// Space the group belonged to.
    pub space: SpaceId,
}

/// Tab groups manager (`FEATURES.md` §8, P3-06).
///
/// - Empty groups disappear automatically.
/// - Closing a group pushes it onto the reopen stack so `⇧⌘T` restores it whole.
/// - `fold_all(true/false)` handles `⌥⌘[` and `⌥⌘]`.
/// - Derives a stable positive 31-bit integer ID for Chrome's `chrome.tabGroups`
///   extension API from the group's UUID (`FEATURES.md` §8).
#[derive(Clone, Debug, Default)]
pub struct TabGroupsManager {
    groups: Vec<(SpaceId, TabGroupItem)>,
    closed_groups: Vec<ClosedTabGroup>,
}

impl TabGroupsManager {
    /// Create an empty tab groups manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Active groups in `space`.
    #[must_use]
    pub fn groups_in_space(&self, space: SpaceId) -> Vec<&TabGroupItem> {
        self.groups
            .iter()
            .filter(|(s, _)| *s == space)
            .map(|(_, g)| g)
            .collect()
    }

    /// Create a group in `space` with non-empty `tabs`.
    pub fn create_group(
        &mut self,
        space: SpaceId,
        name: impl Into<String>,
        color: SpaceColor,
        tabs: Vec<TabId>,
    ) -> Result<GroupId> {
        if tabs.is_empty() {
            return Err(CoreError::Invalid(
                "cannot create an empty tab group".into(),
            ));
        }
        let id = GroupId::new();
        let mut group = TabGroupItem::new(id, name, tabs);
        group.color = color;
        self.groups.push((space, group));
        Ok(id)
    }

    /// Remove a tab from its group; if the group becomes empty, it disappears automatically (`FEATURES.md` §8).
    pub fn remove_tab_from_group(&mut self, group_id: GroupId, tab_id: TabId) -> bool {
        let Some(pos) = self.groups.iter().position(|(_, g)| g.id == group_id) else {
            return false;
        };
        self.groups[pos].1.tabs.retain(|&t| t != tab_id);
        if self.groups[pos].1.is_empty() {
            self.groups.remove(pos);
        }
        true
    }

    /// When a tab is opened from `opener_tab`, it automatically joins `opener_tab`'s group (`FEATURES.md` §8).
    pub fn inherit_group_from_opener(
        &mut self,
        opener_tab: TabId,
        child_tab: TabId,
    ) -> Option<GroupId> {
        for (_, group) in &mut self.groups {
            if let Some(pos) = group.tabs.iter().position(|&t| t == opener_tab) {
                if !group.tabs.contains(&child_tab) {
                    group.tabs.insert(pos + 1, child_tab);
                }
                return Some(group.id);
            }
        }
        None
    }

    /// Fold (`collapsed = true`, `⌥⌘[`) or unfold (`collapsed = false`, `⌥⌘]`) all groups in `space`.
    pub fn set_all_collapsed(&mut self, space: SpaceId, collapsed: bool) {
        for (s, group) in &mut self.groups {
            if *s == space {
                group.collapsed = collapsed;
            }
        }
    }

    /// Move an entire group into `target_space`, keeping its tabs and state intact (`FEATURES.md` §8).
    pub fn move_group_to_space(&mut self, group_id: GroupId, target_space: SpaceId) -> Result<()> {
        let entry = self
            .groups
            .iter_mut()
            .find(|(_, g)| g.id == group_id)
            .ok_or_else(|| CoreError::Invalid("no such tab group".into()))?;
        entry.0 = target_space;
        Ok(())
    }

    /// Close an entire group, pushing it to the undo stack so `⇧⌘T` restores it whole (`FEATURES.md` §8).
    pub fn close_group(&mut self, group_id: GroupId) -> Result<ClosedTabGroup> {
        let Some(pos) = self.groups.iter().position(|(_, g)| g.id == group_id) else {
            return Err(CoreError::Invalid("no such tab group".into()));
        };
        let (space, group) = self.groups.remove(pos);
        let closed = ClosedTabGroup { group, space };
        self.closed_groups.insert(0, closed.clone());
        Ok(closed)
    }

    /// Restore the most recently closed group (`⇧⌘T`, `FEATURES.md` §8).
    pub fn reopen_last_closed_group(&mut self) -> Option<ClosedTabGroup> {
        if self.closed_groups.is_empty() {
            return None;
        }
        let closed = self.closed_groups.remove(0);
        self.groups.push((closed.space, closed.group.clone()));
        Some(closed)
    }

    /// Derive a positive non-zero 31-bit integer ID for Chrome's `tabGroups` extension API (`FEATURES.md` §8).
    #[must_use]
    pub fn extension_group_number(group_id: GroupId) -> i32 {
        let uuid = group_id.as_uuid();
        let bytes = uuid.as_bytes();
        let raw = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) & 0x7FFF_FFFF;
        (raw.max(1)) as i32
    }
}

// ============================================================================
// P3-07 — Split View stage layout & divider snapping (`FEATURES.md` §3.7)
// ============================================================================

/// Gutter width between split panes (`7 pt`, `FEATURES.md` §3.7).
pub const SPLIT_GUTTER_PT: f64 = 7.0;
/// Minimum width of a split pane (`250 pt`, `FEATURES.md` §3.7).
pub const SPLIT_MIN_PANE_PT: f64 = 250.0;
/// Distance from even center within which a dragged divider snaps (`15 pt`, `FEATURES.md` §3.7).
pub const SPLIT_SNAP_PT: f64 = 15.0;
/// Distance required to break out of an already-snapped even divider (`20 pt`, `FEATURES.md` §3.7).
pub const SPLIT_UNSNAP_PT: f64 = 20.0;

/// Computed pane rectangle on the split stage (`P3-07`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitPaneFrame {
    /// Tab inside this pane.
    pub tab: TabId,
    /// Left offset in stage points.
    pub x: f64,
    /// Width in stage points.
    pub width: f64,
    /// Whether this pane has keyboard/find focus.
    pub focused: bool,
}

/// Lay out a [`SplitItem`] across `stage_width_pt` (`FEATURES.md` §3.7, P3-07):
/// - If `stage_width_pt` cannot fit every pane at [`SPLIT_MIN_PANE_PT`] (`250 pt`)
///   plus [`SPLIT_GUTTER_PT`] (`7 pt`) gutters, **only the focused pane is shown**
///   at full stage width while keeping the split definition intact for when the
///   window widens (`FEATURES.md` §3.7).
#[must_use]
pub fn layout_split_stage(split: &SplitItem, stage_width_pt: f64) -> Vec<SplitPaneFrame> {
    let n = split.panes.len();
    if n == 0 {
        return Vec::new();
    }
    let total_gutters = (n.saturating_sub(1) as f64) * SPLIT_GUTTER_PT;
    let min_required = (n as f64) * SPLIT_MIN_PANE_PT + total_gutters;
    let focused_idx = split.focused.min(n - 1);

    if stage_width_pt < min_required {
        return vec![SplitPaneFrame {
            tab: split.panes[focused_idx],
            x: 0.0,
            width: stage_width_pt.max(0.0),
            focused: true,
        }];
    }

    let available = stage_width_pt - total_gutters;
    let mut frames = Vec::with_capacity(n);
    let mut cursor_x = 0.0;
    for (i, &tab) in split.panes.iter().enumerate() {
        let share = split.shares.get(i).copied().unwrap_or(1.0 / (n as f64));
        let width = available * share;
        frames.push(SplitPaneFrame {
            tab,
            x: cursor_x,
            width,
            focused: i == focused_idx,
        });
        cursor_x += width + SPLIT_GUTTER_PT;
    }
    frames
}

/// Drag a 2-pane split divider to `pointer_x` inside `stage_width_pt`, applying
/// the `15 pt` even snap, `20 pt` unsnap hysteresis, and `20–80%` / `250 pt`
/// minimum pane clamps (`FEATURES.md` §3.7, P3-07).
///
/// Returns `(new_first_share, is_snapped_even)`.
pub fn drag_two_pane_divider(
    split: &mut SplitItem,
    pointer_x: f64,
    stage_width_pt: f64,
    was_snapped_even: bool,
) -> Result<(f64, bool)> {
    if split.panes.len() != 2 {
        return Err(CoreError::Invalid(
            "drag_two_pane_divider requires a 2-pane split".into(),
        ));
    }
    let available = (stage_width_pt - SPLIT_GUTTER_PT).max(2.0 * SPLIT_MIN_PANE_PT);
    let center_x = available * 0.5;
    let delta_from_center = (pointer_x - center_x).abs();
    let snap_threshold = if was_snapped_even {
        SPLIT_UNSNAP_PT
    } else {
        SPLIT_SNAP_PT
    };

    if delta_from_center <= snap_threshold {
        split.set_shares(&[0.5, 0.5])?;
        return Ok((0.5, true));
    }

    let min_ratio_by_pt = (SPLIT_MIN_PANE_PT / available).clamp(0.20, 0.50);
    let raw_ratio = (pointer_x / available).clamp(min_ratio_by_pt, 1.0 - min_ratio_by_pt);
    split.set_shares(&[raw_ratio, 1.0 - raw_ratio])?;
    Ok((split.shares[0], false))
}

/// Move focus left (`delta = -1`, `⌃⌘←`) or right (`delta = 1`, `⌃⌘→`) across split panes (`FEATURES.md` §3.7).
pub fn move_split_focus(split: &mut SplitItem, delta: isize) -> TabId {
    let n = split.panes.len();
    let next = (split.focused as isize + delta).clamp(0, (n as isize) - 1) as usize;
    split.focused = next;
    split.panes[next]
}

// ============================================================================
// P3-08 — Tab switcher (`⌃Tab`, `FEATURES.md` §3.4)
// ============================================================================

/// Hold duration in milliseconds before the `⌃Tab` switcher grid is drawn (`150 ms`, `FEATURES.md` §3.4).
pub const SWITCHER_HOLD_DELAY_MS: u64 = 150;
/// Columns in the `⌃Tab` tab switcher grid (`5`, `FEATURES.md` §3.4).
pub const SWITCHER_COLUMNS: usize = 5;
/// Maximum tab cards shown in the `⌃Tab` grid (`10`, `FEATURES.md` §3.4).
pub const SWITCHER_MAX_CARDS: usize = 10;
/// Maximum small-window "moon" cards shown beside the grid (`9`, `FEATURES.md` §3.4).
pub const SWITCHER_MAX_MOONS: usize = 9;
/// Minimum pointer movement in points after the switcher opens before hover can change selection (`2.0 pt`, `FEATURES.md` §3.4).
pub const SWITCHER_HOVER_GUARD_PT: f64 = 2.0;

/// A small window ("Little") candidate for a moon card beside the `⌃Tab` grid (`FEATURES.md` §3.4, P3-08).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoonWindowCandidate {
    /// Small window ID.
    pub window: WindowId,
    /// Page title.
    pub title: String,
    /// Page length in screens (`>= 1.0`), used for log₂ card height scaling.
    pub page_screens: f64,
}

impl MoonWindowCandidate {
    /// Normalized moon card height factor `0.0..=1.0` on a `log₂` curve:
    /// `1` screen is smallest (`0.0`), `16+` screens is full column (`1.0`, `FEATURES.md` §3.4).
    #[must_use]
    pub fn log2_height_scale(&self) -> f64 {
        let screens = if self.page_screens.is_finite() {
            self.page_screens.clamp(1.0, 16.0)
        } else {
            1.0
        };
        (screens.log2() / 4.0).clamp(0.0, 1.0)
    }
}

/// While-held action inside the `⌃Tab` switcher (`⌃W`, `⌃R`, `⌃M`, `⌃O`, `FEATURES.md` §3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitcherHeldAction {
    /// `⌃W`: close the highlighted tab or moon window.
    Close,
    /// `⌃R`: reload the highlighted tab.
    Reload,
    /// `⌃M`: toggle audio mute on the highlighted tab.
    ToggleMute,
    /// `⌃O`: keep a highlighted small window ("moon") into the main tab row.
    KeepMoonInBrowser,
}

/// State machine for the `⌃Tab` recent-tab switcher (`FEATURES.md` §3.4, P3-08).
#[derive(Clone, Debug, PartialEq)]
pub struct TabSwitcherState {
    /// Ordered tab cards (current tab first, then MRU up to 10 cards).
    pub cards: Vec<TabId>,
    /// Up to 9 small-window moons beside the grid.
    pub moons: Vec<MoonWindowCandidate>,
    /// Selected card index in `cards`.
    pub selected_index: usize,
    /// Whether the 0.15 s hold threshold has elapsed and the grid is drawn.
    pub grid_visible: bool,
    /// Pointer coordinates when the grid appeared (for the `>2 pt` hover guard).
    pub open_pointer: (f64, f64),
}

impl TabSwitcherState {
    /// Open a switcher session from `mru_tabs` (most-recently-used first, with
    /// active tab at index 0), optionally filtering out pins (`switcherLeavesOutPins`).
    #[must_use]
    pub fn begin(
        mru_tabs: &[(TabId, bool)],
        moons: Vec<MoonWindowCandidate>,
        leave_out_pins: bool,
        backwards: bool,
        pointer: (f64, f64),
    ) -> Self {
        let cards: Vec<TabId> = mru_tabs
            .iter()
            .filter(|(_, pinned)| !leave_out_pins || !*pinned)
            .map(|(id, _)| *id)
            .take(SWITCHER_MAX_CARDS)
            .collect();
        let selected_index = if cards.len() <= 1 {
            0
        } else if backwards {
            cards.len() - 1
        } else {
            1
        };
        Self {
            cards,
            moons: moons.into_iter().take(SWITCHER_MAX_MOONS).collect(),
            selected_index,
            grid_visible: false,
            open_pointer: pointer,
        }
    }

    /// Update hold duration; once `elapsed_ms >= 150`, the grid becomes visible (`FEATURES.md` §3.4).
    pub fn on_hold_elapsed(&mut self, elapsed_ms: u64) -> bool {
        if elapsed_ms >= SWITCHER_HOLD_DELAY_MS {
            self.grid_visible = true;
        }
        self.grid_visible
    }

    /// Step selection forward (`⌃Tab` / Right arrow) or backward (`⇧⌃Tab` / Left arrow) with wrap-around.
    pub fn step(&mut self, delta: isize) -> Option<TabId> {
        if self.cards.is_empty() {
            return None;
        }
        let len = self.cards.len() as isize;
        self.selected_index = ((self.selected_index as isize + delta).rem_euclid(len)) as usize;
        self.cards.get(self.selected_index).copied()
    }

    /// Move up (`-5`) or down (`+5`) within the 5-column grid without wrapping across columns (`FEATURES.md` §3.4).
    pub fn step_vertical(&mut self, down: bool) -> Option<TabId> {
        if self.cards.is_empty() {
            return None;
        }
        let candidate = if down {
            self.selected_index.checked_add(SWITCHER_COLUMNS)
        } else {
            self.selected_index.checked_sub(SWITCHER_COLUMNS)
        };
        if let Some(idx) = candidate
            && idx < self.cards.len()
        {
            self.selected_index = idx;
        }
        self.cards.get(self.selected_index).copied()
    }

    /// Hover-select `card_index` only if the pointer has moved more than `2 pt`
    /// since the panel appeared (`FEATURES.md` §3.4).
    pub fn on_pointer_hover(&mut self, x: f64, y: f64, card_index: usize) -> bool {
        let dx = x - self.open_pointer.0;
        let dy = y - self.open_pointer.1;
        if (dx * dx + dy * dy).sqrt() <= SWITCHER_HOVER_GUARD_PT || card_index >= self.cards.len() {
            return false;
        }
        self.selected_index = card_index;
        true
    }

    /// Release `⌃`: returns `(selected_tab, grid_was_drawn)`.
    #[must_use]
    pub fn commit_release(&self) -> (Option<TabId>, bool) {
        (
            self.cards.get(self.selected_index).copied(),
            self.grid_visible,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_group_and_reopening_restores_whole_and_empty_group_disappears() {
        let mut mgr = TabGroupsManager::new();
        let space = SpaceId::new();
        let t1 = TabId::new(1);
        let t2 = TabId::new(2);

        let gid = mgr
            .create_group(space, "Research", SpaceColor::Violet, vec![t1, t2])
            .expect("create group");
        assert!(TabGroupsManager::extension_group_number(gid) > 0);

        // Child tab opened from t1 automatically joins the group.
        let t3 = TabId::new(3);
        assert_eq!(mgr.inherit_group_from_opener(t1, t3), Some(gid));
        assert_eq!(mgr.groups_in_space(space)[0].tabs, vec![t1, t3, t2]);

        // Close group and ⇧⌘T restores it whole (`P3-06`).
        mgr.close_group(gid).expect("close group");
        assert!(mgr.groups_in_space(space).is_empty());
        let restored = mgr.reopen_last_closed_group().expect("reopen group");
        assert_eq!(restored.group.id, gid);
        assert_eq!(mgr.groups_in_space(space).len(), 1);

        // Removing all tabs makes the empty group disappear automatically.
        mgr.remove_tab_from_group(gid, t1);
        mgr.remove_tab_from_group(gid, t2);
        mgr.remove_tab_from_group(gid, t3);
        assert!(mgr.groups_in_space(space).is_empty());
    }

    #[test]
    fn four_panes_layout_and_divider_snaps_within_15pt_with_20pt_unsnap() {
        let t1 = TabId::new(1);
        let t2 = TabId::new(2);
        let t3 = TabId::new(3);
        let t4 = TabId::new(4);

        let mut four = SplitItem::new("s4", vec![t1, t2, t3, t4]).expect("4-pane split");
        four.focused = 2;

        // At 1200 pt width, all 4 panes fit (4 * 250 + 3 * 7 = 1021 <= 1200).
        let frames = layout_split_stage(&four, 1200.0);
        assert_eq!(frames.len(), 4);
        assert!(frames[2].focused);

        // At 800 pt width (< 1021), only the focused pane (t3) is shown, but the split stays intact (`FEATURES.md` §3.7).
        let narrow = layout_split_stage(&four, 800.0);
        assert_eq!(narrow.len(), 1);
        assert_eq!(narrow[0].tab, t3);
        assert_eq!(four.panes.len(), 4);

        // 2-pane divider snap within 15 pt and unsnap past 20 pt.
        let mut two = SplitItem::new("s2", vec![t1, t2]).expect("2-pane split");
        // Stage width 1007 -> available 1000 -> center 500.
        let (share, snapped) =
            drag_two_pane_divider(&mut two, 512.0, 1007.0, false).expect("drag within 15pt");
        assert!(snapped);
        assert!((share - 0.5).abs() < 1e-9);

        // At 518 pt (+18 pt from center, < 20 pt unsnap hysteresis), stays snapped!
        let (_, still_snapped) =
            drag_two_pane_divider(&mut two, 518.0, 1007.0, true).expect("hysteresis");
        assert!(still_snapped);

        // At 530 pt (+30 pt > 20 pt), unsnaps!
        let (new_share, unsnapped) =
            drag_two_pane_divider(&mut two, 530.0, 1007.0, true).expect("unsnap");
        assert!(!unsnapped);
        assert!((new_share - 0.53).abs() < 1e-6);
    }

    #[test]
    fn quick_ctrl_tab_switches_without_drawing_grid_and_moons_scale_log2() {
        let tabs = vec![
            (TabId::new(1), true),
            (TabId::new(2), false),
            (TabId::new(3), false),
        ];
        let moon_short = MoonWindowCandidate {
            window: WindowId::new(),
            title: "Short".into(),
            page_screens: 1.0,
        };
        let moon_long = MoonWindowCandidate {
            window: WindowId::new(),
            title: "Long".into(),
            page_screens: 16.0,
        };
        assert!((moon_short.log2_height_scale() - 0.0).abs() < 1e-9);
        assert!((moon_long.log2_height_scale() - 1.0).abs() < 1e-9);

        let mut sw = TabSwitcherState::begin(
            &tabs,
            vec![moon_short, moon_long],
            false,
            false,
            (100.0, 100.0),
        );
        // Quick release at 50 ms (< 150 ms) selects previous tab (Tab 2) without drawing the grid!
        assert!(!sw.on_hold_elapsed(50));
        assert_eq!(sw.commit_release(), (Some(TabId::new(2)), false));

        // Holding past 150 ms draws the grid; resting cursor (<= 2 pt movement) does not steal selection.
        assert!(sw.on_hold_elapsed(160));
        assert!(!sw.on_pointer_hover(101.0, 100.5, 2));
        assert!(sw.on_pointer_hover(110.0, 100.0, 2));
        assert_eq!(sw.commit_release(), (Some(TabId::new(3)), true));
    }
}
