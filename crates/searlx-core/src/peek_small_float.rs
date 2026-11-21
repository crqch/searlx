//! Link Peek (`P3-09`), Small Windows / "Little" (`P3-10`), and Floating Video (`P3-11`).
//!
//! Behavioural specifications:
//! - `FEATURES.md` §3.3, §21, §23 — Link Peek (`Shift-click`, `⌘↩`, `⌥⌘↩`, `links.peek.stays`, pin peek)
//! - `FEATURES.md` §18.3 — Small windows (`900×640` default, `420×320` min, cascade, `⌘O`, Escape vs `⌘W`, >60% on-screen check)
//! - `FEATURES.md` §19 — Floating video & PiP (`⇧⌘P`, `float.leave`, `float.away`, corner flicks, private-tab policy)

use crate::data::model::ScrollPosition;
use crate::ids::{SpaceId, TabId};
use crate::session::WindowFrame;
use crate::url::PageUrl;

// ============================================================================
// P3-09 — Link Peek (`FEATURES.md` §3.3, §21, §23)
// ============================================================================

/// Modifier click gesture configured to trigger Link Peek (`links.peekClick`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PeekClickModifier {
    /// `Shift`-click on a link (default).
    #[default]
    ShiftClick,
    /// `Option+Shift`-click on a link.
    OptionShiftClick,
    /// `Option+Command`-click on a link.
    OptionCommandClick,
}

impl PeekClickModifier {
    /// Whether the given modifier state matches this peek click trigger.
    #[must_use]
    pub const fn matches(self, shift: bool, alt: bool, cmd: bool) -> bool {
        match self {
            Self::ShiftClick => shift && !alt && !cmd,
            Self::OptionShiftClick => shift && alt && !cmd,
            Self::OptionCommandClick => !shift && alt && cmd,
        }
    }
}

/// Snapshot of the opener tab's exact state before opening a peek panel (`P3-09`).
#[derive(Clone, Debug, PartialEq)]
pub struct OpenerPageState {
    /// Tab that opened the peek panel.
    pub opener_tab_id: TabId,
    /// Committed URL of the opener tab.
    pub opener_url: PageUrl,
    /// Exact scroll offset of the opener page (`FEATURES.md` §3.6: page underneath keeps its scroll).
    pub scroll: ScrollPosition,
    /// CSS selector of the focused element in the opener page, if any.
    pub focused_element_selector: Option<String>,
    /// Whether the opener tab is a pinned tab (`pins.peek`).
    pub opened_from_pin: bool,
}

/// Outcome of promoting or closing a peek panel (`P3-09`).
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum PeekResolution {
    /// Closed via `Escape`, outside click, or close cross; restores exact opener state without reload.
    ClosedRestoreOpener {
        /// Opener state restored without reloading.
        restored: OpenerPageState,
        /// The peek tab view to discard.
        discarded_peek_tab: TabId,
        /// Always `false`: closing a peek never reloads the opener page.
        opener_reloaded: bool,
    },
    /// Kept as a regular tab in the row (`⌘↩` or Keep button); the peek's live view becomes the tab without reloading.
    KeptAsTab {
        /// The promoted tab ID (same webview instance, no reload).
        promoted_tab_id: TabId,
        /// Always `false`: keeping a peek never reloads the page (`TODO.md` P3-09).
        reloaded: bool,
    },
    /// Kept beside the opener page in a 2-pane Split View (`⌥⌘↩` or `⌥`-click Keep button).
    KeptBesideInSplit {
        /// Left pane (opener tab).
        left_tab_id: TabId,
        /// Right pane (promoted peek tab).
        right_tab_id: TabId,
        /// Always `false`: keeping a peek beside in Split View does not reload either page.
        reloaded: bool,
    },
}

/// Active link-peek panel attached to an opener tab (`FEATURES.md` §3.3, P3-09).
#[derive(Clone, Debug, PartialEq)]
pub struct PeekSession {
    /// Saved state of the opener page underneath the peek panel.
    pub opener: OpenerPageState,
    /// Live tab ID hosting the peeked page.
    pub peek_tab_id: TabId,
    /// Current URL inside the peek panel.
    pub peek_url: PageUrl,
    /// Whether the peek panel is currently visible over its opener tab.
    pub visible: bool,
    /// Whether audio/video inside the peek is paused while hidden (`links.peek.stays`).
    pub media_paused: bool,
    /// Whether camera/microphone capture inside the peek is muted while hidden (`links.peek.stays`).
    pub capture_muted: bool,
}

