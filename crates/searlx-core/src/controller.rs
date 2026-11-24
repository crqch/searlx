//! Platform-independent browser controller state machine.
//!
//! Coordinates every domain manager in `searlx-core` behind an event/command
//! loop returning [`EngineDirective`]s for the host shell (gpui on macOS/Windows,
//! GTK on Linux, or `FakeEngine` in `searlx-testkit`) and exposing read-only
//! [`UiSnapshot`] views (`AGENTS.md` §3.1, §3.3).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::address::{
    AddressCommand, AddressInvalidReason, AddressResolution, ICloudTabCandidate, OpenTabCandidate,
    ParseOptions, PastSiteSearchRecord, SiteSearch, SubmitDisposition, SuggestionContext,
    SuggestionExtras, SuggestionRow, build_suggestions_full, builtin_site_searches,
    inline_completion, inspect_host_lookalike, resolve_address, should_query_search_suggestions,
};
use crate::ai_agents_bench::AgentSessionController;
use crate::blocking::{
    HideElementsManager, SiteCssManager, compile_webkit_content_rules_with_shield,
    normalize_site_key, youtube_ad_scripts,
};
use crate::bookmarks::{BookmarkMenuAction, BookmarksManager};
use crate::commands::{CommandId, CommandRegistry, KeyChord};
use crate::data::{ArchivedTab, DataStore, ScrollPosition, SessionFile, Split, TabGroup};
use crate::domain::{
    BookmarkItem, BookmarkTree, PermissionDecision, PermissionKind, PinItem, SpaceColor, SplitItem,
    TabGroupItem,
};
use crate::downloads::{DownloadEntry, DownloadsManager, HttpMethod};
use crate::error::{CoreError, Result};
use crate::find::{FindSession, FindTarget};
use crate::gestures_lifecycle::{TabLifecycleCandidate, evaluate_tab_lifecycle};
use crate::groups_splits_switcher::{
    MoonWindowCandidate, SplitPaneFrame, TabGroupsManager, TabSwitcherState, drag_two_pane_divider,
    layout_split_stage, move_split_focus,
};
use crate::history::{ClearHistoryScope, HistoryDayGroup, HistoryManager, RecentlyClosedItem};
use crate::ids::{GroupId, PinId, ProfileId, SpaceId, TabId, TabIdAllocator, WindowId};
use crate::menus_and_pages::{
    ConnectionState, InternalPageKind, SiteCardInput, SiteCardModel, SiteCardShortcuts,
    clean_link_url, open_internal_page, step_zoom_percent,
};
use crate::navigation::{InPageError, TabNavigation};
use crate::page_tools::{
    DUSK_FILTER_FALLBACK_CSS, DuskBudget, DuskController, DuskDecision, TranslationManager,
};
use crate::peek_small_float::{
    FloatStyle as CoreFloatStyle, FloatTrigger, FloatingVideoController, FloatingVideoSession,
    OpenerPageState, PeekResolution, PeekSession, SmallWindowsManager,
};
use crate::pins_and_folders::{PinFolderNode, PinsAndFoldersManager};
use crate::privacy::{PermissionStore, StoreCompositor};
use crate::reader::{
    ReaderArticle, ReaderTheme, detect_markdown_twin_link, exit_reader, extract_article,
};
use crate::recovery::{CrashRecoveryAction, CrashRecoveryManager, append_crash_log};
use crate::session::{
    RestoreMode, RestoredTab, RestoredWindow, WindowFrame, restore_session, save_window_session,
};
use crate::settings::{FloatStyle as SettingsFloatStyle, Settings, SettingsPage, TabLayoutMode};
use crate::spaces_profiles::{
    SpaceEntry, SpaceGradientPreset, SpaceStorePolicy, SpacesAndProfilesManager, WashStrength,
};
use crate::sync_panel_launcher_routes::{
    LauncherController, SidePanelContent, SidePanelController, SpaceRouter,
};
use crate::tabs::{DragPayload, DropTarget, TabEntry, TabModel};
use crate::url::PageUrl;
use crate::vault::{PasswordVault, PostSignInAction};

// ============================================================================
// Engine-independent protocol types (no dependency on `searlx-engine`)
// ============================================================================

/// Engine capabilities reported to [`BrowserController`] at startup (`AGENTS.md` §3.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct EngineCapabilities {
    /// Engine name (e.g. `"WebKit"`, `"WebView2"`, `"WebKitGTK"`, `"FakeEngine"`).
    pub engine: String,
    /// Web extensions support.
    pub extensions: bool,
    /// Platform passkeys / WebAuthn.
    pub passkeys: bool,
    /// Engine-compiled content blockers.
    pub content_blockers: bool,
    /// Native picture-in-picture.
    pub native_pip: bool,
    /// Per-site process isolation.
    pub site_isolation: bool,
    /// Per-tab user agent override support.
    pub custom_user_agent: bool,
    /// Printing support.
    pub print: bool,
    /// PDF viewing support.
    pub pdf: bool,
    /// Find-in-page inside PDFs.
    pub find_while_pdf: bool,
    /// Download pause/resume support.
    pub downloads_pause: bool,
    /// Importing cookies from another browser.
    pub cookie_import: bool,
    /// Wayland subsurface overlay support.
    pub wayland_overlay: bool,
    /// Floating video reparenting without reloading.
    pub floating_video: bool,
}

impl EngineCapabilities {
    /// Minimal baseline capabilities for a backend named `engine`.
    #[must_use]
    pub fn baseline(engine: impl Into<String>) -> Self {
        Self {
            engine: engine.into(),
            extensions: false,
            passkeys: false,
            content_blockers: false,
            native_pip: false,
            site_isolation: false,
            custom_user_agent: false,
            print: false,
            pdf: false,
            find_while_pdf: false,
            downloads_pause: false,
            cookie_import: false,
            wayland_overlay: false,
            floating_video: false,
        }
    }
}

impl Default for EngineCapabilities {
    fn default() -> Self {
        Self::baseline("baseline")
    }
}

/// Logical point rectangle passed to engine bounds/snapshot directives.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EngineRect {
    /// X coordinate in points.
    pub x: f64,
    /// Y coordinate in points.
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

/// Script execution world (`AGENTS.md` §3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptWorld {
    /// Page's own JavaScript world.
    Page,
    /// Browser's isolated script world.
    Isolated,
}

/// Document injection timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InjectAt {
    /// Before document parsing completes.
    DocumentStart,
    /// After DOMContentLoaded / end of document load.
    DocumentEnd,
}

/// Options used by the host shell when creating an engine webview for a tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EngineTabOptions {
    /// Space whose data store owns this tab.
    pub space: SpaceId,
    /// Non-persistent private store flag.
    pub private: bool,
    /// Optional initial URL (`None` lets `Inject`/`ApplyUserStylesheet` install before `Navigate`).
    pub url: Option<PageUrl>,
    /// Initial mute state.
    pub muted: bool,
    /// Initial zoom factor (`1.0` = 100%).
    pub zoom_factor: f64,
}

/// Find-in-page options emitted with `EngineDirective::Find`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineFindOptions {
    /// Match exact letter case.
    pub case_sensitive: bool,
    /// Match whole words only (suppressed on PDFs).
    pub whole_words: bool,
    /// Search backward (`⇧⌘G`).
    pub backwards: bool,
}

/// Data kinds cleared from an engine data store (`EngineDirective::ClearData`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct EngineDataKinds {
    /// Cookies & HTTP state.
    pub cookies: bool,
    /// Network/memory cache.
    pub cache: bool,
    /// LocalStorage / SessionStorage.
    pub storage: bool,
    /// IndexedDB / WebSQL.
    pub indexed_db: bool,
}

impl EngineDataKinds {
    /// All browsed site data kinds.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            cookies: true,
            cache: true,
            storage: true,
            indexed_db: true,
        }
    }
}

/// Engine permission kind reported by `EngineEvent::PermissionRequested`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EnginePermissionKind {
    /// Camera access.
    Camera,
    /// Microphone access.
    Microphone,
    /// Geolocation.
    Geolocation,
    /// Web notifications.
    Notifications,
    /// Screen / window capture.
    ScreenCapture,
    /// Clipboard read.
    ClipboardRead,
}

impl EnginePermissionKind {
    /// Convert to persistent core [`PermissionKind`] where persisted per origin.
    #[must_use]
    pub const fn to_core_kind(self) -> Option<PermissionKind> {
        match self {
            Self::Camera => Some(PermissionKind::Camera),
            Self::Microphone => Some(PermissionKind::Microphone),
            Self::Geolocation => Some(PermissionKind::Location),
            Self::Notifications => Some(PermissionKind::Notifications),
            Self::ScreenCapture | Self::ClipboardRead => None,
        }
    }

    /// Convert from persistent core [`PermissionKind`].
    #[must_use]
    pub const fn from_core_kind(kind: PermissionKind) -> Self {
        match kind {
            PermissionKind::Camera => Self::Camera,
            PermissionKind::Microphone => Self::Microphone,
            PermissionKind::Location => Self::Geolocation,
            PermissionKind::Notifications => Self::Notifications,
            PermissionKind::ScreenCapture => Self::ScreenCapture,
            PermissionKind::ClipboardRead => Self::ClipboardRead,
        }
    }
}

/// JavaScript dialog category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineDialogKind {
    /// `window.alert`
    Alert,
    /// `window.confirm`
    Confirm,
    /// `window.prompt`
    Prompt,
    /// `beforeunload` confirmation.
    BeforeUnload,
}

/// Disposition requested by a new-window event (`window.open` / `target=_blank`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineNewWindowDisposition {
    /// Open in a foreground tab.
    ForegroundTab,
    /// Open in a background tab.
    BackgroundTab,
    /// Popup window.
    Popup,
    /// Small window.
    SmallWindow,
}

/// Non-webview host dialog request (print dialog / file picker hint).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExternalDialogRequest {
    /// Invoke OS print sheet for `tab`.
    Print { tab: TabId },
    /// Open OS file picker for `tab`.
    FilePicker { tab: TabId, multiple: bool },
}

/// Directives emitted by [`BrowserController`] for the host shell to execute on its `Engine`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EngineDirective {
    /// Materialize a live engine webview for core `tab`.
    CreateTab {
        /// Core tab handle.
        tab: TabId,
        /// Initial store/privacy/audio/zoom options (`url` is `None` so rules/scripts install first).
        options: EngineTabOptions,
    },
    /// Release the engine webview for `tab` while preserving core tab metadata.
    CloseTab {
        /// Core tab handle.
        tab: TabId,
        /// Engine handle if previously bound.
        engine_tab_id: Option<TabId>,
    },
    /// Navigate a materialized tab to `url`.
    Navigate { tab: TabId, url: PageUrl },
    /// Load synthesized HTML (e.g. Reader Mode rendered article) into `tab`.
    LoadHtml {
        tab: TabId,
        base_url: Option<PageUrl>,
        html: String,
    },
    /// Step back in engine web history.
    Back { tab: TabId },
    /// Step forward in engine web history.
    Forward { tab: TabId },
    /// Reload `tab`.
    Reload { tab: TabId, from_origin: bool },
    /// Stop active loading on `tab`.
    Stop { tab: TabId },
    /// Evaluate JavaScript in a world on `tab`.
    Eval {
        tab: TabId,
        world: ScriptWorld,
        js: String,
    },
    /// Register a persistent user script on `tab` before first paint or at document end.
    Inject {
        tab: TabId,
        world: ScriptWorld,
        at: InjectAt,
        js: String,
    },
    /// Resize/position a webview in points.
    SetBounds { tab: TabId, rect: EngineRect },
    /// Show or hide a webview.
    SetVisible { tab: TabId, visible: bool },
    /// Give keyboard focus to `tab`'s webview.
    Focus { tab: TabId },
    /// Capture an RGBA snapshot of `tab`.
    Snapshot { tab: TabId, rect: EngineRect },
    /// Find query inside `tab`.
    Find {
        tab: TabId,
        query: String,
        options: EngineFindOptions,
    },
    /// Clear active find highlights on `tab`.
    ClearFind { tab: TabId },
    /// Apply page zoom factor (`1.0` = 100%).
    SetZoom { tab: TabId, zoom: f64 },
    /// Update tab mute state.
    SetMuted { tab: TabId, muted: bool },
    /// Clear storage for private session or a deleted `OwnStore` space.
    ClearData {
        space: Option<SpaceId>,
        private: bool,
        kinds: EngineDataKinds,
    },
    /// Apply compiled WebKit-format content blocker JSON (`FEATURES.md` §5.3).
    ApplyContentRules {
        tab: Option<TabId>,
        rules_json: String,
        shield_enabled: bool,
    },
    /// Install or update before-paint stylesheet (`hide_elements` + `site_css` + `dusk`).
    ApplyUserStylesheet {
        tab: TabId,
        style_id: String,
        css: String,
    },
    /// Answer an engine permission prompt.
    RespondPermission {
        tab: TabId,
        origin: String,
        kind: EnginePermissionKind,
        allow: bool,
    },
    /// Answer a JavaScript alert/confirm/prompt/beforeunload dialog.
    RespondScriptDialog {
        tab: TabId,
        accept: bool,
        prompt_text: Option<String>,
    },
    /// Answer an HTTP authentication challenge.
    RespondAuth {
        tab: TabId,
        username: Option<String>,
        password: Option<String>,
    },
    /// Answer an untrusted TLS certificate challenge.
    RespondCertificateError { tab: TabId, proceed: bool },
    /// Copy text to the system clipboard (with macOS/Wayland local-only / transient flags).
    CopyToClipboard {
        text: String,
        local_only: bool,
        transient: bool,
    },
    /// Ask host UI to open print sheet or file picker.
    OpenExternalDialog { request: ExternalDialogRequest },
}

/// Engine events delivered to [`BrowserController::handle_engine_event`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EngineEvent {
    /// A navigation started on `tab`.
    NavigationStarted { tab: TabId, url: PageUrl },
    /// A navigation committed on `tab`.
    NavigationCommitted { tab: TabId, url: PageUrl },
    /// A navigation finished on `tab`.
    NavigationFinished { tab: TabId, url: PageUrl },
    /// A navigation failed on `tab`.
    NavigationFailed {
        tab: TabId,
        url: PageUrl,
        reason: String,
    },
    /// Document title updated.
    TitleChanged { tab: TabId, title: String },
    /// Address changed (history API or redirect).
    UrlChanged { tab: TabId, url: PageUrl },
    /// Favicon URL updated.
    FaviconChanged { tab: TabId, url: Option<PageUrl> },
    /// Loading indicator toggled.
    LoadingChanged { tab: TabId, loading: bool },
    /// Engine history back/forward availability updated.
    HistoryChanged {
        tab: TabId,
        can_go_back: bool,
        can_go_forward: bool,
    },
    /// Page requested a popup or new tab.
    NewWindowRequested {
        tab: TabId,
        url: PageUrl,
        disposition: EngineNewWindowDisposition,
    },
    /// Page started a download.
    DownloadRequested {
        tab: TabId,
        url: PageUrl,
        method: HttpMethod,
        has_request_body: bool,
        suggested_name: Option<String>,
        total_bytes: Option<u64>,
    },
    /// Host download progressed.
    DownloadProgress {
        download_id: String,
        bytes_received: u64,
    },
    /// Host download failed.
    DownloadFailed { download_id: String, reason: String },
    /// Page asked for camera/mic/location/notifications/etc.
    PermissionRequested {
        tab: TabId,
        origin: String,
        kind: EnginePermissionKind,
    },
    /// Page opened a JS dialog.
    ScriptDialog {
        tab: TabId,
        kind: EngineDialogKind,
        message: String,
    },
    /// Page asked for a file picker.
    FilePickerRequested { tab: TabId, multiple: bool },
    /// Page requires HTTP basic/digest auth.
    AuthRequired { tab: TabId, origin: String },
    /// Untrusted TLS certificate blocked load.
    CertificateError {
        tab: TabId,
        url: PageUrl,
        reason: String,
    },
    /// HTTPS page loaded insecure HTTP subresource (`ConnectionState::MixedContent`).
    MixedContentDetected { tab: TabId },
    /// Audio/mute changed.
    MediaStateChanged {
        tab: TabId,
        audible: bool,
        muted: bool,
    },
    /// Fullscreen state changed.
    FullscreenChanged { tab: TabId, fullscreen: bool },
    /// Renderer/WebProcess terminated unexpectedly.
    ProcessCrashed { tab: TabId, reason: String },
    /// Script posted a JSON/text message back to core.
    ScriptMessage {
        tab: TabId,
        world: ScriptWorld,
        body: String,
    },
    /// Engine reported find-in-page counts.
    FindResults {
        tab: TabId,
        total_found: u32,
        active_index: u32,
    },
    /// Page pixel background color sampled for Dusk light/dark guard.
    PageRenderedLuminance { tab: TabId, r: u8, g: u8, b: u8 },
    /// Host readback/snapshot finished.
    SnapshotReady { tab: TabId, width: u32, height: u32 },
    /// Host extracted DOM HTML (and optional `.md` twin link) for Reader Mode.
    PageArticleExtracted {
        tab: TabId,
        html: String,
        markdown_twin_url: Option<PageUrl>,
    },
}

// ============================================================================
// Split Workspace manager
// ============================================================================

/// Owns all active multi-pane splits (`2..=4` panes per split) keyed by [`SpaceId`].
#[derive(Clone, Debug, Default)]
pub struct SplitWorkspace {
    splits_by_space: BTreeMap<SpaceId, Vec<SplitItem>>,
    snapped_even: BTreeSet<String>,
    stage_width_pt: f64,
    next_split_seq: u64,
}

impl SplitWorkspace {
    /// Default viewport width in points when no window resize event has arrived yet.
    pub const DEFAULT_STAGE_WIDTH_PT: f64 = 1280.0;

    /// Create an empty split workspace.
    #[must_use]
    pub fn new() -> Self {
        Self {
            splits_by_space: BTreeMap::new(),
            snapped_even: BTreeSet::new(),
            stage_width_pt: Self::DEFAULT_STAGE_WIDTH_PT,
            next_split_seq: 1,
        }
    }

    /// Current viewport width in points used for split layout clamping.
    #[must_use]
    pub const fn stage_width_pt(&self) -> f64 {
        self.stage_width_pt
    }

    /// Update viewport stage width in points (`FEATURES.md` §3.7).
    pub fn set_stage_width_pt(&mut self, width_pt: f64) {
        self.stage_width_pt = width_pt.max(0.0);
    }

    /// All splits defined in `space`.
    #[must_use]
    pub fn splits_for_space(&self, space: SpaceId) -> &[SplitItem] {
        self.splits_by_space.get(&space).map_or(&[], Vec::as_slice)
    }

    /// Find the split containing `tab` in `space`.
    #[must_use]
    pub fn split_containing(&self, space: SpaceId, tab: TabId) -> Option<&SplitItem> {
        self.splits_for_space(space)
            .iter()
            .find(|s| s.panes.contains(&tab))
    }

    /// Mutable lookup of the split containing `tab` in `space`.
    pub fn split_containing_mut(&mut self, space: SpaceId, tab: TabId) -> Option<&mut SplitItem> {
        self.splits_by_space
            .get_mut(&space)
            .and_then(|list| list.iter_mut().find(|s| s.panes.contains(&tab)))
    }

    /// Create a split in `space` from `panes` (after removing `panes` from any older split).
    pub fn create_split(&mut self, space: SpaceId, panes: Vec<TabId>) -> Result<String> {
        for &pane in &panes {
            let _ = self.remove_tab(space, pane)?;
        }
        let id = format!("split-{}", self.next_split_seq);
        self.next_split_seq = self.next_split_seq.saturating_add(1);
        let item = SplitItem::new(id.clone(), panes)?;
        self.splits_by_space.entry(space).or_default().push(item);
        Ok(id)
    }

    /// Add `new_tab` to the split containing `anchor_tab`.
    pub fn add_pane(&mut self, space: SpaceId, anchor_tab: TabId, new_tab: TabId) -> Result<()> {
        let _ = self.remove_tab(space, new_tab)?;
        let Some(split) = self.split_containing_mut(space, anchor_tab) else {
            self.create_split(space, vec![anchor_tab, new_tab])?;
            return Ok(());
        };
        split.add_pane(new_tab)?;
        let split_id = split.id.clone();
        self.snapped_even.remove(&split_id);
        Ok(())
    }

    /// Remove `tab` from any split in `space`, dissolving the split if fewer than 2 panes remain.
    pub fn remove_tab(&mut self, space: SpaceId, tab: TabId) -> Result<bool> {
        let Some(list) = self.splits_by_space.get_mut(&space) else {
            return Ok(false);
        };
        let Some(idx) = list.iter().position(|s| s.panes.contains(&tab)) else {
            return Ok(false);
        };
        let keep = list[idx].remove_pane(tab)?;
        if !keep {
            let removed = list.remove(idx);
            self.snapped_even.remove(&removed.id);
        }
        Ok(true)
    }

    /// Sync focused pane index inside the split containing `tab` to match `tab`.
    pub fn focus_tab_if_in_split(&mut self, space: SpaceId, tab: TabId) {
        if let Some(split) = self.split_containing_mut(space, tab)
            && let Some(pos) = split.panes.iter().position(|&p| p == tab)
        {
            split.focused = pos;
        }
    }

    /// Resize a two-pane divider using snap/unsnap hysteresis (`FEATURES.md` §3.7).
    pub fn resize_divider(
        &mut self,
        space: SpaceId,
        active_tab: TabId,
        stage_width_pt: f64,
        pointer_x: f64,
    ) -> Result<(f64, bool)> {
        self.stage_width_pt = stage_width_pt.max(0.0);
        let split_id = self
            .split_containing(space, active_tab)
            .ok_or_else(|| CoreError::Invalid("active tab is not in a split view".into()))?
            .id
            .clone();
        let was_snapped = self.snapped_even.contains(&split_id);
        let split = self
            .split_containing_mut(space, active_tab)
            .ok_or_else(|| CoreError::Invalid("active tab is not in a split view".into()))?;
        let (share, snapped) =
            drag_two_pane_divider(split, pointer_x, stage_width_pt, was_snapped)?;
        if snapped {
            self.snapped_even.insert(split_id);
        } else {
            self.snapped_even.remove(&split_id);
        }
        Ok((share, snapped))
    }

    /// Compute the frames currently rendered on stage for `active_tab`.
    #[must_use]
    pub fn visible_frames_for(&self, space: SpaceId, active_tab: TabId) -> Vec<SplitPaneFrame> {
        if let Some(split) = self.split_containing(space, active_tab) {
            layout_split_stage(split, self.stage_width_pt)
        } else {
            Vec::new()
        }
    }

    /// Restore pre-validated splits from a restored session.
    pub fn restore_space_splits(&mut self, space: SpaceId, items: Vec<SplitItem>) {
        if items.is_empty() {
            self.splits_by_space.remove(&space);
        } else {
            self.splits_by_space.insert(space, items);
        }
    }
}

// ============================================================================
// UI Command & Read-Only Snapshot Surface
// ============================================================================

/// All high-level user/UI actions handled by [`BrowserController::dispatch`].
#[derive(Clone, Debug, PartialEq)]
pub enum AppCommand {
    /// Open a new tab (`⌘T`). Reuses an empty `search://` tab if `url` is `None`.
    NewTab {
        url: Option<PageUrl>,
        activate: bool,
    },
    /// Open a new private tab (`⇧⌘N`).
    NewPrivateTab {
        url: Option<PageUrl>,
        activate: bool,
    },
    /// Open a web-only coding agent tab (`AGENTS.md` §9).
    NewAgentTab { url: PageUrl },
    /// Close `tab` (or active tab when `None`). Handles put-down pins & put-away grouped tabs.
    CloseTab { tab: Option<TabId> },
    /// Reopen last closed loose tab (`⇧⌘T`).
    ReopenClosedTab,
    /// Select and wake/materialize `tab`.
    ActivateTab { tab: TabId },
    /// Duplicate `tab` (or active tab when `None`).
    DuplicateTab { tab: Option<TabId> },
    /// Pin `tab` in the front section (`private` tabs fail).
    PinTab { tab: TabId },
    /// Unpin a pinned `tab`.
    UnpinTab { tab: TabId },
    /// Toggle pinned state for `tab` (or active tab).
    TogglePin { tab: Option<TabId> },
    /// Return a wandered pin to its home URL or replace home with current URL.
    RehomePin { tab: TabId, replace_home: bool },
    /// Mute `tab`.
    MuteTab { tab: TabId },
    /// Unmute `tab`.
    UnmuteTab { tab: TabId },
    /// Toggle mute on `tab` (or active tab).
    ToggleMute { tab: Option<TabId> },
    /// Assign or clear a custom tab title (`Rename`).
    RenameTab { tab: TabId, name: Option<String> },
    /// Put an inactive, quiet, non-loading background tab to sleep (`CloseTab` directive).
    SleepTab { tab: TabId },
    /// Reorder `tab` within pins or loose tabs.
    MoveTabToIndex { tab: TabId, index: usize },
    /// Copy current tab's cleaned address to clipboard (`⇧⌘C`).
    CopyAddress { tab: Option<TabId> },
    /// Copy a link after stripping click-tracking query parameters (`Copy Clean Link`).
    CopyCleanLink { url: PageUrl },

    /// Create a named, colored tab group in the active space.
    CreateTabGroup {
        name: String,
        color: SpaceColor,
        tabs: Vec<TabId>,
    },
    /// Add `tab` to existing `group`.
    AddTabToGroup { tab: TabId, group: GroupId },
    /// Remove `tab` from `group` (empty groups disappear).
    RemoveTabFromGroup { tab: TabId, group: GroupId },
    /// Collapse or expand all groups in the active space.
    CollapseAllGroups { collapsed: bool },
    /// Move a group (and its member tabs) to `space`.
    MoveGroupToSpace { group: GroupId, space: SpaceId },
    /// Close a tab group as a unit (`ClosedTabGroup` for `⇧⌘T`).
    CloseGroup { group: GroupId },
    /// Reopen the most recently closed group.
    ReopenClosedGroup,
    /// Create a pin folder in the sidebar (`<= 3` levels deep).
    CreatePinFolder {
        name: String,
        parent_id: Option<String>,
    },
    /// Move `pin` into `folder_id`.
    AddPinToFolder { pin: PinId, folder_id: String },
    /// Delete a pin folder, lifting its children up.
    DeletePinFolder { folder_id: String },

    /// Begin a unified drag session (`AGENTS.md` §3.3).
    BeginDrag {
        payload: DragPayload,
        x: f64,
        y: f64,
    },
    /// Update pointer position and hover drop target for the active drag.
    UpdateDrag {
        x: f64,
        y: f64,
        target: Option<DropTarget>,
    },
    /// Commit the active drag session (reorder/pin/unpin/group/split).
    CommitDrag,
    /// Cancel any active drag session.
    CancelDrag,

    /// Focus the address bar (`⌘L`).
    FocusAddressBar,
    /// Update typed address bar text (and suggestions / inline completion).
    SetAddressText { text: String },
    /// Lock or clear a site-search chip (`Tab`-to-search).
    LockSiteSearchChip { site: Option<SiteSearch> },
    /// Update live engine suggestions fetched asynchronously for the active query.
    UpdateEngineSuggestions {
        query: String,
        suggestions: Vec<String>,
    },
    /// Submit typed address/search/command (`Return`, `⌘Return`, `⇧⌘Return`, `⇧Return`).
    SubmitAddress {
        input: String,
        disposition: SubmitDisposition,
        shift_pressed: bool,
    },
    /// Dismiss any invalid-address shake banner.
    DismissAddressShake,
    /// Navigate `tab` directly to a resolved `PageUrl`.
    NavigateTab { tab: TabId, url: PageUrl },
    /// Navigate back in `tab` (or active tab).
    GoBack { tab: Option<TabId> },
    /// Navigate forward in `tab` (or active tab).
    GoForward { tab: Option<TabId> },
    /// Jump to a specific entry index in `tab`'s session history.
    JumpToHistoryEntry { tab: Option<TabId>, index: usize },
    /// Reload `tab` (or active tab).
    Reload {
        tab: Option<TabId>,
        from_origin: bool,
    },
    /// Stop active load on `tab` (`⌘.`).
    Stop { tab: Option<TabId> },
    /// Manually retry a crashed tab (`CrashRecoveryManager`).
    RetryCrashedTab { tab: Option<TabId> },

