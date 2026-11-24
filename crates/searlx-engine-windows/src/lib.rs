//! WebView2 backend (Windows) — `P0-04`, `P1-05`.
//!
//! Provides the `Engine` capability descriptor, per-(profile, space) user data
//! folder resolution, and non-native unsupported handling for Windows
//! (`WebView2Engine`). Physical Windows 11 WebView2 child-window overlay
//! verification requires a Windows host (`TODO.md` P0-04, P1-05).

use std::path::{Path, PathBuf};

use searlx_core::{PageUrl, ProfileId, SpaceId, TabId};
use searlx_engine::{
    Capabilities, DataKinds, DataStoreHandle, Engine, EngineError, EngineEvent, EngineResult,
    FindOptions, FindState, Image, InjectAt, Rect, ScriptWorld, TabOptions,
};

/// The backend's name.
pub const ENGINE_NAME: &str = "webview2";

#[cfg(not(target_os = "windows"))]
const UNSUPPORTED_MSG: &str = "WebView2 is only supported on Windows";
#[cfg(target_os = "windows")]
const UNSUPPORTED_MSG: &str = "WebView2 native windows-rs backend is not yet wired on this build";

/// Capabilities supported by Windows `WebView2` (`docs/spike/capabilities.md`).
#[must_use]
pub fn target_capabilities() -> Capabilities {
    let mut capabilities = Capabilities::baseline(ENGINE_NAME);
    capabilities.passkeys = true;
    capabilities.content_blockers = false;
    capabilities.native_pip = true;
    capabilities.custom_user_agent = true;
    capabilities.print = true;
    capabilities.pdf = true;
    capabilities.downloads_pause = true;
    capabilities.cookie_import = true;
    capabilities
}

/// Resolve the WebView2 user-data folder path for `(profile, space)`, or `None` for ephemeral InPrivate tabs (`P1-05`).
#[must_use]
pub fn webview2_user_data_folder(
    base_app_data: &Path,
    profile: ProfileId,
    space: SpaceId,
    is_private: bool,
) -> Option<PathBuf> {
    if is_private {
        None
    } else {
        Some(
            base_app_data
                .join("searlx")
                .join("WebView2")
                .join(format!("profile-{profile}"))
                .join(format!("space-{space}")),
        )
    }
}

/// Windows `WebView2` engine handle behind `searlx_engine::Engine` (`P1-05`).
///
/// On non-Windows hosts (and until native `windows-rs` WebView2 bindings are
/// wired on Windows), every engine operation honestly returns
/// [`EngineError::NotSupported`] rather than delegating to a fake engine
/// (`AGENTS.md` §2.4).
#[derive(Debug, Default)]
pub struct WebView2Engine {
    _private: (),
}

impl WebView2Engine {
    /// Construct a `WebView2Engine` handle.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Attempt to initialize a live native `WebView2` engine.
    ///
    /// Returns [`EngineError::NotSupported`] on non-Windows platforms or when
    /// the native `windows-rs` WebView2 backend is unavailable.
    pub fn try_new() -> EngineResult<Self> {
        Err(EngineError::NotSupported(UNSUPPORTED_MSG))
    }
}

impl Engine for WebView2Engine {
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
    fn target_capabilities_and_user_data_folder_isolation_are_accurate() {
        let caps = target_capabilities();
        assert_eq!(caps.engine, ENGINE_NAME);
        assert!(caps.passkeys);
        assert!(!caps.content_blockers);
        assert!(caps.native_pip);
        assert!(caps.custom_user_agent);
        assert!(caps.print);
        assert!(caps.pdf);
        assert!(caps.downloads_pause);
        assert!(caps.cookie_import);
        assert!(!caps.extensions);
        assert!(!caps.wayland_overlay);

        let space = SpaceId::new();
        let profile = ProfileId::new();
        assert!(webview2_user_data_folder(Path::new("C:/AppData"), profile, space, true).is_none());
        let persistent = webview2_user_data_folder(Path::new("C:/AppData"), profile, space, false)
            .expect("persistent path");
        let expected_suffix =
            PathBuf::from(format!("profile-{profile}")).join(format!("space-{space}"));
        assert!(persistent.ends_with(expected_suffix));
    }

    #[test]
    fn webview2_engine_honestly_reports_unsupported_without_native_binding() {
        assert!(matches!(
            WebView2Engine::try_new(),
            Err(EngineError::NotSupported(_))
        ));

        let mut engine = WebView2Engine::new();
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
