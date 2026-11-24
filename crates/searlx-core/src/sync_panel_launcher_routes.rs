//! Folder sync (`P3-14`), Side panel (`P3-16`), Global Launcher (`P3-17`), and Space routes / link chooser / webmail (`P3-18`).
//!
//! Behavioural specifications:
//! - `FEATURES.md` §16.1 — Folder sync (`Search Sync/<scope>/<device>.json`, 17 settings, spaces/pins/bookmarks, 365-day tombstones, mass-deletion guard, profile UUID scoping)
//! - `FEATURES.md` §3.8 — Side panel (Website/Extension/Downloads, `300–640 pt`, `380 pt` default, per-space store reload, private hold-back, no `⌘W` close)
//! - `FEATURES.md` §18.2 — Launcher (floating field, key validation, local-only suggestions, Return routing order, draft retention)
//! - `FEATURES.md` §18.1 — Space routes, link chooser (`1–9`/`0`, `Space` toggles Always, 200-site memory), and RFC 6068 `mailto:` webmail compose URLs

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::address::SearchEngine;
use crate::commands::{KeyChord, KeyModifiers};
use crate::data::model::Pin;
use crate::error::{CoreError, Result};
use crate::ids::{ProfileId, SpaceId, TabId};
use crate::settings::{AutoCloseAfter, LookPreference, NewTabPlace, Settings, TabGlyphMode};
use crate::url::PageUrl;

// ============================================================================
// P3-14 — Folder sync (`FEATURES.md` §16.1)
// ============================================================================

/// Retention window for deleted item tombstones in Folder Sync (`365 days`, `FEATURES.md` §16.1).
pub const SYNC_TOMBSTONE_RETENTION_MS: u64 = 365 * 86_400 * 1000;

/// Sync scope directory under `<folder>/Search Sync/<scope>/<device>.json` (`FEATURES.md` §16.1).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SyncScope {
    /// App-wide scope (`app`), holding the 17 synced settings.
    App,
    /// Per-profile scope (`profile-<uuid>`), syncing only with another device's profile of the exact same UUID.
    Profile(ProfileId),
}

impl SyncScope {
    /// Directory name under `Search Sync/`.
    #[must_use]
    pub fn folder_name(&self) -> String {
        match self {
            Self::App => "app".to_owned(),
            Self::Profile(id) => format!("profile-{id}"),
        }
    }

    /// Relative path `<folder>/Search Sync/<scope>/<device>.json`.
    #[must_use]
    pub fn relative_device_path(&self, device_id: &str) -> String {
        format!("Search Sync/{}/{device_id}.json", self.folder_name())
    }
}

/// The 17 settings synchronized by Folder Sync (`FEATURES.md` §16.1, P3-14).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedSettingsSubset {
    /// 1. `look`
    pub look: LookPreference,
    /// 2. `glyph`
    pub glyph: TabGlyphMode,
    /// 3. `search.engine` + 4. `search.custom`
    pub search_engine: SearchEngine,
    /// 5. `search.suggest`
    pub search_suggest: bool,
    /// 6. `search.keywords`
    pub search_keywords: bool,
    /// 7. `spaces`
    pub spaces: bool,
    /// 8. `pins.list`
    pub pins_list: bool,
    /// 9. `pins.home`
    pub pins_home: bool,
    /// 10. `tabs.close.after`
    pub tabs_close_after: AutoCloseAfter,
    /// 11. `tabs.newPlace`
    pub tabs_new_place: NewTabPlace,
    /// 12. `tabs.shortcuts`
    pub tabs_shortcuts: bool,
    /// 13. `shield`
    pub shield: bool,
    /// 14. `privacy.clickIDs`
    pub privacy_click_ids: bool,
    /// 15. `privacy.gpc`
    pub privacy_gpc: bool,
    /// 16. `bookmarks.bar`
    pub bookmarks_bar: bool,
    /// 17. `quit.confirm`
    pub quit_confirm: bool,
}

impl SyncedSettingsSubset {
    /// Project the 17 synced settings from [`Settings`].
    #[must_use]
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            look: s.look,
            glyph: s.glyph,
            search_engine: s.search_engine.clone(),
            search_suggest: s.search_suggest,
            search_keywords: s.search_keywords,
            spaces: s.spaces,
            pins_list: s.pins_list,
            pins_home: s.pins_home,
            tabs_close_after: s.tabs_close_after,
            tabs_new_place: s.tabs_new_place,
            tabs_shortcuts: s.tabs_shortcuts,
            shield: s.shield,
            privacy_click_ids: s.privacy_click_ids,
            privacy_gpc: s.privacy_gpc,
            bookmarks_bar: s.bookmarks_bar,
            quit_confirm: s.quit_confirm,
        }
    }

    /// Apply the 17 synced settings onto a local [`Settings`] struct without touching local-only settings.
    pub fn apply_to_settings(&self, s: &mut Settings) {
        s.look = self.look;
        s.glyph = self.glyph;
        s.search_engine.clone_from(&self.search_engine);
        s.search_suggest = self.search_suggest;
        s.search_keywords = self.search_keywords;
        s.spaces = self.spaces;
        s.pins_list = self.pins_list;
        s.pins_home = self.pins_home;
        s.tabs_close_after = self.tabs_close_after;
        s.tabs_new_place = self.tabs_new_place;
        s.tabs_shortcuts = self.tabs_shortcuts;
        s.shield = self.shield;
        s.privacy_click_ids = self.privacy_click_ids;
        s.privacy_gpc = self.privacy_gpc;
        s.bookmarks_bar = self.bookmarks_bar;
        s.quit_confirm = self.quit_confirm;
    }
}

