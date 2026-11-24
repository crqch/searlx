//! Unified tab row model, chrome layout state, and single drag session (`FEATURES.md` §2.2, §3.1–3.6, `AGENTS.md` §3.3, P1-06).
//!
//! Key rules:
//! - **One drag session model** ([`DragSession`]) handles tabs, pins, groups,
//!   pin folders, split-edge drops, and window detachment (`AGENTS.md` §3.3).
//! - **Close semantics (`⌘W`, `FEATURES.md` §3.2):**
//!   - A pinned tab is **put down** (page released / `sleeping = true`, wakes
//!     at `home` when `pins_home` is on), not removed.
//!   - An awake tab in a group is **put away** (`sleeping = true`, `put_away = true`);
//!     closing an already put-away grouped tab deletes it.
//!   - Closing the last non-pin tab creates a fresh blank tab.
//! - **New tab (`⌘T`, `FEATURES.md` §3.2):** reuses an existing blank ungrouped
//!   tab if one exists; otherwise inserts at `Next`, `Top`, or `End`.
//! - **Fold (`⌘S`, `FEATURES.md` §2.2):** folds/unfolds the sidebar, or toggles
//!   icons-only mode when `fold_to_icons` is enabled.

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::ids::{GroupId, PinId, TabId, TabIdAllocator};
use crate::settings::{NewTabPlace, TabLayoutMode};
use crate::url::PageUrl;

/// Visual state of a single tab or pin in the row (`FEATURES.md` §3.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabEntry {
    /// Runtime tab handle.
    pub id: TabId,
    /// Current committed or target URL.
    pub url: PageUrl,
    /// Home URL when pinned (`FEATURES.md` §7.1).
    pub home: Option<PageUrl>,
    /// Display title.
    pub title: String,
    /// Custom user-assigned title (`Rename`); a named tab is "kept" and never auto-closes (`FEATURES.md` §3.2, §3.5).
    pub custom_name: Option<String>,
    /// Optional favicon URL.
    pub favicon: Option<PageUrl>,
    /// Single-character letter glyph.
    pub letter: char,
    /// Whether this tab is pinned in the front section.
    pub pinned: bool,
    /// Stable pin UUID when pinned.
    pub pin_id: Option<PinId>,
    /// Optional tab group membership.
    pub group: Option<GroupId>,
    /// Whether a grouped tab has been "put away" by `⌘W` (`FEATURES.md` §3.2).
    pub put_away: bool,
    /// Whether a navigation is currently loading.
    pub loading: bool,
    /// Whether the tab is producing audio.
    pub audible: bool,
    /// Whether audio is muted.
    pub muted: bool,
    /// Whether the tab's web view is released and replaced by its sleep placeholder.
    pub sleeping: bool,
    /// Snapshot label/description shown while sleeping (`FEATURES.md` §3.1, §3.6).
    pub sleep_placeholder: Option<String>,
    /// Whether this tab uses an ephemeral private store (`⇧⌘N`).
    pub private: bool,
    /// Reading scroll progress `0.0..=1.0` (`tabs.reading`, `FEATURES.md` §3.1).
    pub reading_progress: f32,
    /// Live engine webview handle when materialized, or `None` while sleeping (`FEATURES.md` §3.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_tab_id: Option<TabId>,
}

impl TabEntry {
    /// Create a new tab entry.
    #[must_use]
    pub fn new(id: TabId, url: PageUrl, title: impl Into<String>, private: bool) -> Self {
        let title = title.into();
        let letter = compute_letter(&url, &title);
        Self {
            id,
            url,
            home: None,
            title,
            custom_name: None,
            favicon: None,
            letter,
            pinned: false,
            pin_id: None,
            group: None,
            put_away: false,
            loading: false,
            audible: false,
            muted: false,
            sleeping: false,
            sleep_placeholder: None,
            private,
            reading_progress: 0.0,
            engine_tab_id: None,
        }
    }

    /// Whether the tab is blank (`search://`, `search://newtab`, or `about:blank`) and not in a group or pin.
    #[must_use]
    pub fn is_reusable_blank(&self) -> bool {
        !self.private
            && !self.pinned
            && self.group.is_none()
            && matches!(
                self.url.as_str(),
                "search://" | "search://newtab" | "about:blank"
            )
    }

    /// Effective title shown in the row.
    #[must_use]
    pub fn display_title(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.title)
    }

    /// Whether this tab is protected from auto-close ("kept", `FEATURES.md` §3.5).
    #[must_use]
    pub fn is_kept(&self) -> bool {
        self.pinned || self.custom_name.is_some() || self.group.is_some()
    }
}

fn compute_letter(url: &PageUrl, title: &str) -> char {
    url.host()
        .and_then(|h| h.trim_start_matches("www.").chars().next())
        .or_else(|| title.trim().chars().next())
        .map_or('·', |c| c.to_ascii_uppercase())
}

/// Which edge of a page a dragged tab is hovering over for Split View creation (`FEATURES.md` §3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDropSide {
    /// Left band (`max(120 pt, 24% of page width)`).
    Left,
    /// Right band (`max(120 pt, 24% of page width)`).
    Right,
}