    /// Create a space in `profile` (or active window's profile).
    CreateSpace {
        profile: Option<ProfileId>,
        name: String,
        color: SpaceColor,
        icon: String,
        store_policy: SpaceStorePolicy,
    },
    /// Rename an existing space.
    RenameSpace { space: SpaceId, name: String },
    /// Update a space's visual styling.
    UpdateSpaceAppearance {
        space: SpaceId,
        color: SpaceColor,
        icon: String,
        gradient: Option<SpaceGradientPreset>,
        strength: WashStrength,
    },
    /// Switch the active window to `space` (parking outgoing space tabs).
    SwitchSpace { space: SpaceId },
    /// Delete a non-home space (queueing `ClearData` for `OwnStore`).
    DeleteSpace { space: SpaceId },
    /// Create a new profile with its own home space & window.
    CreateProfile { name: String },
    /// Delete a non-Personal profile and erase its stores.
    DeleteProfile { profile: ProfileId },

    /// Toggle Split View (with `partner_tab`, or active tab + next neighbor / blank).
    ToggleSplitView {
        target_tab: Option<TabId>,
        partner_tab: Option<TabId>,
    },
    /// Add `tab` as an additional pane (up to 4) in the active split.
    AddSplitPane { tab: TabId },
    /// Remove `tab` from its split view.
    RemoveSplitPane { tab: TabId },
    /// Drag the 2-pane split divider to `pointer_x`.
    ResizeSplit { stage_width_pt: f64, pointer_x: f64 },
    /// Explicitly set pane ratios on the active split.
    SetSplitShares { shares: Vec<f64> },
    /// Swap panes in the active split (`⌥⌘S`).
    SwapSplitPanes,
    /// Reset panes in the active split to equal ratios.
    EvenSplitPanes,
    /// Move keyboard focus across panes in the active split (`⌃⌘←` / `⌃⌘→`).
    MoveSplitFocus { delta: isize },
    /// Update viewport stage width (for responsive split folding <250pt/pane).
    SetViewportStageWidth { width_pt: f64 },

    /// Toggle sidebar fold (`⌘S` -> collapses or switches to `IconsOnly`).
    ToggleSidebarFold,
    /// Open or toggle a website in the side panel.
    ToggleSidePanelWebsite { url: PageUrl },
    /// Open or toggle compact Downloads in the side panel.
    ToggleSidePanelDownloads,
    /// Toggle an extension panel in the side panel.
    ToggleSidePanelExtension {
        extension_id: String,
        allowed_in_private: bool,
    },
    /// Close the side panel (`⌘W` never closes it).
    CloseSidePanel,
    /// Resize the side panel (`300..=640 pt`).
    SetSidePanelWidth { width_pt: f64 },
    /// Double-click divider reset to `380 pt`.
    ResetSidePanelWidth,

    /// Step the `⌃Tab` recent-tab switcher.
    SwitcherStep { backwards: bool, elapsed_ms: u64 },
    /// Release `⌃Tab` and switch to the highlighted card.
    SwitcherRelease,
    /// Cancel switcher without switching tabs.
    SwitcherCancel,

    /// Open the find bar (`⌘F`).
    OpenFindBar,
    /// Close the find bar (`Esc`).
    CloseFindBar,
    /// Run a find query inside the active tab (`txt_hint` provides optional text for headless/PDF matching).
    FindInPage {
        query: String,
        case_sensitive: bool,
        whole_words: bool,
        text_hint: Option<String>,
    },
    /// Step to next match (`⌘G`).
    FindNext,
    /// Step to previous match (`⇧⌘G`).
    FindPrevious,

    /// Step page zoom up/down in 5% increments (`⌘+` / `⌘-`).
    ZoomStep { delta_steps: i16 },
    /// Reset page zoom to 100% (`⌘0`).
    ZoomReset,
    /// Toggle Reader Mode (`⇧⌘R`). Supply `html` if already read or let engine extract.
    ToggleReaderMode { html: Option<String> },
    /// Change Reader Mode theme (`Paper` / `Sepia` / `Night` / `Auto`).
    SetReaderTheme { theme: ReaderTheme },
    /// Toggle Dusk per site (`⇧⌘D`).
    ToggleDuskForSite,
    /// Toggle global session-only Dusk pause (`⌥⇧⌘D`).
    ToggleDuskPausedEverywhere,
    /// Inform Dusk whether the browser window chrome is currently dark.
    SetFrameDark { dark: bool },
    /// Toggle per-site ad/tracker blocker pause from the Site Card.
    ToggleSiteBlocker { site_host: Option<String> },
    /// Hide an element selector on the active site (`Hide Elements`).
    HideElement { selector: String },
    /// Undo last hidden element (`⌘Z`).
    UndoHideElement,
    /// Restore a specific hidden selector on the active site.
    RestoreHiddenElement { selector: String },
    /// Restore all hidden selectors on the active site.
    RestoreAllHiddenElements,
    /// Update user CSS (`<style id="office-style">`) for `site_host` (read-only in private tabs).
    UpdateSiteCss {
        site_host: Option<String>,
        css: String,
    },
    /// Toggle per-site `"Keep This Site Awake"` on active tab.
    ToggleKeepAwake,
    /// Toggle per-site `"Play Sound by Itself"` on active tab.
    ToggleAutoplaySound,
    /// Toggle per-site `"Don't Float Videos Here"` on active tab.
    ToggleDontFloatVideos,

    /// Save/apply user permission response for `tab`.
    DecidePermission {
        tab: TabId,
        origin: String,
        kind: EnginePermissionKind,
        decision: PermissionDecision,
    },
    /// Resolve a port-spelling conflict from the Site Card (`Keep one`).
    ResolvePermissionPortConflict { url: PageUrl, keep_origin: String },
    /// `"Forget choices"` in Settings › Privacy.
    ForgetAllPermissions,
    /// Answer pending JS dialog.
    RespondJsDialog {
        tab: TabId,
        accept: bool,
        prompt_text: Option<String>,
    },
    /// Answer pending HTTP auth dialog.
    RespondAuthChallenge {
        tab: TabId,
        username: Option<String>,
        password: Option<String>,
    },
    /// Answer pending untrusted TLS cert decision.
    ProceedWithUntrustedCertificate { tab: TabId, proceed: bool },

    /// Respond to a pending password save/update offer (`Save` / `Not Now` / `Never Here`).
    SavePasswordOffer {
        tab: TabId,
        accept: bool,
        never_for_host: bool,
    },
    /// Fill saved credential into `tab` after verifying current origin == offered origin.
    FillCredential {
        tab: TabId,
        offered_origin: String,
        username: String,
        is_main_frame: bool,
    },
    /// Add or remove bookmark for the active tab (`⇧⌘B`).
    ToggleBookmark {
        parent_folder_id: Option<String>,
        title: Option<String>,
    },
    /// Add an explicit bookmark to the active profile.
    AddBookmark {
        parent_folder_id: Option<String>,
        title: String,
        url: PageUrl,
    },
    /// Add a bookmark folder to the active profile.
    AddBookmarkFolder {
        parent_folder_id: Option<String>,
        title: String,
    },
    /// Remove a bookmark node by ID.
    RemoveBookmark { id: String },
    /// Rename a bookmark node by ID.
    RenameBookmark { id: String, title: String },
    /// Clear history + all dependent artifacts (`search://history` › Clear History).
    ClearHistory { scope: ClearHistoryScope },
    /// Pause an active GET download.
    PauseDownload { id: String },
    /// Resume a paused GET download.
    ResumeDownload { id: String },
    /// Cancel a download.
    CancelDownload { id: String },
    /// Retry a failed download (or POST error).
    RetryDownload { id: String },

    /// Open/focus an internal `search://` page (never converts private tabs).
    OpenInternalPage {
        kind: InternalPageKind,
        detail: Option<String>,
    },
    /// Replace settings and propagate live state (`shield`, `sites_styles`, `dusk`, `notifications_ask`, layout, rules).
    UpdateSettings { settings: Box<Settings> },
    /// Override a keyboard shortcut (`Settings › Shortcuts`).
    OverrideShortcut { command: CommandId, chord: KeyChord },
    /// Reset a keyboard shortcut override back to default.
    ResetShortcut { command: CommandId },

    /// Manually float the active tab's video (`⌥⌘V` / Picture-in-Picture).
    StartFloatingVideo,
    /// Play/pause the active floating video.
    ToggleFloatingVideoPlayPause,
    /// Skip floating video playback by `delta_secs`.
    SkipFloatingVideo { delta_secs: f64 },
    /// Flick floating video to a corner.
    FlickFloatingVideo { vx: f64, vy: f64 },
    /// Return floating video back into its source tab.
    ReturnFloatingVideoToTab,

    /// Open a link in Peek preview over the active tab.
    OpenPeek { url: PageUrl },
    /// Keep the active Peek preview as an ordinary tab.
    KeepPeekAsTab,
    /// Keep the active Peek preview beside the opener in Split View.
    KeepPeekInSplit,
    /// Dismiss the active Peek preview (`Esc`).
    DismissPeek,

    /// Run background sleep / auto-close sweep using `Settings::tabs_sleep_pace` & `tabs_close_after`.
    TickLifecycle { now_secs: u64 },
    /// Advance controller clock used for timestamps in history, vault, and crash logs.
    AdvanceClock { now_secs: u64, now_iso: String },
    /// Persist session, windows, bookmarks, history, settings, hidden selectors, styles, and downloads.
    SaveSession,
    /// Execute a whole-word command matched by the address bar.
    ExecuteAddressCommand(AddressCommand),
}

// ============================================================================
// Read-Only UI Snapshot Types (`UiSnapshot`)
// ============================================================================

/// Summary of one space for the sidebar space switcher.
#[derive(Clone, Debug, PartialEq)]
pub struct UiSpaceItem {
    /// Space identifier.
    pub id: SpaceId,
    /// Owning profile identifier.
    pub profile: ProfileId,
    /// Display name.
    pub name: String,
    /// Accent color.
    pub color: SpaceColor,
    /// SF Symbol / icon name.
    pub icon: String,
    /// Optional custom gradient preset.
    pub gradient: Option<SpaceGradientPreset>,
    /// Background wash strength.
    pub strength: WashStrength,
    /// Cookie/history storage policy.
    pub store_policy: SpaceStorePolicy,
    /// Whether this space is currently displayed.
    pub active: bool,
    /// Total tabs inside this space.
    pub tab_count: usize,
}

/// Summary of one profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiProfileItem {
    /// Profile identifier.
    pub id: ProfileId,
    /// Profile name.
    pub name: String,
    /// Whether this is the immutable default `"Personal"` profile.
    pub is_personal: bool,
    /// Default space ID for this profile.
    pub home_space: SpaceId,
    /// Whether active in the current window.
    pub active: bool,
}

/// Read-only view state for a single tab or pin in the active space.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct UiTabItem {
    /// Core tab identifier.
    pub id: TabId,
    /// Bound engine webview handle (`None` when sleeping or `search://` internal).
    pub engine_tab_id: Option<TabId>,
    /// Current URL.
    pub url: PageUrl,
    /// Formatted display string (no scheme/`www.`).
    pub display_url: String,
    /// Editable address string shown on focus (strips `https://` when round-trip matches).
    pub editable_address: String,
    /// Effective display title (respects user custom rename).
    pub title: String,
    /// User-assigned custom title (`Rename`), if any.
    pub custom_name: Option<String>,
    /// Single-letter fallback badge.
    pub letter: char,
    /// Optional cached favicon URL.
    pub favicon: Option<PageUrl>,
    /// Pinned in the front section.
    pub pinned: bool,
    /// Pin ID if pinned.
    pub pin_id: Option<PinId>,
    /// Parent group ID if grouped.
    pub group: Option<GroupId>,
    /// Grouped tab put away with `⌘W`.
    pub put_away: bool,
    /// Navigation currently in progress.
    pub loading: bool,
    /// Playing audio.
    pub audible: bool,
    /// Muted.
    pub muted: bool,
    /// Asleep (webview released / unmaterialized).
    pub sleeping: bool,
    /// Placeholder text rendered for a sleeping tab.
    pub sleep_placeholder: Option<String>,
    /// Private tab (`⇧⌘N`).
    pub private: bool,
    /// Web-only agent tab.
    pub agent: bool,
    /// Active selection in the tab row.
    pub active: bool,
    /// Part of a split view in this space.
    pub in_split: bool,
    /// Focused pane within a split view.
    pub split_focused: bool,
    /// Whether the host triggered IDN lookalike protection.
    pub is_lookalike: bool,
    /// Reading scroll progress `0.0..=1.0`.
    pub reading_progress: f32,
    /// Whether this tab can step back.
    pub can_go_back: bool,
    /// Whether this tab can step forward.
    pub can_go_forward: bool,
    /// Page zoom percentage (`40..=300` or `20..=600` for PDF).
    pub zoom_percent: u16,
    /// In-page navigation error banner if last load failed.
    pub in_page_error: Option<InPageError>,
    /// Crash recovery retry message after >=2 crashes.
    pub crash_retry_message: Option<String>,
}

/// Active split pane geometry and state.
#[derive(Clone, Debug, PartialEq)]
pub struct UiSplitPaneItem {
    /// Tab ID rendered in this pane.
    pub tab: TabId,
    /// Live engine handle, if materialized.
    pub engine_tab_id: Option<TabId>,
    /// Left offset in stage points.
    pub x: f64,
    /// Width in stage points.
    pub width: f64,
    /// Focused pane flag.
    pub focused: bool,
    /// Current URL.
    pub url: PageUrl,
    /// Current title.
    pub title: String,
    /// Sleeping flag (false for visible panes).
    pub sleeping: bool,
    /// In-page load error, if any.
    pub in_page_error: Option<InPageError>,
    /// Crash retry prompt if loop suppressed.
    pub crash_retry_message: Option<String>,
}

/// Address field state & autocomplete rows for the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiAddressBarState {
    /// Draft text in the address field.
    pub text: String,
    /// Formatted URL display when unfocused.
    pub current_url_display: String,
    /// Pre-selected editable string when focusing the field (`⌘L`).
    pub editable_address: String,
    /// Grey inline suffix completion (`Return` accepts).
    pub inline_completion: Option<String>,
    /// Whether the address bar currently holds focus.
    pub focused: bool,
    /// Locked `Tab`-to-search site chip, if any.
    pub site_search_chip: Option<SiteSearch>,
    /// Rejection reason when last submit shook the bar.
    pub shake_reason: Option<AddressInvalidReason>,
    /// Whether active page's host is an IDN lookalike (`xn--…`).
    pub is_lookalike: bool,
    /// Safe host string (preserves punycode on lookalikes).
    pub safe_host: String,
    /// Ordered suggestion rows under the field.
    pub suggestions: Vec<SuggestionRow>,
}

/// Find-in-page bar state (`⌘F`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct UiFindState {
    /// Whether the find bar is visible.
    pub open: bool,
    /// Current query text.
    pub query: String,
    /// Rendered `"2 of 9"` / `"Not found"` status string.
    pub status_text: String,
    /// Case sensitivity toggle.
    pub case_sensitive: bool,
    /// Whole words toggle.
    pub whole_words: bool,
    /// False when viewing a PDF (`find.whole_words` suppressed).
    pub whole_words_available: bool,
    /// Total matches.
    pub total_matches: u32,
    /// 1-based active match index.
    pub active_match: u32,
}

/// Summary of Downloads state for chrome badge & compact panel.
#[derive(Clone, Debug, PartialEq)]
pub struct UiDownloadsSummary {
    /// Non-private visible downloads (private downloads are excluded).
    pub entries: Vec<DownloadEntry>,
    /// Total currently active downloads (includes private downloads in flight).
    pub active_count: usize,
    /// Aggregated bytes received across all active downloads.
    pub received_bytes: u64,
    /// Aggregated known total bytes across all active downloads.
    pub total_bytes: u64,
    /// Whether toolbar/sidebar download indicator should show (`Settings::downloads_button` or active > 0).
    pub show_button: bool,
}

/// Side panel state for UI layout.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct UiSidePanelState {
    /// Panel is logically open.
    pub open: bool,
    /// Panel is temporarily hidden because active tab is private and an extension isn't allowed there.
    pub held_back_for_private: bool,
    /// Panel width in points (`300..=640`).
    pub width_pt: f64,
    /// Active panel content.
    pub content: Option<SidePanelContent>,
}

/// Launcher overlay state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiLauncherState {
    /// Visible flag.
    pub visible: bool,
    /// Preserved draft text across Escape dismiss.
    pub draft: String,
}

/// Pending engine permission prompt waiting for user input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPermissionPrompt {
    /// Core tab ID.
    pub tab: TabId,
    /// Requesting origin.
    pub origin: String,
    /// Requested permission kind.
    pub kind: EnginePermissionKind,
}

/// Pending JavaScript dialog waiting for user response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingJsDialog {
    /// Core tab ID.
    pub tab: TabId,
    /// Alert / Confirm / Prompt / BeforeUnload.
    pub kind: EngineDialogKind,
    /// Dialog text.
    pub message: String,
}

/// Pending HTTP auth challenge waiting for credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingAuthChallenge {
    /// Core tab ID.
    pub tab: TabId,
    /// Origin requesting authentication.
    pub origin: String,
}

/// Pending untrusted certificate challenge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCertError {
    /// Core tab ID.
    pub tab: TabId,
    /// Failing URL.
    pub url: PageUrl,
    /// Error reason.
    pub reason: String,
}

/// Pending save/update password prompt for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPasswordOffer {
    /// Core tab ID.
    pub tab: TabId,
    /// Site origin.
    pub origin: String,
    /// Username candidate.
    pub username: String,
    /// True when updating an existing account password, false when saving a new one.
    pub is_update: bool,
}

/// Data payload rendered for the active internal `search://` page (`FEATURES.md` §26).
#[derive(Clone, Debug, PartialEq)]
pub enum InternalPageData {
    /// `search://settings` (and optional sub-section).
    Settings {
        /// Active Settings section (`General` by default).
        section: SettingsPage,
        /// Available sections in Settings.
        visible_sections: Vec<SettingsPage>,
        /// Saved choices for Dusk per site (`dusk.sites`).
        dusk_sites: BTreeMap<String, bool>,
        /// Stored permissions count.
        saved_permissions_count: usize,
        /// Engine support label.
        engine_name: String,
    },
    /// `search://history`.
    History {
        /// Visits grouped by Today / Yesterday / earlier.
        days: Vec<HistoryDayGroup>,
        /// Recently closed loose tabs (`<= 10`).
        recently_closed: Vec<RecentlyClosedItem>,
    },
    /// `search://downloads`.
    Downloads {
        /// Visible non-private downloads.
        entries: Vec<DownloadEntry>,
    },
    /// `search://bookmarks`.
    Bookmarks {
        /// Active profile bookmark tree.
        tree: BookmarkTree,
    },
    /// `search://passwords` (masked password rows only — never exposes plaintext secrets).
    Passwords {
        /// `(host, username, last_used, saved_from_clear_http)`
        accounts: Vec<(String, String, String, bool)>,
    },
    /// `search://archive`.
    Archive {
        /// Auto-closed archived tabs.
        archived_tabs: Vec<ArchivedTab>,
    },
    /// `search://import`.
    Import {
        /// Selected sub-source (`chrome`, `safari`, `search`, etc.).
        source: Option<String>,
    },
    /// `search://release-notes`.
    ReleaseNotes {
        /// Headline version string.
        version: String,
    },
}

/// Complete read-only UI snapshot produced by [`BrowserController::ui_snapshot`].
#[derive(Clone, Debug, PartialEq)]
pub struct UiSnapshot {
    /// Active window ID.
    pub window_id: WindowId,
    /// Active space ID.
    pub active_space: SpaceId,
    /// Active profile ID.
    pub active_profile: ProfileId,
    /// Spaces visible in the current profile/window.
    pub spaces: Vec<UiSpaceItem>,
    /// All profiles.
    pub profiles: Vec<UiProfileItem>,
    /// Tabs and pins for `active_space` (pins first).
    pub tabs: Vec<UiTabItem>,
    /// Pin folders in the sidebar.
    pub pin_folders: Vec<PinFolderNode>,
    /// Tab groups in `active_space`.
    pub groups: Vec<TabGroupItem>,
    /// Active tab ID in `active_space`.
    pub active_tab: Option<TabId>,
    /// Split panes currently sharing the content stage (empty if not in split).
    pub split_panes: Vec<UiSplitPaneItem>,
    /// Sidebar/strip layout mode.
    pub sidebar_layout: TabLayoutMode,
    /// Sidebar collapsed flag.
    pub sidebar_folded: bool,
    /// Address bar & autocomplete state.
    pub address: UiAddressBarState,
    /// Capability-gated site card for the active tab (None on internal `search://` blank).
    pub site_card: Option<SiteCardModel>,
    /// Active find bar state (when open).
    pub find: Option<UiFindState>,
    /// Bookmarks bar items for the active profile.
    pub bookmarks_bar: Vec<BookmarkItem>,
    /// Whether the active page is already bookmarked.
    pub is_active_page_bookmarked: bool,
    /// Downloads status summary.
    pub downloads: UiDownloadsSummary,
    /// Side panel visibility & content.
    pub side_panel: UiSidePanelState,
    /// Active floating video player, if any.
    pub floating_video: Option<FloatingVideoSession>,
    /// Active link peek preview, if any.
    pub peek: Option<PeekSession>,
    /// Active `⌃Tab` switcher overlay, if any.
    pub tab_switcher: Option<TabSwitcherState>,
    /// Global launcher overlay state.
    pub launcher: UiLauncherState,
    /// Pending permission prompt for the user.
    pub pending_permission: Option<PendingPermissionPrompt>,
    /// Pending JS dialog prompt.
    pub pending_dialog: Option<PendingJsDialog>,
    /// Pending auth challenge.
    pub pending_auth: Option<PendingAuthChallenge>,
    /// Pending TLS cert prompt.
    pub pending_cert_error: Option<PendingCertError>,
    /// Pending password save/update banner.
    pub pending_password_offer: Option<PendingPasswordOffer>,
    /// Structured data for the active `search://` internal page (`None` when blank or web page).
    pub active_internal_page: Option<InternalPageData>,
    /// Extracted article when Reader Mode is active on the active tab.
    pub active_reader_article: Option<ReaderArticle>,
    /// Ephemeral status/error message for the chrome banner.
    pub status_banner: Option<String>,
    /// Backend engine capabilities.
    pub capabilities: EngineCapabilities,
    /// Snapshot of current browser settings.
    pub settings: Settings,
}

// ============================================================================
// Per-tab runtime state & closed-tab stack
// ============================================================================

#[derive(Clone, Debug)]
enum TabCrashState {
    ManualRetry { reason: String },
}

#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools)]
struct TabRuntime {
    navigation: TabNavigation,
    find: Option<FindSession>,
    find_bar_open: bool,
    zoom_percent: u16,
    reader_active: bool,
    reader_article: Option<ReaderArticle>,
    reader_theme: ReaderTheme,
    available_markdown_twin: Option<PageUrl>,
    dusk_measured_light: Option<bool>,
    connection: ConnectionState,
    mixed_content: bool,
    certificate_error: bool,
    media_playing: bool,
    video_duration_secs: f64,
    keep_awake: bool,
    agent_tab: bool,
    crash_state: Option<TabCrashState>,
    skip_nav_stack_push_once: bool,
    pending_scroll_restore: Option<(f64, f64)>,
    last_active_unix_secs: u64,
    last_committed_url: Option<PageUrl>,
    pending_load_url: Option<PageUrl>,
    engine_can_go_back: bool,
    engine_can_go_forward: bool,
}

impl TabRuntime {
    fn new(is_small_window: bool, default_zoom: u16, now_secs: u64) -> Self {
        Self {
            navigation: TabNavigation::new(is_small_window),
            find: None,
            find_bar_open: false,
            zoom_percent: default_zoom.clamp(40, 300),
            reader_active: false,
            reader_article: None,
            reader_theme: ReaderTheme::System,
            available_markdown_twin: None,
            dusk_measured_light: None,
            connection: ConnectionState::LocalOrInternal,
            mixed_content: false,
            certificate_error: false,
            media_playing: false,
            video_duration_secs: 0.0,
            keep_awake: false,
            agent_tab: false,
            crash_state: None,
            skip_nav_stack_push_once: false,
            pending_scroll_restore: None,
            last_active_unix_secs: now_secs,
            last_committed_url: None,
            pending_load_url: None,
            engine_can_go_back: false,
            engine_can_go_forward: false,
        }
    }
}

#[derive(Clone, Debug)]
struct ClosedTabSnapshot {
    space: SpaceId,
    url: PageUrl,
    title: String,
    group: Option<GroupId>,
    scroll: (f64, f64),
    navigation: TabNavigation,
}

// ============================================================================
// BrowserController
// ============================================================================