/// Versioned sync wrapper (`v`/tombstone, `t` stamp, `d` device, `FEATURES.md` §16.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedSyncItem<T> {
    /// Item ID.
    pub id: String,
    /// `None` when deleted (tombstone), or `Some(value)` when live.
    pub value: Option<T>,
    /// Modification or deletion timestamp in Unix milliseconds (`t`).
    pub t_ms: u64,
    /// Writing device UUID (`d`).
    pub device_id: String,
}

impl<T: Serialize> VersionedSyncItem<T> {
    /// Whether this item is a tombstone.
    #[must_use]
    pub const fn is_tombstone(&self) -> bool {
        self.value.is_none()
    }

    /// Deterministic ordering rule from `FEATURES.md` §16.1:
    /// larger `t_ms` wins, then larger `device_id`, then tombstone over live value, then larger serialized text.
    #[must_use]
    pub fn wins_over(&self, other: &Self) -> bool {
        if self.t_ms != other.t_ms {
            return self.t_ms > other.t_ms;
        }
        if self.device_id != other.device_id {
            return self.device_id > other.device_id;
        }
        if self.is_tombstone() != other.is_tombstone() {
            return self.is_tombstone();
        }
        let self_json = serde_json::to_string(&self.value).unwrap_or_default();
        let other_json = serde_json::to_string(&other.value).unwrap_or_default();
        self_json >= other_json
    }
}

/// Sanitize a [`Pin`] for Folder Sync (`FEATURES.md` §16.1: pin folders never sync and stay local).
#[must_use]
pub fn sanitize_pin_for_folder_sync(pin: &Pin) -> Pin {
    let mut cleaned = pin.clone();
    cleaned.extra.remove("folder_id");
    cleaned.extra.remove("folder");
    cleaned
}

/// Merge two device maps of [`VersionedSyncItem`], pruning tombstones older than 365 days
/// and enforcing the "whole kind vanished at once" mass-deletion guard (`FEATURES.md` §16.1, P3-14).
#[must_use]
pub fn merge_sync_items<T: Clone + PartialEq + Serialize>(
    agreed_baseline: &BTreeMap<String, VersionedSyncItem<T>>,
    incoming_devices: &[BTreeMap<String, VersionedSyncItem<T>>],
    now_ms: u64,
) -> BTreeMap<String, VersionedSyncItem<T>> {
    // Mass-deletion guard: if the baseline had live items and an incoming snapshot is completely empty
    // (no live items and no tombstones, e.g., a bad reset or missing folder), ignore the empty snapshot.
    let baseline_had_live = agreed_baseline.values().any(|item| !item.is_tombstone());
    let mut merged = agreed_baseline.clone();

    for device_map in incoming_devices {
        if baseline_had_live && device_map.is_empty() {
            continue;
        }
        for (id, candidate) in device_map {
            match merged.get(id) {
                None => {
                    merged.insert(id.clone(), candidate.clone());
                }
                Some(existing) => {
                    if candidate.wins_over(existing) && candidate != existing {
                        merged.insert(id.clone(), candidate.clone());
                    }
                }
            }
        }
    }

    // Purge tombstones older than 365 days (`FEATURES.md` §16.1).
    merged.retain(|_, item| {
        if item.is_tombstone() {
            now_ms.saturating_sub(item.t_ms) < SYNC_TOMBSTONE_RETENTION_MS
        } else {
            true
        }
    });

    merged
}

/// Check mutual exclusion between Folder Sync and iCloud Sync (`FEATURES.md` §16.1, §16.2).
pub fn validate_sync_mode_exclusivity(folder_sync_on: bool, icloud_sync_on: bool) -> Result<()> {
    if folder_sync_on && icloud_sync_on {
        return Err(CoreError::Invalid(
            "Folder Sync and iCloud Sync cannot be enabled at the same time".to_owned(),
        ));
    }
    Ok(())
}

// ============================================================================
// P3-16 — Side panel (`FEATURES.md` §3.8)
// ============================================================================

/// Default width of the side panel (`380 pt`, `FEATURES.md` §3.8).
pub const SIDE_PANEL_DEFAULT_WIDTH_PT: f64 = 380.0;
/// Minimum width of the side panel (`300 pt`, `FEATURES.md` §3.8).
pub const SIDE_PANEL_MIN_WIDTH_PT: f64 = 300.0;
/// Maximum width of the side panel (`640 pt`, `FEATURES.md` §3.8).
pub const SIDE_PANEL_MAX_WIDTH_PT: f64 = 640.0;

