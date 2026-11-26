//! Controller-backed gpui browser shell (`AGENTS.md` §3.1, §3.3; `FEATURES.md` §1–§5, §22).
//!
//! `Shell` owns a `searlx_core::BrowserController` through [`ShellModel`],
//! rendering `UiSnapshot` views and dispatching `AppCommand` actions without
//! touching any concrete web engine.

use crate::UiError;
use crate::a11y::{AccessibilityTree, FocusController, FocusTarget};
use crate::field::{AddressField, FieldEvent, key_bindings};
use crate::metrics;
use crate::theme::{Palette, SpaceTint, Theme, focus_ring, tab_glyph_letter};
use gpui::{
    App, Application, Bounds, ClipboardItem, Context, FocusHandle, Focusable, KeyBinding,
    MouseButton, Render, SharedString, Task, TitlebarOptions, Window, WindowAppearance,
    WindowBounds, WindowDecorations, WindowOptions, actions, div, point, prelude::*, px, relative,
    size,
};
use searlx_core::{
    AddressCommand, AddressInvalidReason, AppCommand, BookmarkItem, BrowserController,
    ClearHistoryScope, ConnectionState, EngineCapabilities, EngineDirective, InternalPageData,
    InternalPageKind, LookPreference, PageUrl, RestoreMode, Settings, SettingsPage, SpaceColor,
    SubmitDisposition, SuggestionRow, TabGroupItem, TabId, TabLayoutMode, UiAddressBarState,
    UiFindState, UiSnapshot, UiSplitPaneItem, UiTabItem, controller::EngineEvent,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static TEMP_DIR_SEQ: AtomicU64 = AtomicU64::new(1);

actions!(
    searlx,
    [
        NewTab,
        NewPrivateTab,
        CloseTab,
        ReopenClosedTab,
        FocusAddress,
        SearchTabs,
        NextTab,
        PreviousTab,
        ToggleSidebar,
        CycleTabLayout,
        OpenSettings,
        PinActiveTab,
        DuplicateTab,
        ReloadPage,
        GoBack,
        GoForward,
        OpenFind,
        FindNext,
        FindPrevious,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ToggleDusk,
        CopyAddress,
        ToggleSplitView,
        SplitFocusPrev,
        SplitFocusNext,
        DismissOverlay,
        Quit
    ]
);

/// Default key bindings for the gpui shell (`FEATURES.md` §22).
#[must_use]
pub fn shell_key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("ctrl-t", NewTab, None),
        KeyBinding::new("cmd-shift-n", NewPrivateTab, None),
        KeyBinding::new("ctrl-shift-n", NewPrivateTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("cmd-shift-t", ReopenClosedTab, None),
        KeyBinding::new("ctrl-shift-t", ReopenClosedTab, None),
        KeyBinding::new("cmd-l", FocusAddress, None),
        KeyBinding::new("ctrl-l", FocusAddress, None),
        KeyBinding::new("cmd-k", SearchTabs, None),
        KeyBinding::new("ctrl-k", SearchTabs, None),
        KeyBinding::new("cmd-s", ToggleSidebar, None),
        KeyBinding::new("ctrl-s", ToggleSidebar, None),
        KeyBinding::new("cmd-shift-s", CycleTabLayout, None),
        KeyBinding::new("ctrl-shift-s", CycleTabLayout, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
        KeyBinding::new("cmd-d", DuplicateTab, None),
        KeyBinding::new("ctrl-d", DuplicateTab, None),
        KeyBinding::new("cmd-r", ReloadPage, None),
        KeyBinding::new("ctrl-r", ReloadPage, None),
        KeyBinding::new("cmd-[", GoBack, None),
        KeyBinding::new("ctrl-[", GoBack, None),
        KeyBinding::new("cmd-]", GoForward, None),
        KeyBinding::new("ctrl-]", GoForward, None),
        KeyBinding::new("cmd-f", OpenFind, None),
        KeyBinding::new("ctrl-f", OpenFind, None),
        KeyBinding::new("cmd-g", FindNext, None),
        KeyBinding::new("ctrl-g", FindNext, None),
        KeyBinding::new("cmd-shift-g", FindPrevious, None),
        KeyBinding::new("ctrl-shift-g", FindPrevious, None),
        KeyBinding::new("cmd-=", ZoomIn, None),
        KeyBinding::new("ctrl-=", ZoomIn, None),
        KeyBinding::new("cmd-+", ZoomIn, None),
        KeyBinding::new("ctrl-+", ZoomIn, None),
        KeyBinding::new("cmd--", ZoomOut, None),
        KeyBinding::new("ctrl--", ZoomOut, None),
        KeyBinding::new("cmd-0", ZoomReset, None),
        KeyBinding::new("ctrl-0", ZoomReset, None),
        KeyBinding::new("cmd-shift-d", ToggleDusk, None),
        KeyBinding::new("ctrl-shift-d", ToggleDusk, None),
        KeyBinding::new("cmd-shift-c", CopyAddress, None),
        KeyBinding::new("ctrl-shift-c", CopyAddress, None),
        KeyBinding::new("cmd-alt-n", ToggleSplitView, None),
        KeyBinding::new("ctrl-alt-n", ToggleSplitView, None),
        KeyBinding::new("ctrl-cmd-left", SplitFocusPrev, None),
        KeyBinding::new("ctrl-alt-left", SplitFocusPrev, None),
        KeyBinding::new("ctrl-cmd-right", SplitFocusNext, None),
        KeyBinding::new("ctrl-alt-right", SplitFocusNext, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
        KeyBinding::new("escape", DismissOverlay, None),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("ctrl-q", Quit, None),
    ]
}

/// Which mode opened the floating address overlay (`FEATURES.md` §4.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AddressOverlayMode {
    /// `⌘L` over-page address entry; edits/navigates the active tab.
    #[default]
    OverPage,
    /// `⌘T` when `newtab.floating` is on; preserves draft until Return opens a tab.
    FloatingNewTab,
    /// `⌘K` open-tab switcher & quick address search.
    SearchTabs,
}

#[derive(Debug)]
struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Result<Self, UiError> {
        let seq = TEMP_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&path)
            .map_err(|e| UiError::Controller(format!("failed to create temp store: {e}")))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Controller-backed UI state model driving [`BrowserController`] and [`AccessibilityTree`].
pub struct ShellModel {
    controller: BrowserController,
    snapshot: UiSnapshot,
    focus: FocusController,
    a11y_tree: AccessibilityTree,
    palette: Palette,
    theme: Theme,
    high_contrast: bool,
    reduce_motion: bool,
    text_scale: f32,
    system_dark: bool,
    last_frame_dark: bool,
    address_overlay_open: bool,
    address_overlay_mode: AddressOverlayMode,
    floating_newtab_draft: String,
    selected_suggestion: Option<usize>,
    site_card_open: bool,
    space_menu_open: bool,
    shake_generation: u64,
    shake_step: usize,
    shaking: bool,
    last_directives: Vec<EngineDirective>,
    last_copied_text: Option<String>,
    local_notice: Option<String>,
    _temp_dir: Option<TempDirGuard>,
}

impl std::fmt::Debug for ShellModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellModel")
            .field("tabs", &self.snapshot.tabs.len())
            .field("active_tab", &self.snapshot.active_tab)
            .field("theme", &self.theme)
            .field("high_contrast", &self.high_contrast)
            .field("reduce_motion", &self.reduce_motion)
            .field("address_overlay_open", &self.address_overlay_open)
            .field("site_card_open", &self.site_card_open)
            .finish_non_exhaustive()
    }
}

impl ShellModel {
    /// Open a persistent [`DataStore`] directory with the given [`RestoreMode`].
    pub fn open(
        data_dir: impl AsRef<Path>,
        capabilities: EngineCapabilities,
        restore_mode: RestoreMode,
        system_dark: bool,
    ) -> Result<Self, UiError> {
        let (controller, directives) =
            BrowserController::open(data_dir, capabilities, restore_mode)
                .map_err(|e| UiError::Controller(e.to_string()))?;
        Ok(Self::from_parts(controller, directives, system_dark, None))
    }

    /// Open an isolated temporary [`DataStore`] cleaned up on drop.
    pub fn with_temp_store(
        capabilities: EngineCapabilities,
        restore_mode: RestoreMode,
        system_dark: bool,
    ) -> Result<Self, UiError> {
        let guard = TempDirGuard::new("searlx-ui-store")?;
        let (controller, directives) =
            BrowserController::open(guard.path(), capabilities, restore_mode)
                .map_err(|e| UiError::Controller(e.to_string()))?;
        Ok(Self::from_parts(
            controller,
            directives,
            system_dark,
            Some(guard),
        ))
    }

    /// Wrap an already-constructed [`BrowserController`].
    #[must_use]
    pub fn with_controller(controller: BrowserController, system_dark: bool) -> Self {
        Self::from_parts(controller, Vec::new(), system_dark, None)
    }

    fn from_parts(
        controller: BrowserController,
        directives: Vec<EngineDirective>,
        system_dark: bool,
        temp_dir: Option<TempDirGuard>,
    ) -> Self {
        let snapshot = controller.ui_snapshot();
        let focus = FocusController::new();
        let a11y_tree = AccessibilityTree::from_snapshot(&snapshot, focus.current(), false, false);
        let theme = Theme::from_look(snapshot.settings.look);
        let high_contrast = snapshot.settings.tabs_high_contrast;
        let reduce_motion = snapshot.settings.reduce_motion;
        let text_scale = snapshot.settings.text_scale;
        let palette = Palette::for_settings(&snapshot.settings, system_dark);
        let initial_dark = Palette::is_dark(&snapshot.settings, system_dark);

        let mut model = Self {
            controller,
            snapshot,
            focus,
            a11y_tree,
            palette,
            theme,
            high_contrast,
            reduce_motion,
            text_scale,
            system_dark,
            last_frame_dark: !initial_dark,
            address_overlay_open: false,
            address_overlay_mode: AddressOverlayMode::OverPage,
            floating_newtab_draft: String::new(),
            selected_suggestion: None,
            site_card_open: false,
            space_menu_open: false,
            shake_generation: 0,
            shake_step: metrics::SHAKE_OFFSETS_PX.len().saturating_sub(1),
            shaking: false,
            last_directives: directives,
            last_copied_text: None,
            local_notice: None,
            _temp_dir: temp_dir,
        };
        model.sync_frame_dark();
        model
    }

    /// Read-only view of the current [`UiSnapshot`].
    #[must_use]
    pub const fn snapshot(&self) -> &UiSnapshot {
        &self.snapshot
    }

    /// Read-only access to the owned [`BrowserController`].
    #[must_use]
    pub const fn controller(&self) -> &BrowserController {
        &self.controller
    }

    /// Synchronized chrome [`AccessibilityTree`].
    #[must_use]
    pub const fn a11y_tree(&self) -> &AccessibilityTree {
        &self.a11y_tree
    }

    /// Explicit keyboard [`FocusController`].
    #[must_use]
    pub const fn focus_controller(&self) -> &FocusController {
        &self.focus
    }

    /// Current resolved color [`Palette`].
    #[must_use]
    pub const fn palette(&self) -> Palette {
        self.palette
    }

    /// Active [`Theme`] preference from `UiSnapshot.settings`.
    #[must_use]
    pub const fn theme(&self) -> Theme {
        self.theme
    }

    /// Whether high contrast text/borders are active from `UiSnapshot.settings`.
    #[must_use]
    pub const fn high_contrast(&self) -> bool {
        self.high_contrast
    }

    /// Whether animations are reduced from `UiSnapshot.settings`.
    #[must_use]
    pub const fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// Effective text scale factor from `UiSnapshot.settings`.
    #[must_use]
    pub const fn text_scale(&self) -> f32 {
        self.text_scale
    }

    /// Whether the effective appearance is dark.
    #[must_use]
    pub fn is_dark(&self) -> bool {
        Palette::is_dark(&self.snapshot.settings, self.system_dark)
    }

    /// Whether the floating address overlay is currently open.
    #[must_use]
    pub const fn is_address_overlay_open(&self) -> bool {
        self.address_overlay_open
    }

    /// Current floating address overlay mode.
    #[must_use]
    pub const fn address_overlay_mode(&self) -> AddressOverlayMode {
        self.address_overlay_mode
    }

    /// Currently highlighted suggestion index in the address overlay.
    #[must_use]
    pub const fn selected_suggestion(&self) -> Option<usize> {
        self.selected_suggestion
    }

    /// Whether the Site Card popover is currently open.
    #[must_use]
    pub const fn is_site_card_open(&self) -> bool {
        self.site_card_open
    }

    /// Whether the Space Switcher popover is currently open.
    #[must_use]
    pub const fn is_space_menu_open(&self) -> bool {
        self.space_menu_open
    }

    /// Current invalid-address shake generation counter.
    #[must_use]
    pub const fn shake_generation(&self) -> u64 {
        self.shake_generation
    }

    /// Current horizontal pixel offset for invalid-address shake (`0.0` when `reduce_motion`).
    #[must_use]
    pub fn shake_offset_px(&self) -> f32 {
        if !self.shaking {
            return 0.0;
        }
        metrics::shake_offset_px(self.shake_step, self.reduce_motion)
    }

    /// Directives returned by the last controller dispatch or event reconciliation.
    #[must_use]
    pub fn last_directives(&self) -> &[EngineDirective] {
        &self.last_directives
    }

    /// Most recent clipboard copy text emitted via `EngineDirective::CopyToClipboard`.
    #[must_use]
    pub fn last_copied_text(&self) -> Option<&str> {
        self.last_copied_text.as_deref()
    }

    /// Ephemeral local UI notice (e.g. copy confirmation or validation hint).
    #[must_use]
    pub fn local_notice(&self) -> Option<&str> {
        self.local_notice.as_deref()
    }

    /// Update the OS dark-mode signal and resynchronize palette + Dusk frame state.
    pub fn set_system_dark(&mut self, system_dark: bool) {
        if self.system_dark != system_dark {
            self.system_dark = system_dark;
            self.sync_theme_from_snapshot();
            self.sync_frame_dark();
        }
    }

    /// Dispatch an [`AppCommand`] into [`BrowserController`] and refresh snapshot, theme, and a11y tree.
    pub fn dispatch(&mut self, command: AppCommand) -> Result<Vec<EngineDirective>, UiError> {
        let directives = self
            .controller
            .dispatch(command)
            .map_err(|e| UiError::Controller(e.to_string()))?;
        self.record_directives(&directives);
        self.refresh_snapshot();
        Ok(directives)
    }

    /// Reconcile an [`EngineEvent`] from a host engine and refresh snapshot + a11y tree.
    pub fn handle_engine_event(
        &mut self,
        event: EngineEvent,
    ) -> Result<Vec<EngineDirective>, UiError> {
        let directives = self
            .controller
            .handle_engine_event(event)
            .map_err(|e| UiError::Controller(e.to_string()))?;
        self.record_directives(&directives);
        self.refresh_snapshot();
        Ok(directives)
    }

    /// Open the floating address overlay in `mode` (`⌘L` / `⌘T` / `⌘K`).
    pub fn open_address_overlay(&mut self, mode: AddressOverlayMode) {
        self.site_card_open = false;
        self.space_menu_open = false;
        self.address_overlay_open = true;
        self.address_overlay_mode = mode;
        self.selected_suggestion = None;
        self.shaking = false;
        self.local_notice = None;
        self.focus.focus_address_field();

        match mode {
            AddressOverlayMode::OverPage => {
                let _ = self.dispatch(AppCommand::FocusAddressBar);
            }
            AddressOverlayMode::FloatingNewTab => {
                let draft = self.floating_newtab_draft.clone();
                let _ = self.dispatch(AppCommand::FocusAddressBar);
                let _ = self.dispatch(AppCommand::SetAddressText { text: draft });
            }
            AddressOverlayMode::SearchTabs => {
                let _ = self.dispatch(AppCommand::FocusAddressBar);
                let _ = self.dispatch(AppCommand::SetAddressText {
                    text: String::new(),
                });
            }
        }
    }

    /// Dismiss the floating address overlay, preserving any `FloatingNewTab` draft (`FEATURES.md` §4.1).
    pub fn close_address_overlay(&mut self) {
        if self.address_overlay_open
            && self.address_overlay_mode == AddressOverlayMode::FloatingNewTab
        {
            self.floating_newtab_draft = self.snapshot.address.text.clone();
        }
        self.address_overlay_open = false;
        self.selected_suggestion = None;
        self.shaking = false;
        let _ = self.dispatch(AppCommand::DismissAddressShake);
        self.focus.dismiss_overlay_to_page();
        self.sync_a11y();
    }