impl PeekSession {
    /// Open a new peek panel over `opener` (`Shift-click` or off-domain pin link click).
    #[must_use]
    pub const fn open(opener: OpenerPageState, peek_tab_id: TabId, peek_url: PageUrl) -> Self {
        Self {
            opener,
            peek_tab_id,
            peek_url,
            visible: true,
            media_paused: false,
            capture_muted: false,
        }
    }

    /// Handle switching active tabs when `links.peek.stays` is configured (`FEATURES.md` §3.3).
    ///
    /// Returns `true` if the peek session should remain alive (hidden with media paused),
    /// or `false` if the peek session should close immediately.
    pub fn on_active_tab_changed(
        &mut self,
        new_active_tab: TabId,
        peek_stays_with_tab: bool,
    ) -> bool {
        if new_active_tab == self.opener.opener_tab_id {
            self.visible = true;
            self.media_paused = false;
            self.capture_muted = false;
            true
        } else if peek_stays_with_tab {
            self.visible = false;
            self.media_paused = true;
            self.capture_muted = true;
            true
        } else {
            false
        }
    }

    /// Close the peek panel (`Escape`, click outside, or close button), returning the exact opener state.
    #[must_use]
    pub fn close_with_escape(self) -> PeekResolution {
        PeekResolution::ClosedRestoreOpener {
            restored: self.opener,
            discarded_peek_tab: self.peek_tab_id,
            opener_reloaded: false,
        }
    }

    /// Keep the peeked page as a normal tab in the row (`⌘↩` or click Keep).
    #[must_use]
    pub fn keep_as_tab(self) -> PeekResolution {
        PeekResolution::KeptAsTab {
            promoted_tab_id: self.peek_tab_id,
            reloaded: false,
        }
    }

    /// Keep the peeked page beside the opener in Split View (`⌥⌘↩` or `⌥`-click Keep).
    #[must_use]
    pub fn keep_beside_in_split(self) -> PeekResolution {
        PeekResolution::KeptBesideInSplit {
            left_tab_id: self.opener.opener_tab_id,
            right_tab_id: self.peek_tab_id,
            reloaded: false,
        }
    }
}

// ============================================================================
// P3-10 — Small windows / "Little" (`FEATURES.md` §18.3)
// ============================================================================

/// Default width of a small window (`900 pt`, `FEATURES.md` §18.3).
pub const SMALL_WINDOW_DEFAULT_WIDTH: f64 = 900.0;
/// Default height of a small window (`640 pt`, `FEATURES.md` §18.3).
pub const SMALL_WINDOW_DEFAULT_HEIGHT: f64 = 640.0;
/// Minimum width of a small window (`420 pt`, `FEATURES.md` §18.3).
pub const SMALL_WINDOW_MIN_WIDTH: f64 = 420.0;
/// Minimum height of a small window (`320 pt`, `FEATURES.md` §18.3).
pub const SMALL_WINDOW_MIN_HEIGHT: f64 = 320.0;
/// Height of the thin site header line on a small window (`34 pt`, `FEATURES.md` §18.3).
pub const SMALL_WINDOW_HEADER_PT: f64 = 34.0;
/// Cascade step in points when opening a new small window over an existing visible one.
pub const SMALL_WINDOW_CASCADE_STEP_PT: f64 = 24.0;

/// How a small window was created (`FEATURES.md` §18.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmallWindowKind {
    /// Opened from an external application link (`links.little`) or Launcher (`launcher.little`).
    ExternalLink,
    /// Lifted out of a browser window's tab row (`Open in Small Window`).
    LiftedTab {
        /// Space the tab belonged to when lifted (restored on quit).
        origin_space: SpaceId,
    },
    /// Sized `window.open` popup (`tabs.popupWindows`; has no `"Open in Search"` button).
    SizedPopup {
        /// Tab that opened this popup.
        opener_tab: TabId,
    },
}