/// Built-in offered websites for the Side Panel (`FEATURES.md` §3.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidePanelPresetSite {
    /// Claude (`https://claude.ai/new`)
    Claude,
    /// ChatGPT (`https://chatgpt.com`)
    ChatGpt,
    /// Gemini (`https://gemini.google.com/app`)
    Gemini,
    /// Perplexity (`https://perplexity.ai`)
    Perplexity,
}

impl SidePanelPresetSite {
    /// Canonical HTTPS URL for the preset site.
    #[must_use]
    pub fn page_url(self) -> PageUrl {
        let raw = match self {
            Self::Claude => "https://claude.ai/new",
            Self::ChatGpt => "https://chatgpt.com/",
            Self::Gemini => "https://gemini.google.com/app",
            Self::Perplexity => "https://perplexity.ai/",
        };
        PageUrl::parse(raw).expect("valid preset url")
    }
}

/// Content hosted inside the Side Panel (`FEATURES.md` §3.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidePanelContent {
    /// A web page signed into `space_id`'s data store (`http`/`https` only).
    Website {
        /// Current URL in the panel.
        url: PageUrl,
        /// Space whose data store backs this panel.
        space_id: SpaceId,
        /// Recent crash timestamps (seconds) for the 2-crashes-per-minute reload breaker.
        crash_timestamps: Vec<u64>,
    },
    /// An extension panel (optionally bound to one tab).
    Extension {
        /// Extension identifier.
        extension_id: String,
        /// Whether the extension has been granted access to private tabs.
        allowed_in_private: bool,
        /// Optional tab binding.
        bound_tab: Option<TabId>,
    },
    /// Compact Downloads list.
    Downloads,
}

/// Controller for the Side Panel (`FEATURES.md` §3.8, P3-16).
#[derive(Clone, Debug)]
pub struct SidePanelController {
    content: Option<SidePanelContent>,
    width_pt: f64,
}

impl Default for SidePanelController {
    fn default() -> Self {
        Self {
            content: None,
            width_pt: SIDE_PANEL_DEFAULT_WIDTH_PT,
        }
    }
}

impl SidePanelController {
    /// Create a closed side panel at `380 pt` default width.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current panel content, if open.
    #[must_use]
    pub const fn content(&self) -> Option<&SidePanelContent> {
        self.content.as_ref()
    }

    /// Current panel width in points (`300..=640`).
    #[must_use]
    pub const fn width_pt(&self) -> f64 {
        self.width_pt
    }

    /// `⌘W` never closes the side panel (`FEATURES.md` §3.8).
    #[must_use]
    pub const fn closes_on_cmd_w() -> bool {
        false
    }

    /// Resize the side panel edge (`300..=640 pt`, `FEATURES.md` §3.8).
    pub fn set_width_pt(&mut self, width: f64) {
        self.width_pt = width.clamp(SIDE_PANEL_MIN_WIDTH_PT, SIDE_PANEL_MAX_WIDTH_PT);
    }

    /// Double-clicking the side panel edge resets width to `380 pt` (`FEATURES.md` §3.8).
    pub fn reset_width(&mut self) {
        self.width_pt = SIDE_PANEL_DEFAULT_WIDTH_PT;
    }

    /// Open a website in the side panel (must be `http` or `https`, signed in as `space_id`).
    pub fn open_website(&mut self, url: PageUrl, space_id: SpaceId) -> Result<()> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(CoreError::Invalid(
                "side panel websites must use http or https".to_owned(),
            ));
        }
        self.content = Some(SidePanelContent::Website {
            url,
            space_id,
            crash_timestamps: Vec::new(),
        });
        Ok(())
    }

    /// Open the compact Downloads list in the side panel (`FEATURES.md` §3.8).
    pub fn open_downloads(&mut self) {
        self.content = Some(SidePanelContent::Downloads);
    }

    /// Close the side panel (`FEATURES.md` §3.8).
    pub fn close(&mut self) {
        self.content = None;
    }

    /// Press an extension's toolbar button: opens its panel, or closes it if already open (`FEATURES.md` §3.8).
    pub fn toggle_extension_panel(
        &mut self,
        extension_id: impl Into<String>,
        allowed_in_private: bool,
        bound_tab: Option<TabId>,
    ) -> bool {
        let ext = extension_id.into();
        if matches!(
            &self.content,
            Some(SidePanelContent::Extension { extension_id: cur, .. }) if *cur == ext
        ) {
            self.content = None;
            return false;
        }
        self.content = Some(SidePanelContent::Extension {
            extension_id: ext,
            allowed_in_private,
            bound_tab,
        });
        true
    }

    /// Switching space reloads a website side panel into the new space's data store (`FEATURES.md` §3.8).
    ///
    /// Returns `true` if the panel needed a store reload.
    pub fn on_space_switched(&mut self, new_space_id: SpaceId) -> bool {
        if let Some(SidePanelContent::Website { space_id, .. }) = &mut self.content
            && *space_id != new_space_id
        {
            *space_id = new_space_id;
            return true;
        }
        false
    }

    /// Whether the panel is temporarily held back (hidden so its width returns to the page)
    /// because the active tab is private and an open extension panel is not permitted in private tabs (`FEATURES.md` §3.8).
    #[must_use]
    pub fn is_held_back_for_active_tab(&self, active_tab_is_private: bool) -> bool {
        matches!(
            &self.content,
            Some(SidePanelContent::Extension {
                allowed_in_private: false,
                ..
            }) if active_tab_is_private
        )
    }

    /// Record a crash in the website panel; reloads at most twice a minute then gives up (`FEATURES.md` §3.8).
    pub fn on_website_panel_crash(&mut self, now_secs: u64) -> bool {
        let Some(SidePanelContent::Website {
            crash_timestamps, ..
        }) = &mut self.content
        else {
            return false;
        };
        crash_timestamps.retain(|t| now_secs.saturating_sub(*t) <= 60);
        crash_timestamps.push(now_secs);
        crash_timestamps.len() <= 2
    }
}