    /// Handle `Ctrl/Cmd+T`: either opens a floating new-tab overlay (`newtab.floating`)
    /// or creates a new blank tab and focuses the floating address overlay.
    pub fn open_new_tab(&mut self) {
        if self.snapshot.settings.newtab_floating {
            self.open_address_overlay(AddressOverlayMode::FloatingNewTab);
            return;
        }
        let _ = self.dispatch(AppCommand::NewTab {
            url: None,
            activate: true,
        });
        self.open_address_overlay(AddressOverlayMode::OverPage);
    }

    /// Update live typed text in the address field, refreshing controller suggestions & completion.
    pub fn set_address_text(&mut self, text: String) {
        self.selected_suggestion = None;
        self.shaking = false;
        if self.address_overlay_open
            && self.address_overlay_mode == AddressOverlayMode::FloatingNewTab
        {
            self.floating_newtab_draft = text.clone();
        }
        let _ = self.dispatch(AppCommand::SetAddressText { text });
    }

    /// Clear locked site-search chip when Backspace is pressed on an empty field.
    pub fn clear_site_search_chip(&mut self) {
        if self.snapshot.address.site_search_chip.is_some() {
            let _ = self.dispatch(AppCommand::LockSiteSearchChip { site: None });
        }
    }

    /// Step the highlighted suggestion row (`↑`/`↓` clears selection when stepping above 0).
    pub fn step_suggestion(&mut self, delta: isize) {
        let len = self.snapshot.address.suggestions.len();
        if len == 0 {
            self.selected_suggestion = None;
            return;
        }
        self.selected_suggestion = match (self.selected_suggestion, delta) {
            (None, d) if d > 0 => Some(0),
            (None, d) if d < 0 => Some(len - 1),
            (Some(0), d) if d < 0 => None,
            (Some(idx), d) => {
                let next = (idx as isize + d).clamp(0, (len as isize) - 1) as usize;
                Some(next)
            }
            _ => None,
        };
    }

    /// Handle `Tab` / `⇧Tab` in the address field (`FEATURES.md` §4.3, §29):
    /// locks a `SiteSearchOffer` chip when present; otherwise walks suggestions.
    /// Never accepts the grey inline completion.
    pub fn handle_address_tab(&mut self, shift: bool) -> bool {
        if !shift && self.snapshot.address.site_search_chip.is_none() {
            let offer = self
                .selected_suggestion
                .and_then(|idx| self.snapshot.address.suggestions.get(idx))
                .or_else(|| self.snapshot.address.suggestions.first());
            if let Some(SuggestionRow::SiteSearchOffer { site }) = offer.cloned() {
                let _ = self.dispatch(AppCommand::LockSiteSearchChip { site: Some(site) });
                let _ = self.dispatch(AppCommand::SetAddressText {
                    text: String::new(),
                });
                self.selected_suggestion = None;
                return true;
            }
        }
        self.step_suggestion(if shift { -1 } else { 1 });
        false
    }

    /// Submit address/search/command input (`Return`, `⌘Return`, `⇧⌘Return`, `⇧Return`).
    /// Returns `true` if navigation/command succeeded and `false` if invalid input triggered shake.
    pub fn submit_address(&mut self, typed: &str, command: bool, shift: bool) -> bool {
        if let Some(idx) = self.selected_suggestion
            && idx < self.snapshot.address.suggestions.len()
        {
            return self.activate_suggestion(idx, command, shift);
        }

        let is_exact_cmd =
            self.snapshot.settings.commandbar && AddressCommand::match_exact(typed).is_some();
        let trimmed = typed.trim();
        let resolved_input = if !is_exact_cmd
            && self.snapshot.address.site_search_chip.is_none()
            && !trimmed.is_empty()
            && !trimmed.chars().any(char::is_whitespace)
            && let Some(suffix) = &self.snapshot.address.inline_completion
        {
            format!("{trimmed}{suffix}")
        } else {
            typed.to_owned()
        };

        let disposition = if self.address_overlay_open
            && self.address_overlay_mode == AddressOverlayMode::FloatingNewTab
            && !command
        {
            SubmitDisposition::NewTabInFront
        } else {
            SubmitDisposition::from_modifiers(command, shift)
        };

        let _ = self.dispatch(AppCommand::SubmitAddress {
            input: resolved_input,
            disposition,
            shift_pressed: shift,
        });

        if self.snapshot.address.shake_reason.is_some() {
            self.address_overlay_open = true;
            self.shaking = true;
            self.shake_step = 0;
            self.shake_generation = self.shake_generation.saturating_add(1);
            self.focus.focus_address_field();
            self.sync_a11y();
            return false;
        }

        self.finish_address_submit();
        true
    }

    /// Activate a specific suggestion row by index.
    pub fn activate_suggestion(&mut self, index: usize, command: bool, shift: bool) -> bool {
        let Some(row) = self.snapshot.address.suggestions.get(index).cloned() else {
            return false;
        };
        let disposition = if self.address_overlay_open
            && self.address_overlay_mode == AddressOverlayMode::FloatingNewTab
            && !command
        {
            SubmitDisposition::NewTabInFront
        } else {
            SubmitDisposition::from_modifiers(command, shift)
        };

        match row {
            SuggestionRow::SiteSearchOffer { site } => {
                let _ = self.dispatch(AppCommand::LockSiteSearchChip { site: Some(site) });
                let _ = self.dispatch(AppCommand::SetAddressText {
                    text: String::new(),
                });
                self.selected_suggestion = None;
                return false;
            }
            SuggestionRow::OpenTab { tab, .. } => {
                let _ = self.dispatch(AppCommand::ActivateTab { tab });
            }
            SuggestionRow::Command(cmd) => {
                let _ = self.dispatch(AppCommand::ExecuteAddressCommand(cmd));
            }
            SuggestionRow::Bookmark { url: None, .. } => return false,
            SuggestionRow::PastSiteSearch { url, .. }
            | SuggestionRow::History { url, .. }
            | SuggestionRow::Bookmark { url: Some(url), .. }
            | SuggestionRow::ICloudTab { url, .. }
            | SuggestionRow::KnownSite { url, .. }
            | SuggestionRow::EngineSuggestion { url, .. }
            | SuggestionRow::InternalPage { url, .. }
            | SuggestionRow::SearchWords { url, .. } => match disposition {
                SubmitDisposition::CurrentTab => {
                    if let Some(active) = self.snapshot.active_tab {
                        let _ = self.dispatch(AppCommand::NavigateTab { tab: active, url });
                    }
                }
                SubmitDisposition::NewTabBehind => {
                    let _ = self.dispatch(AppCommand::NewTab {
                        url: Some(url),
                        activate: false,
                    });
                }
                SubmitDisposition::NewTabInFront => {
                    let _ = self.dispatch(AppCommand::NewTab {
                        url: Some(url),
                        activate: true,
                    });
                }
            },
        }
        self.finish_address_submit();
        true
    }

    fn finish_address_submit(&mut self) {
        self.address_overlay_open = false;
        self.floating_newtab_draft.clear();
        self.selected_suggestion = None;
        self.shaking = false;
        self.shake_step = metrics::SHAKE_OFFSETS_PX.len().saturating_sub(1);
        self.focus.dismiss_overlay_to_page();
        self.sync_a11y();
    }

    /// Advance the damped invalid-address shake step; returns `true` if more steps remain.
    pub fn advance_shake_step(&mut self, generation: u64) -> bool {
        if generation != self.shake_generation || !self.shaking || self.reduce_motion {
            self.shaking = false;
            return false;
        }
        if self.shake_step + 1 < metrics::SHAKE_OFFSETS_PX.len() {
            self.shake_step += 1;
            true
        } else {
            self.shaking = false;
            false
        }
    }

    /// Toggle the Site Card popover (`FEATURES.md` §5.8).
    pub fn toggle_site_card(&mut self) {
        if self.snapshot.site_card.is_none() {
            self.open_address_overlay(AddressOverlayMode::OverPage);
            return;
        }
        self.site_card_open = !self.site_card_open;
        if self.site_card_open {
            self.address_overlay_open = false;
            self.space_menu_open = false;
            self.focus.focus_target(FocusTarget::SiteCard);
        } else {
            self.focus.dismiss_overlay_to_page();
        }
        self.sync_a11y();
    }

    /// Close the Site Card popover.
    pub fn close_site_card(&mut self) {
        if self.site_card_open {
            self.site_card_open = false;
            self.focus.dismiss_overlay_to_page();
            self.sync_a11y();
        }
    }

    /// Toggle the Space Switcher popover.
    pub fn toggle_space_menu(&mut self) {
        self.space_menu_open = !self.space_menu_open;
        if self.space_menu_open {
            self.site_card_open = false;
            self.address_overlay_open = false;
        }
    }

    /// Handle clicking a tab row: opens Site Card if already active (`FEATURES.md` §1),
    /// otherwise activates/wakes the clicked tab.
    pub fn click_tab(&mut self, tab_id: TabId) {
        self.space_menu_open = false;
        let is_already_active = self.snapshot.active_tab == Some(tab_id);
        if is_already_active {
            if self.snapshot.site_card.is_some() {
                self.toggle_site_card();
            } else {
                self.open_address_overlay(AddressOverlayMode::OverPage);
            }
            return;
        }
        self.site_card_open = false;
        self.address_overlay_open = false;
        let _ = self.dispatch(AppCommand::ActivateTab { tab: tab_id });
    }

    /// Close a loose tab, put down a pinned tab, or put away/delete a grouped tab.
    pub fn close_or_put_down_tab(&mut self, tab_id: TabId) {
        let _ = self.dispatch(AppCommand::CloseTab { tab: Some(tab_id) });
    }

    /// Cycle through open tabs by `delta` (`Ctrl+Tab` / `Ctrl+Shift+Tab`).
    pub fn cycle_tabs(&mut self, delta: isize) {
        let tabs = &self.snapshot.tabs;
        if tabs.is_empty() {
            return;
        }
        let current = tabs.iter().position(|t| t.active).unwrap_or(0) as isize;
        let next_idx = (current + delta).rem_euclid(tabs.len() as isize) as usize;
        if let Some(target) = tabs.get(next_idx) {
            let _ = self.dispatch(AppCommand::ActivateTab { tab: target.id });
        }
    }

    /// Toggle sidebar fold (`Ctrl/Cmd+S` -> `AppCommand::ToggleSidebarFold`).
    pub fn toggle_sidebar_fold(&mut self) {
        let _ = self.dispatch(AppCommand::ToggleSidebarFold);
    }

    /// Cycle chrome tab layout (`LeftColumn` -> `RightColumn` -> `TopTabs` -> `IconsOnly`).
    pub fn cycle_tab_layout(&mut self) {
        let next = match self.snapshot.sidebar_layout {
            TabLayoutMode::LeftColumn => TabLayoutMode::RightColumn,
            TabLayoutMode::RightColumn => TabLayoutMode::TopTabs,
            TabLayoutMode::TopTabs => TabLayoutMode::IconsOnly,
            TabLayoutMode::IconsOnly => TabLayoutMode::LeftColumn,
        };
        let mut settings = self.snapshot.settings.clone();
        settings.tab_layout = next;
        let _ = self.dispatch(AppCommand::UpdateSettings {
            settings: Box::new(settings),
        });
    }

    /// Update theme look preference (`System` / `Light` / `Dark`) via `AppCommand::UpdateSettings`.
    pub fn set_look_preference(&mut self, look: LookPreference) {
        let mut settings = self.snapshot.settings.clone();
        settings.look = look;
        let _ = self.dispatch(AppCommand::UpdateSettings {
            settings: Box::new(settings),
        });
    }

    /// Update high contrast setting via `AppCommand::UpdateSettings`.
    pub fn set_high_contrast(&mut self, enabled: bool) {
        let mut settings = self.snapshot.settings.clone();
        settings.tabs_high_contrast = enabled;
        let _ = self.dispatch(AppCommand::UpdateSettings {
            settings: Box::new(settings),
        });
    }

    /// Update reduce motion setting via `AppCommand::UpdateSettings`.
    pub fn set_reduce_motion(&mut self, enabled: bool) {
        let mut settings = self.snapshot.settings.clone();
        settings.reduce_motion = enabled;
        let _ = self.dispatch(AppCommand::UpdateSettings {
            settings: Box::new(settings),
        });
    }

    /// Mutate settings cleanly through [`AppCommand::UpdateSettings`].
    pub fn update_settings(&mut self, mutator: impl FnOnce(&mut Settings)) {
        let mut settings = self.snapshot.settings.clone();
        mutator(&mut settings);
        let _ = self.dispatch(AppCommand::UpdateSettings {
            settings: Box::new(settings),
        });
    }

    /// Open the floating Find bar (`⌘F`).
    pub fn open_find_bar(&mut self) {
        self.address_overlay_open = false;
        self.site_card_open = false;
        self.space_menu_open = false;
        self.focus.focus_target(FocusTarget::FindBar);
        let _ = self.dispatch(AppCommand::OpenFindBar);
    }

    /// Close the Find bar.
    pub fn close_find_bar(&mut self) {
        let _ = self.dispatch(AppCommand::CloseFindBar);
        self.focus.dismiss_overlay_to_page();
        self.sync_a11y();
    }

    /// Run Find in the active page using rendered page text as fallback hint.
    pub fn find_in_page(&mut self, query: &str) {
        let (case_sensitive, whole_words) = self
            .snapshot
            .find
            .as_ref()
            .map_or((false, false), |f| (f.case_sensitive, f.whole_words));
        let text_hint = self.rendered_page_text_hint();
        let _ = self.dispatch(AppCommand::FindInPage {
            query: query.to_owned(),
            case_sensitive,
            whole_words,
            text_hint,
        });
    }

    /// Toggle case sensitivity on the Find bar and re-run the query.
    pub fn toggle_find_case(&mut self) {
        let Some(find) = self.snapshot.find.clone() else {
            return;
        };
        let text_hint = self.rendered_page_text_hint();
        let _ = self.dispatch(AppCommand::FindInPage {
            query: find.query,
            case_sensitive: !find.case_sensitive,
            whole_words: find.whole_words,
            text_hint,
        });
    }

    /// Toggle whole-word matching on the Find bar and re-run the query.
    pub fn toggle_find_whole_words(&mut self) {
        let Some(find) = self.snapshot.find.clone() else {
            return;
        };
        if !find.whole_words_available {
            return;
        }
        let text_hint = self.rendered_page_text_hint();
        let _ = self.dispatch(AppCommand::FindInPage {
            query: find.query,
            case_sensitive: find.case_sensitive,
            whole_words: !find.whole_words,
            text_hint,
        });
    }

    /// Dismiss the top-most active overlay (`SiteCard` -> `SpaceMenu` -> `AddressOverlay` -> `FindBar`).
    pub fn dismiss_active_overlay(&mut self) {
        if self.site_card_open {
            self.close_site_card();
        } else if self.space_menu_open {
            self.space_menu_open = false;
        } else if self.address_overlay_open {
            self.close_address_overlay();
        } else if self.snapshot.find.is_some() {
            self.close_find_bar();
        } else {
            self.focus.dismiss_overlay_to_page();
            self.sync_a11y();
        }
    }

    /// Update viewport stage width for responsive Split View pane layout.
    pub fn set_stage_width_pt(&mut self, width_pt: f64) {
        if (self.controller.split_workspace().stage_width_pt() - width_pt).abs() > 1.0 {
            let _ = self.dispatch(AppCommand::SetViewportStageWidth { width_pt });
        }
    }

    /// Searchable plain text representing the currently rendered page content for Find fallback.
    #[must_use]
    pub fn rendered_page_text_hint(&self) -> Option<String> {
        if let Some(internal) = &self.snapshot.active_internal_page {
            return Some(internal_page_searchable_text(internal));
        }
        if let Some(article) = &self.snapshot.active_reader_article {
            return Some(format!("{} {}", article.title, article.content_html));
        }
        self.snapshot
            .tabs
            .iter()
            .find(|t| t.active)
            .map(|t| format!("{} {}", t.title, t.display_url))
    }

    fn record_directives(&mut self, directives: &[EngineDirective]) {
        for directive in directives {
            if let EngineDirective::CopyToClipboard { text, .. } = directive {
                self.last_copied_text = Some(text.clone());
                self.local_notice = Some("Copied address".to_owned());
            }
        }
        self.last_directives = directives.to_vec();
    }

    fn refresh_snapshot(&mut self) {
        self.snapshot = self.controller.ui_snapshot();
        if self.snapshot.site_card.is_none() {
            self.site_card_open = false;
        }
        self.sync_theme_from_snapshot();
        self.sync_frame_dark();
        self.sync_a11y();
    }