/// State of one small window (`FEATURES.md` §18.3, P3-10).
#[derive(Clone, Debug, PartialEq)]
pub struct SmallWindowEntry {
    /// Live tab ID hosted in the small window.
    pub tab_id: TabId,
    /// Current page URL.
    pub url: PageUrl,
    /// Page title (used in `⌃Tab` moon cards).
    pub title: String,
    /// Estimated vertical page length in screens (for `⌃Tab` moon log₂ scaling).
    pub screens_tall: f64,
    /// Window frame on screen.
    pub frame: WindowFrame,
    /// Whether this frame was cascaded from another visible small window.
    pub cascaded: bool,
    /// Origin kind of the small window.
    pub kind: SmallWindowKind,
    /// Space associated with this small window.
    pub space_id: SpaceId,
}

impl SmallWindowEntry {
    /// Whether the thin 34 pt header shows `"Open in Search"` (`⌘O`).
    ///
    /// Sized popups reuse `LittleView` without `"Open in Search"` (`FEATURES.md` §18.3).
    #[must_use]
    pub const fn shows_open_in_search(&self) -> bool {
        !matches!(self.kind, SmallWindowKind::SizedPopup { .. })
    }

    /// Whether pressing `Escape` closes this small window (`FEATURES.md` §18.3).
    ///
    /// Rule: `Esc` closes a link window (`ExternalLink`) or popup, but does **not**
    /// close a lifted tab because it is the user's own tab.
    #[must_use]
    pub const fn closes_on_escape(&self) -> bool {
        !matches!(self.kind, SmallWindowKind::LiftedTab { .. })
    }
}

/// Outcome of moving a small window into the main browser window via `⌘O` / `"Open in Search"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenInSearchOutcome {
    /// Existing tab ID moved into the browser window's tab row (after the current tab, never among pins).
    pub tab_id: TabId,
    /// Space in which the tab should be placed.
    pub target_space: SpaceId,
    /// Always `false`: moving a small window into the row never reloads the page (`FEATURES.md` §18.3).
    pub reloaded: bool,
    /// Always `true`: `"Open in Search"` brings the main browser window forward.
    pub activates_browser_window: bool,
}

/// Outcome of closing a small window (`Escape` or `⌘W`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmallWindowCloseOutcome {
    /// `Escape` was pressed on a lifted tab; window remains open.
    IgnoredEscapeOnLiftedTab,
    /// Link/popup window closed and its temporary tab discarded.
    DiscardedLinkTab(TabId),
    /// Lifted tab closed via `⌘W`; recorded in recently-closed stack so `⇧⌘T` restores it.
    ClosedRepeatableTab {
        /// Closed tab ID.
        tab_id: TabId,
        /// Original space for `⇧⌘T` restoration.
        origin_space: SpaceId,
    },
}

/// Check whether a saved small-window frame still has >60% of its area visible on `screen` (`FEATURES.md` §18.3).
#[must_use]
pub fn is_small_frame_mostly_on_screen(frame: &WindowFrame, screen: &WindowFrame) -> bool {
    let area = frame.width.max(1.0) * frame.height.max(1.0);
    let left = frame.x.max(screen.x);
    let top = frame.y.max(screen.y);
    let right = (frame.x + frame.width).min(screen.x + screen.width);
    let bottom = (frame.y + frame.height).min(screen.y + screen.height);
    let overlap_w = (right - left).max(0.0);
    let overlap_h = (bottom - top).max(0.0);
    (overlap_w * overlap_h) / area > 0.60
}

/// Manager for Small Windows / "Little" (`FEATURES.md` §18.3, P3-10).
#[derive(Clone, Debug, Default)]
pub struct SmallWindowsManager {
    windows: Vec<SmallWindowEntry>,
    saved_frame: Option<WindowFrame>,
}

impl SmallWindowsManager {
    /// Create an empty small-windows manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Active small windows in creation order (newest last).
    #[must_use]
    pub fn windows(&self) -> &[SmallWindowEntry] {
        &self.windows
    }

    /// Persisted `little.frame`, if any.
    #[must_use]
    pub const fn saved_frame(&self) -> Option<&WindowFrame> {
        self.saved_frame.as_ref()
    }