// ============================================================================
// P3-17 — Global Launcher (`FEATURES.md` §18.2)
// ============================================================================

/// Recommended global launcher shortcut (`⇧⌘Space`, `FEATURES.md` §18.2).
#[must_use]
pub fn recommended_launcher_hotkey() -> KeyChord {
    KeyChord::new(
        KeyModifiers {
            cmd: true,
            alt: false,
            shift: true,
            ctrl: false,
        },
        "Space",
    )
}

/// Validate a recorded global hotkey for the Launcher (`FEATURES.md` §18.2, P3-17).
///
/// Rules:
/// - Requires `⌘`, `⌥`, `⌃`, or a function key (`F1`..`F12`).
/// - Rejects reserved macOS/app shortcuts (`⌘C/V/X/A/Z/Q/W/L/T/N/Space`, `⌃⌘F`).
pub fn validate_launcher_hotkey(chord: &KeyChord) -> Result<()> {
    let upper_key = chord.key.to_ascii_uppercase();
    let is_function_key = upper_key.starts_with('F')
        && upper_key[1..]
            .parse::<u8>()
            .is_ok_and(|n| (1..=12).contains(&n));
    let has_modifier = chord.modifiers.cmd || chord.modifiers.alt || chord.modifiers.ctrl;
    if !has_modifier && !is_function_key {
        return Err(CoreError::Invalid(
            "launcher hotkey requires ⌘, ⌥, ⌃, or a function key".to_owned(),
        ));
    }

    let only_cmd = chord.modifiers.cmd
        && !chord.modifiers.alt
        && !chord.modifiers.shift
        && !chord.modifiers.ctrl;
    if only_cmd
        && matches!(
            upper_key.as_str(),
            "C" | "V" | "X" | "A" | "Z" | "Q" | "W" | "L" | "T" | "N" | "SPACE"
        )
    {
        return Err(CoreError::Invalid(format!(
            "shortcut {} is reserved by the system or application",
            chord.key
        )));
    }
    Ok(())
}

/// Whether Search should currently register the global Carbon/OS launcher hotkey (`wants`, `FEATURES.md` §18.2).
#[must_use]
pub const fn launcher_wants_global_hotkey(
    launcher_enabled: bool,
    hotkey_configured: bool,
    is_recording_shortcut: bool,
    launcher_visible: bool,
    search_active_with_window: bool,
) -> bool {
    if !launcher_enabled || !hotkey_configured || is_recording_shortcut {
        return false;
    }
    launcher_visible || !search_active_with_window
}

/// Outcome of pressing `Return` in the Launcher (`FEATURES.md` §18.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LauncherSubmitAction {
    /// 1. Switch to a chosen open tab in the target profile.
    SwitchToOpenTab(TabId),
    /// 2. Execute a matched browser command.
    ExecuteCommand(String),
    /// 3. Open an internal `search://` page in its own tab.
    OpenInternalPage(PageUrl),
    /// 4. Open destination in a small window (`launcher.little = true`).
    OpenInSmallWindow(PageUrl),
    /// 5. Focus an existing tab already showing the destination URL.
    FocusExistingTabWithUrl(TabId),
    /// 6. Deliver destination into the front browser window.
    OpenInFrontBrowserWindow(PageUrl),
}

/// Floating global Launcher state machine (`FEATURES.md` §18.2, P3-17).
#[derive(Clone, Debug, Default)]
pub struct LauncherController {
    /// Whether the floating launcher panel is currently visible over another app.
    pub visible: bool,
    /// Preserved draft text across Escape dismissals (`FEATURES.md` §18.2).
    pub draft: String,
    /// Configured global hotkey.
    pub hotkey: Option<KeyChord>,
}

impl LauncherController {
    /// Create a launcher controller.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Launcher never makes search-engine network queries while typing (`FEATURES.md` §18.2).
    #[must_use]
    pub const fn allows_network_suggestions() -> bool {
        false
    }