    fn sync_theme_from_snapshot(&mut self) {
        self.theme = Theme::from_look(self.snapshot.settings.look);
        self.high_contrast = self.snapshot.settings.tabs_high_contrast;
        self.reduce_motion = self.snapshot.settings.reduce_motion;
        self.text_scale = self.snapshot.settings.text_scale;
        self.palette = Palette::for_settings(&self.snapshot.settings, self.system_dark);
    }

    fn sync_frame_dark(&mut self) {
        let frame_dark = self.is_dark();
        if frame_dark != self.last_frame_dark {
            self.last_frame_dark = frame_dark;
            if let Ok(dirs) = self
                .controller
                .dispatch(AppCommand::SetFrameDark { dark: frame_dark })
            {
                self.last_directives.extend(dirs);
                self.snapshot = self.controller.ui_snapshot();
                self.sync_a11y();
            }
        }
    }

    fn sync_a11y(&mut self) {
        self.a11y_tree.sync(
            &self.snapshot,
            self.focus.current(),
            self.address_overlay_open,
            self.site_card_open,
        );
    }
}

/// The window's root gpui view backed by [`ShellModel`].
pub struct Shell {
    model: ShellModel,
    address_field: gpui::Entity<AddressField>,
    find_field: gpui::Entity<AddressField>,
    focus_handle: FocusHandle,
    shake_task: Option<Task<()>>,
    last_stage_width_px: f32,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl Shell {
    /// Construct a `Shell` wrapping an already-initialized [`ShellModel`].
    pub fn from_model(
        mut model: ShellModel,
        address_field: gpui::Entity<AddressField>,
        find_field: gpui::Entity<AddressField>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        model.set_system_dark(is_dark(window.appearance()));
        let palette = model.palette();
        address_field.update(cx, |field, cx| {
            field.set_palette(palette);
            field.set_text(model.snapshot().address.editable_address.clone(), cx);
            cx.notify();
        });
        find_field.update(cx, |field, cx| {
            field.set_palette(palette);
            field.set_placeholder("Find on page", cx);
            cx.notify();
        });

        cx.observe_window_appearance(window, |shell, window, cx| {
            shell.model.set_system_dark(is_dark(window.appearance()));
            shell.sync_field_palettes(cx);
            cx.notify();
        })
        .detach();

        cx.subscribe(&address_field, |shell, _field, event, cx| {
            shell.handle_address_field_event(event, cx);
        })
        .detach();

        cx.subscribe(&find_field, |shell, _field, event, cx| {
            shell.handle_find_field_event(event, cx);
        })
        .detach();

        Self {
            model,
            address_field,
            find_field,
            focus_handle: cx.focus_handle(),
            shake_task: None,
            last_stage_width_px: 0.0,
        }
    }

    /// Open a `Shell` backed by a persistent [`searlx_core::DataStore`] at `data_dir`.
    pub fn open(
        data_dir: impl AsRef<Path>,
        restore_mode: RestoreMode,
        capabilities: EngineCapabilities,
        address_field: gpui::Entity<AddressField>,
        find_field: gpui::Entity<AddressField>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, UiError> {
        let model = ShellModel::open(
            data_dir,
            capabilities,
            restore_mode,
            is_dark(window.appearance()),
        )?;
        Ok(Self::from_model(
            model,
            address_field,
            find_field,
            window,
            cx,
        ))
    }

    /// Open a `Shell` backed by an isolated temporary [`searlx_core::DataStore`].
    pub fn with_temp_store(
        capabilities: EngineCapabilities,
        address_field: gpui::Entity<AddressField>,
        find_field: gpui::Entity<AddressField>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, UiError> {
        let model = ShellModel::with_temp_store(
            capabilities,
            RestoreMode::FreshStart,
            is_dark(window.appearance()),
        )?;
        Ok(Self::from_model(
            model,
            address_field,
            find_field,
            window,
            cx,
        ))
    }

    /// Read-only access to the live [`UiSnapshot`].
    #[must_use]
    pub const fn snapshot(&self) -> &UiSnapshot {
        self.model.snapshot()
    }

    /// Read-only access to the owned [`BrowserController`].
    #[must_use]
    pub const fn controller(&self) -> &BrowserController {
        self.model.controller()
    }

    /// Read-only access to the synchronized [`AccessibilityTree`].
    #[must_use]
    pub const fn a11y_tree(&self) -> &AccessibilityTree {
        self.model.a11y_tree()
    }

    /// Access the underlying [`ShellModel`].
    #[must_use]
    pub const fn model(&self) -> &ShellModel {
        &self.model
    }

    /// Dispatch a core [`AppCommand`] and sync gpui fields/clipboard/window state.
    pub fn dispatch_command(&mut self, command: AppCommand, cx: &mut Context<Self>) {
        if let Ok(directives) = self.model.dispatch(command) {
            self.apply_directives_to_gpui(&directives, cx);
        }
        self.sync_field_palettes(cx);
        cx.notify();
    }

    fn apply_directives_to_gpui(&self, directives: &[EngineDirective], cx: &mut Context<Self>) {
        for directive in directives {
            if let EngineDirective::CopyToClipboard { text, .. } = directive {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            }
        }
    }

    fn sync_field_palettes(&mut self, cx: &mut Context<Self>) {
        let palette = self.model.palette();
        let completion = self.model.snapshot().address.inline_completion.clone();
        self.address_field.update(cx, |field, cx| {
            field.set_palette(palette);
            field.set_inline_completion(completion, cx);
            cx.notify();
        });
        self.find_field.update(cx, |field, cx| {
            field.set_palette(palette);
            cx.notify();
        });
    }

    fn handle_address_field_event(&mut self, event: &FieldEvent, cx: &mut Context<Self>) {
        match event {
            FieldEvent::Changed(text) => {
                self.model.set_address_text(text.clone());
                let completion = self.model.snapshot().address.inline_completion.clone();
                self.address_field.update(cx, |field, cx| {
                    field.set_inline_completion(completion, cx);
                });
            }
            FieldEvent::Submitted(text) => {
                self.submit_from_field(text, false, false, cx);
            }
            FieldEvent::SubmittedWith {
                text,
                command,
                shift,
            } => {
                self.submit_from_field(text, *command, *shift, cx);
            }
            FieldEvent::Cancelled => {
                self.model.close_address_overlay();
                self.sync_field_palettes(cx);
            }
            FieldEvent::SuggestionStep(delta) => {
                self.model.step_suggestion(*delta);
            }
            FieldEvent::TabPressed { shift } => {
                if self.model.handle_address_tab(*shift) {
                    let next_text = self.model.snapshot().address.text.clone();
                    let completion = self.model.snapshot().address.inline_completion.clone();
                    self.address_field.update(cx, |field, cx| {
                        field.set_text(next_text, cx);
                        field.set_inline_completion(completion, cx);
                    });
                }
            }
            FieldEvent::BackspaceOnEmpty => {
                self.model.clear_site_search_chip();
            }
        }
        cx.notify();
    }

    fn handle_find_field_event(&mut self, event: &FieldEvent, cx: &mut Context<Self>) {
        match event {
            FieldEvent::Changed(query) => self.model.find_in_page(query),
            FieldEvent::Submitted(_) => {
                let _ = self.model.dispatch(AppCommand::FindNext);
            }
            FieldEvent::SubmittedWith { shift: true, .. } => {
                let _ = self.model.dispatch(AppCommand::FindPrevious);
            }
            FieldEvent::SubmittedWith { .. } => {
                let _ = self.model.dispatch(AppCommand::FindNext);
            }
            FieldEvent::Cancelled => self.model.close_find_bar(),
            FieldEvent::BackspaceOnEmpty
            | FieldEvent::SuggestionStep(_)
            | FieldEvent::TabPressed { .. } => {}
        }
        cx.notify();
    }

    fn submit_from_field(
        &mut self,
        text: &str,
        command: bool,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        if self.model.submit_address(text, command, shift) {
            let dirs = self.model.last_directives().to_vec();
            self.apply_directives_to_gpui(&dirs, cx);
            let next_text = self.model.snapshot().address.editable_address.clone();
            self.address_field.update(cx, |field, cx| {
                field.set_text(next_text, cx);
                field.set_inline_completion(None, cx);
            });
            self.sync_field_palettes(cx);
        } else {
            let dirs = self.model.last_directives().to_vec();
            self.apply_directives_to_gpui(&dirs, cx);
            if self.model.snapshot().address.shake_reason.is_some() {
                self.schedule_shake_animation(cx);
            }
        }
    }

    fn schedule_shake_animation(&mut self, cx: &mut Context<Self>) {
        if self.model.reduce_motion() {
            self.shake_task = None;
            return;
        }
        let generation = self.model.shake_generation();
        let step_dur = Duration::from_millis(metrics::SHAKE_STEP_MS);
        self.shake_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(step_dur).await;
                let mut keep_going = false;
                let _ = this.update(cx, |shell, cx| {
                    keep_going = shell.model.advance_shake_step(generation);
                    cx.notify();
                });
                if !keep_going {
                    break;
                }
            }
        }));
    }

    fn open_overlay_and_focus(
        &mut self,
        mode: AddressOverlayMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.open_address_overlay(mode);
        let text = self.model.snapshot().address.text.clone();
        let placeholder = match mode {
            AddressOverlayMode::SearchTabs => "Search open tabs or enter address",
            AddressOverlayMode::FloatingNewTab => "New tab: enter address or search",
            AddressOverlayMode::OverPage => "Search or enter address",
        };
        let completion = self.model.snapshot().address.inline_completion.clone();
        self.address_field.update(cx, |field, cx| {
            field.set_placeholder(placeholder, cx);
            field.set_text(text, cx);
            field.set_inline_completion(completion, cx);
            if mode == AddressOverlayMode::OverPage {
                field.select_all_text(cx);
            }
        });
        let handle = self.address_field.read(cx).focus_handle(cx);
        window.focus(&handle);
        cx.notify();
    }

    fn sync_address_field_after_tab_action(&mut self, cx: &mut Context<Self>) {
        let next_text = self.model.snapshot().address.editable_address.clone();
        self.address_field.update(cx, |field, cx| {
            field.set_text(next_text, cx);
            field.set_inline_completion(None, cx);
        });
        self.sync_field_palettes(cx);
    }

    // ------------------------------------------------------------------------
    // Action handlers
    // ------------------------------------------------------------------------

    fn on_new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.snapshot().settings.newtab_floating {
            self.open_overlay_and_focus(AddressOverlayMode::FloatingNewTab, window, cx);
        } else {
            self.model.open_new_tab();
            self.open_overlay_and_focus(AddressOverlayMode::OverPage, window, cx);
        }
    }

    fn on_new_private_tab(
        &mut self,
        _: &NewPrivateTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = self.model.dispatch(AppCommand::NewPrivateTab {
            url: None,
            activate: true,
        });
        self.open_overlay_and_focus(AddressOverlayMode::OverPage, window, cx);
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_address_overlay_open() {
            self.model.close_address_overlay();
            window.focus(&self.focus_handle);
            cx.notify();
            return;
        }
        let _ = self.model.dispatch(AppCommand::CloseTab { tab: None });
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_reopen_closed_tab(
        &mut self,
        _: &ReopenClosedTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = self.model.dispatch(AppCommand::ReopenClosedTab);
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_focus_address(&mut self, _: &FocusAddress, window: &mut Window, cx: &mut Context<Self>) {
        self.open_overlay_and_focus(AddressOverlayMode::OverPage, window, cx);
    }

    fn on_search_tabs(&mut self, _: &SearchTabs, window: &mut Window, cx: &mut Context<Self>) {
        self.open_overlay_and_focus(AddressOverlayMode::SearchTabs, window, cx);
    }

    fn on_next_tab(&mut self, _: &NextTab, _: &mut Window, cx: &mut Context<Self>) {
        self.model.cycle_tabs(1);
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_previous_tab(&mut self, _: &PreviousTab, _: &mut Window, cx: &mut Context<Self>) {
        self.model.cycle_tabs(-1);
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.model.toggle_sidebar_fold();
        cx.notify();
    }

    fn on_cycle_tab_layout(&mut self, _: &CycleTabLayout, _: &mut Window, cx: &mut Context<Self>) {
        self.model.cycle_tab_layout();
        cx.notify();
    }

    fn on_open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::OpenInternalPage {
            kind: InternalPageKind::Settings,
            detail: None,
        });
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_pin_active_tab(&mut self, _: &PinActiveTab, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::TogglePin { tab: None });
        cx.notify();
    }

    fn on_duplicate_tab(&mut self, _: &DuplicateTab, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::DuplicateTab { tab: None });
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_reload_page(&mut self, _: &ReloadPage, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::Reload {
            tab: None,
            from_origin: false,
        });
        cx.notify();
    }

    fn on_go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::GoBack { tab: None });
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::GoForward { tab: None });
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_open_find(&mut self, _: &OpenFind, window: &mut Window, cx: &mut Context<Self>) {
        self.model.open_find_bar();
        let query = self
            .model
            .snapshot()
            .find
            .as_ref()
            .map_or_else(String::new, |f| f.query.clone());
        self.find_field.update(cx, |field, cx| {
            field.set_text(query, cx);
            field.select_all_text(cx);
        });
        let handle = self.find_field.read(cx).focus_handle(cx);
        window.focus(&handle);
        cx.notify();
    }

    fn on_find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::FindNext);
        cx.notify();
    }

    fn on_find_previous(&mut self, _: &FindPrevious, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::FindPrevious);
        cx.notify();
    }

    fn on_zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::ZoomStep { delta_steps: 1 });
        cx.notify();
    }

    fn on_zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self
            .model
            .dispatch(AppCommand::ZoomStep { delta_steps: -1 });
        cx.notify();
    }

    fn on_zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::ZoomReset);
        cx.notify();
    }

    fn on_toggle_dusk(&mut self, _: &ToggleDusk, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::ToggleDuskForSite);
        cx.notify();
    }

    fn on_copy_address(&mut self, _: &CopyAddress, _: &mut Window, cx: &mut Context<Self>) {
        if let Ok(dirs) = self.model.dispatch(AppCommand::CopyAddress { tab: None }) {
            self.apply_directives_to_gpui(&dirs, cx);
        }
        cx.notify();
    }

    fn on_toggle_split_view(
        &mut self,
        _: &ToggleSplitView,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = self.model.dispatch(AppCommand::ToggleSplitView {
            target_tab: None,
            partner_tab: None,
        });
        cx.notify();
    }

    fn on_split_focus_prev(&mut self, _: &SplitFocusPrev, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self
            .model
            .dispatch(AppCommand::MoveSplitFocus { delta: -1 });
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_split_focus_next(&mut self, _: &SplitFocusNext, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::MoveSplitFocus { delta: 1 });
        self.sync_address_field_after_tab_action(cx);
        cx.notify();
    }

    fn on_dismiss_overlay(
        &mut self,
        _: &DismissOverlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.dismiss_active_overlay();
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn on_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.model.dispatch(AppCommand::SaveSession);
        cx.quit();
    }

    // ------------------------------------------------------------------------
    // Modular render builders
    // ------------------------------------------------------------------------

    fn chrome_bg(&self) -> gpui::Hsla {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        let active_space = snap.spaces.iter().find(|s| s.active);
        let Some(space) = active_space else {
            return palette.chrome;
        };
        SpaceTint::from_space_color(space.color).wash_over(
            palette.chrome,
            self.model.is_dark(),
            space.strength,
        )
    }

    fn scaled(&self, base_pt: f32) -> gpui::Pixels {
        metrics::scaled_pt(base_pt, self.model.text_scale())
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        let icons_only = snap.sidebar_layout == TabLayoutMode::IconsOnly;
        let is_right = snap.sidebar_layout == TabLayoutMode::RightColumn;
        let width = if icons_only {
            metrics::SIDEBAR_ICONS
        } else {
            metrics::SIDEBAR_DEFAULT
        };
        let active_space = snap.spaces.iter().find(|s| s.active);
        let space_name: SharedString = active_space
            .map_or("Personal", |s| s.name.as_str())
            .to_owned()
            .into();
        let dot_color = active_space
            .and_then(|s| SpaceTint::from_space_color(s.color).dot_color(self.model.is_dark()))
            .map_or(palette.accent, Into::into);

        let pinned_tabs: Vec<&UiTabItem> = snap.tabs.iter().filter(|t| t.pinned).collect();
        let loose_tabs: Vec<&UiTabItem> = snap
            .tabs
            .iter()
            .filter(|t| !t.pinned && t.group.is_none())
            .collect();

        let mut column = div()
            .flex()
            .flex_col()
            .w(px(width))
            .h_full()
            .bg(self.chrome_bg())
            .border_color(palette.line);
        column = if is_right {
            column.border_l_1()
        } else {
            column.border_r_1()
        };

        let mut group_views = Vec::with_capacity(snap.groups.len());
        for group in &snap.groups {
            group_views.push(self.render_tab_group(group, icons_only, cx));
        }
        let mut loose_views = Vec::with_capacity(loose_tabs.len());
        for tab in &loose_tabs {
            loose_views.push(self.render_tab_row(tab, icons_only, cx));
        }

        column
            .child(self.render_sidebar_header(&space_name, dot_color, icons_only, cx))
            .when(!icons_only, |col| col.child(self.render_nav_controls(cx)))
            .when(!pinned_tabs.is_empty(), |col| {
                col.child(self.render_pinned_section(&pinned_tabs, icons_only, cx))
            })
            .child(
                div()
                    .id("tab-scroll-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .py_1()
                    .children(group_views)
                    .children(loose_views),
            )
            .when(self.model.is_space_menu_open() && !icons_only, |col| {
                col.child(self.render_space_switcher_popover(cx))
            })
            .child(self.render_sidebar_footer(icons_only, cx))
    }

    fn render_sidebar_header(
        &self,
        space_name: &SharedString,
        dot_color: gpui::Hsla,
        icons_only: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = self.model.palette();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .h(px(38.0))
            .child(
                div()
                    .id("space-switcher-btn")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_1p5()
                    .py_1()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|shell, _, _, cx| {
                            shell.model.toggle_space_menu();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .size(px(metrics::SPACE_DOT))
                            .rounded_full()
                            .bg(dot_color),
                    )
                    .when(!icons_only, |btn| {
                        btn.child(
                            div()
                                .text_size(self.scaled(11.5))
                                .text_color(palette.text)
                                .child(space_name.clone()),
                        )
                    }),
            )
            .when(!icons_only, |hdr| {
                hdr.child(div().flex_1()).child(
                    div()
                        .id("new-tab")
                        .w(px(22.0))
                        .h(px(22.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(self.scaled(15.0))
                        .text_color(palette.muted)
                        .cursor_pointer()
                        .hover(move |style| style.bg(palette.hover).text_color(palette.text))
                        .child("+")
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|shell, _, window, cx| {
                                if shell.model.snapshot().settings.newtab_floating {
                                    shell.open_overlay_and_focus(
                                        AddressOverlayMode::FloatingNewTab,
                                        window,
                                        cx,
                                    );
                                } else {
                                    shell.model.open_new_tab();
                                    shell.open_overlay_and_focus(
                                        AddressOverlayMode::OverPage,
                                        window,
                                        cx,
                                    );
                                }
                            }),
                        ),
                )
            })
    }

    fn render_nav_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.model.palette();
        let active = self.model.snapshot().tabs.iter().find(|t| t.active);
        let can_back = active.is_some_and(|t| t.can_go_back);
        let can_fwd = active.is_some_and(|t| t.can_go_forward);
        let loading = active.is_some_and(|t| t.loading);

        let mut row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .pb_1p5()
            .child(
                div()
                    .id("nav-back")
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(5.0))
                    .text_size(self.scaled(11.5))
                    .text_color(if can_back {
                        palette.text
                    } else {
                        palette.muted
                    })
                    .opacity(if can_back { 1.0 } else { 0.45 })
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover))
                    .child("‹")
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|shell, _, _, cx| {
                            shell.dispatch_command(AppCommand::GoBack { tab: None }, cx);
                            shell.sync_address_field_after_tab_action(cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("nav-forward")
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(5.0))
                    .text_size(self.scaled(11.5))
                    .text_color(if can_fwd { palette.text } else { palette.muted })
                    .opacity(if can_fwd { 1.0 } else { 0.45 })
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover))
                    .child("›")
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|shell, _, _, cx| {
                            shell.dispatch_command(AppCommand::GoForward { tab: None }, cx);
                            shell.sync_address_field_after_tab_action(cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("nav-reload")
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(5.0))
                    .text_size(self.scaled(11.0))
                    .text_color(palette.muted)
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover).text_color(palette.text))
                    .child(if loading { "×" } else { "↻" })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |shell, _, _, cx| {
                            if loading {
                                shell.dispatch_command(AppCommand::Stop { tab: None }, cx);
                            } else {
                                shell.dispatch_command(
                                    AppCommand::Reload {
                                        tab: None,
                                        from_origin: false,
                                    },
                                    cx,
                                );
                            }
                        }),
                    ),
            )
            .child(div().flex_1());

        if let Some(chip) = self.render_site_chip(cx) {
            row = row.child(chip);
        }
        row
    }

    fn render_site_chip(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        let active = snap.tabs.iter().find(|t| t.active)?;
        let label = if snap.address.is_lookalike {
            format!("! {}", snap.address.safe_host)
        } else if active.url.is_insecure() {
            format!("Not secure · {}", active.display_url)
        } else if active.url.is_internal() {
            active.title.clone()
        } else {
            active.url.host().map_or_else(
                || active.display_url.clone(),
                |h| h.trim_start_matches("www.").to_owned(),
            )
        };
        let chip_color = if snap.address.is_lookalike || active.url.is_insecure() {
            palette.danger
        } else {
            palette.muted
        };
        Some(
            div()
                .id("site-chip")
                .max_w(px(136.0))
                .px_2()
                .py_0p5()
                .rounded(px(5.0))
                .bg(if self.model.is_site_card_open() {
                    palette.selected
                } else {
                    palette.chrome
                })
                .border_1()
                .border_color(palette.line)
                .text_size(self.scaled(10.5))
                .text_color(chip_color)
                .truncate()
                .cursor_pointer()
                .hover(move |s| s.bg(palette.hover).text_color(palette.text))
                .child(label)
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|shell, _, _, cx| {
                        shell.model.toggle_site_card();
                        cx.notify();
                    }),
                ),
        )
    }

    fn render_pinned_section(
        &self,
        pinned: &[&UiTabItem],
        icons_only: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        if snap.settings.pins_list && !icons_only {
            let pin_rows: Vec<_> = pinned
                .iter()
                .map(|tab| self.render_tab_row(tab, false, cx))
                .collect();
            let folder_rows: Vec<_> = snap
                .pin_folders
                .iter()
                .map(|folder| {
                    div()
                        .px_2()
                        .py_0p5()
                        .text_size(self.scaled(10.5))
                        .text_color(palette.muted)
                        .child(format!(
                            "{} {} ({})",
                            if folder.collapsed { "▸" } else { "▾" },
                            folder.name,
                            folder.pins.len()
                        ))
                })
                .collect();
            return div()
                .flex()
                .flex_col()
                .gap_0p5()
                .px_1()
                .pb_1()
                .border_b_1()
                .border_color(palette.line)
                .children(pin_rows)
                .children(folder_rows);
        }

        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1p5()
            .px_2()
            .py_1p5()
            .border_b_1()
            .border_color(palette.line)
            .children(pinned.iter().map(|tab| {
                let id = tab.id;
                let active = tab.active;
                let sleeping = tab.sleeping;
                let glyph = tab_glyph_letter(&tab.title, tab.url.host()).to_string();
                div()
                    .id(("pin-square", id.get() as usize))
                    .relative()
                    .w(px(metrics::PIN_SQUARE_SIZE))
                    .h(px(metrics::PIN_SQUARE_SIZE))
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(if active {
                        palette.selected
                    } else {
                        palette.hover
                    })
                    .border_1()
                    .border_color(if active { palette.accent } else { palette.line })
                    .opacity(if sleeping { 0.52 } else { 1.0 })
                    .cursor_pointer()
                    .text_size(self.scaled(13.0))
                    .text_color(palette.text)
                    .child(glyph)
                    .when(tab.audible && !tab.muted, |sq| {
                        sq.child(
                            div()
                                .absolute()
                                .bottom(px(2.0))
                                .right(px(3.0))
                                .text_size(px(8.5))
                                .text_color(palette.accent)
                                .child("♪"),
                        )
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |shell, _, _, cx| {
                            shell.model.click_tab(id);
                            shell.sync_address_field_after_tab_action(cx);
                            cx.notify();
                        }),
                    )
            }))
    }

    fn render_tab_group(
        &self,
        group: &TabGroupItem,
        icons_only: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        let gid = group.id;
        let dot_color: gpui::Hsla = SpaceTint::from_space_color(group.color)
            .dot_color(self.model.is_dark())
            .map_or(palette.accent, Into::into);
        let member_tabs: Vec<&UiTabItem> =
            snap.tabs.iter().filter(|t| t.group == Some(gid)).collect();

        let mut container = div()
            .flex()
            .flex_col()
            .gap_0p5()
            .mx_1()
            .pl_1p5()
            .border_l_2()
            .border_color(dot_color);
        if !icons_only {
            container = container.child(
                div()
                    .id((
                        "group-header",
                        member_tabs.first().map_or(0, |t| t.id.get() as usize),
                    ))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(5.0))
                    .text_size(self.scaled(11.0))
                    .text_color(palette.muted)
                    .child(if group.collapsed { "▸" } else { "▾" })
                    .child(div().size(px(7.0)).rounded_full().bg(dot_color))
                    .child(div().flex_1().truncate().child(group.name.clone()))
                    .child(format!("{}", member_tabs.len()))
                    .child(
                        div()
                            .id((
                                "group-close",
                                member_tabs.first().map_or(0, |t| t.id.get() as usize),
                            ))
                            .px_1()
                            .cursor_pointer()
                            .hover(move |s| s.text_color(palette.danger))
                            .child("×")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |shell, _, _, cx| {
                                    cx.stop_propagation();
                                    shell.dispatch_command(
                                        AppCommand::CloseGroup { group: gid },
                                        cx,
                                    );
                                }),
                            ),
                    ),
            );
        }
        if !group.collapsed {
            let member_views: Vec<_> = member_tabs
                .iter()
                .map(|tab| self.render_tab_row(tab, icons_only, cx))
                .collect();
            container = container.children(member_views);
        }
        container
    }

    fn render_tab_row(
        &self,
        tab: &UiTabItem,
        icons_only: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let palette = self.model.palette();
        let snap = self.model.snapshot();
        let tab_id = tab.id;
        let active = tab.active;
        let sleeping = tab.sleeping;
        let row_opacity = if sleeping && (snap.settings.tabs_sleep_dim || tab.put_away) {
            0.52
        } else if sleeping {
            0.72
        } else {
            1.0
        };
        let row_height = metrics::tab_row_height(snap.settings.sidebar_roomy);
        let close_glyph = if tab.pinned || (tab.group.is_some() && !tab.put_away) {
            "−"
        } else {
            "×"
        };
        let display_title = if tab.title == "New Tab" && !tab.url.is_internal() {
            tab.display_url.clone()
        } else {
            tab.title.clone()
        };
        let glyph = if tab.letter == '\0' {
            tab_glyph_letter(&tab.title, tab.url.host()).to_string()
        } else {
            tab.letter.to_string()
        };

        div()
            .id(("tab-row", tab_id.get() as usize))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .h(px(row_height))
            .px_2()
            .mx_1()
            .rounded_md()
            .cursor_pointer()
            .bg(if active {
                palette.selected
            } else {
                self.chrome_bg()
            })
            .opacity(row_opacity)
            .hover(move |style| style.bg(palette.hover))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |shell, _, _, cx| {
                    shell.model.click_tab(tab_id);
                    shell.sync_address_field_after_tab_action(cx);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .size(px(metrics::GLYPH_SIZE_PT))
                    .rounded(px(5.0))
                    .bg(if active {
                        palette.accent
                    } else if tab.private {
                        palette.text
                    } else {
                        palette.line
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(self.scaled(10.0))
                    .text_color(if active || tab.private {
                        palette.selected
                    } else {
                        palette.text
                    })
                    .child(if tab.loading { "…".to_owned() } else { glyph }),
            )
            .when(!icons_only, |row| {
                row.child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .text_size(self.scaled(12.5))
                        .text_color(if active { palette.text } else { palette.muted })
                        .child(display_title),
                )
            })
            .when(!icons_only && tab.is_lookalike, |row| {
                row.child(badge_pill("!", palette.danger, palette.selected))
            })
            .when(!icons_only && tab.private, |row| {
                row.child(badge_pill("Priv", palette.accent, palette.selected))
            })
            .when(!icons_only && tab.in_split, |row| {
                row.child(badge_pill(
                    if tab.split_focused { "Split*" } else { "Split" },
                    if tab.split_focused {
                        palette.accent
                    } else {
                        palette.hover
                    },
                    if tab.split_focused {
                        palette.selected
                    } else {
                        palette.text
                    },
                ))
            })
            .when(!icons_only && sleeping, |row| {
                row.child(badge_pill("Zzz", palette.hover, palette.muted))
            })
            .when(!icons_only && (tab.audible || tab.muted), |row| {
                row.child(
                    div()
                        .id(("mute", tab_id.get() as usize))
                        .px_1()
                        .rounded(px(4.0))
                        .text_size(self.scaled(10.5))
                        .text_color(if tab.muted {
                            palette.danger
                        } else {
                            palette.accent
                        })
                        .hover(move |s| s.bg(palette.hover))
                        .child(if tab.muted { "🔇" } else { "♪" })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |shell, _, _, cx| {
                                cx.stop_propagation();
                                shell.dispatch_command(
                                    AppCommand::ToggleMute { tab: Some(tab_id) },
                                    cx,
                                );
                            }),
                        ),
                )
            })
            .when(!icons_only, |row| {
                row.child(
                    div()
                        .id(("close", tab_id.get() as usize))
                        .w(px(18.0))
                        .h(px(18.0))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(self.scaled(12.0))
                        .text_color(palette.muted)
                        .hover(move |style| style.bg(palette.hover).text_color(palette.text))
                        .child(close_glyph)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |shell, _, _, cx| {
                                cx.stop_propagation();
                                shell.model.close_or_put_down_tab(tab_id);
                                shell.sync_address_field_after_tab_action(cx);
                                cx.notify();
                            }),
                        ),
                )
            })
            .when(
                snap.settings.tabs_reading && active && tab.reading_progress > 0.0,
                |row| {
                    row.child(
                        div()
                            .absolute()
                            .bottom(px(0.0))
                            .left(px(4.0))
                            .h(px(2.0))
                            .w(relative(tab.reading_progress.clamp(0.0, 1.0)))
                            .bg(palette.accent),
                    )
                },
            )
    }

    fn render_space_switcher_popover(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        div()
            .mx_2()
            .mb_2()
            .p_2()
            .rounded(px(8.0))
            .bg(palette.selected)
            .border_1()
            .border_color(palette.line)
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(self.scaled(10.5))
                    .text_color(palette.muted)
                    .child("Spaces"),
            )
            .children(snap.spaces.iter().enumerate().map(|(idx, sp)| {
                let space_id = sp.id;
                let dot: gpui::Hsla = SpaceTint::from_space_color(sp.color)
                    .dot_color(self.model.is_dark())
                    .map_or(palette.accent, Into::into);
                div()
                    .id(("space-item", idx))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.0))
                    .bg(if sp.active {
                        palette.hover
                    } else {
                        palette.selected
                    })
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |shell, _, _, cx| {
                            shell.model.toggle_space_menu();
                            shell.dispatch_command(AppCommand::SwitchSpace { space: space_id }, cx);
                            shell.sync_address_field_after_tab_action(cx);
                        }),
                    )
                    .child(div().size(px(metrics::SPACE_DOT)).rounded_full().bg(dot))
                    .child(
                        div()
                            .flex_1()
                            .text_size(self.scaled(11.5))
                            .text_color(palette.text)
                            .child(sp.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(self.scaled(10.0))
                            .text_color(palette.muted)
                            .child(format!("{}t", sp.tab_count)),
                    )
            }))
            .child(
                div()
                    .id("create-space-btn")
                    .px_2()
                    .py_1()
                    .rounded(px(6.0))
                    .text_size(self.scaled(11.0))
                    .text_color(palette.accent)
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover))
                    .child("+ New Space")
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|shell, _, _, cx| {
                            let count = shell.model.snapshot().spaces.len() + 1;
                            shell.model.toggle_space_menu();
                            shell.dispatch_command(
                                AppCommand::CreateSpace {
                                    profile: None,
                                    name: format!("Space {count}"),
                                    color: SpaceColor::Violet,
                                    icon: "sparkles".into(),
                                    store_policy: searlx_core::SpaceStorePolicy::SharedWithFirst,
                                },
                                cx,
                            );
                        }),
                    ),
            )
    }

    fn render_sidebar_footer(&self, icons_only: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .py_1p5()
            .border_t_1()
            .border_color(palette.line)
            .when(!icons_only, |foot| {
                foot.child(
                    div()
                        .text_size(self.scaled(10.5))
                        .text_color(palette.muted)
                        .child(format!(
                            "{} tab{}",
                            snap.tabs.len(),
                            if snap.tabs.len() == 1 { "" } else { "s" }
                        )),
                )
                .child(div().flex_1())
            })
            .when(snap.downloads.show_button && !icons_only, |foot| {
                foot.child(
                    div()
                        .id("footer-downloads")
                        .px_1p5()
                        .py_0p5()
                        .rounded(px(5.0))
                        .text_size(self.scaled(10.5))
                        .text_color(palette.accent)
                        .cursor_pointer()
                        .hover(move |s| s.bg(palette.hover))
                        .child(format!("↓ {}", snap.downloads.active_count))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|shell, _, _, cx| {
                                shell.dispatch_command(
                                    AppCommand::OpenInternalPage {
                                        kind: InternalPageKind::Downloads,
                                        detail: None,
                                    },
                                    cx,
                                );
                                shell.sync_address_field_after_tab_action(cx);
                            }),
                        ),
                )
            })
            .child(
                div()
                    .id("footer-settings")
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(5.0))
                    .text_size(self.scaled(11.0))
                    .text_color(palette.muted)
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover).text_color(palette.text))
                    .child("⚙")
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|shell, _, _, cx| {
                            shell.dispatch_command(
                                AppCommand::OpenInternalPage {
                                    kind: InternalPageKind::Settings,
                                    detail: None,
                                },
                                cx,
                            );
                            shell.sync_address_field_after_tab_action(cx);
                        }),
                    ),
            )
    }

    fn render_top_tabs_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        let active_space = snap.spaces.iter().find(|s| s.active);
        let dot_color: gpui::Hsla = active_space
            .and_then(|s| SpaceTint::from_space_color(s.color).dot_color(self.model.is_dark()))
            .map_or(palette.accent, Into::into);

        let mut top_pills = Vec::with_capacity(snap.tabs.len());
        for tab in &snap.tabs {
            top_pills.push(self.render_top_tab_pill(tab, cx));
        }

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .h(px(metrics::TOP_BAR_HEIGHT))
            .w_full()
            .px_2()
            .bg(self.chrome_bg())
            .border_b_1()
            .border_color(palette.line)
            .child(
                div()
                    .id("top-space-dot")
                    .px_1p5()
                    .py_1()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|shell, _, _, cx| {
                            shell.model.toggle_space_menu();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .size(px(metrics::SPACE_DOT))
                            .rounded_full()
                            .bg(dot_color),
                    ),
            )
            .child(
                div()
                    .id("top-tabs-scroll")
                    .flex()
                    .flex_row()
                    .flex_1()
                    .items_center()
                    .overflow_x_scroll()
                    .gap_1()
                    .children(top_pills),
            )
            .when_some(self.render_site_chip(cx), |strip, chip| strip.child(chip))
            .child(
                div()
                    .id("top-new-tab")
                    .px_2()
                    .py_0p5()
                    .rounded(px(5.0))
                    .text_size(self.scaled(14.0))
                    .text_color(palette.muted)
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover).text_color(palette.text))
                    .child("+")
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|shell, _, window, cx| {
                            shell.model.open_new_tab();
                            shell.open_overlay_and_focus(AddressOverlayMode::OverPage, window, cx);
                        }),
                    ),
            )
    }

    fn render_top_tab_pill(
        &self,
        tab: &UiTabItem,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let palette = self.model.palette();
        let tab_id = tab.id;
        div()
            .id(("top-tab", tab_id.get() as usize))
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .h(px(metrics::TAB_ROW_HEIGHT - 2.0))
            .max_w(px(180.0))
            .px_2()
            .rounded(px(6.0))
            .bg(if tab.active {
                palette.selected
            } else {
                self.chrome_bg()
            })
            .opacity(if tab.sleeping { 0.55 } else { 1.0 })
            .cursor_pointer()
            .hover(move |s| s.bg(palette.hover))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |shell, _, _, cx| {
                    shell.model.click_tab(tab_id);
                    shell.sync_address_field_after_tab_action(cx);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .text_size(self.scaled(11.5))
                    .text_color(if tab.active {
                        palette.text
                    } else {
                        palette.muted
                    })
                    .truncate()
                    .child(tab.title.clone()),
            )
            .child(
                div()
                    .id(("top-close", tab_id.get() as usize))
                    .text_size(self.scaled(11.0))
                    .text_color(palette.muted)
                    .hover(move |s| s.text_color(palette.text))
                    .child(if tab.pinned { "−" } else { "×" })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |shell, _, _, cx| {
                            cx.stop_propagation();
                            shell.model.close_or_put_down_tab(tab_id);
                            shell.sync_address_field_after_tab_action(cx);
                            cx.notify();
                        }),
                    ),
            )
    }

    fn render_bookmarks_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px(px(metrics::PAGE_GAP))
            .pb_1()
            .children(
                snap.bookmarks_bar
                    .iter()
                    .take(12)
                    .enumerate()
                    .map(|(i, item)| match item {
                        BookmarkItem::Link { title, url, .. } => {
                            let target = url.clone();
                            div()
                                .id(("bm-bar", i))
                                .px_2()
                                .py_0p5()
                                .rounded(px(5.0))
                                .text_size(self.scaled(11.0))
                                .text_color(palette.muted)
                                .cursor_pointer()
                                .hover(move |s| s.bg(palette.hover).text_color(palette.text))
                                .child(title.clone())
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(move |shell, _, _, cx| {
                                        if let Some(active) = shell.model.snapshot().active_tab {
                                            shell.dispatch_command(
                                                AppCommand::NavigateTab {
                                                    tab: active,
                                                    url: target.clone(),
                                                },
                                                cx,
                                            );
                                            shell.sync_address_field_after_tab_action(cx);
                                        }
                                    }),
                                )
                        }
                        BookmarkItem::Folder { title, .. } => div()
                            .id(("bm-bar-folder", i))
                            .px_2()
                            .py_0p5()
                            .rounded(px(5.0))
                            .text_size(self.scaled(11.0))
                            .text_color(palette.muted)
                            .child(format!("▸ {title}")),
                    }),
            )
    }

    fn render_inline_address_line(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        let width = metrics::address_line_width(snap.settings.address_width);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px(px(metrics::PAGE_GAP))
            .pb_1p5()
            .child(
                div()
                    .id("inline-address-line")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .w(px(width))
                    .h(px(metrics::FIELD_HEIGHT))
                    .px_2p5()
                    .rounded(px(7.0))
                    .bg(palette.selected)
                    .border_1()
                    .border_color(palette.line)
                    .cursor_text()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|shell, _, window, cx| {
                            shell.open_overlay_and_focus(AddressOverlayMode::OverPage, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(self.scaled(12.0))
                            .text_color(if snap.address.current_url_display.is_empty() {
                                palette.muted
                            } else {
                                palette.text
                            })
                            .child(if snap.address.current_url_display.is_empty() {
                                "Search or enter address".to_owned()
                            } else {
                                snap.address.current_url_display.clone()
                            }),
                    )
                    .when_some(self.render_site_chip(cx), |bar, chip| bar.child(chip)),
            )
    }

    fn render_page_stage(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        let use_card = snap.settings.layout_cards;
        let gap = if use_card { metrics::PAGE_GAP } else { 0.0 };
        let radius = if use_card { metrics::PAGE_RADIUS } else { 0.0 };

        let mut stage = div()
            .relative()
            .flex_1()
            .mx(px(gap))
            .mb(px(gap))
            .rounded(px(radius))
            .bg(palette.page)
            .border_1()
            .border_color(palette.line)
            .overflow_hidden();

        if snap.split_panes.is_empty() {
            stage = stage.child(self.render_single_page_surface(cx));
        } else {
            stage = stage.child(self.render_split_panes(&snap.split_panes, cx));
        }

        if let Some(find) = &snap.find {
            stage = stage.child(self.render_find_pill(find, cx));
        }
        if self.model.is_site_card_open()
            && let Some(card) = &snap.site_card
        {
            stage = stage.child(self.render_site_card_popover(card, cx));
        }
        if self.model.is_address_overlay_open() {
            stage = stage.child(self.render_address_overlay(cx));
        }
        if let Some(banner) = snap
            .status_banner
            .as_deref()
            .or_else(|| self.model.local_notice())
        {
            stage = stage.child(self.render_status_banner(banner));
        }
        if snap.sidebar_folded {
            stage = stage.child(
                div()
                    .id("unfold-sidebar-pill")
                    .absolute()
                    .top(px(12.0))
                    .left(px(12.0))
                    .px_2()
                    .py_1()
                    .rounded(px(6.0))
                    .bg(palette.chrome)
                    .border_1()
                    .border_color(palette.line)
                    .text_size(self.scaled(11.0))
                    .text_color(palette.muted)
                    .cursor_pointer()
                    .hover(move |s| s.bg(palette.hover).text_color(palette.text))
                    .child("Show Tabs")
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|shell, _, _, cx| {
                            shell.model.toggle_sidebar_fold();
                            cx.notify();
                        }),
                    ),
            );
        }
        stage
    }

    fn render_single_page_surface(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.model.snapshot();
        if let Some(internal) = &snap.active_internal_page {
            return self.render_internal_page(internal, cx).into_any_element();
        }
        if let Some(article) = &snap.active_reader_article {
            return self.render_reader_surface(article, cx).into_any_element();
        }
        let active = snap.tabs.iter().find(|t| t.active);
        self.render_tab_placeholder_surface(active, cx)
            .into_any_element()
    }

    fn render_split_panes(
        &self,
        panes: &[UiSplitPaneItem],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .bg(palette.chrome)
                    .border_b_1()
                    .border_color(palette.line)
                    .child(
                        div()
                            .text_size(self.scaled(11.0))
                            .text_color(palette.muted)
                            .child(format!("Split View · {} panes", panes.len())),
                    )
                    .child(div().flex_1())
                    .child(small_action_pill(
                        "Swap",
                        "split-swap",
                        palette,
                        cx.listener(|shell, _, _, cx| {
                            shell.dispatch_command(AppCommand::SwapSplitPanes, cx);
                        }),
                    ))
                    .child(small_action_pill(
                        "Even",
                        "split-even",
                        palette,
                        cx.listener(|shell, _, _, cx| {
                            shell.dispatch_command(AppCommand::EvenSplitPanes, cx);
                        }),
                    ))
                    .child(small_action_pill(
                        "Separate",
                        "split-sep",
                        palette,
                        cx.listener(|shell, _, _, cx| {
                            shell.dispatch_command(
                                AppCommand::ToggleSplitView {
                                    target_tab: None,
                                    partner_tab: None,
                                },
                                cx,
                            );
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .size_full()
                    .bg(palette.ground)
                    .children(panes.iter().enumerate().map(|(idx, pane)| {
                        let tab_id = pane.tab;
                        let is_active_internal =
                            snap.active_tab == Some(tab_id) && snap.active_internal_page.is_some();
                        let tab_meta = snap.tabs.iter().find(|t| t.id == tab_id);

                        let mut wrapper = div()
                            .id(("split-pane", tab_id.get() as usize))
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .bg(palette.page)
                            .border_2()
                            .border_color(if pane.focused {
                                palette.accent
                            } else {
                                palette.line
                            })
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |shell, _, _, cx| {
                                    shell.dispatch_command(
                                        AppCommand::ActivateTab { tab: tab_id },
                                        cx,
                                    );
                                    shell.sync_address_field_after_tab_action(cx);
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .py_1()
                                    .bg(palette.chrome)
                                    .border_b_1()
                                    .border_color(palette.line)
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .text_size(self.scaled(11.0))
                                            .text_color(if pane.focused {
                                                palette.text
                                            } else {
                                                palette.muted
                                            })
                                            .child(format!(
                                                "{} · {}",
                                                pane.title,
                                                pane.url.display()
                                            )),
                                    )
                                    .child(
                                        div()
                                            .id(("split-pane-close", tab_id.get() as usize))
                                            .px_1()
                                            .text_size(self.scaled(11.0))
                                            .text_color(palette.muted)
                                            .cursor_pointer()
                                            .hover(move |s| s.text_color(palette.text))
                                            .child("×")
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation();
                                            })
                                            .on_mouse_up(
                                                MouseButton::Left,
                                                cx.listener(move |shell, _, _, cx| {
                                                    cx.stop_propagation();
                                                    shell.dispatch_command(
                                                        AppCommand::RemoveSplitPane { tab: tab_id },
                                                        cx,
                                                    );
                                                }),
                                            ),
                                    ),
                            );

                        if is_active_internal && let Some(internal) = &snap.active_internal_page {
                            wrapper = wrapper.child(self.render_internal_page(internal, cx));
                        } else {
                            wrapper =
                                wrapper.child(self.render_tab_placeholder_surface(tab_meta, cx));
                        }

                        if idx + 1 < panes.len() {
                            div()
                                .flex()
                                .flex_row()
                                .flex_1()
                                .h_full()
                                .child(wrapper)
                                .child(
                                    div()
                                        .id(("split-divider", idx))
                                        .w(px(metrics::SPLIT_GUTTER))
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .bg(palette.ground)
                                        .child(
                                            div()
                                                .w(px(metrics::SPLIT_DIVIDER))
                                                .h(px(32.0))
                                                .rounded_full()
                                                .bg(palette.line),
                                        ),
                                )
                        } else {
                            div().flex().flex_1().h_full().child(wrapper)
                        }
                    })),
            )
    }

    fn render_tab_placeholder_surface(
        &self,
        tab: Option<&UiTabItem>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = self.model.palette();
        let Some(tab) = tab else {
            return div().size_full();
        };
        let is_blank = matches!(
            tab.url.as_str(),
            "search://" | "search://newtab" | "about:blank"
        );
        if is_blank {
            return div()
                .flex()
                .flex_col()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_size(self.scaled(12.0))
                        .text_color(palette.muted)
                        .child("Press Ctrl+L to enter an address or search"),
                );
        }

        let tab_id = tab.id;
        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .px_6()
            .child(
                div()
                    .text_size(self.scaled(22.0))
                    .text_color(palette.text)
                    .child(tab.title.clone()),
            )
            .child(
                div()
                    .text_size(self.scaled(12.5))
                    .text_color(palette.muted)
                    .child(tab.display_url.clone()),
            )
            .when(tab.is_lookalike, |col| {
                col.child(badge_pill(
                    &format!(
                        "Lookalike host · {}",
                        self.model.snapshot().address.safe_host
                    ),
                    palette.danger,
                    palette.selected,
                ))
            })
            .when_some(tab.in_page_error.as_ref(), |col, err| {
                col.child(
                    div()
                        .mt_2()
                        .px_3()
                        .py_2()
                        .rounded(px(8.0))
                        .bg(palette.chrome)
                        .border_1()
                        .border_color(palette.danger)
                        .text_size(self.scaled(12.0))
                        .text_color(palette.danger)
                        .child(err.reason.clone()),
                )
            })
            .when_some(tab.crash_retry_message.as_ref(), |col, msg| {
                col.child(
                    div()
                        .mt_2()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .rounded(px(8.0))
                        .bg(palette.chrome)
                        .border_1()
                        .border_color(palette.danger)
                        .child(
                            div()
                                .text_size(self.scaled(12.0))
                                .text_color(palette.danger)
                                .child(msg.clone()),
                        )
                        .child(small_action_pill(
                            "Retry",
                            "retry-crash",
                            palette,
                            cx.listener(move |shell, _, _, cx| {
                                shell.dispatch_command(
                                    AppCommand::RetryCrashedTab { tab: Some(tab_id) },
                                    cx,
                                );
                            }),
                        )),
                )
            })
            .child(
                div()
                    .mt_2()
                    .text_size(self.scaled(11.0))
                    .text_color(palette.muted)
                    .child(format!(
                        "{}% zoom{}",
                        tab.zoom_percent,
                        if tab.loading { " · loading" } else { "" }
                    )),
            )
    }

    fn render_reader_surface(
        &self,
        article: &searlx_core::ReaderArticle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = self.model.palette();
        div()
            .id("reader-view")
            .flex()
            .flex_col()
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(self.scaled(11.0))
                            .text_color(palette.muted)
                            .child(format!(
                                "Reading Mode · {} chars{}",
                                article.text_len,
                                if article.used_markdown_twin {
                                    " · Markdown twin"
                                } else {
                                    ""
                                }
                            )),
                    )
                    .child(small_action_pill(
                        "Exit Reader",
                        "exit-reader",
                        palette,
                        cx.listener(|shell, _, _, cx| {
                            shell.dispatch_command(AppCommand::ToggleReaderMode { html: None }, cx);
                        }),
                    )),
            )
            .child(
                div()
                    .text_size(self.scaled(22.0))
                    .text_color(palette.text)
                    .child(article.title.clone()),
            )
            .child(
                div()
                    .text_size(self.scaled(12.5))
                    .text_color(palette.muted)
                    .child(article.source_url.display()),
            )
            .child(
                div()
                    .text_size(self.scaled(13.0))
                    .text_color(palette.text)
                    .child(article.content_html.clone()),
            )
    }

    fn render_internal_page(
        &self,
        data: &InternalPageData,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = self.model.palette();
        match data {
            InternalPageData::Settings {
                section,
                visible_sections,
                dusk_sites,
                saved_permissions_count,
                engine_name,
            } => self
                .render_internal_settings(
                    *section,
                    visible_sections,
                    dusk_sites.len(),
                    *saved_permissions_count,
                    engine_name,
                    cx,
                )
                .into_any_element(),
            InternalPageData::History {
                days,
                recently_closed,
            } => div()
                .id("internal-history")
                .flex()
                .flex_col()
                .size_full()
                .overflow_y_scroll()
                .p_6()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(self.scaled(20.0))
                                .text_color(palette.text)
                                .child("History"),
                        )
                        .child(small_action_pill(
                            "Clear History",
                            "clear-history",
                            palette,
                            cx.listener(|shell, _, _, cx| {
                                shell.dispatch_command(
                                    AppCommand::ClearHistory {
                                        scope: ClearHistoryScope::All,
                                    },
                                    cx,
                                );
                            }),
                        )),
                )
                .when(!recently_closed.is_empty(), |col| {
                    col.child(
                        div()
                            .text_size(self.scaled(11.5))
                            .text_color(palette.muted)
                            .child(format!("Recently closed: {}", recently_closed.len())),
                    )
                })
                .children(days.iter().map(|day| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_3()
                        .rounded(px(8.0))
                        .bg(palette.chrome)
                        .border_1()
                        .border_color(palette.line)
                        .child(
                            div()
                                .text_size(self.scaled(12.5))
                                .text_color(palette.text)
                                .child(format!("{} ({})", day.label, day.visits.len())),
                        )
                        .children(day.visits.iter().take(20).map(|v| {
                            div()
                                .text_size(self.scaled(11.5))
                                .text_color(palette.muted)
                                .child(format!("{} — {}", v.title, v.url.display()))
                        }))
                }))
                .into_any_element(),
            InternalPageData::Downloads { entries } => div()
                .id("internal-downloads")
                .flex()
                .flex_col()
                .size_full()
                .overflow_y_scroll()
                .p_6()
                .gap_3()
                .child(
                    div()
                        .text_size(self.scaled(20.0))
                        .text_color(palette.text)
                        .child(format!("Downloads ({})", entries.len())),
                )
                .children(entries.iter().map(|entry| {
                    div()
                        .px_3()
                        .py_2()
                        .rounded(px(8.0))
                        .bg(palette.chrome)
                        .border_1()
                        .border_color(palette.line)
                        .text_size(self.scaled(12.0))
                        .text_color(palette.text)
                        .child(format!(
                            "{} · {:?} · {} bytes",
                            entry.file_name, entry.state, entry.bytes_received
                        ))
                }))
                .into_any_element(),
            InternalPageData::Bookmarks { tree } => div()
                .id("internal-bookmarks")
                .flex()
                .flex_col()
                .size_full()
                .overflow_y_scroll()
                .p_6()
                .gap_3()
                .child(
                    div()
                        .text_size(self.scaled(20.0))
                        .text_color(palette.text)
                        .child(format!("Bookmarks ({})", tree.roots.len())),
                )
                .children(tree.roots.iter().enumerate().map(|(i, item)| match item {
                    BookmarkItem::Link { title, url, .. } => div()
                        .id(("bm-page", i))
                        .px_3()
                        .py_1p5()
                        .rounded(px(6.0))
                        .bg(palette.chrome)
                        .text_size(self.scaled(12.0))
                        .text_color(palette.text)
                        .child(format!("{title} — {}", url.display())),
                    BookmarkItem::Folder {
                        title, children, ..
                    } => div()
                        .id(("bm-folder", i))
                        .px_3()
                        .py_1p5()
                        .rounded(px(6.0))
                        .bg(palette.chrome)
                        .text_size(self.scaled(12.0))
                        .text_color(palette.muted)
                        .child(format!("Folder: {title} ({} items)", children.len())),
                }))
                .into_any_element(),
            InternalPageData::Passwords { accounts } => div()
                .id("internal-passwords")
                .flex()
                .flex_col()
                .size_full()
                .overflow_y_scroll()
                .p_6()
                .gap_3()
                .child(
                    div()
                        .text_size(self.scaled(20.0))
                        .text_color(palette.text)
                        .child(format!("Passwords ({})", accounts.len())),
                )
                .children(accounts.iter().map(|(host, user, last_used, clear_http)| {
                    div()
                        .px_3()
                        .py_2()
                        .rounded(px(8.0))
                        .bg(palette.chrome)
                        .border_1()
                        .border_color(palette.line)
                        .text_size(self.scaled(12.0))
                        .text_color(palette.text)
                        .child(format!(
                            "{host} · {user} · {last_used}{}",
                            if *clear_http { " · http" } else { "" }
                        ))
                }))
                .into_any_element(),
            InternalPageData::Archive { archived_tabs } => div()
                .id("internal-archive")
                .flex()
                .flex_col()
                .size_full()
                .overflow_y_scroll()
                .p_6()
                .gap_2()
                .child(
                    div()
                        .text_size(self.scaled(20.0))
                        .text_color(palette.text)
                        .child(format!("Archived Tabs ({})", archived_tabs.len())),
                )
                .children(archived_tabs.iter().map(|t| {
                    div()
                        .px_3()
                        .py_1p5()
                        .rounded(px(6.0))
                        .bg(palette.chrome)
                        .text_size(self.scaled(12.0))
                        .text_color(palette.text)
                        .child(format!("{} — {}", t.title, t.url))
                }))
                .into_any_element(),
            InternalPageData::Import { source } => div()
                .flex()
                .flex_col()
                .size_full()
                .p_6()
                .gap_2()
                .child(
                    div()
                        .text_size(self.scaled(20.0))
                        .text_color(palette.text)
                        .child("Bring Things Over"),
                )
                .child(
                    div()
                        .text_size(self.scaled(12.5))
                        .text_color(palette.muted)
                        .child(source.clone().unwrap_or_else(|| {
                            "Import bookmarks, history, and settings from Chrome, Safari, Firefox, or Arc."
                                .to_owned()
                        })),
                )
                .into_any_element(),
            InternalPageData::ReleaseNotes { version } => div()
                .flex()
                .flex_col()
                .size_full()
                .p_6()
                .gap_2()
                .child(
                    div()
                        .text_size(self.scaled(20.0))
                        .text_color(palette.text)
                        .child(format!("What's New · {version}")),
                )
                .into_any_element(),
        }
    }

    fn render_internal_settings(
        &self,
        section: SettingsPage,
        visible_sections: &[SettingsPage],
        dusk_site_count: usize,
        saved_permissions_count: usize,
        engine_name: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let snap = self.model.snapshot();
        let palette = self.model.palette();
        let settings = &snap.settings;

        div()
            .flex()
            .flex_row()
            .size_full()
            .child(
                div()
                    .w(px(190.0))
                    .h_full()
                    .p_3()
                    .border_r_1()
                    .border_color(palette.line)
                    .bg(palette.chrome)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(visible_sections.iter().enumerate().map(|(idx, &sec)| {
                        let active = sec == section;
                        div()
                            .id(("settings-sec", idx))
                            .px_2p5()
                            .py_1p5()
                            .rounded(px(6.0))
                            .bg(if active {
                                palette.selected
                            } else {
                                palette.chrome
                            })
                            .text_size(self.scaled(12.0))
                            .text_color(if active {
                                palette.text
                            } else {
                                palette.muted
                            })
                            .cursor_pointer()
                            .hover(move |s| s.bg(palette.hover))
                            .child(sec.title())
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |shell, _, _, cx| {
                                    shell.dispatch_command(
                                        AppCommand::OpenInternalPage {
                                            kind: InternalPageKind::Settings,
                                            detail: Some(sec.slug().to_owned()),
                                        },
                                        cx,
                                    );
                                    shell.sync_address_field_after_tab_action(cx);
                                }),
                            )
                    })),
            )
            .child(
                div()
                    .id("settings-body")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .p_6()
                    .gap_3()
                    .child(
                        div()
                            .text_size(self.scaled(20.0))
                            .text_color(palette.text)
                            .child(section.title()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_2()
                            .child(setting_choice_pill(
                                "System",
                                settings.look == LookPreference::System,
                                "look-system",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.set_look_preference(LookPreference::System);
                                    shell.sync_field_palettes(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(setting_choice_pill(
                                "Light",
                                settings.look == LookPreference::Light,
                                "look-light",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.set_look_preference(LookPreference::Light);
                                    shell.sync_field_palettes(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(setting_choice_pill(
                                "Dark",
                                settings.look == LookPreference::Dark,
                                "look-dark",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.set_look_preference(LookPreference::Dark);
                                    shell.sync_field_palettes(cx);
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_2()
                            .child(setting_choice_pill(
                                "Left Column",
                                settings.tab_layout == TabLayoutMode::LeftColumn,
                                "layout-left",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.update_settings(|s| {
                                        s.tab_layout = TabLayoutMode::LeftColumn;
                                    });
                                    cx.notify();
                                }),
                            ))
                            .child(setting_choice_pill(
                                "Right Column",
                                settings.tab_layout == TabLayoutMode::RightColumn,
                                "layout-right",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.update_settings(|s| {
                                        s.tab_layout = TabLayoutMode::RightColumn;
                                    });
                                    cx.notify();
                                }),
                            ))
                            .child(setting_choice_pill(
                                "Top Tabs",
                                settings.tab_layout == TabLayoutMode::TopTabs,
                                "layout-top",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.update_settings(|s| {
                                        s.tab_layout = TabLayoutMode::TopTabs;
                                    });
                                    cx.notify();
                                }),
                            ))
                            .child(setting_choice_pill(
                                "Icons Only",
                                settings.tab_layout == TabLayoutMode::IconsOnly,
                                "layout-icons",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.update_settings(|s| {
                                        s.tab_layout = TabLayoutMode::IconsOnly;
                                    });
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_2()
                            .child(setting_choice_pill(
                                "High Contrast",
                                settings.tabs_high_contrast,
                                "toggle-hc",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    let next = !shell.model.high_contrast();
                                    shell.model.set_high_contrast(next);
                                    shell.sync_field_palettes(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(setting_choice_pill(
                                "Reduce Motion",
                                settings.reduce_motion,
                                "toggle-rm",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    let next = !shell.model.reduce_motion();
                                    shell.model.set_reduce_motion(next);
                                    cx.notify();
                                }),
                            ))
                            .child(setting_choice_pill(
                                "Always Show Address",
                                settings.address_shown,
                                "toggle-addr",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.update_settings(|s| {
                                        s.address_shown = !s.address_shown;
                                    });
                                    cx.notify();
                                }),
                            ))
                            .child(setting_choice_pill(
                                "Bookmarks Bar",
                                settings.bookmarks_bar,
                                "toggle-bm-bar",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.update_settings(|s| {
                                        s.bookmarks_bar = !s.bookmarks_bar;
                                    });
                                    cx.notify();
                                }),
                            ))
                            .child(setting_choice_pill(
                                "Dusk Darkening",
                                settings.pages_dusk,
                                "toggle-dusk-setting",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.update_settings(|s| {
                                        s.pages_dusk = !s.pages_dusk;
                                    });
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_size(self.scaled(11.5))
                            .text_color(palette.muted)
                            .child(format!(
                                "Engine: {engine_name} · Shield: {} · Saved permissions: {saved_permissions_count} · Dusk sites: {dusk_site_count}",
                                if settings.shield { "On" } else { "Off" }
                            )),
                    ),
            )
    }

    fn render_address_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.model.snapshot();
        let addr = &snap.address;
        let palette = self.model.palette();
        let shake_dx = self.model.shake_offset_px();
        let has_shake = addr.shake_reason.is_some();

        div()
            .id("address-overlay-backdrop")
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size_full()
            .bg(gpui::hsla(0.0, 0.0, 0.0, 0.24))
            .flex()
            .flex_col()
            .items_center()
            .pt(px(metrics::OVERLAY_TOP_OFFSET))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, _, window, cx| {
                    shell.model.close_address_overlay();
                    window.focus(&shell.focus_handle);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("address-overlay-card")
                    .w(px(metrics::FIELD_WIDTH))
                    .ml(px(shake_dx))
                    .rounded(px(12.0))
                    .bg(palette.chrome)
                    .border_2()
                    .border_color(if has_shake {
                        palette.danger
                    } else {
                        focus_ring(&palette)
                    })
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.render_overlay_input_row(addr, cx))
                    .when_some(addr.shake_reason.as_ref(), |card, reason| {
                        card.child(
                            div()
                                .px_2()
                                .py_1()
                                .rounded(px(6.0))
                                .bg(palette.hover)
                                .text_size(self.scaled(11.5))
                                .text_color(palette.danger)
                                .child(format_invalid_reason(reason)),
                        )
                    })
                    .when(!addr.suggestions.is_empty(), |card| {
                        card.child(self.render_suggestion_list(&addr.suggestions, cx))
                    }),
            )
    }

    fn render_overlay_input_row(
        &self,
        addr: &UiAddressBarState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = self.model.palette();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(metrics::FIELD_HEIGHT + 6.0))
            .px_2p5()
            .rounded(px(8.0))
            .bg(palette.selected)
            .border_1()
            .border_color(palette.line)
            .when_some(addr.site_search_chip.as_ref(), |row, site| {
                row.child(
                    div()
                        .id("site-search-chip")
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_0p5()
                        .rounded(px(6.0))
                        .bg(palette.accent)
                        .text_size(self.scaled(11.0))
                        .text_color(palette.selected)
                        .cursor_pointer()
                        .child(format!("Search {} ×", site.name))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|shell, _, _, cx| {
                                shell.model.clear_site_search_chip();
                                cx.notify();
                            }),
                        ),
                )
            })
            .child(div().flex_1().child(self.address_field.clone()))
            .when(addr.is_lookalike, |row| {
                row.child(badge_pill(
                    &format!("Punycode: {}", addr.safe_host),
                    palette.danger,
                    palette.selected,
                ))
            })
    }

    fn render_suggestion_list(
        &self,
        suggestions: &[SuggestionRow],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = self.model.palette();
        let selected = self.model.selected_suggestion();
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .border_t_1()
            .border_color(palette.line)
            .pt_1()
            .children(
                suggestions
                    .iter()
                    .take(metrics::OVERLAY_MAX_SUGGESTIONS)
                    .enumerate()
                    .map(|(idx, row)| {
                        let is_selected = selected == Some(idx);
                        let (kind_tag, title, detail) = format_suggestion_row(row);
                        div()
                            .id(("suggestion-row", idx))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .py_1()
                            .rounded(px(6.0))
                            .bg(if is_selected {
                                palette.selected
                            } else {
                                palette.chrome
                            })
                            .cursor_pointer()
                            .hover(move |s| s.bg(palette.hover))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |shell, _, _, cx| {
                                    cx.stop_propagation();
                                    let changed =
                                        shell.model.activate_suggestion(idx, false, false);
                                    let dirs = shell.model.last_directives().to_vec();
                                    shell.apply_directives_to_gpui(&dirs, cx);
                                    if changed {
                                        shell.sync_address_field_after_tab_action(cx);
                                    } else {
                                        let text = shell.model.snapshot().address.text.clone();
                                        shell.address_field.update(cx, |field, cx| {
                                            field.set_text(text, cx);
                                        });
                                    }
                                    cx.notify();
                                }),
                            )
                            .child(badge_pill(kind_tag, palette.hover, palette.muted))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(self.scaled(12.0))
                                    .text_color(palette.text)
                                    .child(title),
                            )
                            .when_some(detail, |item, sub| {
                                item.child(
                                    div()
                                        .max_w(px(220.0))
                                        .truncate()
                                        .text_size(self.scaled(11.0))
                                        .text_color(palette.muted)
                                        .child(sub),
                                )
                            })
                    }),
            )
    }

    fn render_site_card_popover(
        &self,
        card: &searlx_core::SiteCardModel,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = self.model.palette();
        let conn_label = match card.connection {
            ConnectionState::Secure => "Secure connection",
            ConnectionState::MixedContent => "Mixed content",
            ConnectionState::NotSecure => "Not secure (HTTP)",
            ConnectionState::UntrustedCertificate => "Untrusted certificate",
            ConnectionState::LocalOrInternal => "Local / Internal page",
        };
        div()
            .id("site-card-popover")
            .absolute()
            .top(px(12.0))
            .left(px(12.0))
            .w(px(metrics::SITE_CARD_WIDTH))
            .p_3()
            .rounded(px(10.0))
            .bg(palette.chrome)
            .border_1()
            .border_color(palette.line)
            .flex()
            .flex_col()
            .gap_2()
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(self.scaled(14.0))
                            .text_color(palette.text)
                            .child(card.header.clone()),
                    )
                    .child(
                        div()
                            .id("site-card-close")
                            .px_1()
                            .text_size(self.scaled(12.0))
                            .text_color(palette.muted)
                            .cursor_pointer()
                            .hover(move |s| s.text_color(palette.text))
                            .child("×")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|shell, _, _, cx| {
                                    shell.model.close_site_card();
                                    cx.notify();
                                }),
                            ),
                    ),
            )
            .when(card.is_lookalike, |col| {
                col.child(badge_pill(
                    "Lookalike IDN host — punycode shown",
                    palette.danger,
                    palette.selected,
                ))
            })
            .child(
                div()
                    .text_size(self.scaled(11.5))
                    .text_color(
                        if matches!(
                            card.connection,
                            ConnectionState::NotSecure | ConnectionState::UntrustedCertificate
                        ) {
                            palette.danger
                        } else {
                            palette.muted
                        },
                    )
                    .child(conn_label),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(small_action_pill(
                        &format!("Copy Address ({})", card.copy_address_shortcut),
                        "site-copy-address",
                        palette,
                        cx.listener(|shell, _, _, cx| {
                            shell.dispatch_command(AppCommand::CopyAddress { tab: None }, cx);
                        }),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(small_action_pill(
                                "−",
                                "zoom-out-btn",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.dispatch_command(
                                        AppCommand::ZoomStep { delta_steps: -1 },
                                        cx,
                                    );
                                }),
                            ))
                            .child(small_action_pill(
                                &format!("{}%", card.zoom_percent),
                                "zoom-reset-btn",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.dispatch_command(AppCommand::ZoomReset, cx);
                                }),
                            ))
                            .child(small_action_pill(
                                "+",
                                "zoom-in-btn",
                                palette,
                                cx.listener(|shell, _, _, cx| {
                                    shell.dispatch_command(
                                        AppCommand::ZoomStep { delta_steps: 1 },
                                        cx,
                                    );
                                }),
                            )),
                    ),
            )
            .when_some(card.block_ads_and_trackers, |col, enabled| {
                col.child(site_card_toggle_row(
                    "Block Ads & Trackers",
                    enabled,
                    "site-blocker-toggle",
                    palette,
                    cx.listener(|shell, _, _, cx| {
                        shell.dispatch_command(
                            AppCommand::ToggleSiteBlocker { site_host: None },
                            cx,
                        );
                    }),
                ))
            })
            .when_some(card.darken_this_site, |col, enabled| {
                col.child(site_card_toggle_row(
                    &format!("Darken This Site ({})", card.darken_shortcut),
                    enabled,
                    "site-darken-toggle",
                    palette,
                    cx.listener(|shell, _, _, cx| {
                        shell.dispatch_command(AppCommand::ToggleDuskForSite, cx);
                    }),
                ))
            })
            .when_some(card.keep_this_site_awake, |col, enabled| {
                col.child(site_card_toggle_row(
                    "Keep This Site Awake",
                    enabled,
                    "site-keep-awake",
                    palette,
                    cx.listener(|shell, _, _, cx| {
                        shell.dispatch_command(AppCommand::ToggleKeepAwake, cx);
                    }),
                ))
            })
            .when_some(card.permissions_notice.as_ref(), |col, notice| {
                col.child(
                    div()
                        .text_size(self.scaled(11.0))
                        .text_color(palette.muted)
                        .child(notice.clone()),
                )
            })
    }

    fn render_find_pill(&self, find: &UiFindState, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.model.palette();
        div()
            .id("find-pill")
            .absolute()
            .top(px(12.0))
            .right(px(12.0))
            .w(px(metrics::FIND_PILL_WIDTH))
            .px_2p5()
            .py_1p5()
            .rounded(px(9.0))
            .bg(palette.chrome)
            .border_1()
            .border_color(palette.line)
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .child(
                div()
                    .flex_1()
                    .h(px(24.0))
                    .px_2()
                    .rounded(px(5.0))
                    .bg(palette.selected)
                    .border_1()
                    .border_color(palette.line)
                    .flex()
                    .items_center()
                    .child(self.find_field.clone()),
            )
            .child(
                div()
                    .text_size(self.scaled(10.5))
                    .text_color(palette.muted)
                    .child(find.status_text.clone()),
            )
            .child(setting_choice_pill(
                "Aa",
                find.case_sensitive,
                "find-case",
                palette,
                cx.listener(|shell, _, _, cx| {
                    shell.model.toggle_find_case();
                    cx.notify();
                }),
            ))
            .child(small_action_pill(
                "‹",
                "find-prev",
                palette,
                cx.listener(|shell, _, _, cx| {
                    shell.dispatch_command(AppCommand::FindPrevious, cx);
                }),
            ))
            .child(small_action_pill(
                "›",
                "find-next",
                palette,
                cx.listener(|shell, _, _, cx| {
                    shell.dispatch_command(AppCommand::FindNext, cx);
                }),
            ))
            .child(small_action_pill(
                "×",
                "find-close",
                palette,
                cx.listener(|shell, _, window, cx| {
                    shell.model.close_find_bar();
                    window.focus(&shell.focus_handle);
                    cx.notify();
                }),
            ))
    }

    fn render_status_banner(&self, text: &str) -> impl IntoElement {
        let palette = self.model.palette();
        div()
            .id("status-banner")
            .absolute()
            .bottom(px(12.0))
            .left(px(12.0))
            .px_3()
            .py_1()
            .rounded(px(6.0))
            .bg(palette.chrome)
            .border_1()
            .border_color(palette.line)
            .text_size(self.scaled(11.0))
            .text_color(palette.text)
            .child(text.to_owned())
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.model.set_system_dark(is_dark(window.appearance()));
        let bounds = window.bounds();
        let stage_width = f32::from(bounds.size.width);
        if (stage_width - self.last_stage_width_px).abs() > 2.0 && stage_width > 0.0 {
            self.last_stage_width_px = stage_width;
            self.model.set_stage_width_pt(f64::from(stage_width));
        }

        let snap = self.model.snapshot();
        let palette = self.model.palette();
        let layout = snap.sidebar_layout;
        let folded = snap.sidebar_folded;
        let show_sidebar = !folded && layout != TabLayoutMode::TopTabs;
        let show_top_tabs = !folded && layout == TabLayoutMode::TopTabs;
        let show_bookmarks = snap.settings.bookmarks_bar && !snap.bookmarks_bar.is_empty();
        let show_inline_address = snap.settings.address_shown;

        let content_area = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(div().id("title-drag").h(px(8.0)).w_full().on_mouse_down(
                MouseButton::Left,
                |_, window, _| {
                    window.start_window_move();
                },
            ))
            .when(show_top_tabs, |col| {
                col.child(self.render_top_tabs_strip(cx))
            })
            .when(show_bookmarks, |col| {
                col.child(self.render_bookmarks_bar(cx))
            })
            .when(show_inline_address, |col| {
                col.child(self.render_inline_address_line(cx))
            })
            .child(self.render_page_stage(cx));

        let mut root = div()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_row()
            .size_full()
            .bg(palette.ground)
            .text_color(palette.text)
            .font_family("Inter");

        if show_sidebar && layout == TabLayoutMode::RightColumn {
            root = root.child(content_area).child(self.render_sidebar(cx));
        } else if show_sidebar {
            root = root.child(self.render_sidebar(cx)).child(content_area);
        } else {
            root = root.child(content_area);
        }

        root.on_action(cx.listener(Self::on_new_tab))
            .on_action(cx.listener(Self::on_new_private_tab))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_reopen_closed_tab))
            .on_action(cx.listener(Self::on_focus_address))
            .on_action(cx.listener(Self::on_search_tabs))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_previous_tab))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_cycle_tab_layout))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_pin_active_tab))
            .on_action(cx.listener(Self::on_duplicate_tab))
            .on_action(cx.listener(Self::on_reload_page))
            .on_action(cx.listener(Self::on_go_back))
            .on_action(cx.listener(Self::on_go_forward))
            .on_action(cx.listener(Self::on_open_find))
            .on_action(cx.listener(Self::on_find_next))
            .on_action(cx.listener(Self::on_find_previous))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::on_toggle_dusk))
            .on_action(cx.listener(Self::on_copy_address))
            .on_action(cx.listener(Self::on_toggle_split_view))
            .on_action(cx.listener(Self::on_split_focus_prev))
            .on_action(cx.listener(Self::on_split_focus_next))
            .on_action(cx.listener(Self::on_dismiss_overlay))
            .on_action(cx.listener(Self::on_quit))
    }
}

