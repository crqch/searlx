//! Spaces (`P3-01`), Profiles (`P3-02`), and Profiles in one window (`P3-03`)
//! (`FEATURES.md` §6.1, §6.2, §25, §29).
//!
//! Key rules (`FEATURES.md` §6.1, §6.2, §29):
//! - There is always a `"Personal"` profile and a `"Personal"` first space.
//! - **What a space owns:** its tab row, its parked tabs, its split pairs and
//!   tab groups, its pins and pin folders, its website data store (when it has
//!   its own), its own history (only when it is a signed-out space with its own
//!   store), and an optional downloads folder.
//! - **Bookmarks are per profile, never per space** (`FEATURES.md` §6.1, §29).
//! - Deleting a space closes its tabs, queues its store for deletion with retry
//!   intervals (`3 s`, `15 s`, and launch sweep via `spaces.erasing`), and if
//!   it was a signed-out space, retires its own history while keeping the
//!   profile's bookmarks intact.
//! - **A window belongs to one profile for life** (`FEATURES.md` §6.2, §29).
//! - **Profiles in one window (`P3-03`):** switching to another profile's space
//!   swaps whole profile windows in place (inheriting frame, full-screen, and
//!   fold state), keeping sibling profile windows loaded and `"tucked"`
//!   (`tucked = true`); closing the visible window closes the entire merged
//!   group; turning `profiles.oneWindow` off untucks all windows.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::SpaceColor;
use crate::error::{CoreError, Result};
use crate::ids::{ProfileId, SpaceId, TabId, WindowId};
use crate::session::WindowFrame;

/// Retry schedule in seconds for asynchronous WebKit store erasure (`3 s`, `15 s`, `FEATURES.md` §6.1).
pub const SPACE_STORE_ERASURE_RETRY_SECS: [u64; 2] = [3, 15];

/// Named gradient presets for spaces (`FEATURES.md` §6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpaceGradientPreset {
    /// Sunrise.
    Sunrise,
    /// Lagoon.
    Lagoon,
    /// Orchard.
    Orchard,
    /// Dusk.
    Dusk,
    /// Glacier.
    Glacier,
    /// Ember.
    Ember,
}

/// Wash strength for space colors and gradients (`FEATURES.md` §6.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WashStrength {
    /// 7% light / 11% dark.
    #[default]
    Subtle,
    /// 18% light / 22% dark.
    Rich,
    /// 30% light / 40% dark.
    Vivid,
}

impl WashStrength {
    /// Opacity fraction `(light_opacity, dark_opacity)` (`FEATURES.md` §6.1).
    #[must_use]
    pub const fn opacity_pair(self) -> (f32, f32) {
        match self {
            Self::Subtle => (0.07, 0.11),
            Self::Rich => (0.18, 0.22),
            Self::Vivid => (0.30, 0.40),
        }
    }
}

/// Store-sharing policy for a space (`FEATURES.md` §6.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpaceStorePolicy {
    /// Shares cookies and sign-ins with the profile's first space (`Personal`).
    #[default]
    SharedWithFirst,
    /// Uses its own isolated website data store and its own history file (`history-space-<uuid>.json`).
    OwnStore,
}

/// Runtime model of one Space (`P3-01`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpaceEntry {
    /// Stable space UUID.
    pub id: SpaceId,
    /// Owning profile UUID.
    pub profile: ProfileId,
    /// Display name (e.g. `"Personal"`, `"Work"`).
    pub name: String,
    /// Chrome tint color.
    pub color: SpaceColor,
    /// Icon name from the 24-icon palette.
    pub icon: String,
    /// Optional gradient preset.
    pub gradient: Option<SpaceGradientPreset>,
    /// Wash strength.
    pub strength: WashStrength,
    /// Cookie/storage isolation policy.
    pub store_policy: SpaceStorePolicy,
    /// Optional per-space downloads folder override.
    pub downloads_folder: Option<String>,
    /// Tabs currently open or parked in this space.
    pub tabs: Vec<TabId>,
    /// Active tab index in `tabs`.
    pub active_tab: usize,
}

impl SpaceEntry {
    /// Whether this space uses its own history file (`history-space-<id>.json`)
    /// rather than the profile's shared `history-<profile>.json` (`FEATURES.md` §6.1).
    #[must_use]
    pub fn uses_own_history_file(&self) -> bool {
        self.store_policy == SpaceStorePolicy::OwnStore
    }

