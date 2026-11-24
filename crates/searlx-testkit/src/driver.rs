//! End-to-end driver binding [`BrowserController`] to any [`Engine`] backend.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use searlx_core::controller::EngineEvent as CtrlEvent;
use searlx_core::{
    AppCommand, BrowserController, EngineCapabilities, EngineDirective, EnginePermissionKind,
    ProfileId, RestoreMode, SpaceId, TabId, UiSnapshot,
};
use searlx_engine::{
    Capabilities, DataKinds, DialogKind, Engine, EngineError, EngineEvent as RawEngineEvent,
    FindOptions, InjectAt, NewWindowDisposition, PermissionKind as RawPermKind, Rect, ScriptWorld,
    TabOptions,
};

/// Errors returned while driving a [`BrowserController`] against an [`Engine`].
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// Core controller error.
    #[error("controller error: {0}")]
    Core(#[from] searlx_core::CoreError),
    /// Engine backend error.
    #[error("engine error: {0}")]
    Engine(#[from] EngineError),
    /// Driver loop did not converge within the bounded cycle cap.
    #[error("driver directive/event loop exceeded max cycles")]
    LoopLimit,
}

/// Result type returned by [`ControllerDriver`].
pub type DriverResult<T> = std::result::Result<T, DriverError>;

/// Headless driver orchestrating [`BrowserController`] <-> [`Engine`] interactions
/// (command dispatch, directive execution, `bind_engine_tab`, `pump`, and follow-ups).
#[derive(Debug)]
pub struct ControllerDriver<E: Engine> {
    /// Platform-independent controller state machine.
    pub controller: BrowserController,
    /// Concrete engine implementation (e.g. `FakeEngine`).
    pub engine: E,
    core_to_engine: BTreeMap<TabId, TabId>,
    engine_to_core: BTreeMap<TabId, TabId>,
    private_stores: BTreeSet<(SpaceId, ProfileId)>,
    /// Log of all [`EngineDirective`]s executed since creation (oldest first).
    pub executed_directives: Vec<EngineDirective>,
    /// Log of all translated [`CtrlEvent`]s delivered to the controller.
    pub processed_events: Vec<CtrlEvent>,
}

impl<E: Engine> ControllerDriver<E> {
    /// Convert an engine's [`Capabilities`] to [`EngineCapabilities`].
    #[must_use]
    pub fn convert_capabilities(caps: &Capabilities) -> EngineCapabilities {
        EngineCapabilities {
            engine: caps.engine.clone(),
            extensions: caps.extensions,
            passkeys: caps.passkeys,
            content_blockers: caps.content_blockers,
            native_pip: caps.native_pip,
            site_isolation: caps.site_isolation,
            custom_user_agent: caps.custom_user_agent,
            print: caps.print,
            pdf: caps.pdf,
            find_while_pdf: caps.find_while_pdf,
            downloads_pause: caps.downloads_pause,
            cookie_import: caps.cookie_import,
            wayland_overlay: caps.wayland_overlay,
            floating_video: caps.floating_video,
        }
    }

    /// Open and restore a browser session from `root`, executing initial directives
    /// and draining initial engine events until quiescent.
    pub fn open(root: impl AsRef<Path>, mut engine: E, mode: RestoreMode) -> DriverResult<Self> {
        let caps = Self::convert_capabilities(&engine.capabilities());
        let (controller, init_directives) = BrowserController::open(root, caps, mode)?;
        let _ = &mut engine;
        let mut driver = Self {
            controller,
            engine,
            core_to_engine: BTreeMap::new(),
            engine_to_core: BTreeMap::new(),
            private_stores: BTreeSet::new(),
            executed_directives: Vec::new(),
            processed_events: Vec::new(),
        };
        driver.execute_and_pump(init_directives)?;
        Ok(driver)
    }

    /// Dispatch an [`AppCommand`], execute returned [`EngineDirective`]s on the engine,
    /// and drain any resulting [`EngineEvent`]s back into the controller.
    pub fn dispatch(&mut self, cmd: AppCommand) -> DriverResult<()> {
        let dirs = self.controller.dispatch(cmd)?;
        self.execute_and_pump(dirs)
    }

    /// Deliver an external/synthetic engine event (using core `TabId`) to the
    /// controller and execute any follow-up directives.
    pub fn push_core_event(&mut self, event: CtrlEvent) -> DriverResult<()> {
        self.processed_events.push(event.clone());
        let dirs = self.controller.handle_engine_event(event)?;
        self.execute_and_pump(dirs)
    }

    /// Drain all currently queued raw events from the engine and reconcile them.
    pub fn pump_engine(&mut self) -> DriverResult<()> {
        self.execute_and_pump(Vec::new())
    }

    /// Look up the live engine handle mapped to `core_tab`.
    #[must_use]
    pub fn engine_tab_for(&self, core_tab: TabId) -> Option<TabId> {
        self.core_to_engine.get(&core_tab).copied()
    }

    /// Look up the core `TabId` mapped to an engine `engine_tab`.
    #[must_use]
    pub fn core_tab_for(&self, engine_tab: TabId) -> Option<TabId> {
        self.engine_to_core.get(&engine_tab).copied()
    }

    /// Snapshot current read-only UI view state.
    #[must_use]
    pub fn snapshot(&self) -> UiSnapshot {
        self.controller.ui_snapshot()
    }

    /// Execute `pending_directives`, drain `engine.pump()`, translate events, and repeat until idle.
    pub fn execute_and_pump(&mut self, mut pending: Vec<EngineDirective>) -> DriverResult<()> {
        let mut synthetic_events: Vec<CtrlEvent> = Vec::new();
        for _cycle in 0..64 {
            for directive in pending.drain(..) {
                self.execute_one_directive(directive, &mut synthetic_events)?;
            }

            let raw_events = self.engine.pump()?;
            if raw_events.is_empty() && synthetic_events.is_empty() {
                return Ok(());
            }

            let mut translated = Vec::new();
            for raw in raw_events {
                if let Some(ev) = self.translate_engine_event(raw) {
                    translated.push(ev);
                }
            }
            translated.append(&mut synthetic_events);

            for ev in translated {
                self.processed_events.push(ev.clone());
                let follow_up = self.controller.handle_engine_event(ev)?;
                pending.extend(follow_up);
            }
        }
        Err(DriverError::LoopLimit)
    }

    fn execute_one_directive(
        &mut self,
        directive: EngineDirective,
        synthetic_events: &mut Vec<CtrlEvent>,
    ) -> DriverResult<()> {
        self.executed_directives.push(directive.clone());
        match directive {
            EngineDirective::CreateTab {
                tab: core_id,
                options,
            } => {
                let profile = self.controller.active_profile_id();
                if options.private {
                    self.private_stores.insert((options.space, profile));
                }
                let engine_id = self.engine.create_tab(TabOptions {
                    url: options.url,
                    space: options.space,
                    profile,
                    private: options.private,
                    background: false,
                    opener: None,
                    user_agent: None,
                })?;
                self.core_to_engine.insert(core_id, engine_id);
                self.engine_to_core.insert(engine_id, core_id);
                self.controller.bind_engine_tab(core_id, engine_id);
            }
            EngineDirective::CloseTab { tab, engine_tab_id } => {
                let eid = engine_tab_id.or_else(|| self.core_to_engine.get(&tab).copied());
                self.core_to_engine.remove(&tab);
                if let Some(id) = eid {
                    self.engine_to_core.remove(&id);
                    self.engine.close_tab(id)?;
                }
            }
            EngineDirective::Navigate { tab, url } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.navigate(eid, url)?;
                }
            }
            EngineDirective::LoadHtml {
                tab,
                base_url,
                html,
            } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = base_url;
                    let encoded = format!("window.__searlx_reader_html_len = {};", html.len());
                    self.engine.eval(eid, ScriptWorld::Isolated, &encoded)?;
                }
            }
            EngineDirective::Back { tab } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.back(eid)?;
                }
            }
            EngineDirective::Forward { tab } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.forward(eid)?;
                }
            }
            EngineDirective::Reload { tab, from_origin } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.reload(eid, from_origin)?;
                }
            }
            EngineDirective::Stop { tab } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.stop(eid)?;
                }
            }
            EngineDirective::Eval { tab, world, js } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.eval(eid, map_world(world), &js)?;
                }
            }
            EngineDirective::Inject { tab, world, at, js } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine
                        .inject(eid, map_world(world), map_inject_at(at), &js)?;
                }
            }
            EngineDirective::SetBounds { tab, rect } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.set_bounds(
                        eid,
                        Rect {
                            x: rect.x,
                            y: rect.y,
                            width: rect.width,
                            height: rect.height,
                        },
                    )?;
                }
            }
            EngineDirective::SetVisible { tab, visible } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.set_visible(eid, visible)?;
                }
            }
            EngineDirective::Focus { tab } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.focus(eid)?;
                }
            }
            EngineDirective::Snapshot { tab, .. } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let img = self.engine.snapshot(eid)?;
                    synthetic_events.push(CtrlEvent::SnapshotReady {
                        tab,
                        width: img.width,
                        height: img.height,
                    });
                }
            }
            EngineDirective::Find {
                tab,
                query,
                options,
            } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let state = self.engine.find(
                        eid,
                        &query,
                        FindOptions {
                            case_sensitive: options.case_sensitive,
                            whole_words: options.whole_words,
                            backwards: options.backwards,
                            wrap: true,
                        },
                    )?;
                    synthetic_events.push(CtrlEvent::FindResults {
                        tab,
                        total_found: state.matches,
                        active_index: state.active,
                    });
                }
            }
            EngineDirective::ClearFind { tab } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = self.engine.find(eid, "", FindOptions::default())?;
                }
            }
            EngineDirective::SetZoom { tab, zoom } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.set_zoom(eid, zoom)?;
                }
            }
            EngineDirective::SetMuted { tab, muted } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    self.engine.set_muted(eid, muted)?;
                }
            }
            EngineDirective::ClearData {
                space,
                private,
                kinds,
            } => {
                let target_kinds = DataKinds {
                    cookies: kinds.cookies,
                    cache: kinds.cache,
                    site_data: kinds.storage || kinds.indexed_db,
                    history: true,
                    favicons: true,
                };
                if private {
                    let stores = std::mem::take(&mut self.private_stores);
                    for (sp, prof) in stores {
                        let handle = self.engine.data_store(sp, prof, true)?;
                        self.engine.clear_data(handle, target_kinds)?;
                    }
                } else if let Some(sp) = space {
                    let prof = self.controller.active_profile_id();
                    let handle = self.engine.data_store(sp, prof, false)?;
                    self.engine.clear_data(handle, target_kinds)?;
                }
            }
            EngineDirective::ApplyContentRules { .. }
            | EngineDirective::ApplyUserStylesheet { .. }
            | EngineDirective::RespondPermission { .. }
            | EngineDirective::RespondScriptDialog { .. }
            | EngineDirective::RespondAuth { .. }
            | EngineDirective::RespondCertificateError { .. }
            | EngineDirective::CopyToClipboard { .. }
            | EngineDirective::OpenExternalDialog { .. } => {}
        }
        Ok(())
    }

    fn translate_engine_event(&self, raw: RawEngineEvent) -> Option<CtrlEvent> {
        let map_tab = |eid: TabId| self.engine_to_core.get(&eid).copied();
        Some(match raw {
            RawEngineEvent::NavigationStarted { tab, url } => CtrlEvent::NavigationStarted {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::NavigationCommitted { tab, url } => CtrlEvent::NavigationCommitted {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::NavigationFinished { tab, url } => CtrlEvent::NavigationFinished {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::NavigationFailed { tab, url, reason } => CtrlEvent::NavigationFailed {
                tab: map_tab(tab)?,
                url,
                reason,
            },
            RawEngineEvent::TitleChanged { tab, title } => CtrlEvent::TitleChanged {
                tab: map_tab(tab)?,
                title,
            },
            RawEngineEvent::UrlChanged { tab, url } => CtrlEvent::UrlChanged {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::FaviconChanged { tab, url } => CtrlEvent::FaviconChanged {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::LoadingChanged { tab, loading } => CtrlEvent::LoadingChanged {
                tab: map_tab(tab)?,
                loading,
            },
            RawEngineEvent::HistoryChanged {
                tab,
                can_go_back,
                can_go_forward,
            } => CtrlEvent::HistoryChanged {
                tab: map_tab(tab)?,
                can_go_back,
                can_go_forward,
            },
            RawEngineEvent::NewWindowRequested {
                tab,
                url,
                disposition,
            } => CtrlEvent::NewWindowRequested {
                tab: map_tab(tab)?,
                url,
                disposition: match disposition {
                    NewWindowDisposition::ForegroundTab => {
                        searlx_core::EngineNewWindowDisposition::ForegroundTab
                    }
                    NewWindowDisposition::BackgroundTab => {
                        searlx_core::EngineNewWindowDisposition::BackgroundTab
                    }
                    NewWindowDisposition::Popup => searlx_core::EngineNewWindowDisposition::Popup,
                    NewWindowDisposition::SmallWindow => {
                        searlx_core::EngineNewWindowDisposition::SmallWindow
                    }
                },
            },
            RawEngineEvent::DownloadRequested {
                tab,
                url,
                suggested_name,
            } => CtrlEvent::DownloadRequested {
                tab: map_tab(tab)?,
                url,
                method: searlx_core::HttpMethod::Get,
                has_request_body: false,
                suggested_name,
                total_bytes: None,
            },
            RawEngineEvent::PermissionRequested { tab, origin, kind } => {
                CtrlEvent::PermissionRequested {
                    tab: map_tab(tab)?,
                    origin,
                    kind: match kind {
                        RawPermKind::Camera => EnginePermissionKind::Camera,
                        RawPermKind::Microphone => EnginePermissionKind::Microphone,
                        RawPermKind::Geolocation => EnginePermissionKind::Geolocation,
                        RawPermKind::Notifications => EnginePermissionKind::Notifications,
                        RawPermKind::ScreenCapture => EnginePermissionKind::ScreenCapture,
                        RawPermKind::ClipboardRead => EnginePermissionKind::ClipboardRead,
                    },
                }
            }
            RawEngineEvent::ScriptDialog { tab, kind, message } => CtrlEvent::ScriptDialog {
                tab: map_tab(tab)?,
                kind: match kind {
                    DialogKind::Alert => searlx_core::EngineDialogKind::Alert,
                    DialogKind::Confirm => searlx_core::EngineDialogKind::Confirm,
                    DialogKind::Prompt => searlx_core::EngineDialogKind::Prompt,
                    DialogKind::BeforeUnload => searlx_core::EngineDialogKind::BeforeUnload,
                },
                message,
            },
            RawEngineEvent::FilePickerRequested { tab, multiple } => {
                CtrlEvent::FilePickerRequested {
                    tab: map_tab(tab)?,
                    multiple,
                }
            }
            RawEngineEvent::AuthRequired { tab, origin } => CtrlEvent::AuthRequired {
                tab: map_tab(tab)?,
                origin,
            },
            RawEngineEvent::CertificateError { tab, url, reason } => CtrlEvent::CertificateError {
                tab: map_tab(tab)?,
                url,
                reason,
            },
            RawEngineEvent::MediaStateChanged {
                tab,
                audible,
                muted,
            } => CtrlEvent::MediaStateChanged {
                tab: map_tab(tab)?,
                audible,
                muted,
            },
            RawEngineEvent::FullscreenChanged { tab, fullscreen } => CtrlEvent::FullscreenChanged {
                tab: map_tab(tab)?,
                fullscreen,
            },
            RawEngineEvent::ProcessCrashed { tab, reason } => CtrlEvent::ProcessCrashed {
                tab: map_tab(tab)?,
                reason,
            },
            RawEngineEvent::ScriptMessage { tab, world, body } => CtrlEvent::ScriptMessage {
                tab: map_tab(tab)?,
                world: match world {
                    ScriptWorld::Page => searlx_core::ScriptWorld::Page,
                    ScriptWorld::Isolated => searlx_core::ScriptWorld::Isolated,
                },
                body,
            },
            RawEngineEvent::SnapshotReady { tab, image } => CtrlEvent::SnapshotReady {
                tab: map_tab(tab)?,
                width: image.width,
                height: image.height,
            },
        })
    }
}

const fn map_world(world: searlx_core::ScriptWorld) -> ScriptWorld {
    match world {
        searlx_core::ScriptWorld::Page => ScriptWorld::Page,
        searlx_core::ScriptWorld::Isolated => ScriptWorld::Isolated,
    }
}

const fn map_inject_at(at: searlx_core::InjectAt) -> InjectAt {
    match at {
        searlx_core::InjectAt::DocumentStart => InjectAt::DocumentStart,
        searlx_core::InjectAt::DocumentEnd => InjectAt::DocumentEnd,
    }
}