// ----------------------------------------------------------------------------
// UI component helpers
// ----------------------------------------------------------------------------

fn badge_pill(label: &str, bg: gpui::Hsla, fg: gpui::Hsla) -> gpui::Div {
    div()
        .px_1p5()
        .py_0p5()
        .rounded(px(4.0))
        .bg(bg)
        .text_size(px(9.5))
        .text_color(fg)
        .child(label.to_owned())
}

fn small_action_pill(
    label: &str,
    id: &'static str,
    palette: Palette,
    on_click: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded(px(5.0))
        .bg(palette.selected)
        .border_1()
        .border_color(palette.line)
        .text_size(px(11.0))
        .text_color(palette.text)
        .cursor_pointer()
        .hover(move |s| s.bg(palette.hover))
        .child(label.to_owned())
        .on_mouse_up(MouseButton::Left, on_click)
}

fn setting_choice_pill(
    label: &str,
    active: bool,
    id: &'static str,
    palette: Palette,
    on_click: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2p5()
        .py_1()
        .rounded(px(6.0))
        .bg(if active {
            palette.accent
        } else {
            palette.chrome
        })
        .border_1()
        .border_color(if active { palette.accent } else { palette.line })
        .text_size(px(11.5))
        .text_color(if active {
            palette.selected
        } else {
            palette.text
        })
        .cursor_pointer()
        .hover(move |s| s.opacity(0.9))
        .child(label.to_owned())
        .on_mouse_up(MouseButton::Left, on_click)
}