    /// History file name for this space (`FEATURES.md` §6.1, §25).
    #[must_use]
    pub fn history_file_name(&self, is_default_profile: bool) -> String {
        if self.uses_own_history_file() {
            format!("history-space-{}.json", self.id)
        } else if is_default_profile {
            "history.json".to_owned()
        } else {
            format!("history-{}.json", self.profile)
        }
    }
}

/// Runtime model of a Profile (`P3-02`, `FEATURES.md` §6.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntry {
    /// Stable profile UUID.
    pub id: ProfileId,
    /// Display name (`"Personal"` for the default profile).
    pub name: String,
    /// Whether this is the permanent first profile (`Personal`).
    pub is_personal: bool,
    /// Home space ID for this profile.
    pub home_space: SpaceId,
    /// Bookmarks file name (`bookmarks.json` or `bookmarks-<profile>.json`).
    pub bookmarks_file: String,
    /// Archive file name (`archive.json` or `archive-<profile>.json`).
    pub archive_file: String,
}

/// A profile-bound browser window (`P3-02`, `P3-03`, `FEATURES.md` §6.2).
///
/// The `profile` field is private with no setter so a window can never change
/// its profile after creation (`FEATURES.md` §6.2, §29).
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileWindow {
    /// Window ID.
    pub id: WindowId,
    /// Immutable profile owner for the lifetime of this window.
    profile: ProfileId,
    /// Active space in this window ( always belongs to `self.profile`).
    pub active_space: SpaceId,
    /// Window geometry.
    pub frame: WindowFrame,
    /// Whether the window is in full screen.
    pub fullscreen: bool,
    /// Whether the sidebar is folded (`⌘S`).
    pub folded: bool,
    /// Whether this window is part of a merged one-window group (`profiles.oneWindow`).
    pub merged: bool,
    /// Whether this window is currently tucked (hidden behind the active profile window).
    pub tucked: bool,
}

impl ProfileWindow {
    /// Create a window permanently bound to `profile`.
    #[must_use]
    pub fn new(id: WindowId, profile: ProfileId, active_space: SpaceId) -> Self {
        Self {
            id,
            profile,
            active_space,
            frame: WindowFrame::default(),
            fullscreen: false,
            folded: false,
            merged: false,
            tucked: false,
        }
    }

    /// The profile this window belongs to for life.
    #[must_use]
    pub const fn profile(&self) -> ProfileId {
        self.profile
    }
}

/// Controller for profiles, spaces, store erasure retries, and one-window profile swapping (`P3-01..P3-03`).
#[derive(Clone, Debug)]
pub struct SpacesAndProfilesManager {
    profiles: Vec<ProfileEntry>,
    spaces: Vec<SpaceEntry>,
    windows: Vec<ProfileWindow>,
    /// Store IDs awaiting erasure retry (`spaces.erasing`, `FEATURES.md` §6.1).
    erasing_stores: Vec<SpaceId>,
    /// Own-store space history files retired by Delete Space.
    retired_space_histories: Vec<String>,
    /// Last closed merged window group for `⇧⌘T` restoration.
    closed_merged_group: Vec<ProfileWindow>,
}

impl SpacesAndProfilesManager {
    /// Create a manager initialized with the default `"Personal"` profile,
    /// `"Personal"` space, and primary window.
    #[must_use]
    pub fn new() -> Self {
        let personal_profile_id = ProfileId::new();
        let personal_space_id = SpaceId::new();
        let profile = ProfileEntry {
            id: personal_profile_id,
            name: "Personal".into(),
            is_personal: true,
            home_space: personal_space_id,
            bookmarks_file: "bookmarks.json".into(),
            archive_file: "archive.json".into(),
        };
        let space = SpaceEntry {
            id: personal_space_id,
            profile: personal_profile_id,
            name: "Personal".into(),
            color: SpaceColor::Blue,
            icon: "person".into(),
            gradient: None,
            strength: WashStrength::Subtle,
            store_policy: SpaceStorePolicy::SharedWithFirst,
            downloads_folder: None,
            tabs: Vec::new(),
            active_tab: 0,
        };
        let window = ProfileWindow::new(WindowId::new(), personal_profile_id, personal_space_id);
        Self {
            profiles: vec![profile],
            spaces: vec![space],
            windows: vec![window],
            erasing_stores: Vec::new(),
            retired_space_histories: Vec::new(),
            closed_merged_group: Vec::new(),
        }
    }

    /// All profiles.
    #[must_use]
    pub fn profiles(&self) -> &[ProfileEntry] {
        &self.profiles
    }