/// Platform-independent controller coordinating every core browser subsystem.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct BrowserController {
    /// Persistent JSON backing store (`DataStore`).
    store: DataStore,
    /// Active engine capability flags.
    capabilities: EngineCapabilities,
    /// Persistent settings (`Settings`).
    settings: Settings,
    /// Monotonic runtime tab ID allocator.
    tab_allocator: TabIdAllocator,
    /// Spaces, profiles, windows, and store-erasure queues.
    spaces: SpacesAndProfilesManager,
    /// Active top-level window identifier.
    active_window_id: WindowId,
    /// Per-space tab rows (`TabModel`).
    space_tabs: BTreeMap<SpaceId, TabModel>,
    /// Per-space split views (`SplitWorkspace`).
    splits: SplitWorkspace,
    /// Pins and nested/live pin folders.
    pins: PinsAndFoldersManager,
    /// Named/colored tab groups & closed-group undo stack.
    tab_groups: TabGroupsManager,
    /// Per-tab runtime state (`TabNavigation`, zoom, find, reader, crash).
    runtimes: BTreeMap<TabId, TabRuntime>,
    /// Private-tab boundary & durable history/permissions compositor.
    compositor: StoreCompositor,
    /// Isolated per-space history managers for `SpaceStorePolicy::OwnStore` spaces.
    isolated_histories: BTreeMap<SpaceId, HistoryManager>,
    /// Empty fallback history returned for `OwnStore` spaces before their first visit.
    empty_history: HistoryManager,
    /// Snapshots of member tabs saved when closing a tab group for `ReopenClosedGroup`.
    closed_group_snapshots: BTreeMap<GroupId, Vec<ClosedTabSnapshot>>,
    /// Per-profile bookmarks managers.
    bookmarks: BTreeMap<ProfileId, BookmarksManager>,
    /// File downloads manager.
    downloads: DownloadsManager,
    /// Site blocker per-site pauses (durable vs session private).
    blocker_paused_sites: BTreeSet<String>,
    private_blocker_paused_sites: BTreeSet<String>,
    /// Per-site autoplay allowances (durable vs session private).
    autoplay_allowed_sites: BTreeSet<String>,
    private_autoplay_allowed_sites: BTreeSet<String>,
    /// Per-site "Don't Float Videos Here" suppressions.
    dont_float_sites: Vec<String>,
    private_dont_float_sites: Vec<String>,
    /// Persistent zoom overrides keyed by normalized host (non-private only).
    site_zoom_overrides: BTreeMap<String, u16>,
    /// Hide Elements manager (`selectors` + `⌘Z` undo + private-only map).
    hide_elements: HideElementsManager,
    /// Site CSS manager (`<style id="office-style">`, read-only in private tabs).
    site_css: SiteCssManager,
    /// Dusk darkening controller (`⇧⌘D`, `⌥⇧⌘D`).
    dusk: DuskController,
    /// Page-budget tracker for Dusk.
    dusk_budget: DuskBudget,
    /// Whether the outer browser chrome frame is currently dark.
    frame_is_dark: bool,
    /// In-page translation controller.
    translation: TranslationManager,
    /// Local encrypted password vault logic.
    vault: PasswordVault,
    /// Menu & keyboard shortcut registry.
    commands: CommandRegistry,
    /// Crash loop tracker and local `crash.log` writer.
    crash_recovery: CrashRecoveryManager,
    /// Side panel controller (`Website`, `Extension`, `Downloads`).
    side_panel: SidePanelController,
    /// Floating video controller.
    floating_video: FloatingVideoController,
    /// Domain-to-space external link router.
    space_router: SpaceRouter,
    /// Quick launcher state (`LauncherController`).
    launcher: LauncherController,
    /// Small windows manager.
    small_windows: SmallWindowsManager,
    /// Active link peek session.
    peek: Option<PeekSession>,
    /// Active `⌃Tab` MRU switcher session.
    tab_switcher: Option<TabSwitcherState>,
    /// Coding agent session boundary controller.
    agent_session: AgentSessionController,
    /// Most-recently-used tab order for the switcher & `close_goes_back`.
    mru_tabs: Vec<TabId>,
    /// Rich closed-tab undo stack (`⇧⌘T`).
    closed_tabs: Vec<ClosedTabSnapshot>,
    /// Which tabs currently have engine visibility (`SetVisible(..., true)`).
    visible_engine_tabs: BTreeSet<TabId>,
    /// Pending user prompts keyed globally (rendered in chrome).
    pending_permission: Option<PendingPermissionPrompt>,
    pending_dialog: Option<PendingJsDialog>,
    pending_auth: Option<PendingAuthChallenge>,
    pending_cert_error: Option<PendingCertError>,
    pending_password_offer: Option<PendingPasswordOffer>,
    pending_password_secret: Option<(TabId, String)>,
    /// Address bar state & cached suggestions.
    address_input: String,
    address_focused: bool,
    site_search_chip: Option<SiteSearch>,
    address_shake: Option<AddressInvalidReason>,
    engine_suggestions: Vec<String>,
    past_site_searches: Vec<PastSiteSearchRecord>,
    icloud_tabs: Vec<ICloudTabCandidate>,
    site_searches: Vec<SiteSearch>,
    status_banner: Option<String>,
    /// Logical monotonic clock (seconds + ISO timestamp).
    now_secs: u64,
    now_iso: String,
}

impl BrowserController {
    /// Create a fresh controller using `data_dir` without restoring prior session tabs.
    pub fn with_store(store: DataStore, capabilities: EngineCapabilities) -> Result<Self> {
        let settings = Settings::load_from(&store).unwrap_or_default();
        let hide_elements = HideElementsManager::load_from(&store).unwrap_or_default();
        let site_css = SiteCssManager::load_from(&store, settings.sites_styles)
            .unwrap_or_else(|_| SiteCssManager::new(settings.sites_styles));
        let history = HistoryManager::load_from(&store, None).unwrap_or_default();
        let spaces = SpacesAndProfilesManager::new();
        let personal_profile = spaces.profiles()[0].id;
        let personal_space = spaces.spaces()[0].id;
        let active_window_id = spaces.windows()[0].id;

        let bookmarks_mgr = BookmarksManager::load_from(&store, None).unwrap_or_default();
        let mut bookmarks = BTreeMap::new();
        bookmarks.insert(personal_profile, bookmarks_mgr);

        let mut compositor = StoreCompositor::new();
        compositor.history = history;
        compositor.permissions.notifications_globally_enabled = settings.notifications_ask;

        let mut tab_allocator = TabIdAllocator::new();
        let mut initial_model = TabModel::new(&mut tab_allocator)?;
        apply_settings_to_tab_model(&settings, &mut initial_model);
        let initial_tab_id = initial_model.active_tab().id;

        let now_secs = 1_700_000_000_u64;
        let now_iso = "2026-11-24T00:00:00Z".to_owned();

        let mut runtimes = BTreeMap::new();
        runtimes.insert(
            initial_tab_id,
            TabRuntime::new(false, settings.page_zoom, now_secs),
        );

        let mut space_tabs = BTreeMap::new();
        space_tabs.insert(personal_space, initial_model);

        let mut dusk = DuskController::new(settings.pages_dusk);
        if let Some(serde_json::Value::Object(map)) = settings.extra.get("dusk.sites") {
            for (k, v) in map {
                if let Some(enabled) = v.as_bool() {
                    dusk.durable_sites_mut().insert(k.clone(), enabled);
                }
            }
        }

        let translation = TranslationManager::new(settings.translate_target.clone());
        let pins = PinsAndFoldersManager::new(settings.pins_list);

        Ok(Self {
            store,
            capabilities,
            settings,
            tab_allocator,
            spaces,
            active_window_id,
            space_tabs,
            splits: SplitWorkspace::new(),
            pins,
            tab_groups: TabGroupsManager::new(),
            runtimes,
            compositor,
            isolated_histories: BTreeMap::new(),
            empty_history: HistoryManager::new(),
            closed_group_snapshots: BTreeMap::new(),
            bookmarks,
            downloads: DownloadsManager::new(),
            blocker_paused_sites: BTreeSet::new(),
            private_blocker_paused_sites: BTreeSet::new(),
            autoplay_allowed_sites: BTreeSet::new(),
            private_autoplay_allowed_sites: BTreeSet::new(),
            dont_float_sites: Vec::new(),
            private_dont_float_sites: Vec::new(),
            site_zoom_overrides: BTreeMap::new(),
            hide_elements,
            site_css,
            dusk,
            dusk_budget: DuskBudget::default(),
            frame_is_dark: true,
            translation,
            vault: PasswordVault::new(),
            commands: CommandRegistry::new(),
            crash_recovery: CrashRecoveryManager::new(),
            side_panel: SidePanelController::new(),
            floating_video: FloatingVideoController::new(),
            space_router: SpaceRouter::new(),
            launcher: LauncherController::new(),
            small_windows: SmallWindowsManager::new(),
            peek: None,
            tab_switcher: None,
            agent_session: AgentSessionController::new(),
            mru_tabs: vec![initial_tab_id],
            closed_tabs: Vec::new(),
            visible_engine_tabs: BTreeSet::new(),
            pending_permission: None,
            pending_dialog: None,
            pending_auth: None,
            pending_cert_error: None,
            pending_password_offer: None,
            pending_password_secret: None,
            address_input: String::new(),
            address_focused: false,
            site_search_chip: None,
            address_shake: None,
            engine_suggestions: Vec::new(),
            past_site_searches: Vec::new(),
            icloud_tabs: Vec::new(),
            site_searches: builtin_site_searches(),
            status_banner: None,
            now_secs,
            now_iso,
        })
    }

    /// Open and restore a browser session from `root`, returning initial [`EngineDirective`]s
    /// for ONLY the active tab (and visible active split panes); all inactive tabs stay asleep
    /// with `engine_tab_id: None` (`FEATURES.md` §1, §3.5).
    pub fn open(
        root: impl AsRef<Path>,
        capabilities: EngineCapabilities,
        mode: RestoreMode,
    ) -> Result<(Self, Vec<EngineDirective>)> {
        let store = DataStore::new(root.as_ref());
        let mut controller = Self::with_store(store, capabilities)?;
        let directives = controller.restore_on_launch(mode)?;
        Ok((controller, directives))
    }

    /// Restore the saved session into the active space, keeping inactive tabs asleep (`engine_tab_id: None`).
    pub fn restore_on_launch(&mut self, mode: RestoreMode) -> Result<Vec<EngineDirective>> {
        let effective_mode = if self.settings.start_fresh {
            RestoreMode::FreshStart
        } else {
            mode
        };
        let restored = restore_session(&self.store, effective_mode, &mut self.tab_allocator)?;
        let Some(first_win) = restored.windows.first() else {
            return Ok(Vec::new());
        };
        let space_id = self.active_space_id();
        self.apply_restored_window_to_space(space_id, first_win)?;
        if !restored.quarantined_files.is_empty() {
            self.status_banner = Some(format!(
                "Quarantined {} corrupt data file(s) during startup",
                restored.quarantined_files.len()
            ));
        }

        let mut directives = Vec::new();
        self.reconcile_visible_tabs(&mut directives)?;
        Ok(directives)
    }

    fn apply_restored_window_to_space(
        &mut self,
        space_id: SpaceId,
        win: &RestoredWindow,
    ) -> Result<()> {
        let mut key_to_tab_id: BTreeMap<String, TabId> = BTreeMap::new();
        let mut raw_group_to_id: BTreeMap<String, GroupId> = BTreeMap::new();
        let mut entries = Vec::with_capacity(win.tabs.len());

        // Clear runtimes for previous placeholder tab in this space.
        if let Some(old_model) = self.space_tabs.get(&space_id) {
            for t in old_model.tabs() {
                self.runtimes.remove(&t.id);
                self.mru_tabs.retain(|&id| id != t.id);
            }
        }

        for rtab in &win.tabs {
            key_to_tab_id.insert(rtab.key.clone(), rtab.id);
            let mut entry =
                TabEntry::new(rtab.id, rtab.url.clone(), rtab.title.clone(), rtab.private);
            entry.pinned = rtab.pinned;
            if rtab.pinned {
                let pin_id = PinId::new();
                entry.pin_id = Some(pin_id);
                entry.home = Some(rtab.url.clone());
                let mut pin = PinItem::new(pin_id, rtab.url.clone(), rtab.title.clone());
                pin.space = Some(space_id);
                self.pins.add_pin(pin);
            }
            if let Some(raw_gid) = &rtab.group {
                let gid = *raw_group_to_id.entry(raw_gid.clone()).or_default();
                entry.group = Some(gid);
            }
            entry.muted = rtab.muted;
            entry.sleeping = rtab.sleeping;
            entry.sleep_placeholder = rtab.sleeping.then(|| rtab.title.clone());
            entry.engine_tab_id = None;

            let mut runtime = TabRuntime::new(false, self.settings.page_zoom, self.now_secs);
            runtime
                .navigation
                .commit_navigation(rtab.url.clone(), rtab.title.clone());
            if let Some(scroll) = rtab.scroll {
                runtime.navigation.save_current_scroll(scroll.0, scroll.1);
                if !rtab.sleeping {
                    runtime.pending_scroll_restore = Some(scroll);
                }
            }
            runtime.connection = infer_connection_state(&rtab.url, false, false);
            if !rtab.url.is_internal() && !rtab.sleeping {
                runtime.skip_nav_stack_push_once = true;
            }
            self.runtimes.insert(rtab.id, runtime);
            self.mru_tabs.push(rtab.id);
            entries.push(entry);
        }

        for raw_group in &win.groups {
            let gid = *raw_group_to_id.entry(raw_group.id.clone()).or_default();
            let member_ids: Vec<TabId> = raw_group
                .tab_ids
                .iter()
                .filter_map(|key| key_to_tab_id.get(key).copied())
                .collect();
            if !member_ids.is_empty() {
                let color = SpaceColor::parse(&raw_group.color);
                let _ = self
                    .tab_groups
                    .create_group(space_id, &raw_group.name, color, member_ids);
                if raw_group.collapsed {
                    self.tab_groups.set_all_collapsed(space_id, true);
                }
                let _ = gid;
            }
        }

        let mut model = TabModel::from_tabs(entries, win.active, &mut self.tab_allocator)?;
        apply_settings_to_tab_model(&self.settings, &mut model);
        let active_id = model.active_tab().id;
        self.mru_tabs.retain(|&id| id != active_id);
        self.mru_tabs.insert(0, active_id);
        self.space_tabs.insert(space_id, model);

        let mut split_items = Vec::new();
        for split in &win.splits {
            let panes: Vec<TabId> = split_tab_keys_from_record(split)
                .iter()
                .filter_map(|key| key_to_tab_id.get(key).copied())
                .collect();
            if panes.len() >= 2
                && let Ok(mut item) = SplitItem::new(split.id.clone(), panes)
            {
                if item.panes.len() == 2 && split.ratio > 0.0 && split.ratio < 1.0 {
                    let _ = item.set_shares(&[split.ratio, 1.0 - split.ratio]);
                }
                split_items.push(item);
            }
        }
        self.splits.restore_space_splits(space_id, split_items);
        Ok(())
    }

    // ------------------------------------------------------------------------
    // Accessors for host shell, inspection, and testkit
    // ------------------------------------------------------------------------

    /// Record the engine-assigned `engine_tab_id` for `core_tab_id` after executing `CreateTab`.
    pub fn bind_engine_tab(&mut self, core_tab_id: TabId, engine_tab_id: TabId) {
        for model in self.space_tabs.values_mut() {
            if model.tab(core_tab_id).is_some() {
                model.set_engine_tab_id(core_tab_id, Some(engine_tab_id));
                break;
            }
        }
    }

    /// Look up the core `TabModel` for the active space.
    #[must_use]
    pub fn active_tab_model(&self) -> Option<&TabModel> {
        self.space_tabs.get(&self.active_space_id())
    }

    /// Look up the `TabModel` for any space.
    #[must_use]
    pub fn tab_model(&self, space: SpaceId) -> Option<&TabModel> {
        self.space_tabs.get(&space)
    }

    /// Look up a `TabEntry` anywhere across spaces.
    #[must_use]
    pub fn find_tab(&self, id: TabId) -> Option<(SpaceId, &TabEntry)> {
        for (&space_id, model) in &self.space_tabs {
            if let Some(tab) = model.tab(id) {
                return Some((space_id, tab));
            }
        }
        None
    }

    /// Active space ID.
    #[must_use]
    pub fn active_space_id(&self) -> SpaceId {
        self.spaces
            .windows()
            .iter()
            .find(|w| w.id == self.active_window_id)
            .map_or_else(|| self.spaces.spaces()[0].id, |w| w.active_space)
    }

    /// Active profile ID.
    #[must_use]
    pub fn active_profile_id(&self) -> ProfileId {
        self.spaces
            .windows()
            .iter()
            .find(|w| w.id == self.active_window_id)
            .map_or_else(|| self.spaces.profiles()[0].id, |w| w.profile())
    }

    /// Read-only access to the `DataStore`.
    #[must_use]
    pub const fn store(&self) -> &DataStore {
        &self.store
    }

    /// Read-only access to `Settings`.
    #[must_use]
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Read-only access to `StoreCompositor`.
    #[must_use]
    pub const fn compositor(&self) -> &StoreCompositor {
        &self.compositor
    }

    /// Read-only access to effective history for `space_id` (`OwnStore` spaces have isolated history).
    #[must_use]
    pub fn history_for_space(&self, space_id: SpaceId) -> &HistoryManager {
        if self
            .spaces
            .space(space_id)
            .is_some_and(SpaceEntry::uses_own_history_file)
        {
            return self
                .isolated_histories
                .get(&space_id)
                .unwrap_or(&self.empty_history);
        }
        &self.compositor.history
    }

    fn record_visit_for_space(&mut self, space_id: SpaceId, url: PageUrl, title: &str) {
        let uses_own_history = self
            .spaces
            .space(space_id)
            .is_some_and(SpaceEntry::uses_own_history_file);
        if uses_own_history {
            self.isolated_histories
                .entry(space_id)
                .or_default()
                .record_visit(url, title, &self.now_iso, false, Some(space_id.to_string()));
        } else {
            self.compositor.history.record_visit(
                url,
                title,
                &self.now_iso,
                false,
                Some(space_id.to_string()),
            );
        }
    }

    /// Active profile's `BookmarksManager`.
    #[must_use]
    pub fn bookmarks(&self) -> Option<&BookmarksManager> {
        self.bookmarks.get(&self.active_profile_id())
    }

    /// Read-only access to `DownloadsManager`.
    #[must_use]
    pub const fn downloads(&self) -> &DownloadsManager {
        &self.downloads
    }

    /// Read-only access to `SpacesAndProfilesManager`.
    #[must_use]
    pub const fn spaces(&self) -> &SpacesAndProfilesManager {
        &self.spaces
    }

    /// Read-only access to `PinsAndFoldersManager`.
    #[must_use]
    pub const fn pins(&self) -> &PinsAndFoldersManager {
        &self.pins
    }

    /// Read-only access to `TabGroupsManager`.
    #[must_use]
    pub const fn tab_groups(&self) -> &TabGroupsManager {
        &self.tab_groups
    }

    /// Read-only access to `SplitWorkspace`.
    #[must_use]
    pub const fn split_workspace(&self) -> &SplitWorkspace {
        &self.splits
    }

    /// Read-only access to `HideElementsManager`.
    #[must_use]
    pub const fn hide_elements(&self) -> &HideElementsManager {
        &self.hide_elements
    }

    /// Read-only access to `SiteCssManager`.
    #[must_use]
    pub const fn site_css(&self) -> &SiteCssManager {
        &self.site_css
    }

    /// Read-only access to `DuskController`.
    #[must_use]
    pub const fn dusk(&self) -> &DuskController {
        &self.dusk
    }

    /// Read-only access to `PermissionStore`.
    #[must_use]
    pub const fn permissions(&self) -> &PermissionStore {
        &self.compositor.permissions
    }

    /// Read-only access to `PasswordVault`.
    #[must_use]
    pub const fn password_vault(&self) -> &PasswordVault {
        &self.vault
    }

    /// Read-only access to `CommandRegistry`.
    #[must_use]
    pub const fn commands(&self) -> &CommandRegistry {
        &self.commands
    }

    /// Read-only access to `CrashRecoveryManager`.
    #[must_use]
    pub const fn crash_recovery(&self) -> &CrashRecoveryManager {
        &self.crash_recovery
    }

    /// Read-only access to `SidePanelController`.
    #[must_use]
    pub const fn side_panel(&self) -> &SidePanelController {
        &self.side_panel
    }

    /// Read-only access to `FloatingVideoController`.
    #[must_use]
    pub const fn floating_video(&self) -> &FloatingVideoController {
        &self.floating_video
    }

    /// Read-only access to `SpaceRouter`.
    #[must_use]
    pub const fn space_router(&self) -> &SpaceRouter {
        &self.space_router
    }

    /// Mutable access to `SpaceRouter` for link-routing rule updates.
    pub const fn space_router_mut(&mut self) -> &mut SpaceRouter {
        &mut self.space_router
    }

    /// Read-only access to `AgentSessionController`.
    #[must_use]
    pub const fn agent_session(&self) -> &AgentSessionController {
        &self.agent_session
    }

    // ------------------------------------------------------------------------
    // Command dispatch (`AppCommand` -> `Vec<EngineDirective>`)
    // ------------------------------------------------------------------------