    /// Show the launcher over the current screen without activating Search (`activates_search = false`).
    pub fn show(&mut self) -> bool {
        self.visible = true;
        false
    }

    /// Dismiss the launcher (`Escape`, `⌘W`, click outside, or hotkey again); keeps `draft` and leaves the other app in front.
    pub fn dismiss_keep_draft(&mut self) -> bool {
        self.visible = false;
        false
    }

    /// Filter eligible open tabs for the launcher (max 3, same profile, never private or agent/bench tabs).
    #[must_use]
    pub fn filter_open_tabs<'a>(
        query: &str,
        target_profile: ProfileId,
        tabs: &'a [(TabId, ProfileId, String, PageUrl, bool, bool)],
    ) -> Vec<(TabId, &'a str, &'a PageUrl)> {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        tabs.iter()
            .filter(|(_, profile, title, url, is_private, is_agent)| {
                *profile == target_profile
                    && !*is_private
                    && !*is_agent
                    && (title.to_ascii_lowercase().contains(&needle)
                        || url.as_str().to_ascii_lowercase().contains(&needle))
            })
            .take(3)
            .map(|(id, _, title, url, _, _)| (*id, title.as_str(), url))
            .collect()
    }

    /// Resolve the `Return` action order from `FEATURES.md` §18.2 and clear the draft.
    pub fn submit(
        &mut self,
        picked_open_tab: Option<TabId>,
        picked_command: Option<String>,
        destination: PageUrl,
        launcher_little: bool,
        existing_tab_with_url: Option<TabId>,
    ) -> LauncherSubmitAction {
        self.visible = false;
        self.draft.clear();

        if let Some(tab_id) = picked_open_tab {
            return LauncherSubmitAction::SwitchToOpenTab(tab_id);
        }
        if let Some(cmd) = picked_command {
            return LauncherSubmitAction::ExecuteCommand(cmd);
        }
        if destination.is_internal() {
            return LauncherSubmitAction::OpenInternalPage(destination);
        }
        if launcher_little {
            return LauncherSubmitAction::OpenInSmallWindow(destination);
        }
        if let Some(existing) = existing_tab_with_url {
            return LauncherSubmitAction::FocusExistingTabWithUrl(existing);
        }
        LauncherSubmitAction::OpenInFrontBrowserWindow(destination)
    }
}

// ============================================================================
// P3-18 — Space routes, link chooser, and RFC 6068 webmail (`FEATURES.md` §18.1)
// ============================================================================

/// Maximum number of sites for which the link chooser remembers the last chosen space (`200`, `FEATURES.md` §18.1).
pub const MAX_REMEMBERED_LINK_SITES: usize = 200;
/// Maximum recipients per `to`/`cc`/`bcc` field in a `mailto:` URL (`50`, `FEATURES.md` §18.1).
pub const MAX_MAILTO_RECIPIENTS: usize = 50;
/// Maximum subject length in characters for a `mailto:` URL (`500`, `FEATURES.md` §18.1).
pub const MAX_MAILTO_SUBJECT_CHARS: usize = 500;
/// Maximum total compose URL length in characters (`8,000`, `FEATURES.md` §18.1).
pub const MAX_MAILTO_URL_CHARS: usize = 8000;

/// Origin of a navigation request for space routing (`FEATURES.md` §7.1, §18.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkIntakeSource {
    /// Arrived from an external app (Mail, Slack, messages, Apple Event / desktop URL handler).
    ExternalApp,
    /// Typed directly into Search's address field (never routed by `SpaceRoute`!).
    TypedAddress,
    /// Clicked inside an existing browser tab (never routed by `SpaceRoute`!).
    InTabClick,
}

/// Rule routing external links for a domain (and its subdomains) into a specific space (`FEATURES.md` §7.1, §18.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceRouteRule {
    /// Registrable or specific host (without leading `www.`).
    pub domain: String,
    /// Destination space ID.
    pub target_space: SpaceId,
}

impl SpaceRouteRule {
    /// Whether `host` matches `domain` or any subdomain of `domain`.
    #[must_use]
    pub fn matches_host(&self, host: &str) -> bool {
        let clean_host = host.trim().trim_start_matches("www.").to_ascii_lowercase();
        let clean_domain = self
            .domain
            .trim()
            .trim_start_matches("*.")
            .trim_start_matches("www.")
            .to_ascii_lowercase();
        if clean_domain.is_empty() {
            return false;
        }
        clean_host == clean_domain || clean_host.ends_with(&format!(".{clean_domain}"))
    }
}

/// Routing decision for an incoming link (`FEATURES.md` §18.1, P3-18).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalLinkRoute {
    /// 1. Matched a `SpaceRouteRule` (or direct delivery into the active space for non-external links).
    OpenInSpace(SpaceId),
    /// 2. With `links.little && links.little.tabs`, focus an already-open tab with the URL.
    FocusExistingTab(TabId),
    /// 3. With `links.ask` and `> 1` space, open the Link Chooser panel.
    ShowChooser {
        /// Pre-highlighted space (from the 200-site memory, if known).
        suggested_space: Option<SpaceId>,
    },
    /// 4. With `links.little`, open a Small Window.
    OpenSmallWindow,
    /// 5. Deliver into the front browser window's active space.
    OpenInFrontWindow,
}