    /// All spaces across profiles.
    #[must_use]
    pub fn spaces(&self) -> &[SpaceEntry] {
        &self.spaces
    }

    /// Spaces belonging to `profile`.
    #[must_use]
    pub fn spaces_for_profile(&self, profile: ProfileId) -> Vec<&SpaceEntry> {
        self.spaces
            .iter()
            .filter(|s| s.profile == profile)
            .collect()
    }

    /// Mutable lookup of a space by ID.
    pub fn space_mut(&mut self, id: SpaceId) -> Option<&mut SpaceEntry> {
        self.spaces.iter_mut().find(|s| s.id == id)
    }

    /// Lookup a space by ID.
    #[must_use]
    pub fn space(&self, id: SpaceId) -> Option<&SpaceEntry> {
        self.spaces.iter().find(|s| s.id == id)
    }

    /// All windows.
    #[must_use]
    pub fn windows(&self) -> &[ProfileWindow] {
        &self.windows
    }

    /// Pending store IDs queued in `spaces.erasing` for asynchronous deletion retry (`FEATURES.md` §6.1).
    #[must_use]
    pub fn erasing_stores(&self) -> &[SpaceId] {
        &self.erasing_stores
    }

    /// Create a new space inside `profile` (`P3-01`).
    pub fn create_space(
        &mut self,
        profile: ProfileId,
        name: impl Into<String>,
        color: SpaceColor,
        icon: impl Into<String>,
        store_policy: SpaceStorePolicy,
    ) -> Result<SpaceId> {
        if !self.profiles.iter().any(|p| p.id == profile) {
            return Err(CoreError::Invalid("unknown profile".into()));
        }
        let id = SpaceId::new();
        self.spaces.push(SpaceEntry {
            id,
            profile,
            name: name.into(),
            color,
            icon: icon.into(),
            gradient: None,
            strength: WashStrength::Subtle,
            store_policy,
            downloads_folder: None,
            tabs: Vec::new(),
            active_tab: 0,
        });
        Ok(id)
    }

    /// Delete a space (`FEATURES.md` §6.1, P3-01):
    /// - The profile's first/home space cannot be deleted (`FEATURES.md` §24).
    /// - Queues the space's store in `erasing_stores` (`spaces.erasing`).
    /// - If the space had its own store, retires its own history file while
    ///   leaving the profile's bookmarks untouched.
    pub fn delete_space(&mut self, id: SpaceId) -> Result<()> {
        if self.profiles.iter().any(|p| p.home_space == id) {
            return Err(CoreError::Invalid(
                "the home space of a profile cannot be deleted".into(),
            ));
        }
        let Some(pos) = self.spaces.iter().position(|s| s.id == id) else {
            return Err(CoreError::Invalid("no such space".into()));
        };
        let removed = self.spaces.remove(pos);
        if removed.store_policy == SpaceStorePolicy::OwnStore {
            self.erasing_stores.push(removed.id);
            self.retired_space_histories
                .push(format!("history-space-{}.json", removed.id));
        }
        // Switch any window that was showing `id` back to its profile's home space.
        if let Some(home) = self
            .profiles
            .iter()
            .find(|p| p.id == removed.profile)
            .map(|p| p.home_space)
        {
            for win in &mut self.windows {
                if win.active_space == id {
                    win.active_space = home;
                }
            }
        }
        Ok(())
    }

    /// Create a new profile with its own home space and dedicated window (`P3-02`).
    pub fn create_profile(&mut self, name: impl Into<String>) -> (ProfileId, SpaceId, WindowId) {
        let profile_id = ProfileId::new();
        let home_space_id = SpaceId::new();
        let name_str = name.into();

        self.profiles.push(ProfileEntry {
            id: profile_id,
            name: name_str.clone(),
            is_personal: false,
            home_space: home_space_id,
            bookmarks_file: format!("bookmarks-{profile_id}.json"),
            archive_file: format!("archive-{profile_id}.json"),
        });
        self.spaces.push(SpaceEntry {
            id: home_space_id,
            profile: profile_id,
            name: name_str,
            color: SpaceColor::Green,
            icon: "briefcase".into(),
            gradient: None,
            strength: WashStrength::Subtle,
            store_policy: SpaceStorePolicy::OwnStore,
            downloads_folder: None,
            tabs: Vec::new(),
            active_tab: 0,
        });
        let win_id = WindowId::new();
        self.windows
            .push(ProfileWindow::new(win_id, profile_id, home_space_id));
        (profile_id, home_space_id, win_id)
    }