fn site_card_toggle_row(
    label: &str,
    enabled: bool,
    id: &'static str,
    palette: Palette,
    on_click: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px_2()
        .py_1p5()
        .rounded(px(6.0))
        .bg(palette.selected)
        .border_1()
        .border_color(palette.line)
        .cursor_pointer()
        .hover(move |s| s.bg(palette.hover))
        .on_mouse_up(MouseButton::Left, on_click)
        .child(
            div()
                .text_size(px(12.0))
                .text_color(palette.text)
                .child(label.to_owned()),
        )
        .child(badge_pill(
            if enabled { "On" } else { "Off" },
            if enabled {
                palette.accent
            } else {
                palette.hover
            },
            if enabled {
                palette.selected
            } else {
                palette.muted
            },
        ))
}

fn format_invalid_reason(reason: &AddressInvalidReason) -> String {
    match reason {
        AddressInvalidReason::Empty => "Enter an address or search query".to_owned(),
        AddressInvalidReason::EmailOrUserinfo => {
            "Email addresses cannot be opened as pages".to_owned()
        }
        AddressInvalidReason::UnsupportedScheme(s) => format!("Unsupported scheme: {s}"),
        AddressInvalidReason::MissingLocalFile(p) => {
            format!("File does not exist: {}", p.display())
        }
        AddressInvalidReason::MalformedAddress(raw) => format!("Not a valid address: {raw}"),
    }
}

