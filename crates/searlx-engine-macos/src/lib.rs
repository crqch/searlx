//! WKWebView backend (macOS) — `P0-03`, `P1-04`.
//!
//! Provides the `Engine` capability descriptor and non-native unsupported
//! handling for macOS (`WkWebViewEngine`). Physical `objc2-web-kit` window
//! embedding verification requires a macOS host (`TODO.md` P0-03, P1-04).

use searlx_core::{PageUrl, ProfileId, SpaceId, TabId};
use searlx_engine::{
    Capabilities, DataKinds, DataStoreHandle, Engine, EngineError, EngineEvent, EngineResult,
    FindOptions, FindState, Image, InjectAt, Rect, ScriptWorld, TabOptions,
};

/// The backend's name.
pub const ENGINE_NAME: &str = "wkwebview";

#[cfg(not(target_os = "macos"))]
const UNSUPPORTED_MSG: &str = "WKWebView is only supported on macOS";
#[cfg(target_os = "macos")]
const UNSUPPORTED_MSG: &str =
    "WKWebView native objc2-web-kit backend is not yet wired on this build";

/// Capabilities supported by macOS `WKWebView` (`FEATURES.md` §20, `docs/spike/capabilities.md`).
#[must_use]
pub fn target_capabilities() -> Capabilities {
    let mut capabilities = Capabilities::baseline(ENGINE_NAME);
    capabilities.extensions = true;
    capabilities.passkeys = true;
    capabilities.content_blockers = true;
    capabilities.native_pip = true;
    capabilities.site_isolation = true;
    capabilities.custom_user_agent = true;
    capabilities.print = true;
    capabilities.pdf = true;
    capabilities.find_while_pdf = true;
    capabilities.downloads_pause = true;
    capabilities.cookie_import = true;
    capabilities.floating_video = true;
    capabilities
}

/// macOS `WKWebView` engine handle behind `searlx_engine::Engine` (`P1-04`).
///
/// On non-macOS hosts (and until native `objc2-web-kit` bindings are wired on
/// macOS), every engine operation honestly returns [`EngineError::NotSupported`]
/// rather than delegating to a fake engine (`AGENTS.md` §2.4).
#[derive(Debug, Default)]
pub struct WkWebViewEngine {
    _private: (),
}

impl WkWebViewEngine {
    /// Construct a `WkWebViewEngine` handle.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Attempt to initialize a live native `WKWebView` engine.
    ///
    /// Returns [`EngineError::NotSupported`] on non-macOS platforms or when the
    /// native `objc2-web-kit` backend is unavailable.
    pub fn try_new() -> EngineResult<Self> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }
}

impl Engine for WkWebViewEngine {
    fn create_tab(&mut self, _options: TabOptions) -> EngineResult<TabId> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn close_tab(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn navigate(&mut self, _id: TabId, _url: PageUrl) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn back(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn forward(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn reload(&mut self, _id: TabId, _from_origin: bool) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn stop(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn eval(&mut self, _id: TabId, _world: ScriptWorld, _js: &str) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn inject(
        &mut self,
        _id: TabId,
        _world: ScriptWorld,
        _at: InjectAt,
        _js: &str,
    ) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn set_bounds(&mut self, _id: TabId, _rect: Rect) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn set_visible(&mut self, _id: TabId, _visible: bool) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn focus(&mut self, _id: TabId) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn snapshot(&mut self, _id: TabId) -> EngineResult<Image> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn find(&mut self, _id: TabId, _query: &str, _options: FindOptions) -> EngineResult<FindState> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn set_zoom(&mut self, _id: TabId, _zoom: f64) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn set_muted(&mut self, _id: TabId, _muted: bool) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn data_store(
        &mut self,
        _space: SpaceId,
        _profile: ProfileId,
        _private: bool,
    ) -> EngineResult<DataStoreHandle> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn clear_data(&mut self, _store: DataStoreHandle, _kinds: DataKinds) -> EngineResult<()> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }

    fn capabilities(&self) -> Capabilities {
        target_capabilities()
    }

    fn pump(&mut self) -> EngineResult<Vec<EngineEvent>> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_capabilities_describe_wkwebview_accurately() {
        let caps = target_capabilities();
        assert_eq!(caps.engine, ENGINE_NAME);
        assert!(caps.extensions);
        assert!(caps.passkeys);
        assert!(caps.content_blockers);
        assert!(caps.native_pip);
        assert!(caps.site_isolation);
        assert!(caps.custom_user_agent);
        assert!(caps.print);
        assert!(caps.pdf);
        assert!(caps.find_while_pdf);
        assert!(caps.downloads_pause);
        assert!(caps.cookie_import);
        assert!(caps.floating_video);
        assert!(!caps.wayland_overlay);
    }

    #[test]
    fn wkwebview_engine_honestly_reports_unsupported_without_native_binding() {
        assert!(matches!(
            WkWebViewEngine::try_new(),
            Err(EngineError::NotSupported(_))
        ));

        let mut engine = WkWebViewEngine::new();
        assert_eq!(engine.capabilities(), target_capabilities());
        assert!(matches!(
            engine.create_tab(TabOptions::default()),
            Err(EngineError::NotSupported(_))
        ));
        assert!(matches!(
            engine.data_store(SpaceId::new(), ProfileId::new(), true),
            Err(EngineError::NotSupported(_))
        ));
        assert!(matches!(engine.pump(), Err(EngineError::NotSupported(_))));
    }
}
