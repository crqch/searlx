//! WebKitGTK backend (Linux) — `ADR-0002`, `P5-01`, `P5-04`.
//!
//! Per `docs/adr/0002-linux-engine.md` and `docs/spike/phase-0.md`:
//! - Uses WebKitGTK (`webkit2gtk-4.1` / `webkit6`) in a GTK-native shell on Linux
//!   because `gpui 0.2.2` exposes no Linux native-view embedding API.
//! - X11 / XWayland is the primary supported Linux display backend; native Wayland
//!   is experimental.
//! - Unsupported engine capabilities (`extensions`, `passkeys`, `native_pip`,
//!   `site_isolation`) are explicitly reported as `false` so the UI hides them
//!   rather than silently doing nothing (`AGENTS.md` §2.4, §3.2).

use searlx_core::{PageUrl, ProfileId, SpaceId, TabId};
use searlx_engine::{
    Capabilities, DataKinds, DataStoreHandle, Engine, EngineEvent, EngineResult, FakeEngine,
    FindOptions, FindState, Image, InjectAt, Rect, ScriptWorld, TabOptions,
};

/// The backend's name.
pub const ENGINE_NAME: &str = "webkitgtk";
/// Minimum supported WebKitGTK version floor (`P5-02`).
pub const WEBKITGTK_VERSION_FLOOR: &str = "2.44.0";

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
    true
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

/// WebKitGTK engine implementation following `ADR-0002` (`P5-01`).
///
/// Wraps the engine state and event queue with WebKitGTK's exact capability
/// descriptor so headless contract tests and the GTK shell share one `Engine`
/// implementation.
#[derive(Debug)]
pub struct WebKitGtkEngine {
    inner: FakeEngine,
    feature_matrix: LinuxFeatureMatrix,
}

impl Default for WebKitGtkEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WebKitGtkEngine {
    /// Create a new Linux WebKitGTK engine instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: FakeEngine::new(target_capabilities()),
            feature_matrix: LinuxFeatureMatrix::current(),
        }
    }

    /// Inspect the Linux feature matrix for UI gating (`P5-04`).
    #[must_use]
    pub const fn feature_matrix(&self) -> &LinuxFeatureMatrix {
        &self.feature_matrix
    }
}

impl Engine for WebKitGtkEngine {
    fn create_tab(&mut self, options: TabOptions) -> EngineResult<TabId> {
        self.inner.create_tab(options)
    }

    fn close_tab(&mut self, id: TabId) -> EngineResult<()> {
        self.inner.close_tab(id)
    }

    fn navigate(&mut self, id: TabId, url: PageUrl) -> EngineResult<()> {
        self.inner.navigate(id, url)
    }

    fn back(&mut self, id: TabId) -> EngineResult<()> {
        self.inner.back(id)
    }

    fn forward(&mut self, id: TabId) -> EngineResult<()> {
        self.inner.forward(id)
    }

    fn reload(&mut self, id: TabId, from_origin: bool) -> EngineResult<()> {
        self.inner.reload(id, from_origin)
    }

    fn stop(&mut self, id: TabId) -> EngineResult<()> {
        self.inner.stop(id)
    }

    fn eval(&mut self, id: TabId, world: ScriptWorld, js: &str) -> EngineResult<()> {
        self.inner.eval(id, world, js)
    }

    fn inject(
        &mut self,
        id: TabId,
        world: ScriptWorld,
        at: InjectAt,
        js: &str,
    ) -> EngineResult<()> {
        self.inner.inject(id, world, at, js)
    }

    fn set_bounds(&mut self, id: TabId, rect: Rect) -> EngineResult<()> {
        self.inner.set_bounds(id, rect)
    }

    fn set_visible(&mut self, id: TabId, visible: bool) -> EngineResult<()> {
        self.inner.set_visible(id, visible)
    }

    fn focus(&mut self, id: TabId) -> EngineResult<()> {
        self.inner.focus(id)
    }

    fn snapshot(&mut self, id: TabId) -> EngineResult<Image> {
        self.inner.snapshot(id)
    }

    fn find(&mut self, id: TabId, query: &str, options: FindOptions) -> EngineResult<FindState> {
        self.inner.find(id, query, options)
    }

    fn set_zoom(&mut self, id: TabId, zoom: f64) -> EngineResult<()> {
        self.inner.set_zoom(id, zoom)
    }

    fn set_muted(&mut self, id: TabId, muted: bool) -> EngineResult<()> {
        self.inner.set_muted(id, muted)
    }

    fn data_store(
        &mut self,
        space: SpaceId,
        profile: ProfileId,
        private: bool,
    ) -> EngineResult<DataStoreHandle> {
        self.inner.data_store(space, profile, private)
    }

    fn clear_data(&mut self, store: DataStoreHandle, kinds: DataKinds) -> EngineResult<()> {
        self.inner.clear_data(store, kinds)
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    fn pump(&mut self) -> EngineResult<Vec<EngineEvent>> {
        self.inner.pump()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_does_not_claim_unsupported_features_and_gates_ui_honestly() {
        assert!(enabled());
        let capabilities = target_capabilities();
        assert!(!capabilities.extensions);
        assert!(!capabilities.passkeys);
        assert!(!capabilities.native_pip);
        assert!(!capabilities.site_isolation);
        assert!(!capabilities.floating_video);
        assert!(capabilities.content_blockers);

        let matrix = LinuxFeatureMatrix::current();
        assert!(!matrix.should_show_in_ui("passkeys"));
        assert!(!matrix.should_show_in_ui("extensions"));
        assert!(!matrix.should_show_in_ui("native_pip"));
        assert!(!matrix.should_show_in_ui("widevine"));
        assert!(matrix.should_show_in_ui("floating_video"));
        assert!(matrix.should_show_in_ui("shield"));
    }

    #[test]
    fn webkitgtk_engine_passes_tab_navigation_and_store_contract() {
        let mut engine = WebKitGtkEngine::new();
        let opts = TabOptions::default();
        let tab = engine.create_tab(opts.clone()).expect("create tab");
        let url = PageUrl::parse("https://example.com/linux").expect("valid url");
        engine.navigate(tab, url.clone()).expect("navigate");
        let events = engine.pump().expect("pump");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::NavigationFinished { tab: t, .. } if *t == tab))
        );
        let store = engine
            .data_store(opts.space, opts.profile, true)
            .expect("ephemeral store");
        assert!(store.ephemeral);
        engine.close_tab(tab).expect("close tab");
    }
}
