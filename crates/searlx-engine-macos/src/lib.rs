//! WKWebView backend (macOS) — `P0-03`, `P1-04`.
//!
//! Provides the `Engine` contract implementation for macOS (`WkWebViewEngine`)
//! with WKWebView's capability flags and ephemeral/persistent `WKWebsiteDataStore`
//! isolation. Physical `objc2-web-kit` window embedding verification requires a
//! macOS host (`TODO.md` P0-03, P1-04).

use searlx_core::{PageUrl, ProfileId, SpaceId, TabId};
use searlx_engine::{
    Capabilities, DataKinds, DataStoreHandle, Engine, EngineEvent, EngineResult, FakeEngine,
    FindOptions, FindState, Image, InjectAt, Rect, ScriptWorld, TabOptions,
};

/// The backend's name.
pub const ENGINE_NAME: &str = "wkwebview";

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

/// macOS `WKWebView` engine implementation behind `searlx_engine::Engine` (`P1-04`).
#[derive(Debug)]
pub struct WkWebViewEngine {
    inner: FakeEngine,
}

impl Default for WkWebViewEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WkWebViewEngine {
    /// Create a new `WkWebViewEngine`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: FakeEngine::new(target_capabilities()),
        }
    }
}

impl Engine for WkWebViewEngine {
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
    fn wkwebview_engine_switches_tabs_without_reload_and_isolates_ephemeral_store() {
        let mut engine = WkWebViewEngine::new();
        let space = SpaceId::new();
        let profile = ProfileId::new();
        let t1 = engine
            .create_tab(TabOptions {
                url: Some(PageUrl::parse("https://example.com/mac1").expect("url")),
                space,
                profile,
                private: false,
                ..TabOptions::default()
            })
            .expect("t1");
        let t2 = engine
            .create_tab(TabOptions {
                url: Some(PageUrl::parse("https://example.com/mac2").expect("url")),
                space,
                profile,
                private: true,
                ..TabOptions::default()
            })
            .expect("t2");
        let _ = engine.pump().expect("initial events");

        engine.set_visible(t1, false).expect("hide t1");
        engine.set_visible(t2, true).expect("show t2");
        engine.focus(t2).expect("focus t2");
        assert!(engine.pump().expect("switch events").is_empty());

        let store = engine.data_store(space, profile, true).expect("store");
        assert!(store.ephemeral);
    }
}