fn format_suggestion_row(row: &SuggestionRow) -> (&'static str, String, Option<String>) {
    match row {
        SuggestionRow::OpenTab { title, url, .. } => ("Tab", title.clone(), Some(url.display())),
        SuggestionRow::SiteSearchOffer { site } => (
            "Search",
            format!("Search {}… (Tab)", site.name),
            Some(site.host.clone()),
        ),
        SuggestionRow::PastSiteSearch {
            site_name,
            query,
            url,
            ..
        } => (
            "Recent",
            format!("{site_name}: {query}"),
            Some(url.display()),
        ),
        SuggestionRow::History { title, url, .. } => {
            ("History", title.clone(), Some(url.display()))
        }
        SuggestionRow::Bookmark { title, url } => (
            "Bookmark",
            title.clone(),
            url.as_ref().map(PageUrl::display),
        ),
        SuggestionRow::ICloudTab {
            device_name,
            title,
            url,
        } => (
            "Device",
            format!("{title} ({device_name})"),
            Some(url.display()),
        ),
        SuggestionRow::KnownSite { title, url } => ("Site", title.clone(), Some(url.display())),
        SuggestionRow::EngineSuggestion { query, url } => {
            ("Suggest", query.clone(), Some(url.display()))
        }
        SuggestionRow::InternalPage { label, url, .. } => {
            ("Page", label.clone(), Some(url.display()))
        }
        SuggestionRow::Command(cmd) => ("Command", cmd.label().to_owned(), None),
        SuggestionRow::SearchWords { engine, query, .. } => {
            ("Web", format!("Search {engine} for \"{query}\""), None)
        }
    }
}