/// What item is being carried in a [`DragSession`] (`AGENTS.md` §3.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DragPayload {
    /// An ordinary or pinned tab.
    Tab(TabId),
    /// A whole tab group by its heading.
    Group(GroupId),
    /// A pin folder by its identifier.
    PinFolder(String),
}

/// Where a [`DragSession`] will land if released (`AGENTS.md` §3.3, `FEATURES.md` §3.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DropTarget {
    /// Drop onto the pin section at `index` (pins the tab if it was loose).
    PinSlot(usize),
    /// Drop into the loose tab list at `index` (unpins the tab if it was a pin).
    LooseTabSlot(usize),
    /// Drop into a tab group at `index`.
    GroupSlot {
        /// Target group.
        group: GroupId,
        /// Position within the group.
        index: usize,
    },
    /// Drop into a pin folder.
    PinFolderSlot {
        /// Target folder ID.
        folder_id: String,
    },
    /// Drop onto the left or right edge band of a page to create/join a Split View.
    PageSplitEdge {
        /// Page tab being split.
        target_tab: TabId,
        /// Left or right side.
        side: SplitDropSide,
    },
    /// Dragged outside the window past the 12 pt slack to detach into a new window (`FEATURES.md` §3.3).
    DetachWindow,
}

/// Unified drag session state machine for tabs, pins, groups, folders, and split drops (`AGENTS.md` §3.3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DragSession {
    /// Item being dragged.
    pub payload: DragPayload,
    /// Starting pointer X in window coordinates.
    pub origin_x: f64,
    /// Starting pointer Y in window coordinates.
    pub origin_y: f64,
    /// Current pointer X in window coordinates.
    pub current_x: f64,
    /// Current pointer Y in window coordinates.
    pub current_y: f64,
    /// True once pointer travel exceeds the 5 pt threshold (`FEATURES.md` §3.6),
    /// cancelling tab-give press scale and activating the floating lift preview.
    pub active: bool,
    /// Current drop target under the pointer.
    pub target: Option<DropTarget>,
}

impl DragSession {
    /// Pointer travel threshold (5 pt) before a press becomes a drag (`FEATURES.md` §3.6).
    pub const DRAG_THRESHOLD_PT: f64 = 5.0;
    /// Slack outside the window (12 pt) before a loose tab detaches (`FEATURES.md` §3.3).
    pub const WINDOW_DETACH_SLACK_PT: f64 = 12.0;

    /// Begin a potential drag gesture at `(x, y)`.
    #[must_use]
    pub const fn begin(payload: DragPayload, x: f64, y: f64) -> Self {
        Self {
            payload,
            origin_x: x,
            origin_y: y,
            current_x: x,
            current_y: y,
            active: false,
            target: None,
        }
    }

    /// Update pointer coordinates and target.
    pub fn update(&mut self, x: f64, y: f64, target: Option<DropTarget>) {
        self.current_x = x;
        self.current_y = y;
        let dx = x - self.origin_x;
        let dy = y - self.origin_y;
        if (dx * dx + dy * dy).sqrt() > Self::DRAG_THRESHOLD_PT {
            self.active = true;
        }
        if self.active {
            self.target = target;
        }
    }

    /// Test whether `(x, y)` inside a page rect of width `page_width` falls in
    /// the left or right split drop band (`max(120 pt, 24% of page)`, `FEATURES.md` §3.3).
    #[must_use]
    pub fn classify_page_split_band(local_x: f64, page_width: f64) -> Option<SplitDropSide> {
        if page_width <= 0.0 || local_x < 0.0 || local_x > page_width {
            return None;
        }
        let band = (page_width * 0.24).max(120.0).min(page_width * 0.45);
        if local_x <= band {
            Some(SplitDropSide::Left)
        } else if local_x >= page_width - band {
            Some(SplitDropSide::Right)
        } else {
            None
        }
    }
}

/// Complete tab row, layout, and drag controller (`P1-06`).
#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct TabModel {
    tabs: Vec<TabEntry>,
    active: usize,
    previous_active: Option<usize>,
    layout: TabLayoutMode,
    folded: bool,
    fold_to_icons: bool,
    layout_before_fold_icons: Option<TabLayoutMode>,
    new_tab_place: NewTabPlace,
    close_goes_back: bool,
    pins_go_home: bool,
    new_after_last: bool,
    tabs_lazy: bool,
    drag: Option<DragSession>,
}

impl TabModel {
    /// Create a tab model with an initial blank tab.
    pub fn new(allocator: &mut TabIdAllocator) -> Result<Self> {
        let id = allocator.allocate();
        let blank = TabEntry::new(id, PageUrl::parse("search://")?, "New Tab", false);
        Ok(Self {
            tabs: vec![blank],
            active: 0,
            previous_active: None,
            layout: TabLayoutMode::LeftColumn,
            folded: false,
            fold_to_icons: false,
            layout_before_fold_icons: None,
            new_tab_place: NewTabPlace::Next,
            close_goes_back: false,
            pins_go_home: true,
            new_after_last: false,
            tabs_lazy: false,
            drag: None,
        })
    }

