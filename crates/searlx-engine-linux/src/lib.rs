//! WebKitGTK backend (Linux) — `ADR-0002`, `P5-01`, `P5-04`.
//!
//! Per `docs/adr/0002-linux-engine.md` and `docs/spike/phase-0.md`:
//! - Uses WebKitGTK (`webkit2gtk-4.1`) in a GTK-native shell on Linux because
//!   `gpui 0.2.2` exposes no Linux native-view embedding API.
//! - X11 / XWayland is the primary supported Linux display backend; native Wayland
//!   is experimental.
//! - Unsupported engine capabilities (`extensions`, `passkeys`, `native_pip`,
//!   `site_isolation`) are explicitly reported as `false` so the UI hides them
//!   rather than silently doing nothing (`AGENTS.md` §2.4, §3.2).

#[cfg(target_os = "linux")]
mod filters;
#[cfg(target_os = "linux")]
mod runtime;
#[cfg(target_os = "linux")]
mod webkit_engine;

#[cfg(target_os = "linux")]
pub use webkit_engine::TabDiagnostics;

use std::path::PathBuf;
use std::sync::Arc;

use searlx_core::{PageUrl, ProfileId, SpaceId, TabId};
use searlx_engine::{
    Capabilities, DataKinds, DataStoreHandle, Engine, EngineError, EngineEvent, EngineResult,
    FindOptions, FindState, Image, InjectAt, Rect, ScriptWorld, TabOptions,
};

/// Temporary stub keeping `apps/searlx/src/main.rs` compiling until Wave 2
/// constructs the clean GTK shell directly in `apps/searlx`.
pub mod browser {
    /// Launch placeholder until the Wave 2 GTK shell replaces the monolith.
    pub fn run_gtk_browser(_: &[String]) -> Result<(), String> {
        Err("Linux GTK shell rewrite pending Wave 2".into())
    }
}

/// The backend's name.
pub const ENGINE_NAME: &str = "webkitgtk";
/// Minimum supported WebKitGTK version floor (`P5-02`).
pub const WEBKITGTK_VERSION_FLOOR: &str = "2.44.0";

/// Response returned by an internal `search://` URI scheme handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalPageResponse {
    /// MIME type passed to WebKitGTK (for example `"text/html; charset=utf-8"`).
    pub mime_type: String,
    /// Raw response body bytes.
    pub body: Vec<u8>,
}

impl InternalPageResponse {
    /// Construct an HTML internal page response.
    #[must_use]
    pub fn html(html: impl Into<String>) -> Self {
        Self {
            mime_type: "text/html; charset=utf-8".to_owned(),
            body: html.into().into_bytes(),
        }
    }

    /// Construct a plain-text internal page response.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            mime_type: "text/plain; charset=utf-8".to_owned(),
            body: text.into().into_bytes(),
        }
    }

    /// Construct an arbitrary MIME-typed response.
    #[must_use]
    pub fn bytes(mime_type: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            mime_type: mime_type.into(),
            body: body.into(),
        }
    }
}

/// Callback invoked by WebKitGTK when a tab requests a `search://` URI.
pub type InternalUriHandler = Arc<dyn Fn(&str) -> InternalPageResponse + Send + Sync>;

/// What this backend supports, based on the Phase 0 measurements and the
/// documented WebKitGTK API (`P0-10`, `P5-01`, `P5-04`).
#[must_use]
pub fn target_capabilities() -> Capabilities {
    let mut capabilities = Capabilities::baseline(ENGINE_NAME);
    capabilities.content_blockers = true;
    capabilities.custom_user_agent = true;
    capabilities.print = true;
    capabilities.pdf = true;
    capabilities
}

/// Whether the Linux WebKitGTK engine path (`ADR-0002`) is enabled.
#[must_use]
pub const fn enabled() -> bool {
    cfg!(target_os = "linux")
}

/// Resolve the default Linux WebKitGTK data root (`$XDG_DATA_HOME/searlx/webkitgtk`
/// or `~/.local/share/searlx/webkitgtk`).
#[must_use]
pub fn default_data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg).join("searlx").join("webkitgtk");
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("searlx")
            .join("webkitgtk");
    }
    std::env::temp_dir().join("searlx-webkitgtk")
}