fn internal_page_searchable_text(data: &InternalPageData) -> String {
    match data {
        InternalPageData::Settings {
            section,
            visible_sections,
            engine_name,
            ..
        } => {
            let titles: Vec<&str> = visible_sections.iter().map(|s| s.title()).collect();
            format!(
                "Settings {} {} {}",
                section.title(),
                titles.join(" "),
                engine_name
            )
        }
        InternalPageData::History { days, .. } => days
            .iter()
            .flat_map(|d| d.visits.iter().map(|v| format!("{} {}", v.title, v.url)))
            .collect::<Vec<_>>()
            .join(" "),
        InternalPageData::Downloads { entries } => entries
            .iter()
            .map(|e| e.file_name.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        InternalPageData::Bookmarks { tree } => tree
            .roots
            .iter()
            .map(|b| match b {
                BookmarkItem::Link { title, url, .. } => format!("{title} {url}"),
                BookmarkItem::Folder { title, .. } => title.clone(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        InternalPageData::Passwords { accounts } => accounts
            .iter()
            .map(|(h, u, _, _)| format!("{h} {u}"))
            .collect::<Vec<_>>()
            .join(" "),
        InternalPageData::Archive { archived_tabs } => archived_tabs
            .iter()
            .map(|t| format!("{} {}", t.title, t.url))
            .collect::<Vec<_>>()
            .join(" "),
        InternalPageData::Import { source } => source.clone().unwrap_or_default(),
        InternalPageData::ReleaseNotes { version } => format!("Release Notes {version}"),
    }
}

fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

fn default_data_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("searlx"));
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(home).join("Library/Application Support/Search"));
    }
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(appdata).join("searlx"));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(home).join(".local/share/searlx"));
    }
    None
}

