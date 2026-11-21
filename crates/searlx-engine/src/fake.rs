//! A deterministic in-memory engine for tests and the Phase 0 UI shell.
//!
//! The fake is not a browser. It models the contract closely enough that the
//! app, IPC, and UI can be developed without a native engine, and that contract
//! tests can prove the event stream and state transitions.

use crate::{
    Capabilities, DataKinds, DataStoreHandle, DialogKind, Engine, EngineError, EngineEvent,
    EngineResult, FindOptions, FindState, Image, InjectAt, NewWindowDisposition, Rect, ScriptWorld,
    TabOptions,
};
use searlx_core::{PageUrl, ProfileId, SpaceId, TabId, TabIdAllocator};
use std::collections::{BTreeMap, VecDeque};
use uuid::Uuid;

/// A 1×1 transparent PNG, used by [`FakeEngine::snapshot`].
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// One call made through the engine contract, for assertions.
#[derive(Clone, Debug, PartialEq)]
pub enum FakeCall {
    /// `create_tab`.
    CreateTab,
    /// `close_tab`.
    CloseTab(TabId),
    /// `navigate`.
    Navigate(TabId, String),
    /// `back`.
    Back(TabId),
    /// `forward`.
    Forward(TabId),
    /// `reload`.
    Reload(TabId, bool),
    /// `stop`.
    Stop(TabId),
    /// `eval`.
    Eval(TabId, ScriptWorld),
    /// `inject`.
    Inject(TabId, ScriptWorld, InjectAt),
    /// `set_bounds`.
    SetBounds(TabId, Rect),
    /// `set_visible`.
    SetVisible(TabId, bool),
    /// `focus`.
    Focus(TabId),
    /// `snapshot`.
    Snapshot(TabId),
    /// `find`.
    Find(TabId, String, FindOptions),
    /// `set_zoom`.
    SetZoom(TabId, f64),
    /// `set_muted`.
    SetMuted(TabId, bool),
    /// `data_store`.
    DataStore(SpaceId, ProfileId, bool),
    /// `clear_data`.
    ClearData(DataStoreHandle, DataKinds),
    /// `pump`.
    Pump,
}

/// A tab inside the fake engine.
#[derive(Clone, Debug)]
pub struct FakeTab {
    /// Options the tab was created with.
    pub options: TabOptions,
    /// Current address.
    pub url: Option<PageUrl>,
    /// Current title.
    pub title: String,
    /// Session history, current entry last.
    pub history: Vec<PageUrl>,
    /// Index into `history`.
    pub history_index: usize,
    /// Loading flag.
    pub loading: bool,
    /// Visibility.
    pub visible: bool,
    /// Bounds.
    pub bounds: Rect,
    /// Zoom factor.
    pub zoom: f64,
    /// Muted.
    pub muted: bool,
    /// Audible.
    pub audible: bool,
    /// Injections registered for this tab.
    pub injected: Vec<(ScriptWorld, InjectAt, String)>,
    /// Scripts evaluated for this tab.
    pub evaluated: Vec<(ScriptWorld, String)>,
}

impl FakeTab {
    /// Whether the tab can go back.
    #[must_use]
    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    /// Whether the tab can go forward.
    #[must_use]
    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }
}

/// A deterministic engine that keeps state in memory.
#[derive(Debug)]
pub struct FakeEngine {
    ids: TabIdAllocator,
    tabs: BTreeMap<TabId, FakeTab>,
    stores: BTreeMap<(SpaceId, ProfileId, bool), DataStoreHandle>,
    events: VecDeque<EngineEvent>,
    calls: Vec<FakeCall>,
    capabilities: Capabilities,
    /// When true, navigation is refused with a backend error.
    pub fail_navigation: bool,
}

impl Default for FakeEngine {
    fn default() -> Self {
        Self::new(Capabilities::baseline("fake"))
    }
}

impl FakeEngine {
    /// Create an engine with the given capabilities.
    #[must_use]
    pub fn new(capabilities: Capabilities) -> Self {
        Self {
            ids: TabIdAllocator::new(),
            tabs: BTreeMap::new(),
            stores: BTreeMap::new(),
            events: VecDeque::new(),
            calls: Vec::new(),
            capabilities,
            fail_navigation: false,
        }
    }

    /// The tabs currently open.
    #[must_use]
    pub fn tabs(&self) -> &BTreeMap<TabId, FakeTab> {
        &self.tabs
    }

