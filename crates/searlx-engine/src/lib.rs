//! The engine contract: one tab owns one native web view and one data store.
//!
//! This crate defines types only. A backend lives in a platform crate; the app
//! composes one concrete backend. No engine type may leak into the UI: views
//! send commands and observe events through the app.

use searlx_core::{PageUrl, ProfileId, SpaceId, TabId};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub mod fake;

pub use fake::FakeEngine;

/// A result from an engine call.
pub type EngineResult<T> = std::result::Result<T, EngineError>;

/// An engine failure.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The tab handle is not known to this engine.
    #[error("no such tab: {0}")]
    NoSuchTab(TabId),

    /// The backend does not implement the requested capability.
    #[error("not supported by this engine: {0}")]
    NotSupported(&'static str),

    /// The backend rejected a request.
    #[error("engine error: {0}")]
    Backend(String),
}

/// A rectangle in window coordinates, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// Which JavaScript world a script runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptWorld {
    /// The page's own world, visible to the site.
    Page,
    /// searlx's isolated world; the page cannot see it.
    Isolated,
}

/// When a script is injected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InjectAt {
    /// Before the document starts parsing.
    DocumentStart,
    /// After the document has finished parsing.
    DocumentEnd,
}

/// Options for a new tab.
#[derive(Clone, Debug)]
pub struct TabOptions {
    /// Initial address; `None` makes a blank tab.
    pub url: Option<PageUrl>,
    /// Owning space.
    pub space: SpaceId,
    /// Owning profile.
    pub profile: ProfileId,
    /// Private tabs use an ephemeral store and write no durable state.
    pub private: bool,
    /// Open behind the active tab.
    pub background: bool,
    /// The tab that requested this one, when any.
    pub opener: Option<TabId>,
    /// A per-tab user agent override.
    pub user_agent: Option<String>,
}

impl Default for TabOptions {
    fn default() -> Self {
        Self {
            url: None,
            space: SpaceId::new(),
            profile: ProfileId::new(),
            private: false,
            background: false,
            opener: None,
            user_agent: None,
        }
    }
}

/// Find options for a page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindOptions {
    /// Match case.
    pub case_sensitive: bool,
    /// Whole words only.
    pub whole_words: bool,
    /// Search backwards from the current match.
    pub backwards: bool,
    /// Wrap at the end.
    pub wrap: bool,
}

/// The state of a find session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindState {
    /// The query this state describes.
    pub query: String,
    /// Matches found; capped at 1,000 by the contract.
    pub matches: u32,
    /// The one-based index of the active match, or 0 when none.
    pub active: u32,
    /// The search has finished (so `matches` is final).
    pub finished: bool,
}

/// A snapshot of a page as an encoded image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// PNG bytes.
    pub png: Vec<u8>,
}

/// Kinds of browsing data that can be cleared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataKinds {
    /// Cookies and site sign-ins.
    pub cookies: bool,
    /// HTTP, memory, and offline caches.
    pub cache: bool,
    /// Local storage, IndexedDB, and service workers.
    pub site_data: bool,
    /// History entries scoped to the store.
    pub history: bool,
    /// Favicons cached for the store.
    pub favicons: bool,
}

impl DataKinds {
    /// Everything this engine can clear.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            cookies: true,
            cache: true,
            site_data: true,
            history: true,
            favicons: true,
        }
    }

    /// Nothing.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            cookies: false,
            cache: false,
            site_data: false,
            history: false,
            favicons: false,
        }
    }

    /// Whether any kind is selected.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !(self.cookies || self.cache || self.site_data || self.history || self.favicons)
    }
}

/// An opaque handle to a website data store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DataStoreHandle {
    /// Stable id for this store within the engine.
    pub id: Uuid,
    /// Owning space.
    pub space: SpaceId,
    /// Owning profile.
    pub profile: ProfileId,
    /// Private stores are destroyed when their last tab closes.
    pub ephemeral: bool,
}

/// What a particular backend can do.
///
/// The UI hides or disables features from these flags. A missing capability is
/// never silently ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// The backend name, for the site card and the spike report.
    pub engine: String,
    /// Web extensions.
    pub extensions: bool,
    /// Platform passkeys / WebAuthn.
    pub passkeys: bool,
    /// Content-blocker rule lists compiled by the engine.
    pub content_blockers: bool,
    /// Native picture-in-picture.
    pub native_pip: bool,
    /// Per-site process isolation.
    pub site_isolation: bool,
    /// Per-tab user-agent overrides.
    pub custom_user_agent: bool,
    /// Printing.
    pub print: bool,
    /// PDF rendering.
    pub pdf: bool,
    /// Find inside a PDF.
    pub find_while_pdf: bool,
    /// Pausing and resuming downloads.
    pub downloads_pause: bool,
    /// Importing cookies from another browser.
    pub cookie_import: bool,
    /// Subsurface overlays on Wayland.
    pub wayland_overlay: bool,
    /// A page can be reparented into a floating player without reloading.
    pub floating_video: bool,
}

impl Capabilities {
    /// The minimum a backend must implement to be usable for Phase 0.
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

/// A permission a page may request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionKind {
    /// Camera.
    Camera,
    /// Microphone.
    Microphone,
    /// Location.
    Geolocation,
    /// Web notifications.
    Notifications,
    /// Screen capture.
    ScreenCapture,
    /// Clipboard read.
    ClipboardRead,
}

/// A JavaScript dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DialogKind {
    /// `alert()`.
    Alert,
    /// `confirm()`.
    Confirm,
    /// `prompt()`.
    Prompt,
    /// `beforeunload`.
    BeforeUnload,
}