/// Router for site→space rules and the ask-which-space Link Chooser (`FEATURES.md` §18.1, P3-18).
#[derive(Clone, Debug, Default)]
pub struct SpaceRouter {
    routes: Vec<SpaceRouteRule>,
    /// Last chosen space per host (capped at `MAX_REMEMBERED_LINK_SITES = 200`, most recent last).
    last_space_by_host: Vec<(String, SpaceId)>,
    /// Whether `"Always open <site> links here"` is checked in the active chooser panel.
    pub chooser_always_checked: bool,
}

impl SpaceRouter {
    /// Create a new space router.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Configured site→space routing rules.
    #[must_use]
    pub fn routes(&self) -> &[SpaceRouteRule] {
        &self.routes
    }

    /// Add or replace a site→space rule.
    pub fn set_route(&mut self, domain: impl Into<String>, target_space: SpaceId) {
        let clean = domain
            .into()
            .trim()
            .trim_start_matches("www.")
            .to_ascii_lowercase();
        if let Some(existing) = self.routes.iter_mut().find(|r| r.domain == clean) {
            existing.target_space = target_space;
        } else {
            self.routes.push(SpaceRouteRule {
                domain: clean,
                target_space,
            });
        }
    }

    /// Resolve where `url` should open according to `FEATURES.md` §18.1.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn route_link(
        &self,
        url: &PageUrl,
        source: LinkIntakeSource,
        active_space: SpaceId,
        is_private_tab: bool,
        sends_links_to_spaces: bool,
        ask_which_space: bool,
        total_spaces: usize,
        links_little: bool,
        links_little_find_tabs: bool,
        existing_tab_with_url: Option<TabId>,
    ) -> ExternalLinkRoute {
        // Typed addresses, in-tab link clicks, and private tabs are never routed by SpaceRoute (`FEATURES.md` §7.1, §18.1).
        if source != LinkIntakeSource::ExternalApp || is_private_tab {
            return ExternalLinkRoute::OpenInSpace(active_space);
        }

        let host = url
            .host()
            .unwrap_or_default()
            .trim_start_matches("www.")
            .to_ascii_lowercase();

        // (1) SpaceRoute match
        if sends_links_to_spaces
            && let Some(rule) = self.routes.iter().find(|r| r.matches_host(&host))
        {
            return ExternalLinkRoute::OpenInSpace(rule.target_space);
        }

        // (2) With small links + find-tabs, an already-open page comes forward
        if links_little
            && links_little_find_tabs
            && let Some(tab_id) = existing_tab_with_url
        {
            return ExternalLinkRoute::FocusExistingTab(tab_id);
        }

        // (3) With `links.ask` and >1 space, show the chooser
        if ask_which_space && total_spaces > 1 {
            let suggested_space = self
                .last_space_by_host
                .iter()
                .rev()
                .find(|(h, _)| *h == host)
                .map(|(_, s)| *s);
            return ExternalLinkRoute::ShowChooser { suggested_space };
        }

        // (4) With small links on, open a small window
        if links_little {
            return ExternalLinkRoute::OpenSmallWindow;
        }

        // (5) Otherwise deliver into the browser
        ExternalLinkRoute::OpenInFrontWindow
    }

    /// Map number keys `'1'..='9'` and `'0'` to chooser row indices `0..=9` (`FEATURES.md` §18.1).
    #[must_use]
    pub const fn chooser_digit_to_index(digit_char: char) -> Option<usize> {
        match digit_char {
            '1'..='9' => Some((digit_char as usize) - ('1' as usize)),
            '0' => Some(9),
            _ => None,
        }
    }

    /// Press `Space` inside the chooser to toggle `"Always open <site> links here"` (`FEATURES.md` §18.1).
    pub fn toggle_chooser_always(&mut self) -> bool {
        self.chooser_always_checked = !self.chooser_always_checked;
        self.chooser_always_checked
    }

    /// Confirm a choice in the Link Chooser: remembers the space in the 200-site LRU and optionally creates a `SpaceRouteRule`.
    pub fn confirm_chooser_selection(
        &mut self,
        url: &PageUrl,
        chosen_space: SpaceId,
        sends_links_to_spaces_enabled: bool,
    ) {
        if let Some(raw_host) = url.host() {
            let host = raw_host.trim_start_matches("www.").to_ascii_lowercase();
            self.last_space_by_host.retain(|(h, _)| *h != host);
            if self.last_space_by_host.len() >= MAX_REMEMBERED_LINK_SITES {
                self.last_space_by_host.remove(0);
            }
            self.last_space_by_host.push((host.clone(), chosen_space));

            if self.chooser_always_checked && sends_links_to_spaces_enabled {
                self.set_route(host, chosen_space);
            }
        }
        self.chooser_always_checked = false;
    }
}