/// Linux feature availability matrix (`P5-04`, `README.md`).
///
/// The UI consults this matrix to hide or label Linux-specific gaps honestly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct LinuxFeatureMatrix {
    /// WebAuthn / Passkeys (`false` in WebKitGTK today; `ADR-0005`).
    pub passkeys: bool,
    /// Chrome / WebExtensions (`false` in WebKitGTK; `ADR-0003`).
    pub extensions: bool,
    /// Native OS Picture-in-Picture (`false` on WebKitGTK 2.54.2; uses Search's floating panel fallback).
    pub native_pip: bool,
    /// Search's own floating video panel (`true` via GTK top-level utility window).
    pub search_floating_video_panel: bool,
    /// Ad & tracker blocking (`true` via `WebKitUserContentFilterStore` + request filter).
    pub content_blockers: bool,
    /// GStreamer hardware-accelerated video decoding (`true` via VA-API / V4L2 / NVDEC).
    pub hardware_video_decode: bool,
    /// PipeWire screen-sharing portal (`true` via `xdg-desktop-portal`).
    pub pipewire_screen_share: bool,
    /// Encrypted Media Extensions / Widevine DRM (`false` by default; unbundled).
    pub widevine_drm: bool,
}

impl Default for LinuxFeatureMatrix {
    fn default() -> Self {
        Self::current()
    }
}

impl LinuxFeatureMatrix {
    /// Return the measured Linux feature matrix (`P5-04`).
    #[must_use]
    pub const fn current() -> Self {
        Self {
            passkeys: false,
            extensions: false,
            native_pip: false,
            search_floating_video_panel: true,
            content_blockers: true,
            hardware_video_decode: true,
            pipewire_screen_share: true,
            widevine_drm: false,
        }
    }

    /// Whether a named optional feature should be shown in the Linux UI (`P5-04`).
    #[must_use]
    pub fn should_show_in_ui(&self, feature_key: &str) -> bool {
        match feature_key {
            "passkeys" => self.passkeys,
            "extensions" => self.extensions,
            "native_pip" => self.native_pip,
            "floating_video" => self.search_floating_video_panel,
            "content_blockers" | "shield" => self.content_blockers,
            "hardware_video" => self.hardware_video_decode,
            "screen_share" => self.pipewire_screen_share,
            "drm" | "widevine" => self.widevine_drm,
            _ => true,
        }
    }
}

/// Real WebKitGTK engine implementation following `ADR-0002` (`P5-01`).
///
/// Implements `searlx_engine::Engine` (`Send + 'static`) safely by confining
/// `!Send` GTK/WebKitGTK objects to the GTK main thread via a thread-local
/// registry and `glib::MainContext` dispatch.
#[cfg(target_os = "linux")]
pub struct WebKitGtkEngine {
    id: u64,
    mode: runtime::DispatchMode,
    feature_matrix: LinuxFeatureMatrix,
    events: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<EngineEvent>>>,
    internal_handler: std::sync::Arc<std::sync::RwLock<Option<InternalUriHandler>>>,
}

#[cfg(target_os = "linux")]
impl std::fmt::Debug for WebKitGtkEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebKitGtkEngine")
            .field("id", &self.id)
            .field("mode", &self.mode)
            .field("feature_matrix", &self.feature_matrix)
            .finish()
    }
}

#[cfg(target_os = "linux")]
impl WebKitGtkEngine {
    /// Create a new Linux WebKitGTK engine instance using the default XDG data directory.
    ///
    /// If GTK has already been initialized on the current thread (e.g., by the
    /// GTK browser shell), the engine binds directly to this main thread;
    /// otherwise a shared GTK main-loop worker thread is started.
    pub fn new() -> EngineResult<Self> {
        Self::with_data_root(default_data_dir())
    }

    /// Alias for [`WebKitGtkEngine::new`].
    pub fn try_new() -> EngineResult<Self> {
        Self::new()
    }

    /// Create a new WebKitGTK engine storing persistent site data under `data_root`.
    pub fn with_data_root(data_root: PathBuf) -> EngineResult<Self> {
        let mode = runtime::select_dispatch_mode()?;
        Self::init_with_mode(mode, data_root)
    }