    /// A tab, if it exists.
    #[must_use]
    pub fn tab(&self, id: TabId) -> Option<&FakeTab> {
        self.tabs.get(&id)
    }

    /// Every call made so far, oldest first.
    #[must_use]
    pub fn calls(&self) -> &[FakeCall] {
        &self.calls
    }

    /// Queue an event as if a script in the page posted it.
    pub fn post_script_message(&mut self, tab: TabId, world: ScriptWorld, body: impl Into<String>) {
        self.events.push_back(EngineEvent::ScriptMessage {
            tab,
            world,
            body: body.into(),
        });
    }

    /// Queue an arbitrary event, for UI tests.
    pub fn push_event(&mut self, event: EngineEvent) {
        self.events.push_back(event);
    }

    /// Queue a permission request, for UI tests.
    pub fn request_permission(&mut self, tab: TabId, origin: &str, kind: crate::PermissionKind) {
        self.events.push_back(EngineEvent::PermissionRequested {
            tab,
            origin: origin.to_owned(),
            kind,
        });
    }

    /// Queue a JavaScript dialog, for UI tests.
    pub fn request_dialog(&mut self, tab: TabId, kind: DialogKind, message: &str) {
        self.events.push_back(EngineEvent::ScriptDialog {
            tab,
            kind,
            message: message.to_owned(),
        });
    }

    /// Queue a new-window request, for UI tests.
    pub fn request_new_window(&mut self, tab: TabId, url: PageUrl) {
        self.events.push_back(EngineEvent::NewWindowRequested {
            tab,
            url,
            disposition: NewWindowDisposition::ForegroundTab,
        });
    }

    fn title_for(url: &PageUrl) -> String {
        url.host().map_or_else(
            || url.display(),
            |host| {
                let host = host.trim_start_matches("www.");
                let mut label = host
                    .chars()
                    .next()
                    .map(|first| first.to_ascii_uppercase().to_string())
                    .unwrap_or_default();
                label.push_str(&host[first_char_len(host)..]);
                format!("{label} — Fake")
            },
        )
    }

    fn require_tab(&mut self, id: TabId) -> EngineResult<&mut FakeTab> {
        self.tabs.get_mut(&id).ok_or(EngineError::NoSuchTab(id))
    }

    fn commit_url(&mut self, id: TabId, url: PageUrl) {
        let title = Self::title_for(&url);
        let Some(tab) = self.tabs.get_mut(&id) else {
            return;
        };
        tab.url = Some(url.clone());
        tab.title = title.clone();
        tab.loading = false;
        self.events.push_back(EngineEvent::NavigationStarted {
            tab: id,
            url: url.clone(),
        });
        self.events.push_back(EngineEvent::UrlChanged {
            tab: id,
            url: url.clone(),
        });
        self.events.push_back(EngineEvent::NavigationCommitted {
            tab: id,
            url: url.clone(),
        });
        self.events
            .push_back(EngineEvent::TitleChanged { tab: id, title });
        self.events.push_back(EngineEvent::LoadingChanged {
            tab: id,
            loading: true,
        });
        self.events
            .push_back(EngineEvent::NavigationFinished { tab: id, url });
        self.events.push_back(EngineEvent::LoadingChanged {
            tab: id,
            loading: false,
        });
        let (back, forward) = self.tabs.get(&id).map_or((false, false), |tab| {
            (tab.can_go_back(), tab.can_go_forward())
        });
        self.events.push_back(EngineEvent::HistoryChanged {
            tab: id,
            can_go_back: back,
            can_go_forward: forward,
        });
    }

    fn travel(&mut self, id: TabId, delta: isize) -> EngineResult<()> {
        let tab = self.tabs.get_mut(&id).ok_or(EngineError::NoSuchTab(id))?;
        let target = tab
            .history_index
            .checked_add_signed(delta)
            .ok_or_else(|| EngineError::Backend("history index underflow".into()))?;
        if target >= tab.history.len() {
            return Ok(());
        }
        tab.history_index = target;
        let Some(url) = tab.history.get(target).cloned() else {
            return Ok(());
        };
        self.commit_url(id, url);
        Ok(())
    }
}

fn first_char_len(value: &str) -> usize {
    value.chars().next().map_or(0, char::len_utf8)
}