/// Run the gpui shell app. Blocks until the last window closes.
///
/// # Errors
/// Returns [`UiError::Window`] or [`UiError::Controller`] if startup fails.
pub fn run() -> Result<(), UiError> {
    let initial_model = match default_data_dir() {
        Some(dir) => ShellModel::open(
            dir,
            EngineCapabilities::baseline("gpui-chrome"),
            RestoreMode::Normal,
            false,
        )?,
        None => ShellModel::with_temp_store(
            EngineCapabilities::baseline("gpui-chrome"),
            RestoreMode::FreshStart,
            false,
        )?,
    };

    let startup_error: std::rc::Rc<std::cell::RefCell<Option<UiError>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let error_slot = startup_error.clone();

    Application::new().run(move |cx: &mut App| {
        cx.bind_keys(key_bindings());
        cx.bind_keys(shell_key_bindings());
        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::new(
            point(px(80.0), px(80.0)),
            size(
                px(metrics::WINDOW_DEFAULT_WIDTH),
                px(metrics::WINDOW_DEFAULT_HEIGHT),
            ),
        );
        let window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(
                    px(metrics::WINDOW_MIN_WIDTH),
                    px(metrics::WINDOW_MIN_HEIGHT),
                )),
                titlebar: Some(TitlebarOptions {
                    title: Some("searlx".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                window_decorations: Some(WindowDecorations::Client),
                app_id: Some("searlx".into()),
                ..Default::default()
            },
            move |window, cx| {
                let mut model = initial_model;
                model.set_system_dark(is_dark(window.appearance()));
                let palette = model.palette();
                let address_field = cx.new(|cx| AddressField::new(palette, cx));
                let find_field = cx.new(|cx| AddressField::new(palette, cx));
                cx.new(|cx| Shell::from_model(model, address_field, find_field, window, cx))
            },
        ) {
            Ok(window) => window,
            Err(error) => {
                *error_slot.borrow_mut() = Some(UiError::Window(error.to_string()));
                cx.quit();
                return;
            }
        };

        if let Err(error) = window.update(cx, |shell, window, cx| {
            let handle = shell.focus_handle(cx);
            window.focus(&handle);
            cx.activate(true);
        }) {
            *error_slot.borrow_mut() = Some(UiError::Window(error.to_string()));
            cx.quit();
        }
    });

    match std::rc::Rc::try_unwrap(startup_error) {
        Ok(cell) => match cell.into_inner() {
            Some(error) => Err(error),
            None => Ok(()),
        },
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::A11yRole;
    use searlx_core::EngineCapabilities;

    fn test_model() -> ShellModel {
        let mut caps = EngineCapabilities::baseline("Test");
        caps.print = true;
        caps.content_blockers = true;
        ShellModel::with_temp_store(caps, RestoreMode::FreshStart, false)
            .expect("create temp ShellModel")
    }

    #[test]
    fn controller_backed_tab_creation_switching_pin_mute_sleep_and_close() {
        let mut model = test_model();
        assert_eq!(model.snapshot().tabs.len(), 1);
        let first_id = model.snapshot().tabs[0].id;

        // Open a second tab via controller
        model
            .dispatch(AppCommand::NewTab {
                url: Some(PageUrl::parse("https://example.com/one").expect("url")),
                activate: true,
            })
            .expect("new tab");
        assert_eq!(model.snapshot().tabs.len(), 2);
        let second_id = model.snapshot().active_tab.expect("active tab");
        assert_ne!(first_id, second_id);

        // Pin, mute, and switch back to first tab, then put second tab to sleep
        model
            .dispatch(AppCommand::PinTab { tab: second_id })
            .expect("pin");
        model
            .dispatch(AppCommand::MuteTab { tab: second_id })
            .expect("mute");
        assert!(
            model
                .snapshot()
                .tabs
                .iter()
                .any(|t| t.id == second_id && t.pinned && t.muted)
        );

        model
            .dispatch(AppCommand::ActivateTab { tab: first_id })
            .expect("activate first");
        assert_eq!(model.snapshot().active_tab, Some(first_id));

        model
            .dispatch(AppCommand::SleepTab { tab: second_id })
            .expect("sleep second");
        let slept = model
            .snapshot()
            .tabs
            .iter()
            .find(|t| t.id == second_id)
            .expect("second tab exists");
        assert!(slept.sleeping);
        assert_eq!(slept.engine_tab_id, None);

        // Closing a pinned tab puts it down rather than deleting it
        model.close_or_put_down_tab(second_id);
        assert!(model.snapshot().tabs.iter().any(|t| t.id == second_id));

        // Private tab badge
        model
            .dispatch(AppCommand::NewPrivateTab {
                url: Some(PageUrl::parse("https://private.example.org").expect("priv url")),
                activate: true,
            })
            .expect("new private tab");
        let priv_tab = model
            .snapshot()
            .tabs
            .iter()
            .find(|t| t.active)
            .expect("active private");
        assert!(priv_tab.private);
    }

    #[test]
    fn floating_address_overlay_suggestions_completion_lookalike_and_invalid_shake() {
        let mut model = test_model();
        model.open_address_overlay(AddressOverlayMode::OverPage);
        assert!(model.is_address_overlay_open());

        // Typing yields live suggestions and inline completion
        model.set_address_text("git".into());
        assert!(!model.snapshot().address.suggestions.is_empty());
        assert!(model.snapshot().address.inline_completion.is_some());

        // Tab locks SiteSearchOffer without accepting inline completion
        let locked = model.handle_address_tab(false);
        assert!(locked || model.snapshot().address.site_search_chip.is_some());
        model.clear_site_search_chip();
        assert!(model.snapshot().address.site_search_chip.is_none());

        // Invalid input (email address) triggers shake, keeps overlay open, and does not navigate
        let before_url = model
            .snapshot()
            .tabs
            .iter()
            .find(|t| t.active)
            .expect("active")
            .url
            .clone();
        let ok = model.submit_address("alice@example.com", false, false);
        assert!(!ok);
        assert!(model.is_address_overlay_open());
        assert_eq!(
            model.snapshot().address.shake_reason,
            Some(AddressInvalidReason::EmailOrUserinfo)
        );
        assert_ne!(model.shake_offset_px(), 0.0);
        assert_eq!(
            model
                .snapshot()
                .tabs
                .iter()
                .find(|t| t.active)
                .expect("active")
                .url,
            before_url
        );

        // With reduce_motion enabled, shake_offset_px collapses to 0.0
        model.set_reduce_motion(true);
        assert_eq!(model.shake_offset_px(), 0.0);

        // Valid submission navigates and closes overlay, and lookalike IDN host flags badge
        let ok = model.submit_address("https://рaypal.com", false, false);
        assert!(ok);
        assert!(!model.is_address_overlay_open());
        assert!(model.snapshot().address.is_lookalike);
        assert!(model.snapshot().address.safe_host.starts_with("xn--"));
    }

    #[test]
    fn site_card_state_zoom_blocker_darken_and_copy_address() {
        let mut model = test_model();
        model.update_settings(|s| {
            s.pages_dusk = true;
            s.look = LookPreference::Dark;
        });
        model
            .dispatch(AppCommand::NewTab {
                url: Some(PageUrl::parse("https://example.com/article").expect("url")),
                activate: true,
            })
            .expect("open example");

        // Blank tab has no site card, real page has SiteCardModel
        assert!(model.snapshot().site_card.is_some());
        let active_id = model.snapshot().active_tab.expect("active");
        model.click_tab(active_id);
        assert!(model.is_site_card_open());

        // Zoom step
        model
            .dispatch(AppCommand::ZoomStep { delta_steps: 2 })
            .expect("zoom in");
        assert_eq!(
            model
                .snapshot()
                .site_card
                .as_ref()
                .expect("card")
                .zoom_percent,
            110
        );
        model.dispatch(AppCommand::ZoomReset).expect("zoom reset");
        assert_eq!(
            model
                .snapshot()
                .site_card
                .as_ref()
                .expect("card")
                .zoom_percent,
            100
        );

        // Blocker toggle
        assert_eq!(
            model
                .snapshot()
                .site_card
                .as_ref()
                .expect("card")
                .block_ads_and_trackers,
            Some(true)
        );
        model
            .dispatch(AppCommand::ToggleSiteBlocker { site_host: None })
            .expect("toggle blocker");
        assert_eq!(
            model
                .snapshot()
                .site_card
                .as_ref()
                .expect("card")
                .block_ads_and_trackers,
            Some(false)
        );

        // Darken toggle (Dusk enabled + dark frame)
        assert_eq!(
            model
                .snapshot()
                .site_card
                .as_ref()
                .expect("card")
                .darken_this_site,
            Some(false)
        );
        model
            .dispatch(AppCommand::ToggleDuskForSite)
            .expect("toggle dusk");
        assert_eq!(
            model
                .snapshot()
                .site_card
                .as_ref()
                .expect("card")
                .darken_this_site,
            Some(true)
        );

        // Copy address emits CopyToClipboard directive
        model
            .dispatch(AppCommand::CopyAddress { tab: None })
            .expect("copy address");
        assert_eq!(
            model.last_copied_text(),
            Some("https://example.com/article")
        );
    }

    #[test]
    fn sidebar_fold_layouts_split_find_and_internal_pages() {
        let mut model = test_model();
        assert!(!model.snapshot().sidebar_folded);
        model.toggle_sidebar_fold();
        assert!(model.snapshot().sidebar_folded);
        model.toggle_sidebar_fold();
        assert!(!model.snapshot().sidebar_folded);

        model.cycle_tab_layout();
        assert_eq!(model.snapshot().sidebar_layout, TabLayoutMode::RightColumn);
        model.cycle_tab_layout();
        assert_eq!(model.snapshot().sidebar_layout, TabLayoutMode::TopTabs);
        model.cycle_tab_layout();
        assert_eq!(model.snapshot().sidebar_layout, TabLayoutMode::IconsOnly);

        // Open internal settings page and run Find on page
        model
            .dispatch(AppCommand::OpenInternalPage {
                kind: InternalPageKind::Settings,
                detail: Some("appearance".into()),
            })
            .expect("open settings");
        assert!(matches!(
            model.snapshot().active_internal_page,
            Some(InternalPageData::Settings {
                section: SettingsPage::Appearance,
                ..
            })
        ));

        model.open_find_bar();
        model.find_in_page("Appearance");
        let find = model.snapshot().find.as_ref().expect("find open");
        assert!(find.total_matches >= 1);

        // Split View
        model
            .dispatch(AppCommand::NewTab {
                url: Some(PageUrl::parse("https://example.org").expect("url")),
                activate: true,
            })
            .expect("second tab");
        model
            .dispatch(AppCommand::ToggleSplitView {
                target_tab: None,
                partner_tab: None,
            })
            .expect("toggle split");
        assert_eq!(model.snapshot().split_panes.len(), 2);
        assert!(model.snapshot().tabs.iter().any(|t| t.in_split));
    }

    #[test]
    fn settings_driven_theme_contrast_motion_and_a11y_tree_sync() {
        let mut model = test_model();

        // System dark off -> Light palette
        assert_eq!(model.palette(), Palette::light());
        model.set_system_dark(true);
        assert_eq!(model.palette(), Palette::dark());

        // Explicit Light overrides system dark
        model.set_look_preference(LookPreference::Light);
        assert_eq!(model.theme(), Theme::Light);
        assert_eq!(model.palette(), Palette::light());

        // High contrast + reduce motion propagate from settings
        model.set_high_contrast(true);
        model.set_reduce_motion(true);
        assert!(model.high_contrast());
        assert!(model.reduce_motion());
        assert_eq!(
            model.palette(),
            Palette::for_theme_with_contrast(Theme::Light, true, true)
        );
        assert_eq!(
            metrics::motion::duration_ms(metrics::motion::PILL, model.reduce_motion()),
            0
        );

        // AccessibilityTree stays synced with live UiSnapshot
        model
            .dispatch(AppCommand::NewTab {
                url: Some(PageUrl::parse("https://example.com").expect("url")),
                activate: true,
            })
            .expect("tab");
        model.open_address_overlay(AddressOverlayMode::OverPage);
        model.set_address_text("git".into());
        let tree = model.a11y_tree();
        assert_eq!(tree.tab_nodes().len(), 2);
        assert!(
            tree.focused_node()
                .is_some_and(|n| n.role == A11yRole::SearchBox && n.value.as_deref() == Some("git"))
        );
        assert!(tree.find_by_name("Suggestions").is_some());

        model.close_address_overlay();
        model.toggle_site_card();
        assert!(model.a11y_tree().find_by_name("Site Card").is_some());
        model.dismiss_active_overlay();
        assert!(model.a11y_tree().find_by_name("Site Card").is_none());
    }

    #[test]
    fn session_restore_asleep_spaces_tab_groups_and_engine_events() {
        let dir = TempDirGuard::new("searlx-ui-restore").expect("temp dir");
        let store = searlx_core::data::DataStore::new(dir.path());
        let session = searlx_core::data::SessionFile {
            version: searlx_core::CURRENT_SCHEMA_VERSION,
            active: 1,
            tabs: vec![
                searlx_core::data::SessionTab {
                    id: "t0".into(),
                    url: "https://example.com/asleep".into(),
                    title: "Asleep Tab".into(),
                    ..Default::default()
                },
                searlx_core::data::SessionTab {
                    id: "t1".into(),
                    url: "https://example.com/awake".into(),
                    title: "Awake Tab".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        store.save_session(None, &session).expect("save session");

        let mut model = ShellModel::open(
            dir.path(),
            EngineCapabilities::baseline("Test"),
            RestoreMode::Normal,
            false,
        )
        .expect("open restored model");
        assert_eq!(model.snapshot().tabs.len(), 2);
        let asleep_id = model.snapshot().tabs[0].id;
        let awake_id = model.snapshot().tabs[1].id;
        assert!(model.snapshot().tabs[0].sleeping);
        assert!(!model.snapshot().tabs[1].sleeping);

        // Clicking sleeping tab wakes it
        model.click_tab(asleep_id);
        assert!(
            !model
                .snapshot()
                .tabs
                .iter()
                .find(|t| t.id == asleep_id)
                .expect("woken")
                .sleeping
        );

        // Reconcile EngineEvents into UiSnapshot + AccessibilityTree
        model
            .handle_engine_event(EngineEvent::MediaStateChanged {
                tab: asleep_id,
                audible: true,
                muted: false,
            })
            .expect("media state");
        model
            .handle_engine_event(EngineEvent::TitleChanged {
                tab: asleep_id,
                title: "Playing Audio".into(),
            })
            .expect("title changed");
        let updated = model
            .snapshot()
            .tabs
            .iter()
            .find(|t| t.id == asleep_id)
            .expect("tab");
        assert!(updated.audible);
        assert_eq!(updated.title, "Playing Audio");
        assert!(
            model
                .a11y_tree()
                .tab_nodes()
                .iter()
                .any(|n| n.name == "Playing Audio" && n.description.as_deref() == Some("audible"))
        );

        // Space switcher + tab group
        model
            .dispatch(AppCommand::CreateTabGroup {
                name: "Research".into(),
                color: SpaceColor::Green,
                tabs: vec![asleep_id, awake_id],
            })
            .expect("create group");
        assert_eq!(model.snapshot().groups.len(), 1);

        model
            .dispatch(AppCommand::CreateSpace {
                profile: None,
                name: "Work".into(),
                color: SpaceColor::Violet,
                icon: "briefcase".into(),
                store_policy: searlx_core::SpaceStorePolicy::OwnStore,
            })
            .expect("create space");
        assert_eq!(model.snapshot().spaces.len(), 2);
        let work_id = model
            .snapshot()
            .spaces
            .iter()
            .find(|s| s.name == "Work")
            .expect("work space")
            .id;
        model
            .dispatch(AppCommand::SwitchSpace { space: work_id })
            .expect("switch space");
        assert_eq!(model.snapshot().active_space, work_id);
    }

    #[test]
    fn floating_new_tab_draft_tab_completion_guard_and_fold_to_icons() {
        let mut model = test_model();
        model.update_settings(|s| {
            s.newtab_floating = true;
            s.sidebar_fold_icons = true;
        });

        // Ctrl+T opens FloatingNewTab without creating a tab until Return
        model.open_new_tab();
        assert!(model.is_address_overlay_open());
        assert_eq!(
            model.address_overlay_mode(),
            AddressOverlayMode::FloatingNewTab
        );
        assert_eq!(model.snapshot().tabs.len(), 1);

        // Typing shows inline_completion, and Tab does NOT accept inline completion
        model.set_address_text("git".into());
        assert!(model.snapshot().address.inline_completion.is_some());
        model.step_suggestion(1);
        assert_eq!(model.selected_suggestion(), Some(0));
        model.step_suggestion(-1);
        assert_eq!(model.selected_suggestion(), None);

        // Tab on SiteSearchOffer locks chip rather than accepting inline completion
        let locked = model.handle_address_tab(false);
        assert!(locked);
        assert!(model.snapshot().address.site_search_chip.is_some());
        model.clear_site_search_chip();

        // Escape preserves draft for next Ctrl+T without adding a tab
        model.set_address_text("example.org".into());
        model.close_address_overlay();
        assert!(!model.is_address_overlay_open());
        assert_eq!(model.snapshot().tabs.len(), 1);
        model.open_new_tab();
        assert_eq!(model.snapshot().address.text, "example.org");

        // Submit opens new tab in front
        assert!(model.submit_address("example.org", false, false));
        assert_eq!(model.snapshot().tabs.len(), 2);
        assert_eq!(
            model
                .snapshot()
                .tabs
                .iter()
                .find(|t| t.active)
                .expect("active")
                .url
                .as_str(),
            "https://example.org/"
        );

        // Ctrl+S with fold_to_icons switches to IconsOnly and back
        model.toggle_sidebar_fold();
        assert_eq!(model.snapshot().sidebar_layout, TabLayoutMode::IconsOnly);
        assert!(!model.snapshot().sidebar_folded);
        model.toggle_sidebar_fold();
        assert_eq!(model.snapshot().sidebar_layout, TabLayoutMode::LeftColumn);
    }
}