    /// Open a new small window, cascading down/right from the newest visible one
    /// without bringing the main browser window forward (`FEATURES.md` §18.3).
    pub fn open_small_window(
        &mut self,
        tab_id: TabId,
        url: PageUrl,
        title: impl Into<String>,
        space_id: SpaceId,
        kind: SmallWindowKind,
        screen: &WindowFrame,
    ) -> SmallWindowEntry {
        let base_frame = self
            .saved_frame
            .as_ref()
            .filter(|f| is_small_frame_mostly_on_screen(f, screen))
            .copied()
            .unwrap_or(WindowFrame {
                x: screen.x + ((screen.width - SMALL_WINDOW_DEFAULT_WIDTH) * 0.5).max(0.0),
                y: screen.y + ((screen.height - SMALL_WINDOW_DEFAULT_HEIGHT) * 0.5).max(0.0),
                width: SMALL_WINDOW_DEFAULT_WIDTH,
                height: SMALL_WINDOW_DEFAULT_HEIGHT,
            });

        let (frame, cascaded) = if let Some(last) = self.windows.last() {
            (
                WindowFrame {
                    x: last.frame.x + SMALL_WINDOW_CASCADE_STEP_PT,
                    y: last.frame.y + SMALL_WINDOW_CASCADE_STEP_PT,
                    width: base_frame.width.max(SMALL_WINDOW_MIN_WIDTH),
                    height: base_frame.height.max(SMALL_WINDOW_MIN_HEIGHT),
                },
                true,
            )
        } else {
            (
                WindowFrame {
                    width: base_frame.width.max(SMALL_WINDOW_MIN_WIDTH),
                    height: base_frame.height.max(SMALL_WINDOW_MIN_HEIGHT),
                    ..base_frame
                },
                false,
            )
        };

        let entry = SmallWindowEntry {
            tab_id,
            url,
            title: title.into(),
            screens_tall: 1.0,
            frame,
            cascaded,
            kind,
            space_id,
        };
        self.windows.push(entry.clone());
        entry
    }

    /// Resize or move a small window and persist `little.frame` unless cascaded or fullscreen (`FEATURES.md` §18.3).
    pub fn update_frame(
        &mut self,
        tab_id: TabId,
        new_frame: WindowFrame,
        is_fullscreen: bool,
    ) -> bool {
        let Some(entry) = self.windows.iter_mut().find(|w| w.tab_id == tab_id) else {
            return false;
        };
        entry.frame = WindowFrame {
            x: new_frame.x,
            y: new_frame.y,
            width: new_frame.width.max(SMALL_WINDOW_MIN_WIDTH),
            height: new_frame.height.max(SMALL_WINDOW_MIN_HEIGHT),
        };
        entry.cascaded = false;
        if !is_fullscreen {
            self.saved_frame = Some(entry.frame);
        }
        true
    }

    /// Promote a small window into the main browser window row (`⌘O` / `"Open in Search"`).
    pub fn open_in_search(&mut self, tab_id: TabId) -> Option<OpenInSearchOutcome> {
        let idx = self.windows.iter().position(|w| w.tab_id == tab_id)?;
        if !self.windows[idx].shows_open_in_search() {
            return None;
        }
        let removed = self.windows.remove(idx);
        let target_space = match removed.kind {
            SmallWindowKind::LiftedTab { origin_space } => origin_space,
            SmallWindowKind::ExternalLink | SmallWindowKind::SizedPopup { .. } => removed.space_id,
        };
        Some(OpenInSearchOutcome {
            tab_id: removed.tab_id,
            target_space,
            reloaded: false,
            activates_browser_window: true,
        })
    }

    /// Close a small window via `Escape` (`by_escape = true`) or `⌘W` (`by_escape = false`).
    pub fn close_small_window(
        &mut self,
        tab_id: TabId,
        by_escape: bool,
    ) -> Option<SmallWindowCloseOutcome> {
        let idx = self.windows.iter().position(|w| w.tab_id == tab_id)?;
        if by_escape && !self.windows[idx].closes_on_escape() {
            return Some(SmallWindowCloseOutcome::IgnoredEscapeOnLiftedTab);
        }
        let removed = self.windows.remove(idx);
        Some(match removed.kind {
            SmallWindowKind::LiftedTab { origin_space } => {
                SmallWindowCloseOutcome::ClosedRepeatableTab {
                    tab_id: removed.tab_id,
                    origin_space,
                }
            }
            SmallWindowKind::ExternalLink | SmallWindowKind::SizedPopup { .. } => {
                SmallWindowCloseOutcome::DiscardedLinkTab(removed.tab_id)
            }
        })
    }