/// Supported webmail providers for `mailto:` links (`mail.webmail`, `FEATURES.md` §18.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebmailProvider {
    /// System default mail application (`None`).
    #[default]
    SystemMail,
    /// Gmail (`mail.google.com`)
    Gmail,
    /// Outlook.com (`outlook.live.com`)
    Outlook,
    /// Fastmail (`app.fastmail.com`)
    Fastmail,
    /// Proton Mail (`mail.proton.me`)
    ProtonMail,
}

/// Parsed RFC 6068 `mailto:` message (`FEATURES.md` §18.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailtoMessage {
    /// Primary recipients (capped at 50).
    pub to: Vec<String>,
    /// CC recipients (capped at 50).
    pub cc: Vec<String>,
    /// BCC recipients (capped at 50).
    pub bcc: Vec<String>,
    /// Subject line (capped at 500 characters).
    pub subject: String,
    /// Plaintext body.
    pub body: String,
}

impl MailtoMessage {
    /// Parse an RFC 6068 `mailto:` URI, enforcing the 50-recipient and 500-char subject caps (`FEATURES.md` §18.1).
    pub fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        let Some(rest) = trimmed
            .strip_prefix("mailto:")
            .or_else(|| trimmed.strip_prefix("MAILTO:"))
        else {
            return Err(CoreError::Invalid("not a mailto: URL".to_owned()));
        };

        let (to_part, query_part) = match rest.split_once('?') {
            Some((t, q)) => (t, Some(q)),
            None => (rest, None),
        };

        let mut msg = Self::default();
        Self::push_recipients(&mut msg.to, &percent_decode(to_part));