impl Engine for FakeEngine {
    fn create_tab(&mut self, options: TabOptions) -> EngineResult<TabId> {
        self.calls.push(FakeCall::CreateTab);
        let id = self.ids.allocate();
        let mut tab = FakeTab {
            options: options.clone(),
            url: None,
            title: String::new(),
            history: Vec::new(),
            history_index: 0,
            loading: false,
            visible: !options.background,
            bounds: Rect::default(),
            zoom: 1.0,
            muted: false,
            audible: false,
            injected: Vec::new(),
            evaluated: Vec::new(),
        };
        if let Some(url) = options.url {
            tab.url = Some(url.clone());
            tab.title = Self::title_for(&url);
            tab.history.push(url.clone());
            tab.history_index = 0;
        }
        self.tabs.insert(id, tab);
        if let Some(url) = self.tabs.get(&id).and_then(|tab| tab.url.clone()) {
            self.commit_url(id, url);
        }
        Ok(id)
    }

    fn close_tab(&mut self, id: TabId) -> EngineResult<()> {
        self.calls.push(FakeCall::CloseTab(id));
        self.tabs
            .remove(&id)
            .map(|_| ())
            .ok_or(EngineError::NoSuchTab(id))
    }

    fn navigate(&mut self, id: TabId, url: PageUrl) -> EngineResult<()> {
        self.calls
            .push(FakeCall::Navigate(id, url.as_str().to_owned()));
        if self.fail_navigation {
            return Err(EngineError::Backend("navigation failed".into()));
        }
        let tab = self.tabs.get_mut(&id).ok_or(EngineError::NoSuchTab(id))?;
        tab.history.truncate(tab.history_index + 1);
        tab.history.push(url.clone());
        tab.history_index = tab.history.len() - 1;
        self.commit_url(id, url);
        Ok(())
    }

    fn back(&mut self, id: TabId) -> EngineResult<()> {
        self.calls.push(FakeCall::Back(id));
        self.travel(id, -1)
    }

    fn forward(&mut self, id: TabId) -> EngineResult<()> {
        self.calls.push(FakeCall::Forward(id));
        self.travel(id, 1)
    }

    fn reload(&mut self, id: TabId, from_origin: bool) -> EngineResult<()> {
        self.calls.push(FakeCall::Reload(id, from_origin));
        let url = self
            .tabs
            .get(&id)
            .and_then(|tab| tab.url.clone())
            .ok_or(EngineError::NoSuchTab(id))?;
        self.commit_url(id, url);
        Ok(())
    }

    fn stop(&mut self, id: TabId) -> EngineResult<()> {
        self.calls.push(FakeCall::Stop(id));
        let tab = self.require_tab(id)?;
        tab.loading = false;
        self.events.push_back(EngineEvent::LoadingChanged {
            tab: id,
            loading: false,
        });
        Ok(())
    }

    fn eval(&mut self, id: TabId, world: ScriptWorld, js: &str) -> EngineResult<()> {
        self.calls.push(FakeCall::Eval(id, world));
        let tab = self.require_tab(id)?;
        tab.evaluated.push((world, js.to_owned()));
        Ok(())
    }

    fn inject(
        &mut self,
        id: TabId,
        world: ScriptWorld,
        at: InjectAt,
        js: &str,
    ) -> EngineResult<()> {
        self.calls.push(FakeCall::Inject(id, world, at));
        let tab = self.require_tab(id)?;
        tab.injected.push((world, at, js.to_owned()));
        Ok(())
    }

    fn set_bounds(&mut self, id: TabId, rect: Rect) -> EngineResult<()> {
        self.calls.push(FakeCall::SetBounds(id, rect));
        self.require_tab(id)?.bounds = rect;
        Ok(())
    }

    fn set_visible(&mut self, id: TabId, visible: bool) -> EngineResult<()> {
        self.calls.push(FakeCall::SetVisible(id, visible));
        self.require_tab(id)?.visible = visible;
        Ok(())
    }

    fn focus(&mut self, id: TabId) -> EngineResult<()> {
        self.calls.push(FakeCall::Focus(id));
        self.require_tab(id)?;
        Ok(())
    }

    fn snapshot(&mut self, id: TabId) -> EngineResult<Image> {
        self.calls.push(FakeCall::Snapshot(id));
        self.require_tab(id)?;
        Ok(Image {
            width: 1,
            height: 1,
            png: TINY_PNG.to_vec(),
        })
    }