    /// Initialize GTK on the calling thread (if not already initialized) and create
    /// a main-thread-bound `WebKitGtkEngine` for a GTK shell.
    pub fn new_on_current_thread() -> EngineResult<Self> {
        Self::with_data_root_on_current_thread(default_data_dir())
    }

    /// Initialize GTK on the calling thread and create a `WebKitGtkEngine` with
    /// persistent site data stored under `data_root`.
    pub fn with_data_root_on_current_thread(data_root: PathBuf) -> EngineResult<Self> {
        let mode = runtime::init_current_thread_for_gtk()?;
        Self::init_with_mode(mode, data_root)
    }

    fn init_with_mode(mode: runtime::DispatchMode, data_root: PathBuf) -> EngineResult<Self> {
        let id = runtime::allocate_engine_id();
        let events = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let internal_handler = std::sync::Arc::new(std::sync::RwLock::new(None));
        let events_clone = events.clone();
        let handler_clone = internal_handler.clone();

        runtime::create_and_register_engine(mode, id, move || {
            webkit_engine::InnerWebKitEngine::new(data_root, events_clone, handler_clone)
        })?;

        Ok(Self {
            id,
            mode,
            feature_matrix: LinuxFeatureMatrix::current(),
            events,
            internal_handler,
        })
    }

    /// Inspect the Linux feature matrix for UI gating (`P5-04`).
    #[must_use]
    pub const fn feature_matrix(&self) -> &LinuxFeatureMatrix {
        &self.feature_matrix
    }

    /// Return the underlying `gtk::Widget` for `id` when called on the GTK main
    /// thread (`ADR-0002`). Returns `None` if `id` is unknown or called off-thread.
    #[must_use]
    pub fn webview_widget(&self, id: TabId) -> Option<gtk::Widget> {
        runtime::with_local_engine_ref(self.id, |engine| engine.webview_widget(id)).flatten()
    }

    /// Register a handler serving `search://` internal pages across every
    /// persistent and ephemeral `WebContext` managed by this engine.
    pub fn register_internal_handler<F>(&mut self, handler: F) -> EngineResult<()>
    where
        F: Fn(&str) -> InternalPageResponse + Send + Sync + 'static,
    {
        let mut guard = self
            .internal_handler
            .write()
            .map_err(|_| EngineError::Backend("internal URI handler lock poisoned".into()))?;
        *guard = Some(Arc::new(handler));
        Ok(())
    }

    /// Compile and apply WebKit content-blocker rules (`compile_webkit_content_rules`)
    /// via `WebKitUserContentFilterStore` across all existing and future tabs.
    /// Passing an empty string or `"[]"` removes the filter.
    pub fn apply_content_rules(&mut self, identifier: &str, rules_json: &str) -> EngineResult<()> {
        let engine_id = self.id;
        let identifier = identifier.to_owned();
        let rules_json = rules_json.to_owned();
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine_cell(engine_id, |engine| {
                webkit_engine::InnerWebKitEngine::apply_content_rules_cell(
                    engine,
                    &identifier,
                    &rules_json,
                )
            })
        })
    }

    /// Inject a user stylesheet (`UserStyleSheet`) into tab `id`.
    pub fn apply_user_stylesheet(
        &mut self,
        id: TabId,
        css: &str,
        all_frames: bool,
        user_level: bool,
    ) -> EngineResult<()> {
        let engine_id = self.id;
        let css = css.to_owned();
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| {
                engine.apply_user_stylesheet(id, &css, all_frames, user_level)
            })
        })
    }

    /// Remove all injected user stylesheets from tab `id`.
    pub fn clear_user_stylesheets(&mut self, id: TabId) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.clear_user_stylesheets(id))
        })
    }

    /// Configure the directory where WebKitGTK saves accepted downloads.
    pub fn set_downloads_dir(&mut self, dir: PathBuf) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| {
                engine.set_downloads_dir(dir);
                Ok(())
            })
        })
    }

    /// Terminate the underlying WebKitWebProcess for `id` (emits `ProcessCrashed`).
    pub fn terminate_tab_process(&mut self, id: TabId) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.terminate_tab_process(id))
        })
    }

    /// Inspect runtime state for `id` (bounds, visibility, zoom, mute, ephemerality).
    pub fn tab_diagnostics(&self, id: TabId) -> EngineResult<Option<TabDiagnostics>> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| Ok(engine.tab_diagnostics(id)))
        })
    }
}