    /// Delete a non-Personal profile, removing its spaces and windows and
    /// queueing its stores for erasure (`P3-02`).
    pub fn delete_profile(&mut self, profile_id: ProfileId) -> Result<Vec<String>> {
        let Some(pos) = self.profiles.iter().position(|p| p.id == profile_id) else {
            return Err(CoreError::Invalid("no such profile".into()));
        };
        if self.profiles[pos].is_personal {
            return Err(CoreError::Invalid(
                "the Personal profile cannot be deleted".into(),
            ));
        }
        let removed_profile = self.profiles.remove(pos);
        let mut deleted_files = vec![
            removed_profile.bookmarks_file,
            removed_profile.archive_file,
            format!("history-{profile_id}.json"),
        ];

        let mut i = 0;
        while i < self.spaces.len() {
            if self.spaces[i].profile == profile_id {
                let sp = self.spaces.remove(i);
                self.erasing_stores.push(sp.id);
                deleted_files.push(format!("session-{}.json", sp.id));
                if sp.uses_own_history_file() {
                    deleted_files.push(format!("history-space-{}.json", sp.id));
                }
            } else {
                i += 1;
            }
        }
        self.windows.retain(|w| w.profile != profile_id);
        Ok(deleted_files)
    }

    /// Switch to `target_space` from `current_window_id` (`P3-01`, `P3-03`, `FEATURES.md` §6.1, §6.2):
    /// - If `target_space` belongs to the same profile as `current_window_id`,
    ///   simply switches `active_space` in that window (parking the old space's tabs).
    /// - If `target_space` belongs to **another profile** and `one_window_mode`
    ///   is enabled (`P3-03`): swaps the target profile's window into the
    ///   current window's frame, full-screen state, and fold state, marks both
    ///   `merged = true`, tucks the old window (`tucked = true`), and untucks
    ///   the target profile's window (`tucked = false`). The old window's
    ///   `profile` never changes!
    pub fn switch_to_space(
        &mut self,
        current_window_id: WindowId,
        target_space: SpaceId,
        one_window_mode: bool,
    ) -> Result<WindowId> {
        let space_profile = self
            .space(target_space)
            .ok_or_else(|| CoreError::Invalid("unknown target space".into()))?
            .profile;
        let cur_idx = self
            .windows
            .iter()
            .position(|w| w.id == current_window_id)
            .ok_or_else(|| CoreError::Invalid("unknown current window".into()))?;

        if self.windows[cur_idx].profile == space_profile {
            self.windows[cur_idx].active_space = target_space;
            return Ok(current_window_id);
        }

        if !one_window_mode {
            return Err(CoreError::Invalid(
                "cannot switch a window to another profile's space when profiles.oneWindow is off"
                    .into(),
            ));
        }

        let inherited_frame = self.windows[cur_idx].frame;
        let inherited_fullscreen = self.windows[cur_idx].fullscreen;
        let inherited_folded = self.windows[cur_idx].folded;

        let target_win_idx = self
            .windows
            .iter()
            .position(|w| w.profile == space_profile)
            .ok_or_else(|| CoreError::Invalid("target profile has no window".into()))?;

        for (idx, win) in self.windows.iter_mut().enumerate() {
            win.merged = true;
            if idx == target_win_idx {
                win.tucked = false;
                win.active_space = target_space;
                win.frame = inherited_frame;
                win.fullscreen = inherited_fullscreen;
                win.folded = inherited_folded;
            } else {
                win.tucked = true;
            }
        }

        Ok(self.windows[target_win_idx].id)
    }

    /// Close a visible window; if it is part of a merged one-window group,
    /// closes the entire merged group so `⇧⌘T` can reopen the group (`FEATURES.md` §6.2, P3-03).
    pub fn close_window(&mut self, window_id: WindowId) -> Result<usize> {
        let Some(idx) = self.windows.iter().position(|w| w.id == window_id) else {
            return Err(CoreError::Invalid("no such window".into()));
        };
        if self.windows[idx].merged {
            let (merged, remaining): (Vec<_>, Vec<_>) =
                self.windows.drain(..).partition(|w| w.merged);
            let count = merged.len();
            self.closed_merged_group = merged;
            self.windows = remaining;
            Ok(count)
        } else {
            self.windows.remove(idx);
            Ok(1)
        }
    }

    /// Reopen the most recently closed merged window group (`⇧⌘T`, `P3-03`).
    pub fn reopen_closed_merged_group(&mut self) -> usize {
        let count = self.closed_merged_group.len();
        self.windows.append(&mut self.closed_merged_group);
        count
    }

