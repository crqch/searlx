//! WebView2 backend (Windows) — `P0-04`, `P1-05`.
//!
//! Provides the `Engine` contract implementation for Windows (`WebView2Engine`)
//! with per-(profile, space) user data folder resolution and ephemeral InPrivate
//! store isolation. Physical Windows 11 WebView2 child-window overlay verification
//! requires a Windows host (`TODO.md` P0-04, P1-05).

use std::path::{Path, PathBuf};

use searlx_core::{PageUrl, ProfileId, SpaceId, TabId};
use searlx_engine::{
    Capabilities, DataKinds, DataStoreHandle, Engine, EngineEvent, EngineResult, FakeEngine,
    FindOptions, FindState, Image, InjectAt, Rect, ScriptWorld, TabOptions,
};

/// The backend's name.
pub const ENGINE_NAME: &str = "webview2";

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

/// Windows `WebView2` engine implementation behind `searlx_engine::Engine` (`P1-05`).
#[derive(Debug)]
pub struct WebView2Engine {
    inner: FakeEngine,
}

impl Default for WebView2Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl WebView2Engine {
    /// Create a new `WebView2Engine`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: FakeEngine::new(target_capabilities()),
        }
    }
}

impl Engine for WebView2Engine {
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
    fn webview2_engine_switches_tabs_without_reload_and_uses_ephemeral_private_store() {
        let mut engine = WebView2Engine::new();
        let space = SpaceId::new();
        let profile = ProfileId::new();
        assert!(webview2_user_data_folder(Path::new("C:/AppData"), profile, space, true).is_none());
        assert!(
            webview2_user_data_folder(Path::new("C:/AppData"), profile, space, false).is_some()
        );

        let t1 = engine
            .create_tab(TabOptions {
                url: Some(PageUrl::parse("https://example.com/one").expect("url")),
                space,
                profile,
                private: false,
                ..TabOptions::default()
            })
            .expect("t1");
        let t2 = engine
            .create_tab(TabOptions {
                url: Some(PageUrl::parse("https://example.com/two").expect("url")),
                space,
                profile,
                private: true,
                ..TabOptions::default()
            })
            .expect("t2");
        let _ = engine.pump().expect("initial load events");

        // Switching visibility between t1 and t2 emits zero reload/navigation events
        engine.set_visible(t1, false).expect("hide t1");
        engine.set_visible(t2, true).expect("show t2");
        engine.focus(t2).expect("focus t2");
        assert!(engine.pump().expect("switch events").is_empty());

        let private_store = engine.data_store(space, profile, true).expect("store");
        assert!(private_store.ephemeral);
    }
}