#[cfg(target_os = "linux")]
impl Drop for WebKitGtkEngine {
    fn drop(&mut self) {
        runtime::unregister_engine(self.id, self.mode);
    }
}

#[cfg(target_os = "linux")]
impl Engine for WebKitGtkEngine {
    fn create_tab(&mut self, options: TabOptions) -> EngineResult<TabId> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.create_tab(options))
        })
    }

    fn close_tab(&mut self, id: TabId) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.close_tab(id))
        })
    }

    fn navigate(&mut self, id: TabId, url: PageUrl) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.navigate(id, url))
        })
    }

    fn back(&mut self, id: TabId) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.back(id))
        })
    }

    fn forward(&mut self, id: TabId) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.forward(id))
        })
    }

    fn reload(&mut self, id: TabId, from_origin: bool) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.reload(id, from_origin))
        })
    }

    fn stop(&mut self, id: TabId) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.stop(id))
        })
    }

    fn eval(&mut self, id: TabId, world: ScriptWorld, js: &str) -> EngineResult<()> {
        let engine_id = self.id;
        let js = js.to_owned();
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.eval(id, world, &js))
        })
    }

    fn inject(
        &mut self,
        id: TabId,
        world: ScriptWorld,
        at: InjectAt,
        js: &str,
    ) -> EngineResult<()> {
        let engine_id = self.id;
        let js = js.to_owned();
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.inject(id, world, at, &js))
        })
    }

    fn set_bounds(&mut self, id: TabId, rect: Rect) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.set_bounds(id, rect))
        })
    }

    fn set_visible(&mut self, id: TabId, visible: bool) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.set_visible(id, visible))
        })
    }

    fn focus(&mut self, id: TabId) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.focus(id))
        })
    }

    fn snapshot(&mut self, id: TabId) -> EngineResult<Image> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine_cell(engine_id, |engine| {
                webkit_engine::InnerWebKitEngine::snapshot_cell(engine, id)
            })
        })
    }

    fn find(&mut self, id: TabId, query: &str, options: FindOptions) -> EngineResult<FindState> {
        let engine_id = self.id;
        let query = query.to_owned();
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine_cell(engine_id, |engine| {
                webkit_engine::InnerWebKitEngine::find_cell(engine, id, &query, options)
            })
        })
    }

    fn set_zoom(&mut self, id: TabId, zoom: f64) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.set_zoom(id, zoom))
        })
    }

    fn set_muted(&mut self, id: TabId, muted: bool) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| engine.set_muted(id, muted))
        })
    }

    fn data_store(
        &mut self,
        space: SpaceId,
        profile: ProfileId,
        private: bool,
    ) -> EngineResult<DataStoreHandle> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine(engine_id, |engine| {
                engine.data_store(space, profile, private)
            })
        })
    }

    fn clear_data(&mut self, store: DataStoreHandle, kinds: DataKinds) -> EngineResult<()> {
        let engine_id = self.id;
        runtime::run_on_gtk_thread(self.mode, move || {
            runtime::with_local_engine_cell(engine_id, |engine| {
                webkit_engine::InnerWebKitEngine::clear_data_cell(engine, store, kinds)
            })
        })
    }

    fn capabilities(&self) -> Capabilities {
        target_capabilities()
    }

    fn pump(&mut self) -> EngineResult<Vec<EngineEvent>> {
        if gtk::is_initialized_main_thread() {
            runtime::pump_pending_glib_events();
        } else {
            runtime::run_on_gtk_thread(self.mode, || {
                runtime::pump_pending_glib_events();
                Ok(())
            })?;
        }
        let mut guard = self
            .events
            .lock()
            .map_err(|_| EngineError::Backend("engine event queue poisoned".into()))?;
        Ok(guard.drain(..).collect())
    }
}

/// Non-Linux compile stub: honestly returns `EngineError::NotSupported`.
#[cfg(not(target_os = "linux"))]
#[derive(Debug, Default)]
pub struct WebKitGtkEngine {
    feature_matrix: LinuxFeatureMatrix,
}

#[cfg(not(target_os = "linux"))]
impl WebKitGtkEngine {
    /// Attempt to initialize WebKitGTK (unsupported outside Linux).
    pub fn new() -> EngineResult<Self> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    /// Alias for [`WebKitGtkEngine::new`].
    pub fn try_new() -> EngineResult<Self> {
        Self::new()
    }