    /// Return all lifted tabs so they can be written back into their original spaces at quit (`FEATURES.md` §18.3).
    pub fn drain_lifted_tabs_for_quit(&mut self) -> Vec<(TabId, SpaceId)> {
        let mut lifted = Vec::new();
        self.windows.retain(|w| {
            if let SmallWindowKind::LiftedTab { origin_space } = w.kind {
                lifted.push((w.tab_id, origin_space));
                false
            } else {
                true
            }
        });
        lifted
    }
}

// ============================================================================
// P3-11 — Floating video and Picture-in-Picture (`FEATURES.md` §19)
// ============================================================================

/// Preferred floating video presentation (`float.style`, `FEATURES.md` §19).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatStyle {
    /// Search's own always-on-top panel holding the live page / portable player (default).
    #[default]
    SearchPanel,
    /// Native system Picture-in-Picture where supported by the OS engine (`capabilities.native_pip`).
    SystemPip,
}

/// Event that triggered floating video (`FEATURES.md` §19).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatTrigger {
    /// Explicit `⇧⌘P` shortcut.
    ManualShortcut,
    /// Switching away from a tab playing video (`float.leave`, default on).
    LeaveTab,
    /// Switching away from the application (`float.away`, default off).
    LeaveApp,
}

/// Screen corner to which a floating video panel docks or flicks (`float.flicks`, `FEATURES.md` §19, §23).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatCorner {
    /// Top-left screen corner.
    TopLeft,
    /// Top-right screen corner.
    TopRight,
    /// Bottom-left screen corner.
    BottomLeft,
    /// Bottom-right screen corner (default).
    #[default]
    BottomRight,
}

impl FloatCorner {
    /// Resolve the destination corner from a two-finger flick velocity vector `(vx, vy)`.
    #[must_use]
    pub fn from_flick_velocity(vx: f64, vy: f64) -> Self {
        match (vx >= 0.0, vy >= 0.0) {
            (false, false) => Self::TopLeft,
            (true, false) => Self::TopRight,
            (false, true) => Self::BottomLeft,
            (true, true) => Self::BottomRight,
        }
    }
}

/// Active floating video session (`FEATURES.md` §19, P3-11).
#[derive(Clone, Debug, PartialEq)]
pub struct FloatingVideoSession {
    /// Tab that owns the playing video.
    pub tab_id: TabId,
    /// Space the tab belongs to.
    pub space_id: SpaceId,
    /// Effective presentation style (`SystemPip` falls back to `SearchPanel` when `engine_supports_native_pip` is false).
    pub effective_style: FloatStyle,
    /// Trigger that started this session.
    pub trigger: FloatTrigger,
    /// Whether the tab is a private tab.
    pub is_private_tab: bool,
    /// Whether playback is currently running (`true`) or paused (`false`).
    pub playing: bool,
    /// Current playback timestamp in seconds.
    pub current_time_secs: f64,
    /// Total media duration in seconds.
    pub duration_secs: f64,
    /// Whether the floating window is minimized to a side pill (`float.minimize`).
    pub minimized: bool,
    /// Current docked screen corner.
    pub corner: FloatCorner,
}

/// Outcome of returning from floating video back to the owning tab (`FEATURES.md` §19).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatReturnOutcome {
    /// Owning tab restored to its window stage.
    pub tab_id: TabId,
    /// Owning space.
    pub space_id: SpaceId,
    /// Always `false`: returning from floating video never stops playback (`FEATURES.md` §19).
    pub video_stopped: bool,
}

/// Controller for Floating Video and PiP (`FEATURES.md` §19, P3-11).
///
/// Explicit Private-Tab Policy Decision (`TODO.md` P3-11, `FEATURES.md` §19):
/// A private tab's playing video is permitted to float (`allow_private_tab_float = true`)
/// because the floating panel moves the private tab's live view in memory without writing
/// any state to disk or exposing it to other profiles, and closes immediately if the
/// private tab closes.
#[derive(Clone, Debug, Default)]
pub struct FloatingVideoController {
    active: Option<FloatingVideoSession>,
}

impl FloatingVideoController {
    /// Create an idle floating-video controller.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Currently active floating video session, if any.
    #[must_use]
    pub const fn active(&self) -> Option<&FloatingVideoSession> {
        self.active.as_ref()
    }