    /// Execute a user or UI command and return engine directives for the host shell.
    #[allow(clippy::too_many_lines)]
    pub fn dispatch(&mut self, command: AppCommand) -> Result<Vec<EngineDirective>> {
        let mut directives = Vec::new();
        match command {
            AppCommand::NewTab { url, activate } => {
                self.cmd_new_tab(url, false, activate, &mut directives)?;
            }
            AppCommand::NewPrivateTab { url, activate } => {
                self.cmd_new_tab(url, true, activate, &mut directives)?;
            }
            AppCommand::NewAgentTab { url } => {
                self.cmd_new_agent_tab(url, &mut directives)?;
            }
            AppCommand::CloseTab { tab } => {
                let target = tab.or_else(|| self.active_tab_id());
                if let Some(tab_id) = target {
                    self.cmd_close_tab(tab_id, &mut directives)?;
                }
            }
            AppCommand::ReopenClosedTab => {
                self.cmd_reopen_closed_tab(&mut directives)?;
            }
            AppCommand::ActivateTab { tab } => {
                self.cmd_activate_tab(tab, &mut directives)?;
            }
            AppCommand::DuplicateTab { tab } => {
                let target = tab.or_else(|| self.active_tab_id());
                if let Some(tab_id) = target {
                    self.cmd_duplicate_tab(tab_id, &mut directives)?;
                }
            }
            AppCommand::PinTab { tab } => {
                self.cmd_pin_tab(tab)?;
            }
            AppCommand::UnpinTab { tab } => {
                self.cmd_unpin_tab(tab)?;
            }
            AppCommand::TogglePin { tab } => {
                if let Some(id) = tab.or_else(|| self.active_tab_id())
                    && let Some((_, entry)) = self.find_tab(id)
                {
                    if entry.pinned {
                        self.cmd_unpin_tab(id)?;
                    } else {
                        self.cmd_pin_tab(id)?;
                    }
                }
            }
            AppCommand::RehomePin { tab, replace_home } => {
                self.cmd_rehome_pin(tab, replace_home, &mut directives)?;
            }
            AppCommand::MuteTab { tab } => self.set_tab_muted(tab, true, &mut directives)?,
            AppCommand::UnmuteTab { tab } => self.set_tab_muted(tab, false, &mut directives)?,
            AppCommand::ToggleMute { tab } => {
                if let Some(id) = tab.or_else(|| self.active_tab_id())
                    && let Some((_, entry)) = self.find_tab(id)
                {
                    let next = !entry.muted;
                    self.set_tab_muted(id, next, &mut directives)?;
                }
            }
            AppCommand::RenameTab { tab, name } => {
                if let Some((_, entry_space)) = self.find_tab(tab).map(|(s, _)| (tab, s))
                    && let Some(model) = self.space_tabs.get_mut(&entry_space)
                    && let Some(entry) = model.tab_mut(tab)
                {
                    entry.custom_name = name.filter(|s| !s.trim().is_empty());
                }
            }
            AppCommand::SleepTab { tab } => {
                self.cmd_sleep_tab(tab, &mut directives)?;
            }
            AppCommand::MoveTabToIndex { tab, index } => {
                if let Some((space, _)) = self.find_tab(tab)
                    && let Some(model) = self.space_tabs.get_mut(&space)
                {
                    model.move_tab_to_index(tab, index)?;
                }
            }
            AppCommand::CopyAddress { tab } => {
                if let Some(id) = tab.or_else(|| self.active_tab_id())
                    && let Some((_, entry)) = self.find_tab(id)
                {
                    let clean = clean_link_url(&entry.url);
                    directives.push(EngineDirective::CopyToClipboard {
                        text: clean.as_str().to_owned(),
                        local_only: entry.private,
                        transient: false,
                    });
                }
            }
            AppCommand::CopyCleanLink { url } => {
                let is_private = self.active_tab_entry().is_some_and(|t| t.private);
                directives.push(EngineDirective::CopyToClipboard {
                    text: clean_link_url(&url).as_str().to_owned(),
                    local_only: is_private,
                    transient: false,
                });
            }

            AppCommand::CreateTabGroup { name, color, tabs } => {
                self.cmd_create_tab_group(name, color, tabs)?;
            }
            AppCommand::AddTabToGroup { tab, group } => {
                if let Some((space, _)) = self.find_tab(tab)
                    && let Some(model) = self.space_tabs.get_mut(&space)
                    && let Some(entry) = model.tab_mut(tab)
                    && !entry.pinned
                    && !entry.private
                {
                    entry.group = Some(group);
                }
            }
            AppCommand::RemoveTabFromGroup { tab, group } => {
                if let Some((space, _)) = self.find_tab(tab)
                    && let Some(model) = self.space_tabs.get_mut(&space)
                    && let Some(entry) = model.tab_mut(tab)
                {
                    entry.group = None;
                    entry.put_away = false;
                }
                self.tab_groups.remove_tab_from_group(group, tab);
            }
            AppCommand::CollapseAllGroups { collapsed } => {
                self.tab_groups
                    .set_all_collapsed(self.active_space_id(), collapsed);
            }
            AppCommand::MoveGroupToSpace { group, space } => {
                self.cmd_move_group_to_space(group, space, &mut directives)?;
            }
            AppCommand::CloseGroup { group } => {
                let closed = self.tab_groups.close_group(group)?;
                let mut group_snaps = Vec::new();
                for &tab in &closed.group.tabs {
                    if let Some((space, tab_snap)) = self.find_tab(tab).map(|(s, t)| (s, t.clone()))
                    {
                        if !tab_snap.private {
                            let rt = self.runtimes.get(&tab);
                            let scroll = rt
                                .and_then(|r| r.navigation.current_entry())
                                .map_or((0.0, 0.0), |e| e.scroll);
                            let navigation = rt.map_or_else(
                                || {
                                    let mut nav = TabNavigation::new(false);
                                    nav.commit_navigation(
                                        tab_snap.url.clone(),
                                        tab_snap.display_title().to_owned(),
                                    );
                                    nav
                                },
                                |r| r.navigation.clone(),
                            );
                            group_snaps.push(ClosedTabSnapshot {
                                space,
                                url: tab_snap.url.clone(),
                                title: tab_snap.display_title().to_owned(),
                                group: Some(group),
                                scroll,
                                navigation,
                            });
                        }
                        if let Some(model) = self.space_tabs.get_mut(&space)
                            && let Some(entry) = model.tab_mut(tab)
                        {
                            entry.group = None;
                            entry.put_away = false;
                        }
                        self.close_tab_removing_webview_with_record(
                            space,
                            tab,
                            false,
                            &mut directives,
                        )?;
                    }
                }
                if !group_snaps.is_empty() {
                    self.closed_group_snapshots.insert(group, group_snaps);
                }
                self.reconcile_visible_tabs(&mut directives)?;
            }
            AppCommand::ReopenClosedGroup => {
                self.cmd_reopen_closed_group(&mut directives)?;
            }
            AppCommand::CreatePinFolder { name, parent_id } => {
                let fid = format!("pf-{}", PinId::new());
                self.pins.create_folder(parent_id.as_deref(), fid, &name)?;
            }
            AppCommand::AddPinToFolder { pin, folder_id } => {
                self.pins.add_pin_to_folder(pin, &folder_id)?;
            }
            AppCommand::DeletePinFolder { folder_id } => {
                self.pins.delete_folder_lifting_children(&folder_id)?;
            }

            AppCommand::BeginDrag { payload, x, y } => {
                if let Some(model) = self.space_tabs.get_mut(&self.active_space_id()) {
                    model.begin_drag(payload, x, y);
                }
            }
            AppCommand::UpdateDrag { x, y, target } => {
                if let Some(model) = self.space_tabs.get_mut(&self.active_space_id()) {
                    model.update_drag(x, y, target);
                }
            }
            AppCommand::CommitDrag => {
                self.cmd_commit_drag(&mut directives)?;
            }
            AppCommand::CancelDrag => {
                if let Some(model) = self.space_tabs.get_mut(&self.active_space_id()) {
                    model.cancel_drag();
                }
            }

            AppCommand::FocusAddressBar => {
                self.address_focused = true;
                self.address_shake = None;
                if let Some(tab) = self.active_tab_entry() {
                    self.address_input = tab.url.editable_address();
                }
            }
            AppCommand::SetAddressText { text } => {
                self.address_focused = true;
                self.address_shake = None;
                self.address_input = text;
            }
            AppCommand::LockSiteSearchChip { site } => {
                self.site_search_chip = site;
            }
            AppCommand::UpdateEngineSuggestions { query, suggestions } => {
                let is_private = self.active_tab_entry().is_some_and(|t| t.private);
                if should_query_search_suggestions(&query, self.settings.search_suggest, is_private)
                {
                    self.engine_suggestions = suggestions;
                } else {
                    self.engine_suggestions.clear();
                }
            }
            AppCommand::SubmitAddress {
                input,
                disposition,
                shift_pressed,
            } => {
                self.cmd_submit_address(&input, disposition, shift_pressed, &mut directives)?;
            }
            AppCommand::DismissAddressShake => {
                self.address_shake = None;
            }
            AppCommand::NavigateTab { tab, url } => {
                self.navigate_tab_to(tab, url, &mut directives)?;
            }
            AppCommand::GoBack { tab } => {
                let target = tab.or_else(|| self.active_tab_id());
                if let Some(id) = target {
                    self.cmd_history_step(id, true, &mut directives)?;
                }
            }
            AppCommand::GoForward { tab } => {
                let target = tab.or_else(|| self.active_tab_id());
                if let Some(id) = target {
                    self.cmd_history_step(id, false, &mut directives)?;
                }
            }
            AppCommand::JumpToHistoryEntry { tab, index } => {
                let target = tab.or_else(|| self.active_tab_id());
                if let Some(id) = target {
                    self.cmd_jump_history_entry(id, index, &mut directives)?;
                }
            }
            AppCommand::Reload { tab, from_origin } => {
                let target = tab.or_else(|| self.active_tab_id());
                if let Some(id) = target {
                    self.cmd_reload(id, from_origin, &mut directives)?;
                }
            }
            AppCommand::Stop { tab } => {
                if let Some(id) = tab.or_else(|| self.active_tab_id()) {
                    self.cmd_stop(id, &mut directives);
                }
            }
            AppCommand::RetryCrashedTab { tab } => {
                let target = tab.or_else(|| self.active_tab_id());
                if let Some(id) = target {
                    self.crash_recovery.clear_tab(id);
                    if let Some(rt) = self.runtimes.get_mut(&id) {
                        rt.crash_state = None;
                    }
                    self.cmd_reload(id, false, &mut directives)?;
                }
            }

            AppCommand::CreateSpace {
                profile,
                name,
                color,
                icon,
                store_policy,
            } => {
                let profile_id = profile.unwrap_or_else(|| self.active_profile_id());
                let space_id =
                    self.spaces
                        .create_space(profile_id, name, color, icon, store_policy)?;
                let mut model = TabModel::new(&mut self.tab_allocator)?;
                apply_settings_to_tab_model(&self.settings, &mut model);
                let init_tab = model.active_tab().id;
                self.runtimes.insert(
                    init_tab,
                    TabRuntime::new(false, self.settings.page_zoom, self.now_secs),
                );
                self.space_tabs.insert(space_id, model);
                if store_policy == SpaceStorePolicy::OwnStore {
                    let suffix = format!("space-{space_id}");
                    let hist =
                        HistoryManager::load_from(&self.store, Some(&suffix)).unwrap_or_default();
                    self.isolated_histories.insert(space_id, hist);
                }
            }
            AppCommand::RenameSpace { space, name } => {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    return Err(CoreError::Invalid("space name cannot be empty".into()));
                }
                let entry = self
                    .spaces
                    .space_mut(space)
                    .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
                entry.name = trimmed.to_owned();
            }
            AppCommand::UpdateSpaceAppearance {
                space,
                color,
                icon,
                gradient,
                strength,
            } => {
                let entry = self
                    .spaces
                    .space_mut(space)
                    .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
                entry.color = color;
                entry.icon = icon;
                entry.gradient = gradient;
                entry.strength = strength;
            }
            AppCommand::SwitchSpace { space } => {
                self.cmd_switch_space(space, &mut directives)?;
            }
            AppCommand::DeleteSpace { space } => {
                self.cmd_delete_space(space, &mut directives)?;
            }
            AppCommand::CreateProfile { name } => {
                let (profile_id, home_space, _win_id) = self.spaces.create_profile(name);
                let mut model = TabModel::new(&mut self.tab_allocator)?;
                apply_settings_to_tab_model(&self.settings, &mut model);
                let init_tab = model.active_tab().id;
                self.runtimes.insert(
                    init_tab,
                    TabRuntime::new(false, self.settings.page_zoom, self.now_secs),
                );
                self.space_tabs.insert(home_space, model);
                self.bookmarks.insert(profile_id, BookmarksManager::new());
            }
            AppCommand::DeleteProfile { profile } => {
                let _deleted_files = self.spaces.delete_profile(profile)?;
                self.bookmarks.remove(&profile);
                for space_id in self.spaces.erasing_stores().to_vec() {
                    directives.push(EngineDirective::ClearData {
                        space: Some(space_id),
                        private: false,
                        kinds: EngineDataKinds::all(),
                    });
                    self.space_tabs.remove(&space_id);
                    self.isolated_histories.remove(&space_id);
                }
            }

            AppCommand::ToggleSplitView {
                target_tab,
                partner_tab,
            } => {
                self.cmd_toggle_split_view(target_tab, partner_tab, &mut directives)?;
            }
            AppCommand::AddSplitPane { tab } => {
                let space = self.active_space_id();
                if let Some(active_id) = self.active_tab_id() {
                    self.splits.add_pane(space, active_id, tab)?;
                    self.reconcile_visible_tabs(&mut directives)?;
                }
            }
            AppCommand::RemoveSplitPane { tab } => {
                let space = self.active_space_id();
                if self.splits.remove_tab(space, tab)? {
                    self.reconcile_visible_tabs(&mut directives)?;
                }
            }
            AppCommand::ResizeSplit {
                stage_width_pt,
                pointer_x,
            } => {
                let space = self.active_space_id();
                if let Some(active_id) = self.active_tab_id() {
                    self.splits
                        .resize_divider(space, active_id, stage_width_pt, pointer_x)?;
                    self.emit_split_bounds_directives(&mut directives);
                }
            }
            AppCommand::SetSplitShares { shares } => {
                let space = self.active_space_id();
                if let Some(active_id) = self.active_tab_id()
                    && let Some(split) = self.splits.split_containing_mut(space, active_id)
                {
                    split.set_shares(&shares)?;
                    self.emit_split_bounds_directives(&mut directives);
                }
            }
            AppCommand::SwapSplitPanes => {
                let space = self.active_space_id();
                if let Some(active_id) = self.active_tab_id()
                    && let Some(split) = self.splits.split_containing_mut(space, active_id)
                {
                    split.swap();
                    self.emit_split_bounds_directives(&mut directives);
                }
            }
            AppCommand::EvenSplitPanes => {
                let space = self.active_space_id();
                if let Some(active_id) = self.active_tab_id()
                    && let Some(split) = self.splits.split_containing_mut(space, active_id)
                {
                    split.even_out();
                    self.emit_split_bounds_directives(&mut directives);
                }
            }
            AppCommand::MoveSplitFocus { delta } => {
                let space = self.active_space_id();
                if let Some(active_id) = self.active_tab_id()
                    && let Some(split) = self.splits.split_containing_mut(space, active_id)
                {
                    let next_tab = move_split_focus(split, delta);
                    self.cmd_activate_tab(next_tab, &mut directives)?;
                }
            }
            AppCommand::SetViewportStageWidth { width_pt } => {
                self.splits.set_stage_width_pt(width_pt);
                self.reconcile_visible_tabs(&mut directives)?;
            }

            AppCommand::ToggleSidebarFold => {
                if let Some(model) = self.space_tabs.get_mut(&self.active_space_id()) {
                    model.toggle_fold();
                }
            }
            AppCommand::ToggleSidePanelWebsite { url } => {
                if matches!(
                    self.side_panel.content(),
                    Some(SidePanelContent::Website { url: cur, .. }) if cur.identity() == url.identity()
                ) {
                    self.side_panel.close();
                } else {
                    self.side_panel.open_website(url, self.active_space_id())?;
                }
            }
            AppCommand::ToggleSidePanelDownloads => {
                if matches!(self.side_panel.content(), Some(SidePanelContent::Downloads)) {
                    self.side_panel.close();
                } else {
                    self.side_panel.open_downloads();
                }
            }
            AppCommand::ToggleSidePanelExtension {
                extension_id,
                allowed_in_private,
            } => {
                let bound = self.active_tab_id();
                self.side_panel
                    .toggle_extension_panel(extension_id, allowed_in_private, bound);
            }
            AppCommand::CloseSidePanel => {
                self.side_panel.close();
            }
            AppCommand::SetSidePanelWidth { width_pt } => {
                self.side_panel.set_width_pt(width_pt);
            }
            AppCommand::ResetSidePanelWidth => {
                self.side_panel.reset_width();
            }

            AppCommand::SwitcherStep {
                backwards,
                elapsed_ms,
            } => {
                self.cmd_switcher_step(backwards, elapsed_ms);
            }
            AppCommand::SwitcherRelease => {
                if let Some(switcher) = self.tab_switcher.take() {
                    let (selected, _) = switcher.commit_release();
                    if let Some(target) = selected {
                        self.cmd_activate_tab(target, &mut directives)?;
                    }
                }
            }
            AppCommand::SwitcherCancel => {
                self.tab_switcher = None;
            }

            AppCommand::OpenFindBar => {
                if let Some((tab_id, url)) = self.active_tab_entry().map(|t| (t.id, t.url.clone()))
                {
                    let is_pdf =
                        matches!(FindTarget::for_url(&url), FindTarget::Page { is_pdf: true });
                    if let Some(rt) = self.runtimes.get_mut(&tab_id) {
                        rt.find_bar_open = true;
                        rt.find.get_or_insert_with(|| FindSession::new(is_pdf));
                    }
                }
            }
            AppCommand::CloseFindBar => {
                if let Some(tab_id) = self.active_tab_id() {
                    if let Some(rt) = self.runtimes.get_mut(&tab_id) {
                        rt.find_bar_open = false;
                        rt.find = None;
                    }
                    if self
                        .find_tab(tab_id)
                        .is_some_and(|(_, t)| t.engine_tab_id.is_some())
                    {
                        directives.push(EngineDirective::ClearFind { tab: tab_id });
                    }
                }
            }
            AppCommand::FindInPage {
                query,
                case_sensitive,
                whole_words,
                text_hint,
            } => {
                self.cmd_find_in_page(
                    &query,
                    case_sensitive,
                    whole_words,
                    text_hint.as_deref(),
                    &mut directives,
                );
            }
            AppCommand::FindNext => {
                self.cmd_step_find(false, &mut directives);
            }
            AppCommand::FindPrevious => {
                self.cmd_step_find(true, &mut directives);
            }

            AppCommand::ZoomStep { delta_steps } => {
                self.cmd_zoom_step(Some(delta_steps), &mut directives)?;
            }
            AppCommand::ZoomReset => {
                self.cmd_zoom_step(None, &mut directives)?;
            }
            AppCommand::ToggleReaderMode { html } => {
                self.cmd_toggle_reader_mode(html.as_deref(), &mut directives)?;
            }
            AppCommand::SetReaderTheme { theme } => {
                if let Some(id) = self.active_tab_id() {
                    let has_engine = self
                        .find_tab(id)
                        .is_some_and(|(_, t)| t.engine_tab_id.is_some());
                    let html_directive = self.runtimes.get_mut(&id).and_then(|rt| {
                        rt.reader_theme = theme;
                        if rt.reader_active
                            && let Some(article) = &rt.reader_article
                            && has_engine
                        {
                            Some(EngineDirective::LoadHtml {
                                tab: id,
                                base_url: Some(article.source_url.clone()),
                                html: article.render_document(theme, self.frame_is_dark),
                            })
                        } else {
                            None
                        }
                    });
                    if let Some(d) = html_directive {
                        directives.push(d);
                    }
                }
            }
            AppCommand::ToggleDuskForSite => {
                self.cmd_toggle_dusk_for_site(&mut directives);
            }
            AppCommand::ToggleDuskPausedEverywhere => {
                let _ = self.dusk.toggle_pause_everywhere();
                self.reapply_dusk_on_visible_tabs(&mut directives);
            }
            AppCommand::SetFrameDark { dark } => {
                self.frame_is_dark = dark;
                self.reapply_dusk_on_visible_tabs(&mut directives);
            }
            AppCommand::ToggleSiteBlocker { site_host } => {
                self.cmd_toggle_site_blocker(site_host.as_deref(), &mut directives);
            }
            AppCommand::HideElement { selector } => {
                self.cmd_hide_element(&selector, &mut directives);
            }
            AppCommand::UndoHideElement => {
                if let Some(_undone) = self.hide_elements.undo_last() {
                    self.reapply_active_tab_styles_and_scripts(&mut directives);
                    let _ = self.hide_elements.save_to(&self.store);
                }
            }
            AppCommand::RestoreHiddenElement { selector } => {
                if let Some((host, is_private)) = self.active_tab_host_and_private()
                    && self.hide_elements.restore_one(&host, &selector, is_private)
                {
                    self.reapply_active_tab_styles_and_scripts(&mut directives);
                    if !is_private {
                        let _ = self.hide_elements.save_to(&self.store);
                    }
                }
            }
            AppCommand::RestoreAllHiddenElements => {
                if let Some((host, is_private)) = self.active_tab_host_and_private()
                    && self.hide_elements.restore_all(&host, is_private) > 0
                {
                    self.reapply_active_tab_styles_and_scripts(&mut directives);
                    if !is_private {
                        let _ = self.hide_elements.save_to(&self.store);
                    }
                }
            }
            AppCommand::UpdateSiteCss { site_host, css } => {
                self.cmd_update_site_css(site_host.as_deref(), &css, &mut directives)?;
            }
            AppCommand::ToggleKeepAwake => {
                if let Some(id) = self.active_tab_id()
                    && let Some(rt) = self.runtimes.get_mut(&id)
                {
                    rt.keep_awake = !rt.keep_awake;
                }
            }
            AppCommand::ToggleAutoplaySound => {
                if let Some((host, is_private)) = self.active_tab_host_and_private() {
                    let key = normalize_site_key(&host);
                    let set = if is_private {
                        &mut self.private_autoplay_allowed_sites
                    } else {
                        &mut self.autoplay_allowed_sites
                    };
                    if !set.remove(&key) {
                        set.insert(key);
                    }
                }
            }
            AppCommand::ToggleDontFloatVideos => {
                if let Some((host, is_private)) = self.active_tab_host_and_private() {
                    let key = normalize_site_key(&host);
                    let list = if is_private {
                        &mut self.private_dont_float_sites
                    } else {
                        &mut self.dont_float_sites
                    };
                    if let Some(pos) = list.iter().position(|h| h == &key) {
                        list.remove(pos);
                    } else {
                        list.push(key);
                    }
                }
            }

            AppCommand::DecidePermission {
                tab,
                origin,
                kind,
                decision,
            } => {
                self.cmd_decide_permission(tab, &origin, kind, decision, &mut directives)?;
            }
            AppCommand::ResolvePermissionPortConflict { url, keep_origin } => {
                self.compositor
                    .permissions
                    .resolve_port_conflict(&url, &keep_origin);
            }
            AppCommand::ForgetAllPermissions => {
                self.compositor.permissions.forget_all_choices();
            }
            AppCommand::RespondJsDialog {
                tab,
                accept,
                prompt_text,
            } => {
                if self.pending_dialog.as_ref().is_some_and(|d| d.tab == tab) {
                    self.pending_dialog = None;
                }
                directives.push(EngineDirective::RespondScriptDialog {
                    tab,
                    accept,
                    prompt_text,
                });
            }
            AppCommand::RespondAuthChallenge {
                tab,
                username,
                password,
            } => {
                if self.pending_auth.as_ref().is_some_and(|a| a.tab == tab) {
                    self.pending_auth = None;
                }
                directives.push(EngineDirective::RespondAuth {
                    tab,
                    username,
                    password,
                });
            }
            AppCommand::ProceedWithUntrustedCertificate { tab, proceed } => {
                if self
                    .pending_cert_error
                    .as_ref()
                    .is_some_and(|c| c.tab == tab)
                {
                    self.pending_cert_error = None;
                }
                if !proceed && let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.certificate_error = true;
                    rt.connection = ConnectionState::UntrustedCertificate;
                }
                directives.push(EngineDirective::RespondCertificateError { tab, proceed });
            }

            AppCommand::SavePasswordOffer {
                tab,
                accept,
                never_for_host,
            } => {
                self.cmd_save_password_offer(tab, accept, never_for_host)?;
            }
            AppCommand::FillCredential {
                tab,
                offered_origin,
                username,
                is_main_frame,
            } => {
                self.cmd_fill_credential(
                    tab,
                    &offered_origin,
                    &username,
                    is_main_frame,
                    &mut directives,
                )?;
            }
            AppCommand::ToggleBookmark {
                parent_folder_id,
                title,
            } => {
                self.cmd_toggle_bookmark(parent_folder_id.as_deref(), title.as_deref())?;
            }
            AppCommand::AddBookmark {
                parent_folder_id,
                title,
                url,
            } => {
                let profile = self.active_profile_id();
                let mgr = self.bookmarks.entry(profile).or_default();
                mgr.add_bookmark(parent_folder_id.as_deref(), title, url)?;
            }
            AppCommand::AddBookmarkFolder {
                parent_folder_id,
                title,
            } => {
                let profile = self.active_profile_id();
                let mgr = self.bookmarks.entry(profile).or_default();
                mgr.add_folder(parent_folder_id.as_deref(), title)?;
            }
            AppCommand::RemoveBookmark { id } => {
                let profile = self.active_profile_id();
                let mgr = self.bookmarks.entry(profile).or_default();
                let _ = mgr.remove(&id)?;
            }
            AppCommand::RenameBookmark { id, title } => {
                let profile = self.active_profile_id();
                let mgr = self.bookmarks.entry(profile).or_default();
                mgr.rename(&id, title)?;
            }
            AppCommand::ClearHistory { scope } => {
                self.cmd_clear_history(scope)?;
            }
            AppCommand::PauseDownload { id } => self.downloads.pause(&id)?,
            AppCommand::ResumeDownload { id } => {
                let _ = self.downloads.resume(&id)?;
            }
            AppCommand::CancelDownload { id } => self.downloads.cancel(&id)?,
            AppCommand::RetryDownload { id } => self.downloads.retry(&id)?,

            AppCommand::OpenInternalPage { kind, detail } => {
                let url = kind.to_url(detail.as_deref())?;
                self.open_internal_page_url(url, &mut directives)?;
            }
            AppCommand::UpdateSettings { settings } => {
                self.cmd_update_settings(*settings, &mut directives)?;
            }
            AppCommand::OverrideShortcut { command, chord } => {
                self.commands
                    .set_override(command, Some(chord))
                    .map_err(|e| CoreError::Invalid(format!("shortcut conflict: {e}")))?;
            }
            AppCommand::ResetShortcut { command } => {
                self.commands.reset_override(&command);
            }

            AppCommand::StartFloatingVideo => {
                self.cmd_start_floating_video(FloatTrigger::ManualShortcut);
            }
            AppCommand::ToggleFloatingVideoPlayPause => {
                let _ = self.floating_video.toggle_play_pause();
            }
            AppCommand::SkipFloatingVideo { delta_secs } => {
                let _ = self.floating_video.skip_seconds(delta_secs);
            }
            AppCommand::FlickFloatingVideo { vx, vy } => {
                let _ = self.floating_video.flick_to_corner(vx, vy);
            }
            AppCommand::ReturnFloatingVideoToTab => {
                if let Some(outcome) = self.floating_video.return_to_tab() {
                    if outcome.space_id != self.active_space_id() {
                        self.cmd_switch_space(outcome.space_id, &mut directives)?;
                    }
                    self.cmd_activate_tab(outcome.tab_id, &mut directives)?;
                }
            }

            AppCommand::OpenPeek { url } => {
                if let Some(opener) = self.active_tab_entry().cloned() {
                    let peek_id = self.tab_allocator.allocate();
                    let opener_scroll = self
                        .runtimes
                        .get(&opener.id)
                        .and_then(|rt| rt.navigation.current_entry())
                        .map_or((0.0, 0.0), |e| e.scroll);
                    self.peek = Some(PeekSession::open(
                        OpenerPageState {
                            opener_tab_id: opener.id,
                            opener_url: opener.url.clone(),
                            scroll: ScrollPosition {
                                x: opener_scroll.0,
                                y: opener_scroll.1,
                                extra: serde_json::Map::new(),
                            },
                            focused_element_selector: None,
                            opened_from_pin: opener.pinned,
                        },
                        peek_id,
                        url,
                    ));
                }
            }
            AppCommand::KeepPeekAsTab => {
                if let Some(peek) = self.peek.take() {
                    let peek_url = peek.peek_url.clone();
                    if let PeekResolution::KeptAsTab { .. } = peek.keep_as_tab() {
                        self.cmd_new_tab(Some(peek_url), false, true, &mut directives)?;
                    }
                }
            }
            AppCommand::KeepPeekInSplit => {
                if let Some(peek) = self.peek.take() {
                    let peek_url = peek.peek_url.clone();
                    if let PeekResolution::KeptBesideInSplit { left_tab_id, .. } =
                        peek.keep_beside_in_split()
                    {
                        let new_id = self.cmd_new_tab_returning_id(
                            Some(peek_url),
                            false,
                            false,
                            &mut directives,
                        )?;
                        self.cmd_toggle_split_view(
                            Some(left_tab_id),
                            Some(new_id),
                            &mut directives,
                        )?;
                    }
                }
            }
            AppCommand::DismissPeek => {
                if let Some(peek) = self.peek.take() {
                    let _ = peek.close_with_escape();
                }
            }