        if let Some(query) = query_part {
            for pair in query.split('&') {
                if pair.is_empty() {
                    continue;
                }
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                let decoded = percent_decode(v);
                match k.to_ascii_lowercase().as_str() {
                    "to" => Self::push_recipients(&mut msg.to, &decoded),
                    "cc" => Self::push_recipients(&mut msg.cc, &decoded),
                    "bcc" => Self::push_recipients(&mut msg.bcc, &decoded),
                    "subject" => {
                        msg.subject = decoded.chars().take(MAX_MAILTO_SUBJECT_CHARS).collect();
                    }
                    "body" => {
                        msg.body = decoded;
                    }
                    _ => {}
                }
            }
        }
        Ok(msg)
    }

    fn push_recipients(target: &mut Vec<String>, csv: &str) {
        for addr in csv.split(',') {
            if target.len() >= MAX_MAILTO_RECIPIENTS {
                break;
            }
            let clean = addr.trim();
            if !clean.is_empty() {
                target.push(clean.to_owned());
            }
        }
    }

    /// Build a webmail compose URL capped at `8,000` characters by shortening `body` if needed (`FEATURES.md` §18.1).
    ///
    /// Also returns `opens_in_private_tab = false` because `FEATURES.md` §18.1 specifies:
    /// "A mailto in a page opens the webmail compose in a foreground tab beside it (never a private tab, even from one)".
    #[must_use]
    pub fn build_webmail_compose(&self, provider: WebmailProvider) -> Option<(PageUrl, bool)> {
        let base = match provider {
            WebmailProvider::SystemMail => return None,
            WebmailProvider::Gmail => "https://mail.google.com/mail/?view=cm&fs=1",
            WebmailProvider::Outlook => "https://outlook.live.com/mail/0/deeplink/compose?",
            WebmailProvider::Fastmail => "https://app.fastmail.com/mail/compose?",
            WebmailProvider::ProtonMail => "https://mail.proton.me/u/0/inbox?compose=true",
        };

        let to = percent_encode_component(&self.to.join(","));
        let cc = percent_encode_component(&self.cc.join(","));
        let bcc = percent_encode_component(&self.bcc.join(","));
        let su = percent_encode_component(&self.subject);

        let sep = if base.ends_with('?') { "" } else { "&" };
        let su_key = if provider == WebmailProvider::Gmail {
            "su"
        } else {
            "subject"
        };
        let prefix = format!("{base}{sep}to={to}&cc={cc}&bcc={bcc}&{su_key}={su}&body=");

        let mut body_chars: Vec<char> = self.body.chars().collect();
        loop {
            let candidate_body: String = body_chars.iter().collect();
            let encoded_body = percent_encode_component(&candidate_body);
            let full = format!("{prefix}{encoded_body}");
            if full.len() <= MAX_MAILTO_URL_CHARS || body_chars.is_empty() {
                let url = PageUrl::parse(&full).expect("valid webmail compose url");
                return Some((url, false));
            }
            let new_len = (body_chars.len() * 3) / 4;
            body_chars.truncate(new_len);
        }
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((h << 4) | l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_sync_converges_respects_tombstones_guards_mass_deletion_and_keeps_pin_folders_local()
    {
        let mut extra = serde_json::Map::new();
        extra.insert(
            "folder_id".to_owned(),
            serde_json::Value::String("fld-1".to_owned()),
        );
        let pin = Pin {
            id: "pin-1".to_owned(),
            url: "https://linear.app/".to_owned(),
            home: Some("https://linear.app/".to_owned()),
            title: "Linear".to_owned(),
            letter: "L".to_owned(),
            row: None,
            space: None,
            shared: false,
            extra,
        };
        assert!(
            !sanitize_pin_for_folder_sync(&pin)
                .extra
                .contains_key("folder_id")
        );

        let mut baseline = BTreeMap::new();
        baseline.insert(
            "bm-1".to_owned(),
            VersionedSyncItem {
                id: "bm-1".to_owned(),
                value: Some("https://example.com".to_owned()),
                t_ms: 1_000,
                device_id: "mac-a".to_owned(),
            },
        );

        // Mass-deletion guard ignores a completely empty device map when baseline had live items
        let empty_device: BTreeMap<String, VersionedSyncItem<String>> = BTreeMap::new();
        let after_empty = merge_sync_items(&baseline, &[empty_device], 2_000);
        assert_eq!(after_empty.len(), 1);

        // Newer tombstone wins and does not resurrect from an older live entry
        let mut device_b = BTreeMap::new();
        device_b.insert(
            "bm-1".to_owned(),
            VersionedSyncItem {
                id: "bm-1".to_owned(),
                value: None,
                t_ms: 1_500,
                device_id: "mac-b".to_owned(),
            },
        );
        let merged = merge_sync_items(&baseline, &[device_b], 2_000);
        assert!(merged["bm-1"].is_tombstone());

        // After 365 days, the tombstone is garbage-collected
        let after_year = merge_sync_items(&merged, &[], 2_000 + SYNC_TOMBSTONE_RETENTION_MS + 1);
        assert!(after_year.is_empty());
    }

    #[test]
    fn side_panel_reloads_on_space_switch_and_holds_back_extension_on_private_tab() {
        let mut panel = SidePanelController::new();
        let space_a = SpaceId::new();
        let space_b = SpaceId::new();
        panel
            .open_website(SidePanelPresetSite::Claude.page_url(), space_a)
            .expect("claude opens");
        assert!(panel.on_space_switched(space_b));
        assert!(!SidePanelController::closes_on_cmd_w());

        // Extension without private access is held back when a private tab is active
        assert!(panel.toggle_extension_panel("ext-1", false, None));
        assert!(panel.is_held_back_for_active_tab(true));
        assert!(!panel.is_held_back_for_active_tab(false));
        // Pressing same extension button closes it
        assert!(!panel.toggle_extension_panel("ext-1", false, None));
    }

    #[test]
    fn launcher_validates_hotkeys_preserves_draft_on_escape_and_never_queries_network() {
        let rec = recommended_launcher_hotkey();
        assert!(validate_launcher_hotkey(&rec).is_ok());

        let reserved = KeyChord::new(
            KeyModifiers {
                cmd: true,
                alt: false,
                shift: false,
                ctrl: false,
            },
            "C",
        );
        assert!(validate_launcher_hotkey(&reserved).is_err());

        let mut launcher = LauncherController::new();
        assert!(!LauncherController::allows_network_suggestions());
        assert!(!launcher.show());
        launcher.draft = "linear.app".to_owned();
        assert!(!launcher.dismiss_keep_draft());
        assert_eq!(launcher.draft, "linear.app");
    }

    #[test]
    fn space_routes_only_route_external_links_and_mailto_caps_recipients_and_url_length() {
        let mut router = SpaceRouter::new();
        let personal = SpaceId::new();
        let work = SpaceId::new();
        router.set_route("github.com", work);

        let gh_url = PageUrl::parse("https://gist.github.com/user/123").expect("valid");
        // External link matches subdomain route -> opens in Work space
        assert_eq!(
            router.route_link(
                &gh_url,
                LinkIntakeSource::ExternalApp,
                personal,
                false,
                true,
                false,
                2,
                true,
                false,
                None,
            ),
            ExternalLinkRoute::OpenInSpace(work)
        );
        // Typed address is NOT routed to Work space (`TODO.md` P3-18)
        assert_eq!(
            router.route_link(
                &gh_url,
                LinkIntakeSource::TypedAddress,
                personal,
                false,
                true,
                false,
                2,
                true,
                false,
                None,
            ),
            ExternalLinkRoute::OpenInSpace(personal)
        );

        // RFC 6068 mailto caps recipients at 50, subject at 500, and total URL at 8,000
        let many_to = (0..65)
            .map(|i| format!("u{i}@example.com"))
            .collect::<Vec<_>>()
            .join(",");
        let long_body = "a".repeat(12_000);
        let raw_mailto = format!("mailto:{many_to}?subject=Hello&body={long_body}");
        let parsed = MailtoMessage::parse(&raw_mailto).expect("parsed mailto");
        assert_eq!(parsed.to.len(), MAX_MAILTO_RECIPIENTS);
        let (compose_url, opens_in_private) = parsed
            .build_webmail_compose(WebmailProvider::Gmail)
            .expect("gmail url");
        assert!(compose_url.as_str().len() <= MAX_MAILTO_URL_CHARS);
        assert!(!opens_in_private);
    }
}