    /// Deliberate policy decision for private tabs (`FEATURES.md` §19, P3-11).
    #[must_use]
    pub const fn allows_private_tab_floating() -> bool {
        true
    }

    /// Attempt to start floating video for `tab_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn try_start(
        &mut self,
        tab_id: TabId,
        space_id: SpaceId,
        url: &PageUrl,
        is_private_tab: bool,
        video_playing: bool,
        duration_secs: f64,
        trigger: FloatTrigger,
        preferred_style: FloatStyle,
        engine_supports_native_pip: bool,
        float_on_leave: bool,
        float_on_away: bool,
        disabled_hosts: &[String],
    ) -> Option<FloatingVideoSession> {
        if !video_playing {
            return None;
        }
        let host = url.host().unwrap_or_default().trim_start_matches("www.");
        let site_disabled = disabled_hosts
            .iter()
            .any(|h| h.trim_start_matches("www.").eq_ignore_ascii_case(host));

        match trigger {
            FloatTrigger::ManualShortcut => {}
            FloatTrigger::LeaveTab => {
                if !float_on_leave || site_disabled {
                    return None;
                }
            }
            FloatTrigger::LeaveApp => {
                if !float_on_away || site_disabled {
                    return None;
                }
            }
        }

        let effective_style = match preferred_style {
            FloatStyle::SystemPip if engine_supports_native_pip => FloatStyle::SystemPip,
            FloatStyle::SystemPip | FloatStyle::SearchPanel => FloatStyle::SearchPanel,
        };

        let session = FloatingVideoSession {
            tab_id,
            space_id,
            effective_style,
            trigger,
            is_private_tab,
            playing: true,
            current_time_secs: 0.0,
            duration_secs: duration_secs.max(0.0),
            minimized: false,
            corner: FloatCorner::BottomRight,
        };
        self.active = Some(session.clone());
        Some(session)
    }

    /// Toggle play/pause on the floating video (`FEATURES.md` §19).
    pub fn toggle_play_pause(&mut self) -> Option<bool> {
        let session = self.active.as_mut()?;
        session.playing = !session.playing;
        Some(session.playing)
    }

    /// Skip forward (`+10.0`) or backward (`-10.0`) in seconds (`FEATURES.md` §19).
    pub fn skip_seconds(&mut self, delta_secs: f64) -> Option<f64> {
        let session = self.active.as_mut()?;
        session.current_time_secs =
            (session.current_time_secs + delta_secs).clamp(0.0, session.duration_secs.max(0.0));
        Some(session.current_time_secs)
    }

    /// Snap the floating video panel to a screen corner from a two-finger flick (`float.flicks`).
    pub fn flick_to_corner(&mut self, vx: f64, vy: f64) -> Option<FloatCorner> {
        let session = self.active.as_mut()?;
        let corner = FloatCorner::from_flick_velocity(vx, vy);
        session.corner = corner;
        Some(corner)
    }

    /// Toggle minimized state (`float.minimize`).
    pub fn set_minimized(&mut self, minimized: bool) -> bool {
        let Some(session) = self.active.as_mut() else {
            return false;
        };
        session.minimized = minimized;
        true
    }

    /// Return the floating video back into its owning tab without stopping playback (`FEATURES.md` §19).
    pub fn return_to_tab(&mut self) -> Option<FloatReturnOutcome> {
        let session = self.active.take()?;
        Some(FloatReturnOutcome {
            tab_id: session.tab_id,
            space_id: session.space_id,
            video_stopped: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_escape_restores_exact_page_state_and_keep_never_reloads() {
        let opener_tab = TabId::new(1);
        let peek_tab = TabId::new(2);
        let opener_url = PageUrl::parse("https://example.com/article").expect("valid url");
        let peek_url = PageUrl::parse("https://iana.org/domains").expect("valid url");
        let opener_state = OpenerPageState {
            opener_tab_id: opener_tab,
            opener_url: opener_url.clone(),
            scroll: ScrollPosition {
                x: 12.0,
                y: 840.0,
                extra: Default::default(),
            },
            focused_element_selector: Some("a#ref-2".to_owned()),
            opened_from_pin: false,
        };

        let mut peek = PeekSession::open(opener_state.clone(), peek_tab, peek_url.clone());
        assert!(PeekClickModifier::ShiftClick.matches(true, false, false));

        // Switching away with `links.peek.stays = true` hides and pauses media/capture
        let other_tab = TabId::new(3);
        assert!(peek.on_active_tab_changed(other_tab, true));
        assert!(!peek.visible);
        assert!(peek.media_paused);
        assert!(peek.capture_muted);

        // Switching back restores visibility and unpauses
        assert!(peek.on_active_tab_changed(opener_tab, true));
        assert!(peek.visible);
        assert!(!peek.media_paused);

        // Escape returns exact opener scroll and focus without reload
        let closed = peek.clone().close_with_escape();
        assert_eq!(
            closed,
            PeekResolution::ClosedRestoreOpener {
                restored: opener_state,
                discarded_peek_tab: peek_tab,
                opener_reloaded: false,
            }
        );

        // Keeping as tab or beside in split never reloads
        assert_eq!(
            peek.clone().keep_as_tab(),
            PeekResolution::KeptAsTab {
                promoted_tab_id: peek_tab,
                reloaded: false,
            }
        );
        assert_eq!(
            peek.keep_beside_in_split(),
            PeekResolution::KeptBesideInSplit {
                left_tab_id: opener_tab,
                right_tab_id: peek_tab,
                reloaded: false,
            }
        );
    }

    #[test]
    fn small_windows_cascade_respect_escape_rules_and_move_into_row_on_cmd_o() {
        let mut mgr = SmallWindowsManager::new();
        let screen = WindowFrame {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };
        let space = SpaceId::new();
        let link_tab = TabId::new(10);
        let lifted_tab = TabId::new(11);
        let url = PageUrl::parse("https://example.com/post").expect("valid url");

        let w1 = mgr.open_small_window(
            link_tab,
            url.clone(),
            "Post 1",
            space,
            SmallWindowKind::ExternalLink,
            &screen,
        );
        assert!(!w1.cascaded);
        assert!(w1.shows_open_in_search());
        assert!(w1.closes_on_escape());

        let w2 = mgr.open_small_window(
            lifted_tab,
            url,
            "Lifted Tab",
            space,
            SmallWindowKind::LiftedTab {
                origin_space: space,
            },
            &screen,
        );
        assert!(w2.cascaded);
        assert_eq!(w2.frame.x, w1.frame.x + SMALL_WINDOW_CASCADE_STEP_PT);
        assert!(!w2.closes_on_escape());

        // Escape on lifted tab is ignored; Escape on link window closes and discards it
        assert_eq!(
            mgr.close_small_window(lifted_tab, true),
            Some(SmallWindowCloseOutcome::IgnoredEscapeOnLiftedTab)
        );
        assert_eq!(
            mgr.close_small_window(link_tab, true),
            Some(SmallWindowCloseOutcome::DiscardedLinkTab(link_tab))
        );

        // Cmd+O moves lifted tab back into main window row without reloading
        let moved = mgr.open_in_search(lifted_tab).expect("moved");
        assert_eq!(moved.tab_id, lifted_tab);
        assert!(!moved.reloaded);
        assert!(moved.activates_browser_window);
    }

    #[test]
    fn floating_video_plays_across_tab_switch_flicks_corners_and_returns_intact() {
        let mut ctrl = FloatingVideoController::new();
        let tab = TabId::new(20);
        let space = SpaceId::new();
        let url = PageUrl::parse("https://www.youtube.com/watch?v=abc").expect("valid url");

        assert!(FloatingVideoController::allows_private_tab_floating());

        // On Linux (engine_supports_native_pip = false), SystemPip falls back to SearchPanel
        let session = ctrl
            .try_start(
                tab,
                space,
                &url,
                true,
                true,
                120.0,
                FloatTrigger::LeaveTab,
                FloatStyle::SystemPip,
                false,
                true,
                false,
                &[],
            )
            .expect("should float");
        assert_eq!(session.effective_style, FloatStyle::SearchPanel);
        assert!(session.is_private_tab);

        assert_eq!(ctrl.skip_seconds(10.0), Some(10.0));
        assert_eq!(
            ctrl.flick_to_corner(-250.0, -180.0),
            Some(FloatCorner::TopLeft)
        );

        let returned = ctrl.return_to_tab().expect("returned");
        assert_eq!(returned.tab_id, tab);
        assert!(!returned.video_stopped);
    }
}