            AppCommand::TickLifecycle { now_secs } => {
                self.now_secs = now_secs;
                self.cmd_tick_lifecycle(now_secs, &mut directives)?;
            }
            AppCommand::AdvanceClock { now_secs, now_iso } => {
                self.now_secs = now_secs;
                self.now_iso = now_iso;
            }
            AppCommand::SaveSession => {
                self.persist_all()?;
            }
            AppCommand::ExecuteAddressCommand(cmd) => {
                self.execute_address_command(cmd, &mut directives)?;
            }
        }
        Ok(directives)
    }

    // ------------------------------------------------------------------------
    // Engine event reconciliation (`EngineEvent` -> state + follow-up directives)
    // ------------------------------------------------------------------------

    /// Reconcile an [`EngineEvent`] from the host shell back into core managers and
    /// return follow-up [`EngineDirective`]s (content rules, Hide Elements / Site CSS /
    /// Dusk injection, permission replies, crash auto-reload, etc.).
    #[allow(clippy::too_many_lines)]
    pub fn handle_engine_event(&mut self, event: EngineEvent) -> Result<Vec<EngineDirective>> {
        let mut directives = Vec::new();
        match event {
            EngineEvent::NavigationStarted { tab, url } => {
                if let Some((space, prev_url, entry_private)) = self
                    .find_tab(tab)
                    .map(|(s, e)| (s, e.url.clone(), e.private))
                {
                    if let Some(rt) = self.runtimes.get_mut(&tab) {
                        rt.navigation.begin_load();
                        rt.pending_load_url = Some(url.clone());
                        rt.dusk_measured_light = None;
                        rt.mixed_content = false;
                        rt.certificate_error = false;
                        rt.connection = infer_connection_state(&url, false, false);
                        if rt.reader_active && !prev_url.same_origin(&url) {
                            rt.reader_active = false;
                            rt.reader_article = None;
                        }
                    }
                    if let Some(model) = self.space_tabs.get_mut(&space)
                        && let Some(t) = model.tab_mut(tab)
                    {
                        t.loading = true;
                        t.url = url.clone();
                        t.letter = letter_for(&url, &t.title);
                    }
                    // Pre-navigation content blocking & before-first-paint CSS/JS refresh.
                    directives.push(self.build_content_rules_directive(
                        Some(tab),
                        &url,
                        entry_private,
                    ));
                    self.push_pre_paint_injections(tab, &url, entry_private, &mut directives);
                }
            }
            EngineEvent::NavigationCommitted { tab, url } => {
                let Some((space_id, entry_snap)) = self.find_tab(tab).map(|(s, t)| (s, t.clone()))
                else {
                    return Ok(directives);
                };
                if let Some(model) = self.space_tabs.get_mut(&space_id)
                    && let Some(t) = model.tab_mut(tab)
                {
                    t.url = url.clone();
                    t.letter = letter_for(&url, &t.title);
                }
                let (skip_push, scroll_to_restore, connection_state) =
                    if let Some(rt) = self.runtimes.get_mut(&tab) {
                        let skip = rt.skip_nav_stack_push_once;
                        rt.skip_nav_stack_push_once = false;
                        rt.pending_load_url = None;
                        rt.last_committed_url = Some(url.clone());
                        rt.last_active_unix_secs = self.now_secs;
                        rt.connection =
                            infer_connection_state(&url, rt.mixed_content, rt.certificate_error);
                        if !skip {
                            let cur_title = if entry_snap.title.is_empty() {
                                url.display()
                            } else {
                                entry_snap.title.clone()
                            };
                            rt.navigation.commit_navigation(url.clone(), cur_title);
                        }
                        (skip, rt.pending_scroll_restore.take(), rt.connection)
                    } else {
                        (false, None, ConnectionState::LocalOrInternal)
                    };
                let _ = (skip_push, connection_state);

                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.crash_state = None;
                }

                // Clear any prompts left over from a prior origin on this tab.
                if self
                    .pending_permission
                    .as_ref()
                    .is_some_and(|p| p.tab == tab)
                {
                    self.pending_permission = None;
                }
                if self
                    .pending_password_offer
                    .as_ref()
                    .is_some_and(|o| o.tab == tab)
                {
                    self.pending_password_offer = None;
                    if self
                        .pending_password_secret
                        .as_ref()
                        .is_some_and(|(t, _)| *t == tab)
                    {
                        self.pending_password_secret = None;
                    }
                }

                // Gate all durable writes through StoreCompositor (`AGENTS.md` §9).
                let isolated_space = self
                    .spaces
                    .space(space_id)
                    .is_some_and(|s| s.store_policy == SpaceStorePolicy::OwnStore);
                let is_personal_profile = self
                    .spaces
                    .space(space_id)
                    .and_then(|s| self.spaces.profiles().iter().find(|p| p.id == s.profile))
                    .is_some_and(|p| p.is_personal);
                let allow_icloud = is_personal_profile && !entry_snap.pinned && !url.is_internal();
                let title = if entry_snap.title.is_empty() {
                    url.display()
                } else {
                    entry_snap.title.clone()
                };
                if isolated_space {
                    self.compositor.on_navigation_committed_with_policy(
                        tab,
                        url.clone(),
                        &title,
                        &self.now_iso,
                        Some(space_id.to_string()),
                        true,
                        false,
                    );
                    if self.compositor.allows_durable_writes(tab) {
                        self.isolated_histories
                            .entry(space_id)
                            .or_default()
                            .record_visit(
                                url.clone(),
                                &title,
                                &self.now_iso,
                                false,
                                Some(space_id.to_string()),
                            );
                    }
                } else {
                    self.compositor.on_navigation_committed_with_policy(
                        tab,
                        url.clone(),
                        &title,
                        &self.now_iso,
                        Some(space_id.to_string()),
                        false,
                        allow_icloud,
                    );
                }

                // Update address bar if this committed tab is the currently focused one.
                if self.active_tab_id() == Some(tab) && !self.address_focused {
                    self.address_input = url.editable_address();
                }

                // Apply Hide Elements, Site CSS, Dusk, zoom, and scroll restoration.
                self.push_pre_paint_injections(tab, &url, entry_snap.private, &mut directives);
                if let Some((sx, sy)) = scroll_to_restore
                    && (sx != 0.0 || sy != 0.0)
                {
                    directives.push(EngineDirective::Inject {
                        tab,
                        world: ScriptWorld::Page,
                        at: InjectAt::DocumentEnd,
                        js: format!("window.scrollTo({sx}, {sy});"),
                    });
                }
            }
            EngineEvent::NavigationFinished { tab, url } => {
                if let Some((space, _)) = self.find_tab(tab)
                    && let Some(model) = self.space_tabs.get_mut(&space)
                    && let Some(t) = model.tab_mut(tab)
                {
                    t.loading = false;
                    t.url = url.clone();
                }
                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.navigation.stop_load();
                }
            }
            EngineEvent::NavigationFailed { tab, url, reason } => {
                if let Some((space, _)) = self.find_tab(tab)
                    && let Some(model) = self.space_tabs.get_mut(&space)
                    && let Some(t) = model.tab_mut(tab)
                {
                    t.loading = false;
                }
                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.navigation.fail_navigation(url, reason);
                }
            }
            EngineEvent::TitleChanged { tab, title } => {
                if let Some((space, entry)) = self.find_tab(tab).map(|(s, t)| (s, t.clone())) {
                    if let Some(model) = self.space_tabs.get_mut(&space)
                        && let Some(t) = model.tab_mut(tab)
                    {
                        t.title = title.clone();
                        t.letter = letter_for(&entry.url, &title);
                    }
                    if self.compositor.allows_durable_writes(tab) {
                        self.record_visit_for_space(space, entry.url, &title);
                    }
                }
            }
            EngineEvent::UrlChanged { tab, url } => {
                if let Some((space, entry)) = self.find_tab(tab).map(|(s, t)| (s, t.clone())) {
                    if let Some(model) = self.space_tabs.get_mut(&space)
                        && let Some(t) = model.tab_mut(tab)
                    {
                        t.url = url.clone();
                        t.letter = letter_for(&url, &t.title);
                    }
                    if let Some(rt) = self.runtimes.get_mut(&tab) {
                        if let Some(pending) = &rt.pending_load_url {
                            if pending.identity() != url.identity() {
                                rt.pending_load_url = Some(url.clone());
                            }
                        } else {
                            let prev = rt.last_committed_url.clone();
                            rt.last_committed_url = Some(url.clone());
                            if let Some(from) = prev
                                && from.identity() != url.identity()
                            {
                                rt.navigation.commit_navigation(url.clone(), &entry.title);
                            }
                        }
                    }
                    if self.active_tab_id() == Some(tab) && !self.address_focused {
                        self.address_input = url.editable_address();
                    }
                }
            }
            EngineEvent::FaviconChanged { tab, url: fav } => {
                if let Some((space, entry)) = self.find_tab(tab).map(|(s, t)| (s, t.clone())) {
                    if let Some(model) = self.space_tabs.get_mut(&space)
                        && let Some(t) = model.tab_mut(tab)
                    {
                        t.favicon = fav.clone();
                    }
                    if let Some(favicon_url) = fav {
                        self.compositor
                            .on_favicon_changed(tab, &entry.url, favicon_url);
                    }
                }
            }
            EngineEvent::LoadingChanged { tab, loading } => {
                if let Some((space, _)) = self.find_tab(tab)
                    && let Some(model) = self.space_tabs.get_mut(&space)
                    && let Some(t) = model.tab_mut(tab)
                {
                    t.loading = loading;
                }
                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    if loading {
                        rt.navigation.begin_load();
                    } else {
                        rt.navigation.stop_load();
                    }
                }
            }
            EngineEvent::HistoryChanged {
                tab,
                can_go_back,
                can_go_forward,
            } => {
                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.engine_can_go_back = can_go_back;
                    rt.engine_can_go_forward = can_go_forward;
                }
            }
            EngineEvent::NewWindowRequested {
                tab,
                url,
                disposition,
            } => {
                let Some((space, opener)) = self.find_tab(tab).map(|(s, t)| (s, t.clone())) else {
                    return Ok(directives);
                };
                let is_private = opener.private;
                if self.runtimes.get(&tab).is_some_and(|rt| rt.agent_tab) {
                    return Ok(directives);
                }
                if disposition == EngineNewWindowDisposition::SmallWindow
                    && self.settings.links_little
                {
                    let small_id = self.tab_allocator.allocate();
                    let _ = self.small_windows.open_small_window(
                        small_id,
                        url.clone(),
                        url.display(),
                        space,
                        crate::peek_small_float::SmallWindowKind::SizedPopup { opener_tab: tab },
                        &WindowFrame::default(),
                    );
                    return Ok(directives);
                }
                let activate = matches!(
                    disposition,
                    EngineNewWindowDisposition::ForegroundTab | EngineNewWindowDisposition::Popup
                );
                let _ = self.cmd_new_tab_in_space(
                    space,
                    Some(url),
                    is_private,
                    activate,
                    Some(tab),
                    &mut directives,
                )?;
            }
            EngineEvent::DownloadRequested {
                tab,
                url,
                method,
                has_request_body,
                suggested_name,
                total_bytes,
            } => {
                let rt = self.runtimes.get(&tab);
                if rt.is_some_and(|r| r.agent_tab) {
                    return Ok(directives);
                }
                let private = self.compositor.is_private_download(tab);
                let name = suggested_name.unwrap_or_else(|| "download".to_owned());
                let dest = Path::new(&self.settings.downloads_dir);
                let _id = self.downloads.start_download(
                    url,
                    method,
                    has_request_body,
                    &name,
                    dest,
                    total_bytes,
                    private,
                );
            }
            EngineEvent::DownloadProgress {
                download_id,
                bytes_received,
            } => {
                let _ = self.downloads.record_progress(&download_id, bytes_received);
            }
            EngineEvent::DownloadFailed {
                download_id,
                reason,
            } => {
                let _ = self.downloads.fail(&download_id, reason);
            }
            EngineEvent::PermissionRequested { tab, origin, kind } => {
                self.handle_permission_request(tab, origin, kind, &mut directives);
            }
            EngineEvent::ScriptDialog { tab, kind, message } => {
                if self.runtimes.get(&tab).is_some_and(|rt| rt.agent_tab) {
                    directives.push(EngineDirective::RespondScriptDialog {
                        tab,
                        accept: false,
                        prompt_text: None,
                    });
                } else {
                    self.pending_dialog = Some(PendingJsDialog { tab, kind, message });
                }
            }
            EngineEvent::FilePickerRequested { tab, multiple } => {
                if !self.runtimes.get(&tab).is_some_and(|rt| rt.agent_tab) {
                    directives.push(EngineDirective::OpenExternalDialog {
                        request: ExternalDialogRequest::FilePicker { tab, multiple },
                    });
                }
            }
            EngineEvent::AuthRequired { tab, origin } => {
                self.pending_auth = Some(PendingAuthChallenge { tab, origin });
            }
            EngineEvent::CertificateError { tab, url, reason } => {
                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.certificate_error = true;
                    rt.connection = ConnectionState::UntrustedCertificate;
                }
                self.pending_cert_error = Some(PendingCertError { tab, url, reason });
            }
            EngineEvent::MixedContentDetected { tab } => {
                if let Some(entry_url) = self.find_tab(tab).map(|(_, e)| e.url.clone())
                    && let Some(rt) = self.runtimes.get_mut(&tab)
                {
                    rt.mixed_content = true;
                    rt.connection = infer_connection_state(&entry_url, true, rt.certificate_error);
                }
            }
            EngineEvent::MediaStateChanged {
                tab,
                audible,
                muted,
            } => {
                if let Some((space, _)) = self.find_tab(tab)
                    && let Some(model) = self.space_tabs.get_mut(&space)
                    && let Some(t) = model.tab_mut(tab)
                {
                    t.audible = audible;
                    t.muted = muted;
                }
                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.media_playing = audible;
                }
            }
            EngineEvent::FullscreenChanged { .. } => {}
            EngineEvent::ProcessCrashed { tab, reason } => {
                self.handle_process_crashed(tab, reason, &mut directives);
            }
            EngineEvent::ScriptMessage {
                tab,
                world: _,
                body,
            } => {
                self.handle_script_message(tab, &body, &mut directives);
            }
            EngineEvent::FindResults {
                tab,
                total_found,
                active_index,
            } => {
                if let Some(rt) = self.runtimes.get_mut(&tab)
                    && let Some(find) = &mut rt.find
                {
                    find.apply_engine_count(total_found, active_index);
                }
            }
            EngineEvent::PageRenderedLuminance { tab, r, g, b } => {
                let Some((_, entry)) = self.find_tab(tab).map(|(s, t)| (s, t.clone())) else {
                    return Ok(directives);
                };
                let is_light = crate::page_tools::is_measured_light_page(r, g, b);
                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.dusk_measured_light = Some(is_light);
                }
                let host = entry.url.host().unwrap_or_default();
                let decision = self.dusk.evaluate_page(
                    host,
                    self.frame_is_dark,
                    (r, g, b),
                    false,
                    &[],
                    &mut self.dusk_budget,
                    entry.private,
                );
                let css_owned: String;
                let css: &str = match decision {
                    DuskDecision::Untouched => "",
                    DuskDecision::DarkTwins { css } => {
                        css_owned = css;
                        &css_owned
                    }
                    DuskDecision::FilterFallback { css } => css,
                };
                directives.push(EngineDirective::Eval {
                    tab,
                    world: ScriptWorld::Isolated,
                    js: DuskController::style_injection_script(css),
                });
            }
            EngineEvent::SnapshotReady { .. } => {}
            EngineEvent::PageArticleExtracted {
                tab,
                html,
                markdown_twin_url,
            } => {
                self.activate_reader_with_html(
                    tab,
                    &html,
                    markdown_twin_url.as_ref(),
                    &mut directives,
                );
            }
        }
        Ok(directives)
    }

    // ------------------------------------------------------------------------
    // Snapshot builder (`UiSnapshot`)
    // ------------------------------------------------------------------------

    /// Build a complete read-only [`UiSnapshot`] for the UI layer (`searlx-ui` / GTK).
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn ui_snapshot(&self) -> UiSnapshot {
        let active_space = self.active_space_id();
        let active_profile = self.active_profile_id();
        let active_tab_opt = self.active_tab_entry().cloned();
        let active_tab_id = active_tab_opt.as_ref().map(|t| t.id);

        let spaces = self
            .spaces
            .spaces_for_profile(active_profile)
            .into_iter()
            .map(|sp| UiSpaceItem {
                id: sp.id,
                profile: sp.profile,
                name: sp.name.clone(),
                color: sp.color,
                icon: sp.icon.clone(),
                gradient: sp.gradient,
                strength: sp.strength,
                store_policy: sp.store_policy,
                active: sp.id == active_space,
                tab_count: self.space_tabs.get(&sp.id).map_or(0, |m| m.tabs().len()),
            })
            .collect();

        let profiles = self
            .spaces
            .profiles()
            .iter()
            .map(|p| UiProfileItem {
                id: p.id,
                name: p.name.clone(),
                is_personal: p.is_personal,
                home_space: p.home_space,
                active: p.id == active_profile,
            })
            .collect();

        let split_frames = active_tab_id
            .map(|aid| self.splits.visible_frames_for(active_space, aid))
            .unwrap_or_default();

        let tabs: Vec<UiTabItem> = self
            .space_tabs
            .get(&active_space)
            .map(|model| {
                let split_here =
                    active_tab_id.and_then(|aid| self.splits.split_containing(active_space, aid));
                model
                    .tabs()
                    .iter()
                    .map(|t| {
                        let rt = self.runtimes.get(&t.id);
                        let lookalike = t
                            .url
                            .host()
                            .is_some_and(|h| inspect_host_lookalike(h).is_lookalike);
                        let in_split = self.splits.split_containing(active_space, t.id).is_some();
                        let split_focused = split_here
                            .is_some_and(|sp| sp.panes.get(sp.focused).copied() == Some(t.id));
                        UiTabItem {
                            id: t.id,
                            engine_tab_id: t.engine_tab_id,
                            url: t.url.clone(),
                            display_url: t.url.display(),
                            editable_address: t.url.editable_address(),
                            title: t.display_title().to_owned(),
                            custom_name: t.custom_name.clone(),
                            letter: t.letter,
                            favicon: t.favicon.clone(),
                            pinned: t.pinned,
                            pin_id: t.pin_id,
                            group: t.group,
                            put_away: t.put_away,
                            loading: t.loading,
                            audible: t.audible,
                            muted: t.muted,
                            sleeping: t.sleeping,
                            sleep_placeholder: t.sleep_placeholder.clone(),
                            private: t.private,
                            agent: rt.is_some_and(|r| r.agent_tab),
                            active: Some(t.id) == active_tab_id,
                            in_split,
                            split_focused,
                            is_lookalike: lookalike,
                            reading_progress: t.reading_progress,
                            can_go_back: rt.is_some_and(|r| r.navigation.can_go_back()),
                            can_go_forward: rt.is_some_and(|r| r.navigation.can_go_forward()),
                            zoom_percent: rt.map_or(self.settings.page_zoom, |r| r.zoom_percent),
                            in_page_error: rt.and_then(|r| r.navigation.in_page_error().cloned()),
                            crash_retry_message: rt.and_then(|r| {
                                r.crash_state.as_ref().map(|c| match c {
                                    TabCrashState::ManualRetry { reason } => reason.clone(),
                                })
                            }),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let split_panes = split_frames
            .into_iter()
            .filter_map(|frame| {
                let model = self.space_tabs.get(&active_space)?;
                let tab = model.tab(frame.tab)?;
                let rt = self.runtimes.get(&tab.id);
                Some(UiSplitPaneItem {
                    tab: frame.tab,
                    engine_tab_id: tab.engine_tab_id,
                    x: frame.x,
                    width: frame.width,
                    focused: frame.focused,
                    url: tab.url.clone(),
                    title: tab.display_title().to_owned(),
                    sleeping: tab.sleeping,
                    in_page_error: rt.and_then(|r| r.navigation.in_page_error().cloned()),
                    crash_retry_message: rt.and_then(|r| {
                        r.crash_state.as_ref().map(|c| match c {
                            TabCrashState::ManualRetry { reason } => reason.clone(),
                        })
                    }),
                })
            })
            .collect();

        let (sidebar_layout, sidebar_folded) = self
            .space_tabs
            .get(&active_space)
            .map_or((self.settings.tab_layout, false), |m| {
                (m.layout(), m.is_folded())
            });

        let address =
            self.build_address_snapshot(active_space, active_profile, active_tab_opt.as_ref());
        let site_card = active_tab_opt
            .as_ref()
            .and_then(|t| self.build_site_card_for_tab(t));
        let find = active_tab_id
            .and_then(|id| self.runtimes.get(&id))
            .filter(|rt| rt.find_bar_open)
            .and_then(|rt| {
                rt.find.as_ref().map(|f| UiFindState {
                    open: true,
                    query: f.query.clone(),
                    status_text: f.status_string(),
                    case_sensitive: f.case_sensitive,
                    whole_words: f.whole_words,
                    whole_words_available: f.whole_words_available(),
                    total_matches: f.matches,
                    active_match: f.active,
                })
            });

        let profile_bookmarks = self.bookmarks.get(&active_profile);
        let bookmarks_bar = profile_bookmarks
            .map(|b| b.bar_items().to_vec())
            .unwrap_or_default();
        let is_active_page_bookmarked = active_tab_opt.as_ref().is_some_and(|t| {
            profile_bookmarks.is_some_and(|b| {
                matches!(
                    b.menu_action_for(&t.url),
                    BookmarkMenuAction::EditBookmark { .. }
                )
            })
        });

        let entries: Vec<DownloadEntry> = self
            .downloads
            .listed_entries()
            .into_iter()
            .cloned()
            .collect();
        let (received_bytes, total_bytes) = self.downloads.global_active_progress();
        let active_count = self.downloads.active_count();
        let downloads = UiDownloadsSummary {
            entries,
            active_count,
            received_bytes,
            total_bytes,
            show_button: self.settings.downloads_button || active_count > 0,
        };

        let held_back_for_private = self
            .side_panel
            .is_held_back_for_active_tab(active_tab_opt.as_ref().is_some_and(|t| t.private));
        let side_panel = UiSidePanelState {
            open: self.side_panel.content().is_some(),
            held_back_for_private,
            width_pt: self.side_panel.width_pt(),
            content: self.side_panel.content().cloned(),
        };

        let active_internal_page = active_tab_opt
            .as_ref()
            .and_then(|t| self.build_internal_page_data(&t.url, active_space, active_profile));
        let active_reader_article = active_tab_id
            .and_then(|id| self.runtimes.get(&id))
            .filter(|rt| rt.reader_active)
            .and_then(|rt| rt.reader_article.clone());

        UiSnapshot {
            window_id: self.active_window_id,
            active_space,
            active_profile,
            spaces,
            profiles,
            tabs,
            pin_folders: self.pins.folders().to_vec(),
            groups: self
                .tab_groups
                .groups_in_space(active_space)
                .into_iter()
                .cloned()
                .collect(),
            active_tab: active_tab_id,
            split_panes,
            sidebar_layout,
            sidebar_folded,
            address,
            site_card,
            find,
            bookmarks_bar,
            is_active_page_bookmarked,
            downloads,
            side_panel,
            floating_video: self.floating_video.active().cloned(),
            peek: self.peek.clone(),
            tab_switcher: self.tab_switcher.clone(),
            launcher: UiLauncherState {
                visible: self.launcher.visible,
                draft: self.launcher.draft.clone(),
            },
            pending_permission: self.pending_permission.clone(),
            pending_dialog: self.pending_dialog.clone(),
            pending_auth: self.pending_auth.clone(),
            pending_cert_error: self.pending_cert_error.clone(),
            pending_password_offer: self.pending_password_offer.clone(),
            active_internal_page,
            active_reader_article,
            status_banner: self.status_banner.clone(),
            capabilities: self.capabilities.clone(),
            settings: self.settings.clone(),
        }
    }

    // ========================================================================
    // Internal command handlers & reconciliation helpers
    // ========================================================================

    fn active_tab_id(&self) -> Option<TabId> {
        self.active_tab_entry().map(|t| t.id)
    }

    fn active_tab_entry(&self) -> Option<&TabEntry> {
        self.space_tabs
            .get(&self.active_space_id())
            .map(TabModel::active_tab)
    }

    fn active_tab_host_and_private(&self) -> Option<(String, bool)> {
        let tab = self.active_tab_entry()?;
        let host = tab.url.host()?.to_owned();
        Some((host, tab.private))
    }

    fn cmd_new_tab(
        &mut self,
        url: Option<PageUrl>,
        private: bool,
        activate: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let space = self.active_space_id();
        let _ = self.cmd_new_tab_in_space(space, url, private, activate, None, directives)?;
        Ok(())
    }

    fn cmd_new_tab_returning_id(
        &mut self,
        url: Option<PageUrl>,
        private: bool,
        activate: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<TabId> {
        let space = self.active_space_id();
        self.cmd_new_tab_in_space(space, url, private, activate, None, directives)
    }

    fn cmd_new_tab_in_space(
        &mut self,
        space: SpaceId,
        url: Option<PageUrl>,
        private: bool,
        activate: bool,
        opener: Option<TabId>,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<TabId> {
        self.cmd_new_tab_in_space_reconcile(space, url, private, activate, opener, true, directives)
    }

    #[allow(clippy::too_many_arguments)]
    fn cmd_new_tab_in_space_reconcile(
        &mut self,
        space: SpaceId,
        url: Option<PageUrl>,
        private: bool,
        activate: bool,
        opener: Option<TabId>,
        reconcile: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<TabId> {
        let title = url
            .as_ref()
            .map_or_else(|| "New Tab".to_owned(), PageUrl::display);
        let lazy_sleep = !activate && self.settings.tabs_lazy;
        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown active space".into()))?;
        let new_id = model.create_tab_with_options(
            &mut self.tab_allocator,
            url.clone(),
            title.clone(),
            private,
            activate,
            opener,
            lazy_sleep,
        )?;
        self.compositor.register_tab(new_id, private, false);

        if let Some(op) = opener
            && !private
            && let Some(gid) = self.tab_groups.inherit_group_from_opener(op, new_id)
            && let Some(t) = model.tab_mut(new_id)
        {
            t.group = Some(gid);
        }

        let rt_existed = self.runtimes.contains_key(&new_id);
        let rt = self
            .runtimes
            .entry(new_id)
            .or_insert_with(|| TabRuntime::new(false, self.settings.page_zoom, self.now_secs));
        if !rt_existed && let Some(target_url) = &url {
            rt.navigation.commit_navigation(target_url.clone(), title);
            rt.connection = infer_connection_state(target_url, false, false);
            if !target_url.is_internal() && !lazy_sleep {
                rt.skip_nav_stack_push_once = true;
            }
        }
        if activate {
            self.mru_tabs.retain(|&id| id != new_id);
            self.mru_tabs.insert(0, new_id);
            if space == self.active_space_id() {
                self.address_input = url
                    .as_ref()
                    .map_or_else(String::new, PageUrl::editable_address);
            }
        }
        if reconcile && space == self.active_space_id() {
            self.reconcile_visible_tabs(directives)?;
        }
        Ok(new_id)
    }

    fn cmd_new_agent_tab(
        &mut self,
        url: PageUrl,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        crate::ai_agents_bench::validate_agent_navigation_url(&url)?;
        let id = self.cmd_new_tab_in_space(
            self.active_space_id(),
            Some(url),
            true,
            true,
            None,
            directives,
        )?;
        self.compositor.register_tab(id, true, true);
        if let Some(rt) = self.runtimes.get_mut(&id) {
            rt.agent_tab = true;
        }
        Ok(())
    }

    fn cmd_close_tab(
        &mut self,
        tab_id: TabId,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((space, tab_snap)) = self.find_tab(tab_id).map(|(s, t)| (s, t.clone())) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let is_put_down = tab_snap.pinned;
        let is_first_group_put_away = tab_snap.group.is_some() && !tab_snap.put_away;

        if let Some(eid) = tab_snap.engine_tab_id {
            directives.push(EngineDirective::CloseTab {
                tab: tab_id,
                engine_tab_id: Some(eid),
            });
            self.visible_engine_tabs.remove(&tab_id);
        }

        if is_put_down || is_first_group_put_away {
            let model = self
                .space_tabs
                .get_mut(&space)
                .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
            model.close_tab(tab_id, &mut self.tab_allocator)?;
            self.splits.remove_tab(space, tab_id)?;
            if space == self.active_space_id() {
                self.reconcile_visible_tabs(directives)?;
            }
            return Ok(());
        }

        self.close_tab_removing_webview(space, tab_id, directives)?;
        if space == self.active_space_id() {
            self.reconcile_visible_tabs(directives)?;
        }
        Ok(())
    }

    fn close_tab_removing_webview(
        &mut self,
        space: SpaceId,
        tab_id: TabId,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        self.close_tab_removing_webview_with_record(space, tab_id, true, directives)
    }

    fn close_tab_removing_webview_with_record(
        &mut self,
        space: SpaceId,
        tab_id: TabId,
        record_closed: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let tab_snap = self
            .space_tabs
            .get(&space)
            .and_then(|m| m.tab(tab_id))
            .cloned()
            .ok_or_else(|| CoreError::Invalid(format!("no such tab: {tab_id}")))?;
        let runtime = self.runtimes.remove(&tab_id);

        if let Some(eid) = tab_snap.engine_tab_id
            && !directives
                .iter()
                .any(|d| matches!(d, EngineDirective::CloseTab { tab, .. } if *tab == tab_id))
        {
            directives.push(EngineDirective::CloseTab {
                tab: tab_id,
                engine_tab_id: Some(eid),
            });
            self.visible_engine_tabs.remove(&tab_id);
        }

        if record_closed && !tab_snap.private && !tab_snap.pinned {
            let scroll = runtime
                .as_ref()
                .and_then(|r| r.navigation.current_entry())
                .map_or((0.0, 0.0), |e| e.scroll);
            self.compositor.record_recently_closed(
                RecentlyClosedItem {
                    url: tab_snap.url.clone(),
                    title: tab_snap.display_title().to_owned(),
                    scroll,
                    group: tab_snap.group.map(|g| g.to_string()),
                },
                false,
            );
            if let Some(rt) = runtime {
                self.closed_tabs.insert(
                    0,
                    ClosedTabSnapshot {
                        space,
                        url: tab_snap.url.clone(),
                        title: tab_snap.display_title().to_owned(),
                        group: tab_snap.group,
                        scroll,
                        navigation: rt.navigation,
                    },
                );
                if self.closed_tabs.len() > 20 {
                    self.closed_tabs.truncate(20);
                }
            }
        }

        if let Some(gid) = tab_snap.group {
            self.tab_groups.remove_tab_from_group(gid, tab_id);
        }

        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        model.close_tab(tab_id, &mut self.tab_allocator)?;
        self.mru_tabs.retain(|&id| id != tab_id);
        self.splits.remove_tab(space, tab_id)?;

        // Register runtimes for any freshly created fallback blank tab.
        for t in model.tabs() {
            if !self.runtimes.contains_key(&t.id) {
                self.runtimes.insert(
                    t.id,
                    TabRuntime::new(false, self.settings.page_zoom, self.now_secs),
                );
            }
        }

        if self.compositor.unregister_tab(tab_id) {
            self.cleanup_on_last_private_tab_closed(directives);
        }
        Ok(())
    }

    fn cleanup_on_last_private_tab_closed(&mut self, directives: &mut Vec<EngineDirective>) {
        directives.push(EngineDirective::ClearData {
            space: None,
            private: true,
            kinds: EngineDataKinds::all(),
        });
        self.hide_elements.clear_private_memory();
        self.dusk.clear_private_memory();
        self.private_blocker_paused_sites.clear();
        self.private_autoplay_allowed_sites.clear();
        self.private_dont_float_sites.clear();
        self.compositor.clear_handoff();
    }

    fn cmd_reopen_closed_tab(&mut self, directives: &mut Vec<EngineDirective>) -> Result<()> {
        if let Some(snap) = self.closed_tabs.first().cloned() {
            self.closed_tabs.remove(0);
            let _ = self.compositor.history.pop_recently_closed();
            let space = if self.space_tabs.contains_key(&snap.space) {
                snap.space
            } else {
                self.active_space_id()
            };
            if space != self.active_space_id() {
                self.cmd_switch_space(space, directives)?;
            }
            let new_id = self.cmd_new_tab_in_space(
                space,
                Some(snap.url.clone()),
                false,
                true,
                None,
                directives,
            )?;
            if let Some(model) = self.space_tabs.get_mut(&space)
                && let Some(entry) = model.tab_mut(new_id)
            {
                entry.title = snap.title;
                entry.group = snap.group;
            }
            if let Some(rt) = self.runtimes.get_mut(&new_id) {
                rt.navigation = snap.navigation;
                rt.pending_scroll_restore = Some(snap.scroll);
                if !snap.url.is_internal() {
                    rt.skip_nav_stack_push_once = true;
                }
            }
            self.reconcile_visible_tabs(directives)?;
        }
        Ok(())
    }

    fn cmd_reopen_closed_group(&mut self, directives: &mut Vec<EngineDirective>) -> Result<()> {
        let Some(restored) = self.tab_groups.reopen_last_closed_group() else {
            return Ok(());
        };
        let snapshots = self
            .closed_group_snapshots
            .remove(&restored.group.id)
            .unwrap_or_default();
        if snapshots.is_empty() {
            let _ = self
                .tab_groups
                .replace_group_tabs(restored.group.id, Vec::new());
            return Ok(());
        }
        let target_space = if self.space_tabs.contains_key(&restored.space) {
            restored.space
        } else {
            self.active_space_id()
        };
        if target_space != restored.space {
            let _ = self
                .tab_groups
                .move_group_to_space(restored.group.id, target_space);
        }
        if target_space != self.active_space_id() {
            self.cmd_switch_space_inner(target_space, true, false, directives)?;
        }

        let mut restored_ids = Vec::with_capacity(snapshots.len());
        for (idx, snap) in snapshots.into_iter().enumerate() {
            let activate = idx == 0;
            let new_id = self.cmd_new_tab_in_space_reconcile(
                target_space,
                Some(snap.url.clone()),
                false,
                activate,
                None,
                false,
                directives,
            )?;
            let mut is_sleeping = false;
            if let Some(model) = self.space_tabs.get_mut(&target_space)
                && let Some(entry) = model.tab_mut(new_id)
            {
                entry.title = snap.title;
                entry.letter = letter_for(&entry.url, &entry.title);
                entry.group = Some(restored.group.id);
                entry.put_away = false;
                is_sleeping = entry.sleeping;
            }
            if let Some(rt) = self.runtimes.get_mut(&new_id) {
                rt.navigation = snap.navigation;
                rt.pending_scroll_restore = Some(snap.scroll);
                if !snap.url.is_internal() && !is_sleeping {
                    rt.skip_nav_stack_push_once = true;
                }
            }
            restored_ids.push(new_id);
        }
        let _ = self
            .tab_groups
            .replace_group_tabs(restored.group.id, restored_ids);
        self.reconcile_visible_tabs(directives)?;
        Ok(())
    }

    fn maybe_start_floating_on_leave_tab(&mut self, outgoing_space: SpaceId, prev_tab: &TabEntry) {
        if !self.settings.float_leave {
            return;
        }
        let Some(prev_rt) = self.runtimes.get(&prev_tab.id) else {
            return;
        };
        if !prev_rt.media_playing {
            return;
        }
        let disabled = if prev_tab.private {
            &self.private_dont_float_sites
        } else {
            &self.dont_float_sites
        };
        let preferred_style = match self.settings.float_style {
            SettingsFloatStyle::Search => CoreFloatStyle::SearchPanel,
            SettingsFloatStyle::System => CoreFloatStyle::SystemPip,
        };
        let _ = self.floating_video.try_start(
            prev_tab.id,
            outgoing_space,
            &prev_tab.url,
            prev_tab.private,
            true,
            prev_rt.video_duration_secs,
            FloatTrigger::LeaveTab,
            preferred_style,
            self.capabilities.native_pip,
            self.settings.float_leave,
            self.settings.float_away,
            disabled,
        );
    }

    fn cmd_activate_tab(
        &mut self,
        tab_id: TabId,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((space, _)) = self.find_tab(tab_id) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let prev_space = self.active_space_id();
        if let Some(prev_tab) = self.active_tab_entry().cloned()
            && prev_tab.id != tab_id
        {
            self.maybe_start_floating_on_leave_tab(prev_space, &prev_tab);
        }
        if space != prev_space {
            self.cmd_switch_space_inner(space, false, false, directives)?;
        }

        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        let was_sleeping = model.tab(tab_id).is_some_and(|t| t.sleeping);
        model.select_tab(tab_id);
        self.splits.focus_tab_if_in_split(space, tab_id);

        self.mru_tabs.retain(|&id| id != tab_id);
        self.mru_tabs.insert(0, tab_id);

        if let Some(rt) = self.runtimes.get_mut(&tab_id) {
            rt.last_active_unix_secs = self.now_secs;
            if was_sleeping {
                rt.pending_scroll_restore = rt.navigation.current_entry().map(|e| e.scroll);
                if model.tab(tab_id).is_some_and(|t| !t.url.is_internal()) {
                    rt.skip_nav_stack_push_once = true;
                }
            }
        }
        if let Some(t) = model.tab(tab_id) {
            self.address_input = t.url.editable_address();
            self.address_focused = false;
            self.address_shake = None;
        }
        if let Some(mut peek) = self.peek.take()
            && peek.on_active_tab_changed(tab_id, self.settings.links_peek_stays)
        {
            self.peek = Some(peek);
        }

        self.reconcile_visible_tabs(directives)?;
        Ok(())
    }

    fn cmd_duplicate_tab(
        &mut self,
        tab_id: TabId,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((space, _)) = self.find_tab(tab_id) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        let new_id = model.duplicate_tab(&mut self.tab_allocator, tab_id, true)?;
        let dup = model
            .tab(new_id)
            .cloned()
            .ok_or_else(|| CoreError::Invalid("duplicated tab missing".into()))?;
        self.compositor.register_tab(new_id, dup.private, false);
        let mut rt = TabRuntime::new(false, self.settings.page_zoom, self.now_secs);
        rt.navigation
            .commit_navigation(dup.url.clone(), dup.title.clone());
        rt.connection = infer_connection_state(&dup.url, false, false);
        if !dup.url.is_internal() {
            rt.skip_nav_stack_push_once = true;
        }
        self.runtimes.insert(new_id, rt);
        self.mru_tabs.retain(|&id| id != new_id);
        self.mru_tabs.insert(0, new_id);
        if space == self.active_space_id() {
            self.reconcile_visible_tabs(directives)?;
        }
        Ok(())
    }

    fn cmd_pin_tab(&mut self, tab_id: TabId) -> Result<()> {
        let Some((space, _)) = self.find_tab(tab_id) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        let pin_id = model.pin_tab(tab_id)?;
        if let Some(entry) = model.tab(tab_id) {
            let mut pin = PinItem::new(pin_id, entry.url.clone(), entry.display_title());
            pin.space = Some(space);
            self.pins.add_pin(pin);
        }
        Ok(())
    }

    fn cmd_unpin_tab(&mut self, tab_id: TabId) -> Result<()> {
        let Some((space, _)) = self.find_tab(tab_id) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        model.unpin_tab(tab_id)
    }

    fn cmd_rehome_pin(
        &mut self,
        tab_id: TabId,
        replace_home: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((space, _)) = self.find_tab(tab_id) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        let target_nav = if let Some(entry) = model.tab_mut(tab_id) {
            if !entry.pinned {
                return Err(CoreError::Invalid("tab is not pinned".into()));
            }
            if replace_home {
                entry.home = Some(entry.url.clone());
                None
            } else {
                entry.home.clone()
            }
        } else {
            None
        };
        if let Some(home_url) = target_nav {
            self.navigate_tab_to(tab_id, home_url, directives)?;
        }
        Ok(())
    }

    fn set_tab_muted(
        &mut self,
        tab_id: TabId,
        muted: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((space, _)) = self.find_tab(tab_id) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        let has_engine = if let Some(t) = model.tab_mut(tab_id) {
            t.muted = muted;
            t.engine_tab_id.is_some()
        } else {
            false
        };
        if has_engine {
            directives.push(EngineDirective::SetMuted { tab: tab_id, muted });
        }
        Ok(())
    }

    fn cmd_sleep_tab(
        &mut self,
        tab_id: TabId,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((space, entry_snap)) = self.find_tab(tab_id).map(|(s, t)| (s, t.clone())) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        model.put_to_sleep(tab_id)?;
        if let Some(eid) = entry_snap.engine_tab_id {
            directives.push(EngineDirective::CloseTab {
                tab: tab_id,
                engine_tab_id: Some(eid),
            });
            self.visible_engine_tabs.remove(&tab_id);
        }
        Ok(())
    }

    fn cmd_create_tab_group(
        &mut self,
        name: String,
        color: SpaceColor,
        tabs: Vec<TabId>,
    ) -> Result<()> {
        let space = self.active_space_id();
        let eligible: Vec<TabId> = tabs
            .into_iter()
            .filter(|id| {
                self.space_tabs
                    .get(&space)
                    .and_then(|m| m.tab(*id))
                    .is_some_and(|t| !t.pinned && !t.private)
            })
            .collect();
        let gid = self
            .tab_groups
            .create_group(space, name, color, eligible.clone())?;
        if let Some(model) = self.space_tabs.get_mut(&space) {
            for tid in eligible {
                if let Some(t) = model.tab_mut(tid) {
                    t.group = Some(gid);
                    t.put_away = false;
                }
            }
        }
        Ok(())
    }

    fn cmd_move_group_to_space(
        &mut self,
        group: GroupId,
        target_space: SpaceId,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        self.tab_groups.move_group_to_space(group, target_space)?;
        let from_space = self.active_space_id();
        if from_space == target_space {
            return Ok(());
        }
        let mut moved_entries = Vec::new();
        if let Some(model) = self.space_tabs.get(&from_space) {
            for t in model.tabs() {
                if t.group == Some(group) {
                    moved_entries.push(t.clone());
                }
            }
        }
        for entry in moved_entries {
            self.close_tab_removing_webview(from_space, entry.id, directives)?;
            let _ = self.cmd_new_tab_in_space(
                target_space,
                Some(entry.url),
                false,
                false,
                None,
                directives,
            )?;
        }
        Ok(())
    }

    fn cmd_commit_drag(&mut self, directives: &mut Vec<EngineDirective>) -> Result<()> {
        let space = self.active_space_id();
        let outcome = self
            .space_tabs
            .get_mut(&space)
            .map(TabModel::commit_drag_with_payload)
            .transpose()?
            .flatten();
        if let Some((DragPayload::Tab(dragged_id), DropTarget::PageSplitEdge { target_tab, side })) =
            outcome
            && dragged_id != target_tab
            && self
                .space_tabs
                .get(&space)
                .is_some_and(|m| m.tab(dragged_id).is_some() && m.tab(target_tab).is_some())
        {
            if let Some(split) = self.splits.split_containing(space, target_tab).cloned() {
                if split.panes.len() < 4 && !split.panes.contains(&dragged_id) {
                    let mut new_panes = split.panes;
                    if let Some(pos) = new_panes.iter().position(|&p| p == target_tab) {
                        let insert_at = match side {
                            crate::tabs::SplitDropSide::Left => pos,
                            crate::tabs::SplitDropSide::Right => pos + 1,
                        };
                        new_panes.insert(insert_at, dragged_id);
                    }
                    self.splits.create_split(space, new_panes)?;
                }
            } else {
                let panes = match side {
                    crate::tabs::SplitDropSide::Left => vec![dragged_id, target_tab],
                    crate::tabs::SplitDropSide::Right => vec![target_tab, dragged_id],
                };
                self.splits.create_split(space, panes)?;
            }
            self.reconcile_visible_tabs(directives)?;
        }
        Ok(())
    }

    fn cmd_submit_address(
        &mut self,
        input: &str,
        disposition: SubmitDisposition,
        shift_pressed: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let opts = ParseOptions {
            engine: self.settings.search_engine.clone(),
            shift_engine: self.settings.search_shift_engine.clone(),
            shift_pressed,
            site_chip: self.site_search_chip.clone(),
            keywords_enabled: self.settings.search_keywords,
            site_searches: self.site_searches.clone(),
            commands_enabled: self.settings.commandbar,
            home_dir: std::env::var_os("HOME").map(std::path::PathBuf::from),
        };
        let resolution = resolve_address(input, &opts);
        match resolution {
            AddressResolution::Navigate(url) => {
                self.address_shake = None;
                self.address_focused = false;
                self.site_search_chip = None;
                self.apply_url_with_disposition(url, disposition, directives)?;
            }
            AddressResolution::Search { source, query, url } => {
                self.address_shake = None;
                self.address_focused = false;
                let was_chip = self.site_search_chip.take();
                let is_private = self.active_tab_entry().is_some_and(|t| t.private);
                if !is_private && let Some(site) = was_chip {
                    self.past_site_searches.insert(
                        0,
                        PastSiteSearchRecord {
                            site_name: source,
                            host: site.host,
                            query,
                            url: url.clone(),
                        },
                    );
                    if self.past_site_searches.len() > 24 {
                        self.past_site_searches.truncate(24);
                    }
                }
                self.apply_url_with_disposition(url, disposition, directives)?;
            }
            AddressResolution::Command(cmd) => {
                self.address_shake = None;
                self.address_focused = false;
                self.site_search_chip = None;
                self.execute_address_command(cmd, directives)?;
            }
            AddressResolution::Invalid(reason) => {
                self.address_shake = Some(reason);
            }
        }
        Ok(())
    }

    fn apply_url_with_disposition(
        &mut self,
        url: PageUrl,
        disposition: SubmitDisposition,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        if url.is_internal() && InternalPageKind::from_url(&url).is_some() {
            self.open_internal_page_url(url, directives)?;
            return Ok(());
        }
        let opener = self.active_tab_id();
        let active_is_private = self.active_tab_entry().is_some_and(|t| t.private);
        match disposition {
            SubmitDisposition::CurrentTab => {
                if let Some(tab_id) = opener {
                    self.navigate_tab_to(tab_id, url, directives)?;
                }
            }
            SubmitDisposition::NewTabBehind => {
                let _ = self.cmd_new_tab_in_space(
                    self.active_space_id(),
                    Some(url),
                    active_is_private,
                    false,
                    opener,
                    directives,
                )?;
            }
            SubmitDisposition::NewTabInFront => {
                let _ = self.cmd_new_tab_in_space(
                    self.active_space_id(),
                    Some(url),
                    active_is_private,
                    true,
                    opener,
                    directives,
                )?;
            }
        }
        Ok(())
    }

    fn open_internal_page_url(
        &mut self,
        url: PageUrl,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let space = self.active_space_id();
        let old_tab_snapshot = self.active_tab_entry().cloned();
        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        let target_id = open_internal_page(model, &mut self.tab_allocator, url.clone())?;
        // Ensure runtime exists (e.g. if a new non-private tab was created over an active private tab).
        self.runtimes
            .entry(target_id)
            .or_insert_with(|| TabRuntime::new(false, self.settings.page_zoom, self.now_secs));
        if let Some(rt) = self.runtimes.get_mut(&target_id) {
            rt.navigation.commit_navigation(
                url.clone(),
                InternalPageKind::from_url(&url).map_or("Search", InternalPageKind::title),
            );
            rt.connection = ConnectionState::LocalOrInternal;
        }
        // If reused tab previously held an engine webview, release that webview cleanly.
        if let Some(old) = old_tab_snapshot
            && old.id == target_id
            && let Some(eid) = old.engine_tab_id
        {
            model.set_engine_tab_id(target_id, None);
            directives.push(EngineDirective::CloseTab {
                tab: target_id,
                engine_tab_id: Some(eid),
            });
            self.visible_engine_tabs.remove(&target_id);
        }
        self.mru_tabs.retain(|&id| id != target_id);
        self.mru_tabs.insert(0, target_id);
        self.address_input = url.editable_address();
        self.reconcile_visible_tabs(directives)?;
        Ok(())
    }

    fn navigate_tab_to(
        &mut self,
        tab_id: TabId,
        url: PageUrl,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        if let Some(rt) = self.runtimes.get(&tab_id)
            && rt.agent_tab
        {
            crate::ai_agents_bench::validate_agent_navigation_url(&url)?;
        }
        let Some((space, entry_snap)) = self.find_tab(tab_id).map(|(s, t)| (s, t.clone())) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        if url.is_internal()
            && InternalPageKind::from_url(&url).is_some()
            && !entry_snap.private
            && space == self.active_space_id()
            && self.active_tab_id() == Some(tab_id)
        {
            return self.open_internal_page_url(url, directives);
        }

        let active_id = self.active_tab_id();
        let model = self
            .space_tabs
            .get_mut(&space)
            .ok_or_else(|| CoreError::Invalid("unknown space".into()))?;
        let is_sleeping = entry_snap.sleeping;
        let has_engine = entry_snap.engine_tab_id.is_some();

        if let Some(entry) = model.tab_mut(tab_id) {
            entry.url = url.clone();
            entry.letter = letter_for(&url, &entry.title);
            entry.loading = !url.is_internal() && !is_sleeping;
        }
        self.crash_recovery.clear_tab(tab_id);
        if let Some(rt) = self.runtimes.get_mut(&tab_id) {
            rt.crash_state = None;
            rt.reader_active = false;
            rt.reader_article = None;
            rt.connection = infer_connection_state(&url, false, false);
            if url.is_internal() {
                rt.navigation.commit_navigation(url.clone(), "Search");
            }
        }
        if active_id == Some(tab_id) {
            self.address_input = url.editable_address();
        }

        if url.is_internal() {
            if let Some(eid) = entry_snap.engine_tab_id {
                model.set_engine_tab_id(tab_id, None);
                directives.push(EngineDirective::CloseTab {
                    tab: tab_id,
                    engine_tab_id: Some(eid),
                });
                self.visible_engine_tabs.remove(&tab_id);
            }
            return Ok(());
        }

        if has_engine {
            directives.push(self.build_content_rules_directive(
                Some(tab_id),
                &url,
                entry_snap.private,
            ));
            self.push_pre_paint_injections(tab_id, &url, entry_snap.private, directives);
            directives.push(EngineDirective::Navigate { tab: tab_id, url });
        } else if !is_sleeping && space == self.active_space_id() {
            self.reconcile_visible_tabs(directives)?;
        }
        Ok(())
    }

    fn cmd_history_step(
        &mut self,
        tab_id: TabId,
        back: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((space, entry)) = self.find_tab(tab_id).map(|(s, t)| (s, t.clone())) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let target = {
            let Some(rt) = self.runtimes.get_mut(&tab_id) else {
                return Ok(());
            };
            let nav_entry = if back {
                rt.navigation.go_back().cloned()
            } else {
                rt.navigation.go_forward().cloned()
            };
            if let Some(e) = &nav_entry {
                rt.pending_scroll_restore = Some(e.scroll);
                rt.skip_nav_stack_push_once = true;
            }
            nav_entry
        };
        let Some(next) = target else {
            return Ok(());
        };
        if let Some(model) = self.space_tabs.get_mut(&space)
            && let Some(t) = model.tab_mut(tab_id)
        {
            t.url = next.url.clone();
            t.title = next.title.clone();
            t.letter = letter_for(&next.url, &next.title);
        }
        if self.active_tab_id() == Some(tab_id) {
            self.address_input = next.url.editable_address();
        }
        if next.url.is_internal() {
            if let Some(eid) = entry.engine_tab_id {
                if let Some(model) = self.space_tabs.get_mut(&space) {
                    model.set_engine_tab_id(tab_id, None);
                }
                directives.push(EngineDirective::CloseTab {
                    tab: tab_id,
                    engine_tab_id: Some(eid),
                });
                self.visible_engine_tabs.remove(&tab_id);
            }
            return Ok(());
        }
        if entry.engine_tab_id.is_some() {
            let can_native = self.runtimes.get(&tab_id).is_some_and(|rt| {
                if back {
                    rt.engine_can_go_back
                } else {
                    rt.engine_can_go_forward
                }
            });
            if can_native {
                if back {
                    directives.push(EngineDirective::Back { tab: tab_id });
                } else {
                    directives.push(EngineDirective::Forward { tab: tab_id });
                }
            } else {
                directives.push(EngineDirective::Navigate {
                    tab: tab_id,
                    url: next.url,
                });
            }
        } else {
            self.reconcile_visible_tabs(directives)?;
        }
        Ok(())
    }

    fn cmd_jump_history_entry(
        &mut self,
        tab_id: TabId,
        index: usize,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((space, entry)) = self.find_tab(tab_id).map(|(s, t)| (s, t.clone())) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        let target = if let Some(rt) = self.runtimes.get_mut(&tab_id) {
            let e = rt.navigation.jump_to(index).cloned();
            if let Some(item) = &e {
                rt.pending_scroll_restore = Some(item.scroll);
                rt.skip_nav_stack_push_once = true;
            }
            e
        } else {
            None
        };
        if let Some(item) = target {
            if let Some(model) = self.space_tabs.get_mut(&space)
                && let Some(t) = model.tab_mut(tab_id)
            {
                t.url = item.url.clone();
                t.title = item.title;
            }
            if item.url.is_internal() {
                if let Some(eid) = entry.engine_tab_id {
                    if let Some(model) = self.space_tabs.get_mut(&space) {
                        model.set_engine_tab_id(tab_id, None);
                    }
                    directives.push(EngineDirective::CloseTab {
                        tab: tab_id,
                        engine_tab_id: Some(eid),
                    });
                }
            } else if entry.engine_tab_id.is_some() {
                directives.push(EngineDirective::Navigate {
                    tab: tab_id,
                    url: item.url,
                });
            } else {
                self.reconcile_visible_tabs(directives)?;
            }
        }
        Ok(())
    }

    fn cmd_reload(
        &mut self,
        tab_id: TabId,
        from_origin: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((space, entry)) = self.find_tab(tab_id).map(|(s, t)| (s, t.clone())) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab_id}")));
        };
        self.crash_recovery.clear_tab(tab_id);
        if let Some(rt) = self.runtimes.get_mut(&tab_id) {
            rt.crash_state = None;
            rt.reader_active = false;
            rt.reader_article = None;
            rt.navigation.begin_load();
            if !entry.url.is_internal() {
                rt.skip_nav_stack_push_once = true;
            }
        }
        if let Some(model) = self.space_tabs.get_mut(&space)
            && let Some(t) = model.tab_mut(tab_id)
        {
            t.loading = !entry.url.is_internal();
        }
        if entry.engine_tab_id.is_some() {
            self.push_pre_paint_injections(tab_id, &entry.url, entry.private, directives);
            directives.push(EngineDirective::Reload {
                tab: tab_id,
                from_origin,
            });
        } else if !entry.url.is_internal() && !entry.sleeping {
            self.reconcile_visible_tabs(directives)?;
        }
        Ok(())
    }

    fn cmd_stop(&mut self, tab_id: TabId, directives: &mut Vec<EngineDirective>) {
        if let Some(rt) = self.runtimes.get_mut(&tab_id) {
            rt.navigation.stop_load();
        }
        if let Some((space, entry)) = self.find_tab(tab_id).map(|(s, t)| (s, t.clone())) {
            if let Some(model) = self.space_tabs.get_mut(&space)
                && let Some(t) = model.tab_mut(tab_id)
            {
                t.loading = false;
            }
            if entry.engine_tab_id.is_some() {
                directives.push(EngineDirective::Stop { tab: tab_id });
            }
        }
    }

    fn cmd_switch_space(
        &mut self,
        target_space: SpaceId,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        self.cmd_switch_space_inner(target_space, true, true, directives)
    }

    fn cmd_switch_space_inner(
        &mut self,
        target_space: SpaceId,
        trigger_leave_float: bool,
        reconcile: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let cur_space = self.active_space_id();
        if cur_space == target_space {
            return Ok(());
        }
        if trigger_leave_float && let Some(prev_tab) = self.active_tab_entry().cloned() {
            self.maybe_start_floating_on_leave_tab(cur_space, &prev_tab);
        }
        // Hide visible webviews in outgoing space without destroying them.
        for tab_id in std::mem::take(&mut self.visible_engine_tabs) {
            if self
                .find_tab(tab_id)
                .is_some_and(|(_, t)| t.engine_tab_id.is_some())
            {
                directives.push(EngineDirective::SetVisible {
                    tab: tab_id,
                    visible: false,
                });
            }
        }

        let next_window = self.spaces.switch_to_space(
            self.active_window_id,
            target_space,
            self.settings.profiles_one_window,
        )?;
        self.active_window_id = next_window;
        let _ = self.side_panel.on_space_switched(target_space);

        if let std::collections::btree_map::Entry::Vacant(e) = self.space_tabs.entry(target_space) {
            let mut model = TabModel::new(&mut self.tab_allocator)?;
            apply_settings_to_tab_model(&self.settings, &mut model);
            let tid = model.active_tab().id;
            self.runtimes.insert(
                tid,
                TabRuntime::new(false, self.settings.page_zoom, self.now_secs),
            );
            e.insert(model);
        }
        if let Some(tab) = self.active_tab_entry() {
            self.address_input = tab.url.editable_address();
        }
        if reconcile {
            self.reconcile_visible_tabs(directives)?;
        }
        Ok(())
    }

    fn cmd_delete_space(
        &mut self,
        space: SpaceId,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let had_own_store = self
            .spaces
            .space(space)
            .is_some_and(|s| s.store_policy == SpaceStorePolicy::OwnStore);
        self.spaces.delete_space(space)?;
        if let Some(removed_model) = self.space_tabs.remove(&space) {
            for t in removed_model.tabs() {
                if let Some(eid) = t.engine_tab_id {
                    directives.push(EngineDirective::CloseTab {
                        tab: t.id,
                        engine_tab_id: Some(eid),
                    });
                    self.visible_engine_tabs.remove(&t.id);
                }
                self.runtimes.remove(&t.id);
                self.mru_tabs.retain(|&id| id != t.id);
                if self.compositor.unregister_tab(t.id) {
                    self.cleanup_on_last_private_tab_closed(directives);
                }
            }
        }
        self.isolated_histories.remove(&space);
        if had_own_store {
            directives.push(EngineDirective::ClearData {
                space: Some(space),
                private: false,
                kinds: EngineDataKinds::all(),
            });
        }
        self.reconcile_visible_tabs(directives)?;
        Ok(())
    }

    fn cmd_toggle_split_view(
        &mut self,
        target_tab: Option<TabId>,
        partner_tab: Option<TabId>,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let space = self.active_space_id();
        let active_id = target_tab
            .or_else(|| self.active_tab_id())
            .ok_or_else(|| CoreError::Invalid("no active tab for split view".into()))?;
        if self.splits.split_containing(space, active_id).is_some() && partner_tab.is_none() {
            self.splits.remove_tab(space, active_id)?;
            self.reconcile_visible_tabs(directives)?;
            return Ok(());
        }

        let partner = if let Some(p) = partner_tab {
            p
        } else {
            let candidate = self.space_tabs.get(&space).and_then(|model| {
                model
                    .tabs()
                    .iter()
                    .find(|t| t.id != active_id && !t.put_away)
                    .map(|t| t.id)
            });
            if let Some(existing) = candidate {
                existing
            } else {
                self.cmd_new_tab_in_space(space, None, false, false, Some(active_id), directives)?
            }
        };
        self.splits.create_split(space, vec![active_id, partner])?;
        if let Some(model) = self.space_tabs.get_mut(&space) {
            model.wake_tab(active_id);
            model.wake_tab(partner);
        }
        self.reconcile_visible_tabs(directives)?;
        Ok(())
    }

    fn emit_split_bounds_directives(&self, directives: &mut Vec<EngineDirective>) {
        let space = self.active_space_id();
        if let Some(active_id) = self.active_tab_id() {
            for frame in self.splits.visible_frames_for(space, active_id) {
                if self
                    .find_tab(frame.tab)
                    .is_some_and(|(_, t)| t.engine_tab_id.is_some())
                {
                    directives.push(EngineDirective::SetBounds {
                        tab: frame.tab,
                        rect: EngineRect {
                            x: frame.x,
                            y: 0.0,
                            width: frame.width,
                            height: 760.0,
                        },
                    });
                }
            }
        }
    }

    fn cmd_switcher_step(&mut self, backwards: bool, elapsed_ms: u64) {
        let space = self.active_space_id();
        if let Some(sw) = &mut self.tab_switcher {
            sw.on_hold_elapsed(elapsed_ms);
            sw.step(if backwards { -1 } else { 1 });
            return;
        }
        let Some(model) = self.space_tabs.get(&space) else {
            return;
        };
        let mut mru_pairs: Vec<(TabId, bool)> = self
            .mru_tabs
            .iter()
            .filter_map(|&id| {
                model
                    .tab(id)
                    .filter(|t| !t.put_away)
                    .map(|t| (id, t.pinned))
            })
            .collect();
        if mru_pairs.is_empty() {
            mru_pairs.push((model.active_tab().id, model.active_tab().pinned));
        }
        let moons: Vec<MoonWindowCandidate> = self
            .small_windows
            .windows()
            .iter()
            .map(|w| MoonWindowCandidate {
                window: WindowId::new(),
                title: w.title.clone(),
                page_screens: w.screens_tall,
            })
            .collect();
        let mut sw = TabSwitcherState::begin(
            &mru_pairs,
            moons,
            self.settings.tabs_switcher_leaves_out_pins,
            backwards,
            (0.0, 0.0),
        );
        sw.on_hold_elapsed(elapsed_ms);
        self.tab_switcher = Some(sw);
    }

    fn cmd_find_in_page(
        &mut self,
        query: &str,
        case_sensitive: bool,
        whole_words: bool,
        text_hint: Option<&str>,
        directives: &mut Vec<EngineDirective>,
    ) {
        let Some(entry) = self.active_tab_entry().cloned() else {
            return;
        };
        let is_pdf = matches!(
            FindTarget::for_url(&entry.url),
            FindTarget::Page { is_pdf: true }
        );
        let eff_whole_words = whole_words && !is_pdf;
        if let Some(rt) = self.runtimes.get_mut(&entry.id) {
            rt.find_bar_open = true;
            let session = rt.find.get_or_insert_with(|| FindSession::new(is_pdf));
            session.case_sensitive = case_sensitive;
            session.whole_words = eff_whole_words;
            if let Some(text) = text_hint {
                session.search_in_text(query, text);
            } else {
                session.query = query.to_owned();
            }
        }
        if entry.engine_tab_id.is_some() {
            if query.is_empty() {
                directives.push(EngineDirective::ClearFind { tab: entry.id });
            } else {
                directives.push(EngineDirective::Find {
                    tab: entry.id,
                    query: query.to_owned(),
                    options: EngineFindOptions {
                        case_sensitive,
                        whole_words: eff_whole_words,
                        backwards: false,
                    },
                });
            }
        }
    }

    fn cmd_step_find(&mut self, backwards: bool, directives: &mut Vec<EngineDirective>) {
        let Some(entry) = self.active_tab_entry().cloned() else {
            return;
        };
        let find_payload = if let Some(rt) = self.runtimes.get_mut(&entry.id)
            && let Some(find) = &mut rt.find
        {
            if backwards {
                find.find_previous();
            } else {
                find.find_next();
            }
            Some((
                find.query.clone(),
                EngineFindOptions {
                    case_sensitive: find.case_sensitive,
                    whole_words: find.effective_whole_words(),
                    backwards,
                },
            ))
        } else {
            None
        };
        if entry.engine_tab_id.is_some()
            && let Some((query, options)) = find_payload
            && !query.is_empty()
        {
            directives.push(EngineDirective::Find {
                tab: entry.id,
                query,
                options,
            });
        }
    }

    fn cmd_zoom_step(
        &mut self,
        delta_steps: Option<i16>,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some(entry) = self.active_tab_entry().cloned() else {
            return Ok(());
        };
        let is_pdf = matches!(
            FindTarget::for_url(&entry.url),
            FindTarget::Page { is_pdf: true }
        );
        let next_zoom = {
            let rt = self
                .runtimes
                .get_mut(&entry.id)
                .ok_or_else(|| CoreError::Invalid("missing tab runtime".into()))?;
            rt.zoom_percent = match delta_steps {
                Some(delta) => step_zoom_percent(rt.zoom_percent, delta, is_pdf),
                None => self
                    .settings
                    .page_zoom
                    .clamp(if is_pdf { 20 } else { 40 }, if is_pdf { 600 } else { 300 }),
            };
            rt.zoom_percent
        };
        if !entry.private
            && let Some(host) = entry.url.host()
        {
            let key = normalize_site_key(host);
            if next_zoom == self.settings.page_zoom {
                self.site_zoom_overrides.remove(&key);
            } else {
                self.site_zoom_overrides.insert(key, next_zoom);
            }
        }
        if entry.engine_tab_id.is_some() {
            directives.push(EngineDirective::SetZoom {
                tab: entry.id,
                zoom: f64::from(next_zoom) / 100.0,
            });
        }
        Ok(())
    }

    fn cmd_toggle_reader_mode(
        &mut self,
        html_override: Option<&str>,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some(entry) = self.active_tab_entry().cloned() else {
            return Ok(());
        };
        let currently_active = self
            .runtimes
            .get(&entry.id)
            .is_some_and(|rt| rt.reader_active);
        if currently_active {
            let reload_url = {
                let rt = self
                    .runtimes
                    .get_mut(&entry.id)
                    .ok_or_else(|| CoreError::Invalid("missing tab runtime".into()))?;
                rt.reader_active = false;
                let reload = rt
                    .reader_article
                    .as_ref()
                    .map(|a| exit_reader(a).reload_url)
                    .unwrap_or_else(|| entry.url.clone());
                rt.reader_article = None;
                rt.skip_nav_stack_push_once = true;
                reload
            };
            if entry.engine_tab_id.is_some() {
                directives.push(EngineDirective::Navigate {
                    tab: entry.id,
                    url: reload_url,
                });
            }
            return Ok(());
        }

        if let Some(raw_html) = html_override {
            let twin_url = detect_markdown_twin_link(raw_html).and_then(|href| {
                entry
                    .url
                    .as_url()
                    .join(&href)
                    .ok()
                    .and_then(|u| PageUrl::from_url(&u).ok())
            });
            self.activate_reader_with_html(entry.id, raw_html, twin_url.as_ref(), directives);
        } else if entry.engine_tab_id.is_some() {
            directives.push(EngineDirective::Eval {
                tab: entry.id,
                world: ScriptWorld::Isolated,
                js: "(function(){var link=document.querySelector('link[rel~=\"alternate\"][type=\"text/markdown\"]');return JSON.stringify({kind:'reader_source',html:document.documentElement.outerHTML,markdownTwin:link?link.href:null});})();".to_owned(),
            });
        }
        Ok(())
    }

    fn activate_reader_with_html(
        &mut self,
        tab_id: TabId,
        raw_html: &str,
        markdown_twin: Option<&PageUrl>,
        directives: &mut Vec<EngineDirective>,
    ) {
        let Some((_, entry)) = self.find_tab(tab_id).map(|(s, t)| (s, t.clone())) else {
            return;
        };
        let is_md = entry.url.as_str().ends_with(".md");
        let article = extract_article(raw_html, &entry.url, self.settings.reader_twins, is_md);
        let theme = self
            .runtimes
            .get(&tab_id)
            .map_or(ReaderTheme::System, |r| r.reader_theme);
        let rendered = article.render_document(theme, self.frame_is_dark);
        let twin_resolved = markdown_twin.cloned().or_else(|| {
            article
                .markdown_twin_url
                .as_deref()
                .and_then(|href| entry.url.as_url().join(href).ok())
                .and_then(|u| PageUrl::from_url(&u).ok())
        });
        if let Some(rt) = self.runtimes.get_mut(&tab_id) {
            rt.reader_active = true;
            rt.available_markdown_twin = twin_resolved;
            rt.reader_article = Some(article);
        }
        if entry.engine_tab_id.is_some() {
            directives.push(EngineDirective::LoadHtml {
                tab: tab_id,
                base_url: Some(entry.url),
                html: rendered,
            });
        }
    }

    fn cmd_toggle_dusk_for_site(&mut self, directives: &mut Vec<EngineDirective>) {
        let Some(entry) = self.active_tab_entry().cloned() else {
            return;
        };
        let Some(host) = entry.url.host() else {
            return;
        };
        let currently_active = self.effective_dusk_active(&entry, self.runtimes.get(&entry.id));
        let next = self.dusk.toggle_site(host, currently_active, entry.private);
        if entry.engine_tab_id.is_some() {
            let css = if next { DUSK_FILTER_FALLBACK_CSS } else { "" };
            directives.push(EngineDirective::Eval {
                tab: entry.id,
                world: ScriptWorld::Isolated,
                js: DuskController::style_injection_script(css),
            });
            directives.push(EngineDirective::ApplyUserStylesheet {
                tab: entry.id,
                style_id: "searlx-user-bundle".into(),
                css: self.build_user_stylesheet_for_tab(&entry, self.runtimes.get(&entry.id)),
            });
        }
    }

    fn reapply_dusk_on_visible_tabs(&self, directives: &mut Vec<EngineDirective>) {
        let space = self.active_space_id();
        let Some(model) = self.space_tabs.get(&space) else {
            return;
        };
        for &tab_id in &self.visible_engine_tabs {
            if let Some(entry) = model.tab(tab_id)
                && entry.engine_tab_id.is_some()
                && !entry.url.is_internal()
            {
                let active = self.effective_dusk_active(entry, self.runtimes.get(&tab_id));
                let css = if active { DUSK_FILTER_FALLBACK_CSS } else { "" };
                directives.push(EngineDirective::Eval {
                    tab: tab_id,
                    world: ScriptWorld::Isolated,
                    js: DuskController::style_injection_script(css),
                });
            }
        }
    }

    fn cmd_toggle_site_blocker(
        &mut self,
        site_host: Option<&str>,
        directives: &mut Vec<EngineDirective>,
    ) {
        let Some(entry) = self.active_tab_entry().cloned() else {
            return;
        };
        let host = site_host
            .map(ToOwned::to_owned)
            .or_else(|| entry.url.host().map(ToOwned::to_owned));
        let Some(host) = host else {
            return;
        };
        let key = normalize_site_key(&host);
        let set = if entry.private {
            &mut self.private_blocker_paused_sites
        } else {
            &mut self.blocker_paused_sites
        };
        if !set.remove(&key) {
            set.insert(key);
        }
        directives.push(self.build_content_rules_directive(
            Some(entry.id),
            &entry.url,
            entry.private,
        ));
    }

    fn cmd_hide_element(&mut self, selector: &str, directives: &mut Vec<EngineDirective>) {
        let Some((host, is_private)) = self.active_tab_host_and_private() else {
            return;
        };
        let banner = self
            .hide_elements
            .hide_selector(&host, selector, is_private);
        self.status_banner = Some(banner.to_owned());
        self.reapply_active_tab_styles_and_scripts(directives);
        if !is_private {
            let _ = self.hide_elements.save_to(&self.store);
        }
    }

    fn cmd_update_site_css(
        &mut self,
        site_host: Option<&str>,
        css: &str,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some(entry) = self.active_tab_entry().cloned() else {
            return Ok(());
        };
        let host = site_host
            .map(ToOwned::to_owned)
            .or_else(|| entry.url.host().map(ToOwned::to_owned))
            .ok_or_else(|| CoreError::Invalid("active tab has no host for Site CSS".into()))?;
        self.site_css.set_site_css(&host, css, entry.private)?;
        self.site_css.save_to(&self.store)?;
        self.reapply_active_tab_styles_and_scripts(directives);
        Ok(())
    }

    fn reapply_active_tab_styles_and_scripts(&self, directives: &mut Vec<EngineDirective>) {
        if let Some(entry) = self.active_tab_entry()
            && entry.engine_tab_id.is_some()
            && !entry.url.is_internal()
        {
            directives.push(EngineDirective::ApplyUserStylesheet {
                tab: entry.id,
                style_id: "searlx-user-bundle".into(),
                css: self.build_user_stylesheet_for_tab(entry, self.runtimes.get(&entry.id)),
            });
            if let Some(host) = entry.url.host()
                && let Some(script) = self.site_css.injection_script(host)
            {
                directives.push(EngineDirective::Eval {
                    tab: entry.id,
                    world: ScriptWorld::Isolated,
                    js: script,
                });
            }
        }
    }

    fn cmd_decide_permission(
        &mut self,
        tab: TabId,
        origin: &str,
        kind: EnginePermissionKind,
        decision: PermissionDecision,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        if self
            .pending_permission
            .as_ref()
            .is_some_and(|p| p.tab == tab)
        {
            self.pending_permission = None;
        }
        let allow = matches!(
            decision,
            PermissionDecision::Allow | PermissionDecision::AllowOnce
        );
        if let Some(core_kind) = kind.to_core_kind()
            && let Ok(url) = PageUrl::parse(origin)
        {
            let _ = self
                .compositor
                .save_permission(tab, &url, core_kind, decision)?;
        }
        if self
            .find_tab(tab)
            .is_some_and(|(_, t)| t.engine_tab_id.is_some())
        {
            directives.push(EngineDirective::RespondPermission {
                tab,
                origin: origin.to_owned(),
                kind,
                allow,
            });
        }
        Ok(())
    }

    fn cmd_save_password_offer(
        &mut self,
        tab: TabId,
        accept: bool,
        never_for_host: bool,
    ) -> Result<()> {
        let Some(offer) = self.pending_password_offer.take() else {
            return Ok(());
        };
        if offer.tab != tab {
            self.pending_password_offer = Some(offer);
            return Ok(());
        }
        let Some((url_owned, host_opt, is_private, is_agent)) = self.find_tab(tab).map(|(_, e)| {
            (
                e.url.clone(),
                e.url.host().map(ToOwned::to_owned),
                e.private,
                self.runtimes.get(&tab).is_some_and(|r| r.agent_tab),
            )
        }) else {
            return Ok(());
        };
        let secret = self
            .pending_password_secret
            .take()
            .filter(|(t, _)| *t == tab)
            .map(|(_, s)| s);
        if is_private || is_agent {
            return Ok(());
        }
        if never_for_host && let Some(host) = host_opt {
            self.vault.add_never_save_host(&host);
            return Ok(());
        }
        if accept && let Some(pw) = secret {
            self.vault.commit_save(
                &url_owned,
                &offer.username,
                &pw,
                &self.now_iso,
                is_private,
                is_agent,
            )?;
        }
        Ok(())
    }

    fn cmd_fill_credential(
        &mut self,
        tab: TabId,
        offered_origin: &str,
        username: &str,
        is_main_frame: bool,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let Some((_, entry)) = self.find_tab(tab).map(|(s, t)| (s, t.clone())) else {
            return Err(CoreError::Invalid(format!("no such tab: {tab}")));
        };
        let committed_url = self
            .runtimes
            .get(&tab)
            .and_then(|rt| rt.last_committed_url.as_ref())
            .unwrap_or(&entry.url);
        let (user, pass) =
            self.vault
                .fill_credential(offered_origin, committed_url, username, is_main_frame)?;
        if entry.engine_tab_id.is_some() {
            let u_json = serde_json::to_string(user).unwrap_or_else(|_| "\"\"".into());
            let p_json = serde_json::to_string(pass).unwrap_or_else(|_| "\"\"".into());
            let js = format!(
                "(function(){{var u={u_json},p={p_json};var pf=document.querySelector('input[type=password]');if(pf){{pf.value=p;}}var uf=document.querySelector('input[type=email],input[type=text],input[autocomplete*=user]');if(uf){{uf.value=u;}}}})();"
            );
            directives.push(EngineDirective::Eval {
                tab,
                world: ScriptWorld::Isolated,
                js,
            });
        }
        Ok(())
    }

    fn cmd_toggle_bookmark(
        &mut self,
        parent_folder_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<()> {
        let Some(entry) = self.active_tab_entry().cloned() else {
            return Ok(());
        };
        let profile = self.active_profile_id();
        let mgr = self.bookmarks.entry(profile).or_default();
        match mgr.menu_action_for(&entry.url) {
            BookmarkMenuAction::AddThisPage => {
                let name = title.unwrap_or_else(|| entry.display_title()).to_owned();
                let _ = mgr.add_bookmark(parent_folder_id, name, entry.url)?;
            }
            BookmarkMenuAction::EditBookmark { id, .. } => {
                let _ = mgr.remove(&id)?;
            }
        }
        Ok(())
    }

    fn cmd_clear_history(&mut self, scope: ClearHistoryScope) -> Result<()> {
        match &scope {
            ClearHistoryScope::All => {
                let _ = self
                    .compositor
                    .history
                    .clear_history(&scope, Some(&self.store))?;
                let own_spaces: Vec<SpaceId> = self.isolated_histories.keys().copied().collect();
                for sid in own_spaces {
                    if let Some(hist) = self.isolated_histories.get_mut(&sid) {
                        let _ = hist.clear_history(&ClearHistoryScope::All, None)?;
                        let suffix = format!("space-{sid}");
                        hist.save_to(&self.store, Some(&suffix))?;
                    }
                }
                self.downloads.strip_source_urls();
                self.compositor.favicons.clear();
                self.compositor.clear_handoff();
                self.past_site_searches.clear();
                self.closed_tabs.clear();
                self.closed_group_snapshots.clear();
            }
            ClearHistoryScope::SignedOutSpace(space_str) => {
                let _ = self.compositor.history.clear_history(&scope, None)?;
                if let Ok(sid) = SpaceId::parse(space_str) {
                    if let Some(hist) = self.isolated_histories.get_mut(&sid) {
                        let _ = hist.clear_history(&ClearHistoryScope::All, None)?;
                        let suffix = format!("space-{sid}");
                        hist.save_to(&self.store, Some(&suffix))?;
                    }
                    self.closed_tabs.retain(|c| c.space != sid);
                }
            }
        }
        Ok(())
    }

    fn cmd_update_settings(
        &mut self,
        next: Settings,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let prev_shield = self.settings.shield;
        let prev_dusk = self.settings.pages_dusk;
        let prev_styles = self.settings.sites_styles;
        self.settings = next;
        for model in self.space_tabs.values_mut() {
            apply_settings_to_tab_model(&self.settings, model);
        }
        self.compositor.permissions.notifications_globally_enabled =
            self.settings.notifications_ask;
        self.site_css
            .set_feature_enabled(self.settings.sites_styles);
        self.dusk.enabled = self.settings.pages_dusk;
        self.translation.target_language = self.settings.translate_target.clone();
        self.settings.save_to(&self.store)?;

        if prev_shield != self.settings.shield
            && let Some(entry) = self.active_tab_entry()
        {
            directives.push(self.build_content_rules_directive(None, &entry.url, entry.private));
        }
        if prev_dusk != self.settings.pages_dusk || prev_styles != self.settings.sites_styles {
            self.reapply_dusk_on_visible_tabs(directives);
            self.reapply_active_tab_styles_and_scripts(directives);
        }
        Ok(())
    }

    fn cmd_start_floating_video(&mut self, trigger: FloatTrigger) {
        let Some(entry) = self.active_tab_entry().cloned() else {
            return;
        };
        let Some(rt) = self.runtimes.get(&entry.id) else {
            return;
        };
        let space = self.active_space_id();
        let disabled = if entry.private {
            &self.private_dont_float_sites
        } else {
            &self.dont_float_sites
        };
        let preferred_style = match self.settings.float_style {
            SettingsFloatStyle::Search => CoreFloatStyle::SearchPanel,
            SettingsFloatStyle::System => CoreFloatStyle::SystemPip,
        };
        let _ = self.floating_video.try_start(
            entry.id,
            space,
            &entry.url,
            entry.private,
            rt.media_playing || trigger == FloatTrigger::ManualShortcut,
            rt.video_duration_secs,
            trigger,
            preferred_style,
            self.capabilities.native_pip,
            self.settings.float_leave,
            self.settings.float_away,
            disabled,
        );
    }

    fn cmd_tick_lifecycle(
        &mut self,
        now_secs: u64,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        let active_space = self.active_space_id();
        let active_id = self.active_tab_id();
        let visible_set = self.compute_visible_set(active_space, active_id);

        let sleep_pace = map_sleep_pace(self.settings.tabs_sleep_pace);
        let auto_close = map_auto_close(self.settings.tabs_close_after);
        let mut candidates = Vec::new();
        for (&space_id, model) in &self.space_tabs {
            for t in model.tabs() {
                if t.sleeping {
                    continue;
                }
                let rt = self.runtimes.get(&t.id);
                let split_gid = self
                    .splits
                    .split_containing(space_id, t.id)
                    .map(|sp| split_hash_u64(&sp.id));
                let perm = self.compositor.evaluate_permission(
                    t.id,
                    &t.url,
                    PermissionKind::Notifications,
                );
                let floating = self
                    .floating_video
                    .active()
                    .is_some_and(|fv| fv.tab_id == t.id);
                candidates.push(TabLifecycleCandidate {
                    tab_id: t.id,
                    space_id,
                    url: t.url.clone(),
                    title: t.display_title().to_owned(),
                    last_active_unix_secs: rt.map_or(now_secs, |r| r.last_active_unix_secs),
                    split_group_id: split_gid,
                    on_screen: visible_set.contains(&t.id),
                    pinned: t.pinned,
                    in_current_space: space_id == active_space,
                    site_can_send_notifications: perm == PermissionDecision::Allow,
                    named: t.custom_name.is_some(),
                    grouped: t.group.is_some(),
                    is_private: t.private,
                    is_blank: t.is_reusable_blank(),
                    is_loading: t.loading,
                    playing_sound: t.audible,
                    on_call_or_recording: false,
                    downloading: false,
                    video_floating: floating,
                    question_waiting: self.pending_dialog.as_ref().is_some_and(|d| d.tab == t.id)
                        || self
                            .pending_permission
                            .as_ref()
                            .is_some_and(|p| p.tab == t.id),
                    holding_typed_text: false,
                    kept_awake_per_site: rt.is_some_and(|r| r.keep_awake),
                });
            }
        }

        let (sleep_tabs, auto_close_tabs) = evaluate_tab_lifecycle(
            &candidates,
            now_secs,
            sleep_pace,
            self.settings.low_power_save,
            crate::gestures_lifecycle::MemoryPressureLevel::Normal,
            auto_close,
            &mut self.compositor.history,
        );
        for tid in sleep_tabs {
            let _ = self.cmd_sleep_tab(tid, directives);
        }
        for tid in auto_close_tabs {
            if let Some((space, t)) = self.find_tab(tid).map(|(s, t)| (s, t.clone()))
                && !t.private
            {
                let _ = self.close_tab_removing_webview(space, tid, directives);
            }
        }
        self.reconcile_visible_tabs(directives)?;
        Ok(())
    }

    fn execute_address_command(
        &mut self,
        cmd: AddressCommand,
        directives: &mut Vec<EngineDirective>,
    ) -> Result<()> {
        match cmd {
            AddressCommand::Settings => {
                self.open_internal_page_url(InternalPageKind::Settings.to_url(None)?, directives)
            }
            AddressCommand::NewTab => self.cmd_new_tab(None, false, true, directives),
            AddressCommand::NewPrivateTab => self.cmd_new_tab(None, true, true, directives),
            AddressCommand::NewSpace => {
                let profile = self.active_profile_id();
                let count = self.spaces.spaces_for_profile(profile).len() + 1;
                let name = format!("Space {count}");
                let space_id = self.spaces.create_space(
                    profile,
                    name,
                    SpaceColor::Violet,
                    "sparkles",
                    SpaceStorePolicy::SharedWithFirst,
                )?;
                let mut model = TabModel::new(&mut self.tab_allocator)?;
                apply_settings_to_tab_model(&self.settings, &mut model);
                let init_id = model.active_tab().id;
                self.runtimes.insert(
                    init_id,
                    TabRuntime::new(false, self.settings.page_zoom, self.now_secs),
                );
                self.space_tabs.insert(space_id, model);
                self.cmd_switch_space(space_id, directives)
            }
            AddressCommand::Bookmarks => {
                self.open_internal_page_url(InternalPageKind::Bookmarks.to_url(None)?, directives)
            }
            AddressCommand::History => {
                self.open_internal_page_url(InternalPageKind::History.to_url(None)?, directives)
            }
            AddressCommand::Downloads => {
                self.open_internal_page_url(InternalPageKind::Downloads.to_url(None)?, directives)
            }
            AddressCommand::Passwords => {
                self.open_internal_page_url(InternalPageKind::Passwords.to_url(None)?, directives)
            }
            AddressCommand::ToggleSidebar => {
                if let Some(model) = self.space_tabs.get_mut(&self.active_space_id()) {
                    model.toggle_fold();
                }
                Ok(())
            }
        }
    }

    fn handle_permission_request(
        &mut self,
        tab: TabId,
        origin: String,
        kind: EnginePermissionKind,
        directives: &mut Vec<EngineDirective>,
    ) {
        if self.runtimes.get(&tab).is_some_and(|rt| rt.agent_tab) {
            directives.push(EngineDirective::RespondPermission {
                tab,
                origin,
                kind,
                allow: false,
            });
            return;
        }
        let Ok(url) = PageUrl::parse(&origin) else {
            directives.push(EngineDirective::RespondPermission {
                tab,
                origin,
                kind,
                allow: false,
            });
            return;
        };
        let decision = match kind.to_core_kind() {
            Some(core_kind) => self.compositor.evaluate_permission(tab, &url, core_kind),
            None => {
                if self.compositor.is_private(tab) {
                    PermissionDecision::Refuse
                } else {
                    PermissionDecision::Ask
                }
            }
        };
        match decision {
            PermissionDecision::Allow | PermissionDecision::AllowOnce => {
                directives.push(EngineDirective::RespondPermission {
                    tab,
                    origin,
                    kind,
                    allow: true,
                });
            }
            PermissionDecision::Refuse => {
                directives.push(EngineDirective::RespondPermission {
                    tab,
                    origin,
                    kind,
                    allow: false,
                });
            }
            PermissionDecision::Ask => {
                self.pending_permission = Some(PendingPermissionPrompt { tab, origin, kind });
            }
        }
    }

    fn handle_process_crashed(
        &mut self,
        tab: TabId,
        reason: String,
        directives: &mut Vec<EngineDirective>,
    ) {
        let Some((space, entry)) = self.find_tab(tab).map(|(s, t)| (s, t.clone())) else {
            return;
        };
        if let Some(model) = self.space_tabs.get_mut(&space)
            && let Some(t) = model.tab_mut(tab)
        {
            t.loading = false;
        }
        let _ = append_crash_log(
            self.store.root(),
            &self.now_iso,
            "web-process",
            Some(&entry.url),
            &reason,
        );
        match self
            .crash_recovery
            .on_process_crashed(tab, self.now_secs, &reason)
        {
            CrashRecoveryAction::AutoReload { .. } => {
                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.crash_state = None;
                    rt.skip_nav_stack_push_once = true;
                }
                if entry.engine_tab_id.is_some() {
                    directives.push(EngineDirective::Reload {
                        tab,
                        from_origin: false,
                    });
                }
            }
            CrashRecoveryAction::ShowManualRetry { message, .. } => {
                if let Some(rt) = self.runtimes.get_mut(&tab) {
                    rt.crash_state = Some(TabCrashState::ManualRetry { reason: message });
                }
            }
        }
    }

    fn handle_script_message(
        &mut self,
        tab: TabId,
        body: &str,
        directives: &mut Vec<EngineDirective>,
    ) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            return;
        };
        let Some(kind) = value.get("kind").and_then(|v| v.as_str()) else {
            return;
        };
        match kind {
            "hide_element_pick" => {
                if let Some(selector) = value.get("selector").and_then(|v| v.as_str())
                    && let Some((_, entry)) = self.find_tab(tab).map(|(s, t)| (s, t.clone()))
                    && let Some(host) = entry.url.host()
                {
                    let banner = self
                        .hide_elements
                        .hide_selector(host, selector, entry.private);
                    self.status_banner = Some(banner.to_owned());
                    if entry.engine_tab_id.is_some() {
                        self.push_pre_paint_injections(tab, &entry.url, entry.private, directives);
                    }
                    if !entry.private {
                        let _ = self.hide_elements.save_to(&self.store);
                    }
                }
            }
            "password_submit" => {
                let Some((_, entry)) = self.find_tab(tab).map(|(s, t)| (s, t.clone())) else {
                    return;
                };
                let username = value
                    .get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let password = value
                    .get("password")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let action = self.vault.evaluate_sign_in_success(
                    &entry.url,
                    username,
                    password,
                    &self.now_iso,
                    entry.private,
                    self.runtimes.get(&tab).is_some_and(|r| r.agent_tab),
                    false,
                );
                if matches!(
                    action,
                    PostSignInAction::OfferSave | PostSignInAction::OfferUpdate
                ) && let Some(origin) = entry.url.origin()
                    && self
                        .compositor
                        .on_password_save_candidate(tab, &origin, username)
                {
                    self.pending_password_secret = Some((tab, password.to_owned()));
                    self.pending_password_offer = Some(PendingPasswordOffer {
                        tab,
                        origin,
                        username: username.to_owned(),
                        is_update: action == PostSignInAction::OfferUpdate,
                    });
                }
            }
            "reader_source" => {
                if let Some(html) = value.get("html").and_then(|v| v.as_str()) {
                    let twin = value
                        .get("markdownTwin")
                        .and_then(|v| v.as_str())
                        .and_then(|s| PageUrl::parse(s).ok());
                    self.activate_reader_with_html(tab, html, twin.as_ref(), directives);
                }
            }
            "find_results" => {
                let total = value
                    .get("total")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                let active = value
                    .get("active")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                if let Some(rt) = self.runtimes.get_mut(&tab)
                    && let Some(find) = &mut rt.find
                {
                    find.apply_engine_count(total, active);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------------
    // Visibility, lazy materialization, and pre-paint rule helpers
    // ------------------------------------------------------------------------

    fn compute_visible_set(&self, active_space: SpaceId, active_id: Option<TabId>) -> Vec<TabId> {
        let Some(aid) = active_id else {
            return Vec::new();
        };
        let split_frames = self.splits.visible_frames_for(active_space, aid);
        if split_frames.is_empty() {
            vec![aid]
        } else {
            split_frames.into_iter().map(|f| f.tab).collect()
        }
    }

    fn reconcile_visible_tabs(&mut self, directives: &mut Vec<EngineDirective>) -> Result<()> {
        let active_space = self.active_space_id();
        let active_id = self.active_tab_id();
        let visible_list = self.compute_visible_set(active_space, active_id);
        let new_visible_set: BTreeSet<TabId> = visible_list.iter().copied().collect();

        // Hide previously visible tabs that are no longer on screen (and still exist with engine view).
        let old_visible: Vec<TabId> = self.visible_engine_tabs.iter().copied().collect();
        for old_tab in old_visible {
            if !new_visible_set.contains(&old_tab)
                && self
                    .find_tab(old_tab)
                    .is_some_and(|(_, t)| t.engine_tab_id.is_some() && !t.sleeping)
            {
                directives.push(EngineDirective::SetVisible {
                    tab: old_tab,
                    visible: false,
                });
            }
        }

        // Wake every tab in `visible_list` (active tab + visible split panes).
        if let Some(model) = self.space_tabs.get_mut(&active_space) {
            for &tab_id in &visible_list {
                model.wake_tab(tab_id);
            }
        }

        // Materialize every non-sleeping, non-internal tab in `active_space` lacking an engine
        // view (`visible_list` + background tabs when `tabs_lazy == false`), setting visibility true
        // for visible panes and false for non-visible background tabs.
        let mut tabs_to_reconcile: Vec<TabId> = visible_list;
        if let Some(model) = self.space_tabs.get(&active_space) {
            for t in model.tabs() {
                if !new_visible_set.contains(&t.id)
                    && !t.sleeping
                    && !t.url.is_internal()
                    && t.engine_tab_id.is_none()
                {
                    tabs_to_reconcile.push(t.id);
                }
            }
        }

        for tab_id in tabs_to_reconcile {
            let Some(entry_snap) = self
                .space_tabs
                .get(&active_space)
                .and_then(|model| model.tab(tab_id).cloned())
            else {
                continue;
            };
            if entry_snap.url.is_internal() {
                continue;
            }
            let is_visible = new_visible_set.contains(&tab_id);
            let zoom_pct = self
                .runtimes
                .get(&tab_id)
                .map_or(self.settings.page_zoom, |r| r.zoom_percent);

            if entry_snap.engine_tab_id.is_none()
                && !pending_engine_create_in_batch(directives, tab_id)
            {
                if let Some(rt_mut) = self.runtimes.get_mut(&tab_id) {
                    rt_mut.engine_can_go_back = false;
                    rt_mut.engine_can_go_forward = false;
                }
                directives.push(EngineDirective::CreateTab {
                    tab: tab_id,
                    options: EngineTabOptions {
                        space: active_space,
                        private: entry_snap.private,
                        url: None,
                        muted: entry_snap.muted,
                        zoom_factor: f64::from(zoom_pct) / 100.0,
                    },
                });
                directives.push(self.build_content_rules_directive(
                    Some(tab_id),
                    &entry_snap.url,
                    entry_snap.private,
                ));
                self.push_pre_paint_injections(
                    tab_id,
                    &entry_snap.url,
                    entry_snap.private,
                    directives,
                );
                directives.push(EngineDirective::SetZoom {
                    tab: tab_id,
                    zoom: f64::from(zoom_pct) / 100.0,
                });
                directives.push(EngineDirective::SetMuted {
                    tab: tab_id,
                    muted: entry_snap.muted,
                });
                directives.push(EngineDirective::Navigate {
                    tab: tab_id,
                    url: entry_snap.url.clone(),
                });
            }
            directives.push(EngineDirective::SetVisible {
                tab: tab_id,
                visible: is_visible,
            });
        }

        self.emit_split_bounds_directives(directives);

        if let Some(aid) = active_id
            && self
                .space_tabs
                .get(&active_space)
                .and_then(|m| m.tab(aid))
                .is_some_and(|t| !t.url.is_internal())
        {
            directives.push(EngineDirective::Focus { tab: aid });
        }

        self.visible_engine_tabs = new_visible_set
            .into_iter()
            .filter(|id| {
                self.space_tabs
                    .get(&active_space)
                    .and_then(|m| m.tab(*id))
                    .is_some_and(|t| !t.url.is_internal())
            })
            .collect();
        Ok(())
    }

    fn build_content_rules_directive(
        &self,
        tab: Option<TabId>,
        url: &PageUrl,
        private_tab: bool,
    ) -> EngineDirective {
        let mut paused: Vec<String> = self.blocker_paused_sites.iter().cloned().collect();
        if private_tab {
            for p in &self.private_blocker_paused_sites {
                if !paused.contains(p) {
                    paused.push(p.clone());
                }
            }
        }
        let host_paused = url.host().is_some_and(|h| {
            let key = normalize_site_key(h);
            paused.iter().any(|p| p == &key)
        });
        let shield_effective = self.settings.shield && !host_paused;
        let rules_json = compile_webkit_content_rules_with_shield(self.settings.shield, &paused);
        EngineDirective::ApplyContentRules {
            tab,
            rules_json,
            shield_enabled: shield_effective,
        }
    }

    fn push_pre_paint_injections(
        &self,
        tab_id: TabId,
        url: &PageUrl,
        private_tab: bool,
        directives: &mut Vec<EngineDirective>,
    ) {
        if url.is_internal() {
            return;
        }
        let Some(entry_ref) = self.find_tab(tab_id).map(|(_, t)| t) else {
            return;
        };
        let rt = self.runtimes.get(&tab_id);
        let bundle_css = self.build_user_stylesheet_for_tab(entry_ref, rt);
        directives.push(EngineDirective::ApplyUserStylesheet {
            tab: tab_id,
            style_id: "searlx-user-bundle".into(),
            css: bundle_css,
        });

        if let Some(host) = url.host() {
            if let Some(hidden_css) = self.hide_elements.before_first_paint_css(host, private_tab) {
                let encoded = serde_json::to_string(&hidden_css).unwrap_or_else(|_| "\"\"".into());
                let js = format!(
                    "(function(){{var id='searlx-hide-style';var el=document.getElementById(id);if(!el){{el=document.createElement('style');el.id=id;(document.head||document.documentElement).appendChild(el);}}el.textContent={encoded};}})();"
                );
                directives.push(EngineDirective::Inject {
                    tab: tab_id,
                    world: ScriptWorld::Isolated,
                    at: InjectAt::DocumentStart,
                    js,
                });
            }
            if let Some(site_script) = self.site_css.injection_script(host) {
                directives.push(EngineDirective::Inject {
                    tab: tab_id,
                    world: ScriptWorld::Isolated,
                    at: InjectAt::DocumentStart,
                    js: site_script,
                });
            }
            let dusk_on = self.effective_dusk_active(entry_ref, rt);
            if dusk_on {
                directives.push(EngineDirective::Inject {
                    tab: tab_id,
                    world: ScriptWorld::Isolated,
                    at: InjectAt::DocumentStart,
                    js: DuskController::style_injection_script(DUSK_FILTER_FALLBACK_CSS),
                });
            }
            let mut paused_sites: Vec<String> = self.blocker_paused_sites.iter().cloned().collect();
            if private_tab {
                for p in &self.private_blocker_paused_sites {
                    paused_sites.push(p.clone());
                }
            }
            if let Some((page_js, isolated_js)) =
                youtube_ad_scripts(&entry_ref.url, self.settings.shield, &paused_sites)
            {
                directives.push(EngineDirective::Inject {
                    tab: tab_id,
                    world: ScriptWorld::Page,
                    at: InjectAt::DocumentStart,
                    js: page_js.to_owned(),
                });
                directives.push(EngineDirective::Inject {
                    tab: tab_id,
                    world: ScriptWorld::Isolated,
                    at: InjectAt::DocumentStart,
                    js: isolated_js.to_owned(),
                });
            }
        }
    }

    fn effective_dusk_active(&self, entry: &TabEntry, rt: Option<&TabRuntime>) -> bool {
        if !self.dusk.enabled
            || self.dusk.paused_everywhere
            || !self.frame_is_dark
            || entry.url.is_internal()
        {
            return false;
        }
        let Some(host) = entry.url.host() else {
            return false;
        };
        if let Some(explicit) = self.dusk.is_site_explicit(host, entry.private) {
            return explicit;
        }
        rt.and_then(|r| r.dusk_measured_light).unwrap_or(false)
    }

    fn build_user_stylesheet_for_tab(&self, entry: &TabEntry, rt: Option<&TabRuntime>) -> String {
        let mut css_parts = Vec::new();
        if let Some(host) = entry.url.host() {
            if let Some(hidden) = self
                .hide_elements
                .before_first_paint_css(host, entry.private)
            {
                css_parts.push(hidden);
            }
            if let Some(site_css) = self.site_css.active_css_for_site(host) {
                css_parts.push(site_css.to_owned());
            }
            if self.effective_dusk_active(entry, rt) {
                css_parts.push(DUSK_FILTER_FALLBACK_CSS.to_owned());
            }
        }
        css_parts.join("\n")
    }

    fn build_address_snapshot(
        &self,
        active_space: SpaceId,
        active_profile: ProfileId,
        active_tab: Option<&TabEntry>,
    ) -> UiAddressBarState {
        let open_tabs: Vec<OpenTabCandidate> = self
            .space_tabs
            .get(&active_space)
            .map(|model| {
                model
                    .tabs()
                    .iter()
                    .filter(|t| !t.put_away)
                    .map(|t| OpenTabCandidate {
                        id: t.id,
                        title: t.display_title().to_owned(),
                        url: t.url.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let history = self.history_for_space(active_space).visits();
        let empty_tree = BookmarkTree::new();
        let bookmarks_tree = self
            .bookmarks
            .get(&active_profile)
            .map_or(&empty_tree, BookmarksManager::tree);
        let ctx = SuggestionContext {
            current_tab: active_tab.map(|t| t.id),
            open_tabs: &open_tabs,
            history,
            bookmarks: bookmarks_tree,
            site_searches: &self.site_searches,
            engine: &self.settings.search_engine,
            commands_enabled: self.settings.commandbar,
        };
        let is_private = active_tab.is_some_and(|t| t.private);
        let inline = if self.address_focused {
            inline_completion(&self.address_input, &ctx)
        } else {
            None
        };
        let suggestions = if self.address_focused && !self.address_input.trim().is_empty() {
            build_suggestions_full(
                &self.address_input,
                &ctx,
                &SuggestionExtras {
                    private_tab: is_private,
                    search_suggest_enabled: self.settings.search_suggest,
                    engine_suggestions: &self.engine_suggestions,
                    past_site_searches: &self.past_site_searches,
                    icloud_tabs: &self.icloud_tabs,
                },
            )
        } else {
            Vec::new()
        };

        let lookalike = active_tab
            .and_then(|t| t.url.host())
            .map(inspect_host_lookalike);
        let (is_lookalike, safe_host) = lookalike.map_or_else(
            || (false, String::new()),
            |info| (info.is_lookalike, info.safe_display_host),
        );

        UiAddressBarState {
            text: self.address_input.clone(),
            current_url_display: active_tab.map_or_else(String::new, |t| t.url.display()),
            editable_address: active_tab.map_or_else(String::new, |t| t.url.editable_address()),
            inline_completion: inline,
            focused: self.address_focused,
            site_search_chip: self.site_search_chip.clone(),
            shake_reason: self.address_shake.clone(),
            is_lookalike,
            safe_host,
            suggestions,
        }
    }

    fn build_site_card_for_tab(&self, tab: &TabEntry) -> Option<SiteCardModel> {
        if matches!(
            tab.url.as_str(),
            "search://" | "search://newtab" | "about:blank"
        ) {
            return None;
        }
        let rt = self.runtimes.get(&tab.id);
        let host_key = tab.url.host().map(normalize_site_key);
        let site_blocker_on = host_key.as_ref().is_none_or(|k| {
            !self.blocker_paused_sites.contains(k)
                && !(tab.private && self.private_blocker_paused_sites.contains(k))
        });
        let site_autoplay_sound = host_key.as_ref().is_some_and(|k| {
            self.autoplay_allowed_sites.contains(k)
                || (tab.private && self.private_autoplay_allowed_sites.contains(k))
        });
        let site_dont_float_videos = host_key.as_ref().is_some_and(|k| {
            self.dont_float_sites.iter().any(|h| h == k)
                || (tab.private && self.private_dont_float_sites.iter().any(|h| h == k))
        });
        let site_dusk_on = self.effective_dusk_active(tab, rt);
        let copy_sc = self
            .commands
            .shortcut_for(&CommandId::CopyAddress)
            .map_or_else(|| "⇧⌘C".to_owned(), |c| c.to_spec());
        let darken_sc = self
            .commands
            .shortcut_for(&CommandId::DarkenThisSite)
            .map_or_else(|| "⇧⌘D".to_owned(), |c| c.to_spec());

        Some(SiteCardModel::build_with_shortcuts(
            &SiteCardInput {
                url: &tab.url,
                connection: rt.map_or(ConnectionState::LocalOrInternal, |r| r.connection),
                private_tab: tab.private,
                zoom_percent: rt.map_or(self.settings.page_zoom, |r| r.zoom_percent),
                engine_supports_print: self.capabilities.print,
                engine_supports_floating_video: self.capabilities.floating_video,
                shield_in_trouble: !self.capabilities.content_blockers && !self.settings.shield,
                site_blocker_on,
                dusk_paused: self.dusk.paused_everywhere,
                frame_is_dark: self.frame_is_dark,
                site_dusk_on,
                site_autoplay_sound,
                site_dont_float_videos,
                site_keep_awake: rt.is_some_and(|r| r.keep_awake),
                settings: &self.settings,
                permissions: &self.compositor.permissions,
            },
            &SiteCardShortcuts {
                copy_address: copy_sc,
                darken_site: darken_sc,
            },
        ))
    }

    fn build_internal_page_data(
        &self,
        url: &PageUrl,
        active_space: SpaceId,
        active_profile: ProfileId,
    ) -> Option<InternalPageData> {
        let kind = InternalPageKind::from_url(url)?;
        let display = url.display();
        let detail = display
            .strip_prefix("search://")
            .and_then(|rest| rest.split_once('/').map(|(_, sub)| sub))
            .filter(|sub| !sub.is_empty());

        Some(match kind {
            InternalPageKind::Settings => {
                let section = detail
                    .and_then(SettingsPage::from_slug)
                    .unwrap_or(SettingsPage::General);
                InternalPageData::Settings {
                    section,
                    visible_sections: SettingsPage::visible_pages(self.settings.ai_shipped),
                    dusk_sites: self.dusk.durable_site_preferences().clone(),
                    saved_permissions_count: self.compositor.permissions.len(),
                    engine_name: self.capabilities.engine.clone(),
                }
            }
            InternalPageKind::History => {
                let hist = self.history_for_space(active_space);
                let today = self.now_iso.get(..10).unwrap_or("2026-11-24");
                InternalPageData::History {
                    days: hist.grouped_by_day(today, ""),
                    recently_closed: hist.recently_closed().to_vec(),
                }
            }
            InternalPageKind::Downloads => InternalPageData::Downloads {
                entries: self
                    .downloads
                    .listed_entries()
                    .into_iter()
                    .cloned()
                    .collect(),
            },
            InternalPageKind::Bookmarks => InternalPageData::Bookmarks {
                tree: self
                    .bookmarks
                    .get(&active_profile)
                    .map_or_else(BookmarkTree::new, |b| b.tree().clone()),
            },
            InternalPageKind::Passwords => InternalPageData::Passwords {
                accounts: self.vault.masked_entries(),
            },
            InternalPageKind::Archive => InternalPageData::Archive {
                archived_tabs: self.compositor.history.archive().to_vec(),
            },
            InternalPageKind::Import => InternalPageData::Import {
                source: detail.map(ToOwned::to_owned),
            },
            InternalPageKind::ReleaseNotes => InternalPageData::ReleaseNotes {
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        })
    }

    fn persist_all(&mut self) -> Result<()> {
        let active_space = self.active_space_id();
        let Some(model) = self.space_tabs.get(&active_space) else {
            return Ok(());
        };
        let mut restored_tabs = Vec::new();
        let mut active_idx = 0_usize;
        for (i, t) in model.tabs().iter().enumerate() {
            if !self.compositor.allows_session_persistence(t.id) {
                continue;
            }
            if i == model.active_index() {
                active_idx = restored_tabs.len();
            }
            let scroll = self
                .runtimes
                .get(&t.id)
                .and_then(|rt| rt.navigation.current_entry())
                .map(|e| e.scroll);
            restored_tabs.push(RestoredTab {
                id: t.id,
                key: format!("tab-{}", t.id.get()),
                url: t.url.clone(),
                title: t.display_title().to_owned(),
                pinned: t.pinned,
                muted: t.muted,
                sleeping: t.sleeping,
                should_load_now: !t.sleeping,
                private: false,
                group: t.group.map(|g| g.to_string()),
                space: Some(active_space.to_string()),
                profile: Some(self.active_profile_id().to_string()),
                scroll,
            });
        }

        let groups: Vec<TabGroup> = self
            .tab_groups
            .groups_in_space(active_space)
            .into_iter()
            .map(|g| TabGroup {
                id: g.id.to_string(),
                name: g.name.clone(),
                color: g.color.as_str().to_owned(),
                collapsed: g.collapsed,
                tab_ids: g
                    .tabs
                    .iter()
                    .map(|tid| format!("tab-{}", tid.get()))
                    .collect(),
                extra: serde_json::Map::new(),
            })
            .collect();

        let splits: Vec<Split> = self
            .splits
            .splits_for_space(active_space)
            .iter()
            .filter_map(|s| {
                let first = s.panes.first()?;
                let second = s.panes.get(1)?;
                let mut extra = serde_json::Map::new();
                if s.panes.len() > 2 {
                    let arr = s
                        .panes
                        .iter()
                        .map(|tid| serde_json::Value::String(format!("tab-{}", tid.get())))
                        .collect();
                    extra.insert("panes".into(), serde_json::Value::Array(arr));
                }
                Some(Split {
                    id: s.id.clone(),
                    axis: s.axis.as_str().to_owned(),
                    ratio: s.shares.first().copied().unwrap_or(0.5),
                    first: format!("tab-{}", first.get()),
                    second: format!("tab-{}", second.get()),
                    extra,
                })
            })
            .collect();

        let window_rec = RestoredWindow {
            id: self.active_window_id,
            frame: WindowFrame::default(),
            space: Some(active_space.to_string()),
            active: active_idx,
            tabs: restored_tabs,
            groups,
            splits,
            folded: model.is_folded(),
            merged: false,
            tucked: false,
        };
        save_window_session(&self.store, None, &window_rec)?;

        // Sync dusk.sites into Settings before saving.
        let dusk_map: serde_json::Map<String, serde_json::Value> = self
            .dusk
            .durable_site_preferences()
            .iter()
            .map(|(k, &v)| (k.clone(), serde_json::Value::Bool(v)))
            .collect();
        if dusk_map.is_empty() {
            self.settings.extra.remove("dusk.sites");
        } else {
            self.settings
                .extra
                .insert("dusk.sites".into(), serde_json::Value::Object(dusk_map));
        }

        self.settings.save_to(&self.store)?;
        self.compositor.history.save_to(&self.store, None)?;
        let own_store_spaces: Vec<SpaceId> = self
            .spaces
            .spaces()
            .iter()
            .filter(|s| s.uses_own_history_file())
            .map(|s| s.id)
            .collect();
        for sid in own_store_spaces {
            if let Some(hist) = self.isolated_histories.get_mut(&sid) {
                let suffix = format!("space-{sid}");
                hist.save_to(&self.store, Some(&suffix))?;
            }
        }
        for (&pid, bmgr) in &self.bookmarks {
            let suffix = if self
                .spaces
                .profiles()
                .iter()
                .any(|p| p.id == pid && p.is_personal)
            {
                None
            } else {
                Some(pid.to_string())
            };
            bmgr.save_to(&self.store, suffix.as_deref())?;
        }
        self.downloads.save_to(&self.store)?;
        self.hide_elements.save_to(&self.store)?;
        self.site_css.save_to(&self.store)?;
        let _ = (
            SessionFile::default(),
            ScrollPosition::default(),
            ArchivedTab::default(),
        );
        Ok(())
    }
}

// ============================================================================
// Pure utility helpers
// ============================================================================

fn pending_engine_create_in_batch(directives: &[EngineDirective], tab_id: TabId) -> bool {
    let mut created = false;
    for d in directives {
        match d {
            EngineDirective::CreateTab { tab, .. } if *tab == tab_id => created = true,
            EngineDirective::CloseTab { tab, .. } if *tab == tab_id => created = false,
            _ => {}
        }
    }
    created
}

fn apply_settings_to_tab_model(settings: &Settings, model: &mut TabModel) {
    model.set_layout(settings.tab_layout);
    model.set_fold_to_icons(settings.sidebar_fold_icons);
    model.set_new_tab_place(settings.tabs_new_place);
    model.set_close_goes_back(settings.tabs_close_goes_back);
    model.set_pins_go_home(settings.pins_home);
    model.set_new_after_last(settings.tabs_new_after_last);
    model.set_tabs_lazy(settings.tabs_lazy);
}

fn infer_connection_state(url: &PageUrl, mixed: bool, cert_err: bool) -> ConnectionState {
    if cert_err {
        return ConnectionState::UntrustedCertificate;
    }
    match url.scheme() {
        "https" if mixed => ConnectionState::MixedContent,
        "https" => ConnectionState::Secure,
        "http" => ConnectionState::NotSecure,
        _ => ConnectionState::LocalOrInternal,
    }
}

fn letter_for(url: &PageUrl, title: &str) -> char {
    url.host()
        .and_then(|h| h.trim_start_matches("www.").chars().next())
        .or_else(|| title.trim().chars().next())
        .map_or('·', |c| c.to_ascii_uppercase())
}

fn split_tab_keys_from_record(split: &Split) -> Vec<String> {
    if let Some(serde_json::Value::Array(arr)) = split.extra.get("panes") {
        let keys: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
            .filter(|k| !k.is_empty())
            .collect();
        if keys.len() >= 2 {
            return keys;
        }
    }
    let mut keys = Vec::new();
    if !split.first.is_empty() {
        keys.push(split.first.clone());
    }
    if !split.second.is_empty() && !keys.contains(&split.second) {
        keys.push(split.second.clone());
    }
    keys
}

fn split_hash_u64(id: &str) -> u64 {
    let mut acc: u64 = 1_469_598_103_934_665_603;
    for b in id.as_bytes() {
        acc ^= u64::from(*b);
        acc = acc.wrapping_mul(1_099_511_628_211);
    }
    acc
}

fn map_sleep_pace(pace: crate::settings::SleepPace) -> crate::gestures_lifecycle::SleepPace {
    match pace {
        crate::settings::SleepPace::Sooner => crate::gestures_lifecycle::SleepPace::Sooner10Min,
        crate::settings::SleepPace::Normal => crate::gestures_lifecycle::SleepPace::Normal30Min,
        crate::settings::SleepPace::Later => crate::gestures_lifecycle::SleepPace::Later120Min,
        crate::settings::SleepPace::Never => crate::gestures_lifecycle::SleepPace::Never,
    }
}

fn map_auto_close(
    after: crate::settings::AutoCloseAfter,
) -> crate::gestures_lifecycle::AutoCloseAfter {
    match after {
        crate::settings::AutoCloseAfter::Hours12 => {
            crate::gestures_lifecycle::AutoCloseAfter::Hours12
        }
        crate::settings::AutoCloseAfter::Day1 => crate::gestures_lifecycle::AutoCloseAfter::Day1,
        crate::settings::AutoCloseAfter::Week1 => crate::gestures_lifecycle::AutoCloseAfter::Week1,
        crate::settings::AutoCloseAfter::Month1 => {
            crate::gestures_lifecycle::AutoCloseAfter::Month1
        }
        crate::settings::AutoCloseAfter::Never => crate::gestures_lifecycle::AutoCloseAfter::Never,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::SessionTab;
    use crate::domain::CURRENT_SCHEMA_VERSION;

    #[test]
    fn lazy_tab_materialization_closes_engine_view_on_sleep_and_restores_asleep_session() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());
        let session = SessionFile {
            version: CURRENT_SCHEMA_VERSION,
            active: 2,
            tabs: (0..5)
                .map(|i| SessionTab {
                    id: format!("t-{i}"),
                    url: format!("https://example.com/{i}"),
                    title: format!("Page {i}"),
                    ..SessionTab::default()
                })
                .collect(),
            ..SessionFile::default()
        };
        store.save_session(None, &session).expect("save session");

        let (mut ctrl, init_directives) = BrowserController::open(
            temp.path(),
            EngineCapabilities::baseline("Test"),
            RestoreMode::Normal,
        )
        .expect("open controller");

        let created: Vec<TabId> = init_directives
            .iter()
            .filter_map(|d| match d {
                EngineDirective::CreateTab { tab, .. } => Some(*tab),
                _ => None,
            })
            .collect();
        assert_eq!(
            created.len(),
            1,
            "only active tab should materialize at restore"
        );
        let active_id = ctrl.active_tab_model().expect("model").active_tab().id;
        assert_eq!(created[0], active_id);
        ctrl.bind_engine_tab(active_id, TabId::new(1000));

        let sleeping_tab_id = ctrl.active_tab_model().expect("model").tabs()[0].id;
        assert!(ctrl.active_tab_model().expect("model").tabs()[0].sleeping);
        assert_eq!(
            ctrl.active_tab_model().expect("model").tabs()[0].engine_tab_id,
            None
        );

        // Waking sleeping_tab_id emits CreateTab + Navigate and hides active_id.
        let wake_dirs = ctrl
            .dispatch(AppCommand::ActivateTab {
                tab: sleeping_tab_id,
            })
            .expect("activate sleeping tab");
        assert!(wake_dirs.iter().any(|d| matches!(
            d,
            EngineDirective::CreateTab { tab, .. } if *tab == sleeping_tab_id
        )));
        assert!(wake_dirs.iter().any(|d| matches!(
            d,
            EngineDirective::SetVisible { tab, visible: false } if *tab == active_id
        )));
        ctrl.bind_engine_tab(sleeping_tab_id, TabId::new(1001));

        // Putting background active_id to sleep emits CloseTab and clears its engine_tab_id.
        let sleep_dirs = ctrl
            .dispatch(AppCommand::SleepTab { tab: active_id })
            .expect("sleep active_id");
        assert!(sleep_dirs.iter().any(|d| matches!(
            d,
            EngineDirective::CloseTab { tab, engine_tab_id: Some(eid) } if *tab == active_id && *eid == TabId::new(1000)
        )));
        let slept = ctrl.find_tab(active_id).expect("found").1;
        assert!(slept.sleeping);
        assert_eq!(slept.engine_tab_id, None);
        assert_eq!(slept.url.as_str(), "https://example.com/2");
    }
}