    /// Inspect the Linux feature matrix (`P5-04`).
    #[must_use]
    pub const fn feature_matrix(&self) -> &LinuxFeatureMatrix {
        &self.feature_matrix
    }
}

#[cfg(not(target_os = "linux"))]
impl Engine for WebKitGtkEngine {
    fn create_tab(&mut self, _options: TabOptions) -> EngineResult<TabId> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn close_tab(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn navigate(&mut self, _id: TabId, _url: PageUrl) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn back(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn forward(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn reload(&mut self, _id: TabId, _from_origin: bool) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn stop(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn eval(&mut self, _id: TabId, _world: ScriptWorld, _js: &str) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn inject(
        &mut self,
        _id: TabId,
        _world: ScriptWorld,
        _at: InjectAt,
        _js: &str,
    ) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn set_bounds(&mut self, _id: TabId, _rect: Rect) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn set_visible(&mut self, _id: TabId, _visible: bool) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn focus(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn snapshot(&mut self, _id: TabId) -> EngineResult<Image> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn find(&mut self, _id: TabId, _query: &str, _options: FindOptions) -> EngineResult<FindState> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn set_zoom(&mut self, _id: TabId, _zoom: f64) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn set_muted(&mut self, _id: TabId, _muted: bool) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn data_store(
        &mut self,
        _space: SpaceId,
        _profile: ProfileId,
        _private: bool,
    ) -> EngineResult<DataStoreHandle> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn clear_data(&mut self, _store: DataStoreHandle, _kinds: DataKinds) -> EngineResult<()> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }

    fn capabilities(&self) -> Capabilities {
        target_capabilities()
    }

    fn pump(&mut self) -> EngineResult<Vec<EngineEvent>> {
        Err(EngineError::NotSupported(
            "WebKitGTK is only supported on Linux",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_engine_send_static<T: Engine + Send + 'static>() {}

    #[test]
    fn engine_implements_send_and_static_without_leaking_gtk_types() {
        assert_engine_send_static::<WebKitGtkEngine>();
    }

    #[test]
    fn linux_does_not_claim_unsupported_features_and_gates_ui_honestly() {
        assert_eq!(enabled(), cfg!(target_os = "linux"));
        let capabilities = target_capabilities();
        assert_eq!(capabilities.engine, ENGINE_NAME);
        assert!(!capabilities.extensions);
        assert!(!capabilities.passkeys);
        assert!(!capabilities.native_pip);
        assert!(!capabilities.site_isolation);
        assert!(!capabilities.floating_video);
        assert!(capabilities.content_blockers);
        assert!(capabilities.custom_user_agent);
        assert!(capabilities.print);
        assert!(capabilities.pdf);

        let matrix = LinuxFeatureMatrix::current();
        assert!(!matrix.should_show_in_ui("passkeys"));
        assert!(!matrix.should_show_in_ui("extensions"));
        assert!(!matrix.should_show_in_ui("native_pip"));
        assert!(!matrix.should_show_in_ui("widevine"));
        assert!(matrix.should_show_in_ui("floating_video"));
        assert!(matrix.should_show_in_ui("shield"));
        assert!(matrix.should_show_in_ui("hardware_video"));
        assert!(matrix.should_show_in_ui("screen_share"));
    }

    #[test]
    fn temporary_browser_stub_returns_explicit_wave2_error() {
        let err = browser::run_gtk_browser(&[]).expect_err("placeholder must fail honestly");
        assert!(err.contains("Wave 2"));
    }

    #[test]
    fn internal_page_response_constructors_set_expected_mime_types() {
        let html = InternalPageResponse::html("<h1>hi</h1>");
        assert_eq!(html.mime_type, "text/html; charset=utf-8");
        assert_eq!(html.body, b"<h1>hi</h1>");

        let text = InternalPageResponse::text("plain");
        assert_eq!(text.mime_type, "text/plain; charset=utf-8");
        assert_eq!(text.body, b"plain");

        let custom = InternalPageResponse::bytes("application/json", b"{}".to_vec());
        assert_eq!(custom.mime_type, "application/json");
        assert_eq!(custom.body, b"{}");
    }
}