    /// Turn `profiles.oneWindow` off, untucking and unmerging every window (`FEATURES.md` §6.2, P3-03).
    pub fn unmerge_all_windows(&mut self) {
        for win in &mut self.windows {
            win.merged = false;
            win.tucked = false;
        }
    }

    /// Map of profile ID to its own bookmarks file, proving two profiles never share bookmarks (`P3-02`).
    #[must_use]
    pub fn bookmarks_files_by_profile(&self) -> BTreeMap<ProfileId, String> {
        self.profiles
            .iter()
            .map(|p| (p.id, p.bookmarks_file.clone()))
            .collect()
    }
}

impl Default for SpacesAndProfilesManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_route_history_by_store_policy_and_queue_store_deletion_retries() {
        let mut mgr = SpacesAndProfilesManager::new();
        let personal_profile = mgr.profiles()[0].id;
        let personal_space = mgr.spaces()[0].id;

        assert!(
            !mgr.space(personal_space)
                .expect("personal")
                .uses_own_history_file()
        );
        assert!(
            mgr.delete_space(personal_space).is_err(),
            "home space cannot be deleted"
        );

        // Create a signed-out space with its own store.
        let signed_out = mgr
            .create_space(
                personal_profile,
                "Banking",
                SpaceColor::Orange,
                "lock",
                SpaceStorePolicy::OwnStore,
            )
            .expect("create signed-out space");
        assert!(
            mgr.space(signed_out)
                .expect("space")
                .uses_own_history_file()
        );
        assert_eq!(
            mgr.space(signed_out)
                .expect("space")
                .history_file_name(true),
            format!("history-space-{signed_out}.json")
        );

        // Deleting the signed-out space queues its store in `erasing_stores` and retires its history.
        mgr.delete_space(signed_out).expect("delete space");
        assert!(mgr.erasing_stores().contains(&signed_out));
        assert_eq!(SPACE_STORE_ERASURE_RETRY_SECS, [3, 15]);
    }

    #[test]
    fn profiles_isolate_files_keep_immutable_window_profiles_and_swap_in_one_window() {
        let mut mgr = SpacesAndProfilesManager::new();
        let personal_profile = mgr.profiles()[0].id;
        let personal_win = mgr.windows()[0].id;

        let (work_profile, work_space, work_win) = mgr.create_profile("Work");
        assert_ne!(personal_profile, work_profile);

        // Two profiles have distinct bookmarks/history/archive files.
        let bm_files = mgr.bookmarks_files_by_profile();
        assert_ne!(bm_files[&personal_profile], bm_files[&work_profile]);

        // Set custom frame and fold state on the personal window, then switch to Work space in one-window mode.
        mgr.windows[0].frame = WindowFrame {
            x: 120.0,
            y: 90.0,
            width: 1400.0,
            height: 900.0,
        };
        mgr.windows[0].folded = true;

        let active_win_id = mgr
            .switch_to_space(personal_win, work_space, true)
            .expect("swap profile window in place");
        assert_eq!(active_win_id, work_win);

        // Neither window changed its profile! Personal window is tucked, Work window inherited frame & fold.
        let p_win = mgr
            .windows()
            .iter()
            .find(|w| w.id == personal_win)
            .expect("p win");
        let w_win = mgr
            .windows()
            .iter()
            .find(|w| w.id == work_win)
            .expect("w win");
        assert_eq!(p_win.profile(), personal_profile);
        assert_eq!(w_win.profile(), work_profile);
        assert!(p_win.tucked);
        assert!(!w_win.tucked);
        assert_eq!(w_win.frame.width, 1400.0);
        assert!(w_win.folded);

        // Closing the visible window closes the entire merged group; ⇧⌘T restores both.
        assert_eq!(mgr.close_window(work_win).expect("close merged group"), 2);
        assert!(mgr.windows().is_empty());
        assert_eq!(mgr.reopen_closed_merged_group(), 2);
        assert_eq!(mgr.windows().len(), 2);

        // Turning profiles.oneWindow off untucks all windows.
        mgr.unmerge_all_windows();
        assert!(mgr.windows().iter().all(|w| !w.tucked && !w.merged));

        // Deleting Work profile removes its files and queues its stores.
        let deleted_files = mgr
            .delete_profile(work_profile)
            .expect("delete work profile");
        assert!(deleted_files.iter().any(|f| f.starts_with("bookmarks-")));
        assert_eq!(mgr.profiles().len(), 1);
    }
}