    /// Restore a tab model from existing tabs (creating a blank tab if `tabs` is empty).
    pub fn from_tabs(
        mut tabs: Vec<TabEntry>,
        active: usize,
        allocator: &mut TabIdAllocator,
    ) -> Result<Self> {
        if tabs.is_empty() {
            let id = allocator.allocate();
            tabs.push(TabEntry::new(
                id,
                PageUrl::parse("search://")?,
                "New Tab",
                false,
            ));
        }
        let active = active.min(tabs.len() - 1);
        Ok(Self {
            tabs,
            active,
            previous_active: None,
            layout: TabLayoutMode::LeftColumn,
            folded: false,
            fold_to_icons: false,
            layout_before_fold_icons: None,
            new_tab_place: NewTabPlace::Next,
            close_goes_back: false,
            pins_go_home: true,
            new_after_last: false,
            tabs_lazy: false,
            drag: None,
        })
    }

    /// Slice of all tabs (pins always precede loose tabs).
    #[must_use]
    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }

    /// Mutable access to a tab by ID.
    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut TabEntry> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    /// Lookup a tab by ID.
    #[must_use]
    pub fn tab(&self, id: TabId) -> Option<&TabEntry> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// Currently active tab index.
    #[must_use]
    pub const fn active_index(&self) -> usize {
        self.active
    }

    /// Currently active tab entry.
    #[must_use]
    pub fn active_tab(&self) -> &TabEntry {
        &self.tabs[self.active]
    }

    /// Mutable reference to the currently active tab entry.
    pub fn active_tab_mut(&mut self) -> &mut TabEntry {
        &mut self.tabs[self.active]
    }

    /// Current layout mode (`LeftColumn`, `RightColumn`, `TopTabs`, `IconsOnly`).
    #[must_use]
    pub const fn layout(&self) -> TabLayoutMode {
        self.layout
    }

    /// Set the chrome layout mode.
    pub fn set_layout(&mut self, layout: TabLayoutMode) {
        self.layout = layout;
        self.layout_before_fold_icons = None;
    }

    /// Whether the sidebar/strip is currently folded (`⌘S`).
    #[must_use]
    pub const fn is_folded(&self) -> bool {
        self.folded
    }

    /// Configure whether `⌘S` toggles `IconsOnly` instead of hiding the column (`sidebar.foldIcons`, `FEATURES.md` §2.2).
    pub fn set_fold_to_icons(&mut self, enabled: bool) {
        self.fold_to_icons = enabled;
    }

    /// Configure new-tab placement (`Next`, `Top`, `End`).
    pub fn set_new_tab_place(&mut self, place: NewTabPlace) {
        self.new_tab_place = place;
    }

    /// Configure whether closing the active tab returns to the previously active tab.
    pub fn set_close_goes_back(&mut self, enabled: bool) {
        self.close_goes_back = enabled;
    }

    /// Configure whether `⌘W` on a pin resets its URL to `home` (`pins.home`).
    pub fn set_pins_go_home(&mut self, enabled: bool) {
        self.pins_go_home = enabled;
    }

    /// Whether closing the active tab returns to the previously active tab.
    #[must_use]
    pub const fn close_goes_back(&self) -> bool {
        self.close_goes_back
    }

    /// Whether `⌘W` on a pin resets its URL to `home` (`pins.home`).
    #[must_use]
    pub const fn pins_go_home(&self) -> bool {
        self.pins_go_home
    }

    /// Configure whether closing the last loose tab opens a blank tab even if pins are awake (`tabs.newAfterLast`).
    pub fn set_new_after_last(&mut self, enabled: bool) {
        self.new_after_last = enabled;
    }

    /// Whether closing the last loose tab always opens a blank tab (`tabs.newAfterLast`).
    #[must_use]
    pub const fn new_after_last(&self) -> bool {
        self.new_after_last
    }

    /// Configure whether tabs restore/open lazily (`tabs.lazy`).
    pub fn set_tabs_lazy(&mut self, enabled: bool) {
        self.tabs_lazy = enabled;
    }

    /// Whether tabs open/restore lazily (`tabs.lazy`).
    #[must_use]
    pub const fn tabs_lazy(&self) -> bool {
        self.tabs_lazy
    }

    /// Associate or clear the live engine webview handle on `id`.
    pub fn set_engine_tab_id(&mut self, id: TabId, engine_tab_id: Option<TabId>) {
        if let Some(tab) = self.tab_mut(id) {
            tab.engine_tab_id = engine_tab_id;
        }
    }

    /// Explicitly update sleeping/placeholder metadata on `id`.
    pub fn set_sleeping_state(&mut self, id: TabId, sleeping: bool, placeholder: Option<String>) {
        if let Some(tab) = self.tab_mut(id) {
            tab.sleeping = sleeping;
            tab.sleep_placeholder = placeholder;
            if sleeping {
                tab.loading = false;
                tab.audible = false;
            }
        }
    }

    /// Clear `put_away` and wake a grouped tab.
    pub fn wake_tab(&mut self, id: TabId) {
        if let Some(tab) = self.tab_mut(id) {
            tab.sleeping = false;
            tab.put_away = false;
        }
    }

    /// Toggle sidebar fold (`⌘S`, `FEATURES.md` §2.2).
    pub fn toggle_fold(&mut self) {
        if self.fold_to_icons && self.layout != TabLayoutMode::TopTabs {
            if let Some(prev) = self.layout_before_fold_icons.take() {
                self.layout = prev;
            } else if self.layout == TabLayoutMode::IconsOnly {
                self.layout = TabLayoutMode::LeftColumn;
            } else {
                self.layout_before_fold_icons = Some(self.layout);
                self.layout = TabLayoutMode::IconsOnly;
            }
            self.folded = false;
        } else {
            self.folded = !self.folded;
        }
    }

    /// Select a tab by index, waking it if it was asleep (`FEATURES.md` §3.2).
    pub fn select_index(&mut self, index: usize) -> Option<TabId> {
        if index >= self.tabs.len() {
            return None;
        }
        if index != self.active {
            self.previous_active = Some(self.active);
            self.active = index;
        }
        let tab = &mut self.tabs[self.active];
        tab.sleeping = false;
        tab.put_away = false;
        Some(tab.id)
    }

    /// Select a tab by ID.
    pub fn select_tab(&mut self, id: TabId) -> bool {
        if let Some(index) = self.tabs.iter().position(|t| t.id == id) {
            self.select_index(index);
            true
        } else {
            false
        }
    }

    /// Number of pinned tabs at the front of the row.
    #[must_use]
    pub fn pin_count(&self) -> usize {
        self.tabs.iter().take_while(|t| t.pinned).count()
    }

    /// Open a new tab (`⌘T` or `⇧⌘N`, `FEATURES.md` §3.2).
    ///
    /// For non-private blank tabs, reuses an existing ungrouped blank tab if one
    /// exists; otherwise inserts at the configured [`NewTabPlace`].
    pub fn create_tab(
        &mut self,
        allocator: &mut TabIdAllocator,
        url: Option<PageUrl>,
        title: impl Into<String>,
        private: bool,
        activate: bool,
    ) -> Result<TabId> {
        self.create_tab_with_options(allocator, url, title, private, activate, None, false)
    }

    /// Open a new tab with explicit `opener` (for `NewTabPlace::Next`) and initial `sleeping` flag.
    #[allow(clippy::too_many_arguments)]
    pub fn create_tab_with_options(
        &mut self,
        allocator: &mut TabIdAllocator,
        url: Option<PageUrl>,
        title: impl Into<String>,
        private: bool,
        activate: bool,
        opener: Option<TabId>,
        sleeping: bool,
    ) -> Result<TabId> {
        let is_blank_request = url.is_none();
        if !private
            && is_blank_request
            && let Some(existing_idx) = self.tabs.iter().position(TabEntry::is_reusable_blank)
        {
            if activate {
                self.select_index(existing_idx);
            }
            return Ok(self.tabs[existing_idx].id);
        }

        let target_url = match url {
            Some(u) => u,
            None => PageUrl::parse("search://")?,
        };
        let id = allocator.allocate();
        let mut entry = TabEntry::new(id, target_url, title, private);
        if !activate && sleeping {
            entry.sleeping = true;
            entry.sleep_placeholder = Some(entry.display_title().to_owned());
        }

        let pin_boundary = self.pin_count();
        let next_anchor = opener
            .and_then(|op_id| self.tabs.iter().position(|t| t.id == op_id))
            .unwrap_or(self.active);
        let insert_idx = match self.new_tab_place {
            NewTabPlace::Top => pin_boundary,
            NewTabPlace::End => self.tabs.len(),
            NewTabPlace::Next => (next_anchor + 1).max(pin_boundary).min(self.tabs.len()),
        };

        let had_tabs = !self.tabs.is_empty();
        if had_tabs {
            if insert_idx <= self.active {
                self.active += 1;
            }
            if let Some(prev) = self.previous_active
                && insert_idx <= prev
            {
                self.previous_active = Some(prev + 1);
            }
        }
        self.tabs.insert(insert_idx, entry);
        if activate {
            self.previous_active = had_tabs.then_some(self.active);
            self.active = insert_idx;
        }
        Ok(id)
    }

    /// Duplicate an existing tab right next to it (preserving group membership,
    /// placing duplicated pins as loose tabs at the start of the loose section).
    pub fn duplicate_tab(
        &mut self,
        allocator: &mut TabIdAllocator,
        source_id: TabId,
        activate: bool,
    ) -> Result<TabId> {
        let Some(src_idx) = self.tabs.iter().position(|t| t.id == source_id) else {
            return Err(CoreError::Invalid(format!("no such tab: {source_id}")));
        };
        let src = self.tabs[src_idx].clone();
        let new_id = allocator.allocate();
        let mut copy = TabEntry::new(new_id, src.url, src.title, src.private);
        copy.favicon = src.favicon;
        copy.group = src.group;
        copy.muted = src.muted;
        if !activate {
            copy.sleeping = true;
            copy.sleep_placeholder = Some(copy.display_title().to_owned());
        }
        let pin_boundary = self.pin_count();
        let insert_idx = if src.pinned {
            pin_boundary
        } else {
            (src_idx + 1).max(pin_boundary).min(self.tabs.len())
        };
        if insert_idx <= self.active {
            self.active += 1;
        }
        if let Some(prev) = self.previous_active
            && insert_idx <= prev
        {
            self.previous_active = Some(prev + 1);
        }
        self.tabs.insert(insert_idx, copy);
        if activate {
            self.previous_active = Some(self.active);
            self.active = insert_idx;
        }
        Ok(new_id)
    }

    /// Close a tab (`⌘W`, `FEATURES.md` §3.2):
    /// - A pinned tab is **put down** (`sleeping = true`, returns to `home` if `pins_go_home`).
    /// - An awake grouped tab is **put away** (`sleeping = true`, `put_away = true`);
    ///   closing a put-away grouped tab removes it.
    /// - Removing the last visible loose tab creates a fresh blank tab if `new_after_last` is set or no pin is awake.
    pub fn close_tab(&mut self, id: TabId, allocator: &mut TabIdAllocator) -> Result<()> {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return Err(CoreError::Invalid(format!("no such tab: {id}")));
        };

        if self.tabs[index].pinned {
            {
                let tab = &mut self.tabs[index];
                tab.sleeping = true;
                tab.loading = false;
                tab.audible = false;
                tab.engine_tab_id = None;
                if self.pins_go_home
                    && let Some(home) = tab.home.clone()
                {
                    tab.url = home;
                    tab.letter = compute_letter(&tab.url, &tab.title);
                }
                tab.sleep_placeholder = Some(tab.display_title().to_owned());
            }
            if index == self.active
                && let Some(next_idx) = self.find_fallback_active_after_hide(index)
            {
                self.previous_active = None;
                self.active = next_idx;
                self.tabs[next_idx].sleeping = false;
                self.tabs[next_idx].put_away = false;
            }
            return Ok(());
        }

        if self.tabs[index].group.is_some() && !self.tabs[index].put_away {
            {
                let tab = &mut self.tabs[index];
                tab.sleeping = true;
                tab.put_away = true;
                tab.loading = false;
                tab.audible = false;
                tab.engine_tab_id = None;
                tab.sleep_placeholder = Some(tab.display_title().to_owned());
            }
            if index == self.active {
                if let Some(next_idx) = self.find_fallback_active_after_hide(index) {
                    self.previous_active = None;
                    self.active = next_idx;
                    self.tabs[next_idx].sleeping = false;
                    self.tabs[next_idx].put_away = false;
                } else if self.new_after_last {
                    let blank_id = allocator.allocate();
                    let blank =
                        TabEntry::new(blank_id, PageUrl::parse("search://")?, "New Tab", false);
                    self.tabs.push(blank);
                    self.active = self.tabs.len() - 1;
                    self.previous_active = None;
                }
            }
            return Ok(());
        }

        self.tabs.remove(index);
        self.previous_active = self.previous_active.and_then(|prev| {
            if prev == index {
                None
            } else if prev > index {
                Some(prev - 1)
            } else {
                Some(prev)
            }
        });

        let has_awake_pin = self.tabs.iter().any(|t| t.pinned && !t.sleeping);
        let has_visible_loose = self.tabs.iter().any(|t| !t.pinned && !t.put_away);
        if self.tabs.is_empty() || (!has_visible_loose && (self.new_after_last || !has_awake_pin)) {
            let blank_id = allocator.allocate();
            let blank = TabEntry::new(blank_id, PageUrl::parse("search://")?, "New Tab", false);
            self.tabs.push(blank);
            self.active = self.tabs.len() - 1;
            self.previous_active = None;
            return Ok(());
        }

        if index == self.active {
            if self.close_goes_back
                && let Some(prev) = self.previous_active.take()
                && prev < self.tabs.len()
                && !self.tabs[prev].put_away
                && !(self.tabs[prev].pinned && self.tabs[prev].sleeping)
            {
                self.active = prev;
            } else {
                let candidate = index.min(self.tabs.len() - 1);
                self.active = self.nearest_eligible_index(candidate).unwrap_or(candidate);
            }
        } else if index < self.active {
            self.active -= 1;
        }
        self.tabs[self.active].sleeping = false;
        self.tabs[self.active].put_away = false;
        Ok(())
    }

    /// Pin a tab to the front section (`FEATURES.md` §3.2, §7.1).
    /// Private tabs cannot be pinned.
    pub fn pin_tab(&mut self, id: TabId) -> Result<PinId> {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return Err(CoreError::Invalid(format!("no such tab: {id}")));
        };
        if self.tabs[index].private {
            return Err(CoreError::Invalid("private tabs cannot be pinned".into()));
        }
        if self.tabs[index].pinned {
            return Ok(self.tabs[index].pin_id.unwrap_or_default());
        }

        let active_id = self.tabs[self.active].id;
        let prev_id = self
            .previous_active
            .and_then(|idx| self.tabs.get(idx))
            .map(|t| t.id);
        let mut entry = self.tabs.remove(index);
        let pin_id = PinId::new();
        entry.pinned = true;
        entry.pin_id = Some(pin_id);
        entry.home = Some(entry.url.clone());
        entry.group = None;
        entry.put_away = false;

        let dest = self.pin_count();
        self.tabs.insert(dest, entry);
        self.restore_active_and_prev_by_id(active_id, prev_id);
        Ok(pin_id)
    }

    /// Unpin a pinned tab, moving it to the start of the loose tabs (`FEATURES.md` §7.1).
    pub fn unpin_tab(&mut self, id: TabId) -> Result<()> {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return Err(CoreError::Invalid(format!("no such tab: {id}")));
        };
        if !self.tabs[index].pinned {
            return Ok(());
        }

        let active_id = self.tabs[self.active].id;
        let prev_id = self
            .previous_active
            .and_then(|idx| self.tabs.get(idx))
            .map(|t| t.id);
        let mut entry = self.tabs.remove(index);
        entry.pinned = false;
        entry.pin_id = None;
        entry.home = None;

        let dest = self.pin_count();
        self.tabs.insert(dest, entry);
        self.restore_active_and_prev_by_id(active_id, prev_id);
        Ok(())
    }

    /// Put a background tab to sleep (`Put to Sleep`, `FEATURES.md` §3.2).
    /// Disabled for the active tab, a playing tab, or a loading tab.
    pub fn put_to_sleep(&mut self, id: TabId) -> Result<()> {
        if self.tabs[self.active].id == id {
            return Err(CoreError::Invalid(
                "the active on-screen tab cannot be put to sleep".into(),
            ));
        }
        let tab = self
            .tab_mut(id)
            .ok_or_else(|| CoreError::Invalid(format!("no such tab: {id}")))?;
        if tab.audible || tab.loading {
            return Err(CoreError::Invalid(
                "a busy or audible tab cannot be put to sleep".into(),
            ));
        }
        tab.sleeping = true;
        tab.engine_tab_id = None;
        tab.sleep_placeholder = Some(tab.display_title().to_owned());
        Ok(())
    }

    /// Start a drag session (`AGENTS.md` §3.3).
    pub fn begin_drag(&mut self, payload: DragPayload, x: f64, y: f64) {
        self.drag = Some(DragSession::begin(payload, x, y));
    }

    /// Active drag session, if any.
    #[must_use]
    pub const fn drag_session(&self) -> Option<&DragSession> {
        self.drag.as_ref()
    }

    /// Update the active drag session pointer and drop target.
    pub fn update_drag(&mut self, x: f64, y: f64, target: Option<DropTarget>) {
        if let Some(drag) = &mut self.drag {
            drag.update(x, y, target);
        }
    }

    /// Cancel any in-progress drag session.
    pub fn cancel_drag(&mut self) {
        self.drag = None;
    }

    /// Commit the active drag session and apply its drop operation (`FEATURES.md` §3.3).
    pub fn commit_drag(&mut self) -> Result<Option<DropTarget>> {
        Ok(self.commit_drag_with_payload()?.map(|(_, target)| target))
    }

    /// Commit the active drag session, returning both its [`DragPayload`] and [`DropTarget`].
    pub fn commit_drag_with_payload(&mut self) -> Result<Option<(DragPayload, DropTarget)>> {
        let Some(session) = self.drag.take() else {
            return Ok(None);
        };
        if !session.active {
            return Ok(None);
        }
        let Some(target) = session.target else {
            return Ok(None);
        };
        let payload = session.payload;

        if let DragPayload::Tab(tab_id) = payload {
            match &target {
                DropTarget::PinSlot(slot) => {
                    if !self.tab(tab_id).is_some_and(|t| t.pinned) {
                        self.pin_tab(tab_id)?;
                    }
                    let pin_count = self.pin_count();
                    let clamped = (*slot).min(pin_count.saturating_sub(1));
                    self.move_tab_to_index(tab_id, clamped)?;
                }
                DropTarget::LooseTabSlot(slot) => {
                    if self.tab(tab_id).is_some_and(|t| t.pinned) {
                        self.unpin_tab(tab_id)?;
                    }
                    let pin_count = self.pin_count();
                    let clamped = (*slot).max(pin_count).min(self.tabs.len() - 1);
                    self.move_tab_to_index(tab_id, clamped)?;
                }
                DropTarget::GroupSlot { group, index } => {
                    if self.tab(tab_id).is_some_and(|t| t.pinned) {
                        self.unpin_tab(tab_id)?;
                    }
                    if let Some(tab) = self.tab_mut(tab_id) {
                        tab.group = Some(*group);
                    }
                    let pin_count = self.pin_count();
                    let clamped = (*index).max(pin_count).min(self.tabs.len() - 1);
                    self.move_tab_to_index(tab_id, clamped)?;
                }
                DropTarget::PinFolderSlot { .. }
                | DropTarget::PageSplitEdge { .. }
                | DropTarget::DetachWindow => {}
            }
        }
        Ok(Some((payload, target)))
    }

    /// Move `id` to `target_index` within its section (pins or loose tabs).
    pub fn move_tab_to_index(&mut self, id: TabId, target_index: usize) -> Result<()> {
        let Some(from) = self.tabs.iter().position(|t| t.id == id) else {
            return Err(CoreError::Invalid(format!("no such tab: {id}")));
        };
        let active_id = self.tabs[self.active].id;
        let prev_id = self
            .previous_active
            .and_then(|idx| self.tabs.get(idx))
            .map(|t| t.id);
        let entry = self.tabs.remove(from);
        let pin_count = self.tabs.iter().take_while(|t| t.pinned).count();
        let dest = if entry.pinned {
            target_index.min(pin_count)
        } else {
            target_index.max(pin_count).min(self.tabs.len())
        };
        self.tabs.insert(dest, entry);
        self.restore_active_and_prev_by_id(active_id, prev_id);
        Ok(())
    }

    fn find_fallback_active_after_hide(&self, hiding_index: usize) -> Option<usize> {
        let is_eligible = |idx: usize| {
            idx != hiding_index
                && self
                    .tabs
                    .get(idx)
                    .is_some_and(|t| !t.put_away && !(t.pinned && t.sleeping))
        };
        if self.close_goes_back
            && let Some(prev) = self.previous_active
            && is_eligible(prev)
        {
            return Some(prev);
        }
        ((hiding_index + 1)..self.tabs.len())
            .find(|&idx| is_eligible(idx))
            .or_else(|| (0..hiding_index).rev().find(|&idx| is_eligible(idx)))
    }

    fn nearest_eligible_index(&self, preferred: usize) -> Option<usize> {
        let is_eligible = |idx: usize| {
            self.tabs
                .get(idx)
                .is_some_and(|t| !t.put_away && !(t.pinned && t.sleeping))
        };
        if is_eligible(preferred) {
            return Some(preferred);
        }
        (preferred..self.tabs.len())
            .find(|&idx| is_eligible(idx))
            .or_else(|| (0..preferred).rev().find(|&idx| is_eligible(idx)))
    }

    fn restore_active_and_prev_by_id(&mut self, active_id: TabId, prev_id: Option<TabId>) {
        if let Some(idx) = self.tabs.iter().position(|t| t.id == active_id) {
            self.active = idx;
        } else {
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
        self.previous_active = prev_id.and_then(|pid| self.tabs.iter().position(|t| t.id == pid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_reuses_blank_and_pin_unpin_reorders_across_boundary() {
        let mut alloc = TabIdAllocator::new();
        let mut model = TabModel::new(&mut alloc).expect("new model");
        let first_id = model.active_tab().id;

        // Creating a blank tab while an ungrouped blank tab exists reuses it (`FEATURES.md` §3.2).
        let reused_id = model
            .create_tab(&mut alloc, None, "New Tab", false, true)
            .expect("reuse blank");
        assert_eq!(reused_id, first_id);
        assert_eq!(model.tabs().len(), 1);

        // Navigate first tab, then create a second and third tab.
        model.active_tab_mut().url = PageUrl::parse("https://example.com/one").expect("url 1");
        model.active_tab_mut().title = "One".into();

        let second_id = model
            .create_tab(
                &mut alloc,
                Some(PageUrl::parse("https://example.com/two").expect("url 2")),
                "Two",
                false,
                true,
            )
            .expect("tab 2");
        let third_id = model
            .create_tab(
                &mut alloc,
                Some(PageUrl::parse("https://example.com/three").expect("url 3")),
                "Three",
                false,
                true,
            )
            .expect("tab 3");
        assert_eq!(model.tabs().len(), 3);

        // Pinning the third tab moves it to index 0 ahead of loose tabs.
        model.pin_tab(third_id).expect("pin tab 3");
        assert_eq!(model.tabs()[0].id, third_id);
        assert!(model.tabs()[0].pinned);
        assert_eq!(model.pin_count(), 1);

        // Dragging second_id onto PinSlot(0) pins it and puts it at slot 0.
        model.begin_drag(DragPayload::Tab(second_id), 10.0, 10.0);
        model.update_drag(30.0, 10.0, Some(DropTarget::PinSlot(0)));
        model.commit_drag().expect("commit pin drag");
        assert_eq!(model.pin_count(), 2);
        assert_eq!(model.tabs()[0].id, second_id);
        assert!(model.tabs()[0].pinned);

        // Dragging second_id back to LooseTabSlot(2) unpins it.
        model.begin_drag(DragPayload::Tab(second_id), 10.0, 10.0);
        model.update_drag(10.0, 90.0, Some(DropTarget::LooseTabSlot(2)));
        model.commit_drag().expect("commit unpin drag");
        assert_eq!(model.pin_count(), 1);
        assert!(!model.tab(second_id).expect("tab 2").pinned);
    }

    #[test]
    fn close_puts_down_pins_and_puts_away_grouped_tabs() {
        let mut alloc = TabIdAllocator::new();
        let mut model = TabModel::new(&mut alloc).expect("model");
        let first_id = model.active_tab().id;
        model.active_tab_mut().url = PageUrl::parse("https://news.ycombinator.com/").expect("home");
        model.pin_tab(first_id).expect("pin");

        // Wander away from home, then close with ⌘W: pin stays, sleeps, and returns home.
        model.tab_mut(first_id).expect("pin").url =
            PageUrl::parse("https://example.com/away").expect("away");
        model.close_tab(first_id, &mut alloc).expect("put down pin");
        let pin = model.tab(first_id).expect("pin still exists");
        assert!(pin.sleeping);
        assert_eq!(pin.url.as_str(), "https://news.ycombinator.com/");

        // Grouped tab: first close puts it away; second close removes it.
        let grouped_id = model
            .create_tab(
                &mut alloc,
                Some(PageUrl::parse("https://example.com/grouped").expect("url")),
                "Grouped",
                false,
                true,
            )
            .expect("create grouped");
        let gid = GroupId::new();
        model.tab_mut(grouped_id).expect("grouped").group = Some(gid);

        model
            .close_tab(grouped_id, &mut alloc)
            .expect("first close puts away");
        let grouped = model.tab(grouped_id).expect("still in group");
        assert!(grouped.put_away);
        assert!(grouped.sleeping);

        model
            .close_tab(grouped_id, &mut alloc)
            .expect("second close deletes");
        assert!(model.tab(grouped_id).is_none());
    }

    #[test]
    fn fold_toggles_sidebar_or_icons_only_and_classifies_split_bands() {
        let mut alloc = TabIdAllocator::new();
        let mut model = TabModel::new(&mut alloc).expect("model");
        assert!(!model.is_folded());
        model.toggle_fold();
        assert!(model.is_folded());
        model.toggle_fold();
        assert!(!model.is_folded());

        model.set_fold_to_icons(true);
        model.toggle_fold();
        assert_eq!(model.layout(), TabLayoutMode::IconsOnly);
        assert!(!model.is_folded());
        model.toggle_fold();
        assert_eq!(model.layout(), TabLayoutMode::LeftColumn);

        assert_eq!(
            DragSession::classify_page_split_band(40.0, 1000.0),
            Some(SplitDropSide::Left)
        );
        assert_eq!(DragSession::classify_page_split_band(500.0, 1000.0), None);
        assert_eq!(
            DragSession::classify_page_split_band(920.0, 1000.0),
            Some(SplitDropSide::Right)
        );
    }

    #[test]
    fn active_pin_or_group_close_moves_focus_to_loose_tab_and_reuses_newtab() {
        let mut alloc = TabIdAllocator::new();
        let mut model = TabModel::new(&mut alloc).expect("model");
        let pin_id = model.active_tab().id;
        model.active_tab_mut().url = PageUrl::parse("https://pin.example/").expect("pin url");
        model.pin_tab(pin_id).expect("pin");

        let loose_id = model
            .create_tab(&mut alloc, None, "New Tab", false, false)
            .expect("loose blank");
        model.tab_mut(loose_id).expect("loose").url =
            PageUrl::parse("search://newtab").expect("newtab");
        // Creating another blank tab reuses search://newtab.
        let reused = model
            .create_tab(&mut alloc, None, "Again", false, false)
            .expect("reused");
        assert_eq!(reused, loose_id);

        // Select the active pin with a simulated engine_tab_id, then close (put down):
        // pin clears engine_tab_id, sleeps, returns home, and active shifts to loose_id.
        model.select_tab(pin_id);
        model.set_engine_tab_id(pin_id, Some(TabId::new(900)));
        model.tab_mut(pin_id).expect("pin").url =
            PageUrl::parse("https://pin.example/subpage").expect("subpage");
        model
            .close_tab(pin_id, &mut alloc)
            .expect("put down active pin");
        let pin = model.tab(pin_id).expect("pin kept");
        assert!(pin.sleeping);
        assert_eq!(pin.engine_tab_id, None);
        assert_eq!(pin.url.as_str(), "https://pin.example/");
        assert_eq!(model.active_tab().id, loose_id);

        // Group tab active put-away moves focus back to loose_id cleanly.
        let gid = GroupId::new();
        let g_id = model
            .create_tab(
                &mut alloc,
                Some(PageUrl::parse("https://group.example/").expect("group")),
                "Grouped",
                false,
                true,
            )
            .expect("group tab");
        model.tab_mut(g_id).expect("g").group = Some(gid);
        model.set_engine_tab_id(g_id, Some(TabId::new(901)));
        model.close_tab(g_id, &mut alloc).expect("put away group");
        let g = model.tab(g_id).expect("g kept");
        assert!(g.put_away);
        assert!(g.sleeping);
        assert_eq!(g.engine_tab_id, None);
        assert_eq!(model.active_tab().id, loose_id);
    }
}