/// What kind of new window a page asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NewWindowDisposition {
    /// A normal link or script request.
    ForegroundTab,
    /// A background tab.
    BackgroundTab,
    /// A sized popup window.
    Popup,
    /// A small window.
    SmallWindow,
}

/// Events emitted by an engine and drained by the app.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// A navigation began.
    NavigationStarted { tab: TabId, url: PageUrl },
    /// A navigation committed (the server responded).
    NavigationCommitted { tab: TabId, url: PageUrl },
    /// A navigation finished loading.
    NavigationFinished { tab: TabId, url: PageUrl },
    /// A navigation failed.
    NavigationFailed {
        tab: TabId,
        url: PageUrl,
        reason: String,
    },
    /// The page title changed.
    TitleChanged { tab: TabId, title: String },
    /// The address changed without a new document (history API or redirect).
    UrlChanged { tab: TabId, url: PageUrl },
    /// A favicon is available.
    FaviconChanged { tab: TabId, url: Option<PageUrl> },
    /// Loading state changed.
    LoadingChanged { tab: TabId, loading: bool },
    /// Back/forward availability changed.
    HistoryChanged {
        tab: TabId,
        can_go_back: bool,
        can_go_forward: bool,
    },
    /// The page asked for a new window or tab.
    NewWindowRequested {
        tab: TabId,
        url: PageUrl,
        disposition: NewWindowDisposition,
    },
    /// A download was requested.
    DownloadRequested {
        tab: TabId,
        url: PageUrl,
        suggested_name: Option<String>,
    },
    /// A page requested a permission.
    PermissionRequested {
        tab: TabId,
        origin: String,
        kind: PermissionKind,
    },
    /// A JavaScript dialog is waiting for an answer.
    ScriptDialog {
        tab: TabId,
        kind: DialogKind,
        message: String,
    },
    /// The page requested a file picker.
    FilePickerRequested { tab: TabId, multiple: bool },
    /// The page requested HTTP authentication.
    AuthRequired { tab: TabId, origin: String },
    /// A certificate error blocked a page.
    CertificateError {
        tab: TabId,
        url: PageUrl,
        reason: String,
    },
    /// Audio or mute state changed.
    MediaStateChanged {
        tab: TabId,
        audible: bool,
        muted: bool,
    },
    /// Fullscreen state changed.
    FullscreenChanged { tab: TabId, fullscreen: bool },
    /// The page process ended.
    ProcessCrashed { tab: TabId, reason: String },
    /// A script in a searlx world posted a message.
    ScriptMessage {
        tab: TabId,
        world: ScriptWorld,
        body: String,
    },
    /// A screenshot or page snapshot is ready.
    SnapshotReady { tab: TabId, image: Image },
}

impl fmt::Display for ScriptWorld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Page => f.write_str("page"),
            Self::Isolated => f.write_str("isolated"),
        }
    }
}

/// The engine contract.
///
/// A backend owns native views and its event queue. The app owns app state and
/// reconciles events; it never inspects a concrete engine type.
#[allow(clippy::missing_errors_doc)]
pub trait Engine: Send + 'static {
    /// Create a tab and its web view.
    fn create_tab(&mut self, options: TabOptions) -> EngineResult<TabId>;

    /// Close a tab and release its view. Its data store survives unless private.
    fn close_tab(&mut self, id: TabId) -> EngineResult<()>;

    /// Navigate a tab to a new address.
    fn navigate(&mut self, id: TabId, url: PageUrl) -> EngineResult<()>;

    /// Go back in the tab's history.
    fn back(&mut self, id: TabId) -> EngineResult<()>;

    /// Go forward in the tab's history.
    fn forward(&mut self, id: TabId) -> EngineResult<()>;

    /// Reload the page, optionally asking every resource for a fresh copy.
    fn reload(&mut self, id: TabId, from_origin: bool) -> EngineResult<()>;

    /// Stop loading.
    fn stop(&mut self, id: TabId) -> EngineResult<()>;

    /// Evaluate JavaScript for its side effects.
    fn eval(&mut self, id: TabId, world: ScriptWorld, js: &str) -> EngineResult<()>;

    /// Inject JavaScript at a defined point in every navigation.
    fn inject(&mut self, id: TabId, world: ScriptWorld, at: InjectAt, js: &str)
    -> EngineResult<()>;

    /// Place the page within the window.
    fn set_bounds(&mut self, id: TabId, rect: Rect) -> EngineResult<()>;

    /// Show or hide the page.
    fn set_visible(&mut self, id: TabId, visible: bool) -> EngineResult<()>;

    /// Give the page keyboard focus.
    fn focus(&mut self, id: TabId) -> EngineResult<()>;

    /// Capture the visible page as a PNG.
    fn snapshot(&mut self, id: TabId) -> EngineResult<Image>;

    /// Find text on the page.
    fn find(&mut self, id: TabId, query: &str, options: FindOptions) -> EngineResult<FindState>;

    /// Set the page zoom factor (1.0 is 100%).
    fn set_zoom(&mut self, id: TabId, zoom: f64) -> EngineResult<()>;

    /// Mute or unmute the page.
    fn set_muted(&mut self, id: TabId, muted: bool) -> EngineResult<()>;

    /// Get or create the store for a space/profile pair.
    fn data_store(
        &mut self,
        space: SpaceId,
        profile: ProfileId,
        private: bool,
    ) -> EngineResult<DataStoreHandle>;

    /// Clear selected kinds of data from a store.
    fn clear_data(&mut self, store: DataStoreHandle, kinds: DataKinds) -> EngineResult<()>;

    /// The backend's capabilities.
    fn capabilities(&self) -> Capabilities;

    /// Drain queued events. This must not block.
    fn pump(&mut self) -> EngineResult<Vec<EngineEvent>>;
}