    fn find(&mut self, id: TabId, query: &str, options: FindOptions) -> EngineResult<FindState> {
        self.calls
            .push(FakeCall::Find(id, query.to_owned(), options));
        let tab = self.require_tab(id)?;
        if query.is_empty() {
            return Ok(FindState::default());
        }
        let haystack = if options.case_sensitive {
            format!(
                "{} {}",
                tab.title,
                tab.url.as_ref().map_or("", PageUrl::as_str)
            )
        } else {
            format!(
                "{} {}",
                tab.title.to_lowercase(),
                tab.url.as_ref().map_or("", PageUrl::as_str).to_lowercase()
            )
        };
        let needle = if options.case_sensitive {
            query.to_owned()
        } else {
            query.to_lowercase()
        };
        let matches = haystack.matches(&needle).count().min(1_000) as u32;
        Ok(FindState {
            query: query.to_owned(),
            matches,
            active: u32::from(matches > 0),
            finished: true,
        })
    }

    fn set_zoom(&mut self, id: TabId, zoom: f64) -> EngineResult<()> {
        self.calls.push(FakeCall::SetZoom(id, zoom));
        if !(zoom.is_finite() && (0.25..=5.0).contains(&zoom)) {
            return Err(EngineError::Backend(format!("zoom {zoom} out of range")));
        }
        self.require_tab(id)?.zoom = zoom;
        Ok(())
    }

    fn set_muted(&mut self, id: TabId, muted: bool) -> EngineResult<()> {
        self.calls.push(FakeCall::SetMuted(id, muted));
        self.require_tab(id)?.muted = muted;
        Ok(())
    }

    fn data_store(
        &mut self,
        space: SpaceId,
        profile: ProfileId,
        private: bool,
    ) -> EngineResult<DataStoreHandle> {
        self.calls
            .push(FakeCall::DataStore(space, profile, private));
        Ok(*self
            .stores
            .entry((space, profile, private))
            .or_insert(DataStoreHandle {
                id: Uuid::new_v4(),
                space,
                profile,
                ephemeral: private,
            }))
    }

    fn clear_data(&mut self, store: DataStoreHandle, kinds: DataKinds) -> EngineResult<()> {
        self.calls.push(FakeCall::ClearData(store, kinds));
        Ok(())
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities.clone()
    }

    fn pump(&mut self) -> EngineResult<Vec<EngineEvent>> {
        self.calls.push(FakeCall::Pump);
        Ok(self.events.drain(..).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> PageUrl {
        PageUrl::parse(value).expect("fixture URL")
    }

    #[test]
    fn create_navigate_back_forward_emits_a_replayable_stream() {
        let mut engine = FakeEngine::default();
        let id = engine
            .create_tab(TabOptions {
                url: Some(url("https://example.com/one")),
                ..TabOptions::default()
            })
            .expect("create");
        engine.pump().expect("pump");
        engine
            .navigate(id, url("https://example.com/two"))
            .expect("navigate");
        let events = engine.pump().expect("pump");
        assert!(events.iter().any(|event| matches!(
            event,
            EngineEvent::NavigationCommitted { url, .. } if url.as_str() == "https://example.com/two"
        )));
        engine.back(id).expect("back");
        assert_eq!(
            engine
                .tab(id)
                .and_then(|tab| tab.url.as_ref())
                .map(PageUrl::as_str),
            Some("https://example.com/one")
        );
        engine.forward(id).expect("forward");
        assert_eq!(
            engine
                .tab(id)
                .and_then(|tab| tab.url.as_ref())
                .map(PageUrl::as_str),
            Some("https://example.com/two")
        );
    }

    #[test]
    fn find_counts_title_and_url() {
        let mut engine = FakeEngine::default();
        let id = engine
            .create_tab(TabOptions {
                url: Some(url("https://example.com/hello")),
                ..TabOptions::default()
            })
            .expect("create");
        let state = engine
            .find(id, "hello", FindOptions::default())
            .expect("find");
        assert!(state.matches >= 1);
        assert_eq!(state.active, 1);
    }

    #[test]
    fn stores_are_stable_per_space_profile_and_privacy() {
        let mut engine = FakeEngine::default();
        let space = SpaceId::new();
        let profile = ProfileId::new();
        let a = engine.data_store(space, profile, false).expect("store");
        let b = engine.data_store(space, profile, false).expect("store");
        let private = engine.data_store(space, profile, true).expect("store");
        assert_eq!(a, b);
        assert_ne!(a.id, private.id);
        assert!(private.ephemeral);
    }

    #[test]
    fn closing_an_unknown_tab_is_an_error_not_a_panic() {
        let mut engine = FakeEngine::default();
        assert!(matches!(
            engine.close_tab(TabId::new(999)),
            Err(EngineError::NoSuchTab(_))
        ));
    }
}
