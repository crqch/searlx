//! Real `webkit2gtk`-backed engine implementation (`ADR-0002`, `P5-01`).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use glib::prelude::*;
use gtk::prelude::*;
use searlx_core::fuzz_and_hardening::INTERNAL_PAGE_CSP;
use searlx_core::{PageUrl, ProfileId, SpaceId, TabId, TabIdAllocator, number_duplicate_filename};
use searlx_engine::{
    DataKinds, DataStoreHandle, DialogKind, EngineError, EngineEvent, EngineResult, FindOptions,
    FindState, Image, InjectAt, NewWindowDisposition, PermissionKind, Rect, ScriptWorld,
    TabOptions,
};
use webkit2gtk::{
    AuthenticationRequestExt, CacheModel, DownloadExt, FaviconDatabaseExt, FileChooserRequestExt,
    FindControllerExt, FindOptions as WkFindOptions, LoadEvent, NavigationAction,
    NavigationPolicyDecision, NavigationPolicyDecisionExt, PermissionRequestExt, PolicyDecisionExt,
    PolicyDecisionType, ResponsePolicyDecision, ResponsePolicyDecisionExt, ScriptDialogType,
    SecurityManagerExt, SettingsExt, SnapshotOptions, SnapshotRegion, URIRequestExt,
    URIResponseExt, URISchemeRequestExt, UserContentInjectedFrames, UserContentManager,
    UserContentManagerExt, UserMediaPermissionRequestExt, UserScript, UserScriptInjectionTime,
    UserStyleLevel, UserStyleSheet, WebContext, WebContextExt, WebProcessTerminationReason,
    WebView, WebViewExt, WebViewExtManual, WebsiteDataManager, WebsiteDataManagerExtManual,
    WebsiteDataTypes,
};

use crate::filters::{self, UserContentFilter, UserContentFilterStore};
use crate::runtime::wait_until;
use crate::{InternalPageResponse, InternalUriHandler};

/// Name of Search's isolated `WebKitScriptWorld` (`FEATURES.md` §5.10).
pub(crate) const ISOLATED_WORLD_NAME: &str = "searlx";
/// Message handler name registered in Search's isolated world.
pub(crate) const ISOLATED_HANDLER_NAME: &str = "searlx";
/// Message handler name registered in the page world.
pub(crate) const PAGE_HANDLER_NAME: &str = "searlxPage";

const ASYNC_OP_TIMEOUT: Duration = Duration::from_secs(5);
const FIND_OP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FindQueryKey {
    case_sensitive: bool,
    whole_words: bool,
}

#[derive(Debug, Default)]
struct FindTracker {
    query: String,
    key: Option<FindQueryKey>,
    matches: u32,
    active: u32,
    count_done: bool,
    search_done: bool,
    found: bool,
}

struct LiveWebTab {
    id: TabId,
    space: SpaceId,
    profile: ProfileId,
    private: bool,
    webview: WebView,
    user_content_manager: UserContentManager,
    offscreen: Option<gtk::OffscreenWindow>,
    visible: bool,
    bounds: Rect,
    find_tracker: Rc<RefCell<FindTracker>>,
    applied_filters: Vec<String>,
    user_stylesheets: Vec<UserStyleSheet>,
}

struct ContextEntry {
    handle: DataStoreHandle,
    context: WebContext,
    data_manager: Option<WebsiteDataManager>,
}

/// Main-thread state for one [`crate::WebKitGtkEngine`] instance.
pub(crate) struct InnerWebKitEngine {
    data_root: PathBuf,
    downloads_dir: Rc<RefCell<PathBuf>>,
    ids: TabIdAllocator,
    tabs: BTreeMap<TabId, LiveWebTab>,
    contexts: BTreeMap<(SpaceId, ProfileId, bool), ContextEntry>,
    stores: BTreeMap<(SpaceId, ProfileId, bool), DataStoreHandle>,
    webview_to_tab: Rc<RefCell<BTreeMap<usize, TabId>>>,
    events: Arc<Mutex<VecDeque<EngineEvent>>>,
    internal_handler: Arc<RwLock<Option<InternalUriHandler>>>,
    filter_store: Option<UserContentFilterStore>,
    active_filters: BTreeMap<String, UserContentFilter>,
}

impl InnerWebKitEngine {
    pub(crate) fn new(
        data_root: PathBuf,
        events: Arc<Mutex<VecDeque<EngineEvent>>>,
        internal_handler: Arc<RwLock<Option<InternalUriHandler>>>,
    ) -> EngineResult<Self> {
        let downloads_dir = data_root.join("downloads");
        std::fs::create_dir_all(&data_root).map_err(|err| {
            EngineError::Backend(format!(
                "failed to create WebKitGTK data root {}: {err}",
                data_root.display()
            ))
        })?;
        Ok(Self {
            data_root,
            downloads_dir: Rc::new(RefCell::new(downloads_dir)),
            ids: TabIdAllocator::new(),
            tabs: BTreeMap::new(),
            contexts: BTreeMap::new(),
            stores: BTreeMap::new(),
            webview_to_tab: Rc::new(RefCell::new(BTreeMap::new())),
            events,
            internal_handler,
            filter_store: None,
            active_filters: BTreeMap::new(),
        })
    }

    pub(crate) fn shutdown(&mut self) {
        let tab_ids: Vec<TabId> = self.tabs.keys().copied().collect();
        for id in tab_ids {
            let _ = self.close_tab(id);
        }
        self.contexts.clear();
        self.active_filters.clear();
        self.filter_store = None;
    }

    fn push_event(&self, event: EngineEvent) {
        if let Ok(mut guard) = self.events.lock() {
            guard.push_back(event);
        }
    }

    fn ensure_context(
        &mut self,
        space: SpaceId,
        profile: ProfileId,
        private: bool,
    ) -> EngineResult<ContextEntryRef> {
        if !self.contexts.contains_key(&(space, profile, private)) {
            let handle = *self
                .stores
                .entry((space, profile, private))
                .or_insert_with(|| DataStoreHandle {
                    id: SpaceId::new().as_uuid(),
                    space,
                    profile,
                    ephemeral: private,
                });

            let (context, data_manager) = if private {
                let ctx = WebContext::new_ephemeral();
                let dm = ctx.website_data_manager();
                (ctx, dm)
            } else {
                let base = self
                    .data_root
                    .join("profiles")
                    .join(profile.to_string())
                    .join("spaces")
                    .join(space.to_string());
                let data_dir = base.join("data");
                let cache_dir = base.join("cache");
                std::fs::create_dir_all(&data_dir).map_err(|err| {
                    EngineError::Backend(format!(
                        "failed to create WebKitGTK data dir {}: {err}",
                        data_dir.display()
                    ))
                })?;
                std::fs::create_dir_all(&cache_dir).map_err(|err| {
                    EngineError::Backend(format!(
                        "failed to create WebKitGTK cache dir {}: {err}",
                        cache_dir.display()
                    ))
                })?;

                let dm = WebsiteDataManager::builder()
                    .base_data_directory(data_dir.to_string_lossy().as_ref())
                    .base_cache_directory(cache_dir.to_string_lossy().as_ref())
                    .build();
                let ctx = WebContext::with_website_data_manager(&dm);
                (ctx, Some(dm))
            };

            self.configure_context(&context);
            self.contexts.insert(
                (space, profile, private),
                ContextEntry {
                    handle,
                    context,
                    data_manager,
                },
            );
        }

        let entry = self
            .contexts
            .get(&(space, profile, private))
            .ok_or_else(|| EngineError::Backend("WebContext missing after insert".into()))?;
        Ok(ContextEntryRef {
            handle: entry.handle,
            context: entry.context.clone(),
        })
    }

    fn configure_context(&self, context: &WebContext) {
        context.set_cache_model(CacheModel::WebBrowser);

        if let Some(sec) = context.security_manager() {
            sec.register_uri_scheme_as_secure("search");
            sec.register_uri_scheme_as_display_isolated("search");
        }

        // Register `search://` internal scheme on every context.
        let handler_slot = self.internal_handler.clone();
        context.register_uri_scheme("search", move |request| {
            let uri = request
                .uri()
                .map(|g| g.to_string())
                .unwrap_or_else(|| "search://newtab".to_owned());
            if PageUrl::parse(&uri).is_err() {
                let mut err = glib::Error::new(
                    gio::IOErrorEnum::NotFound,
                    &format!("Unknown internal page: {uri}"),
                );
                request.finish_error(&mut err);
                return;
            }
            let response = match handler_slot.read() {
                Ok(guard) => match guard.as_ref() {
                    Some(handler) => handler(&uri),
                    None => default_internal_page(&uri),
                },
                Err(_) => default_internal_page(&uri),
            };
            let len = i64::try_from(response.body.len()).unwrap_or(i64::MAX);
            let bytes = glib::Bytes::from(&response.body);
            let stream = gio::MemoryInputStream::from_bytes(&bytes);
            request.finish(&stream, len, Some(&response.mime_type));
        });

        // Wire downloads on this context.
        let events = self.events.clone();
        let webview_to_tab = self.webview_to_tab.clone();
        let downloads_dir = self.downloads_dir.clone();
        context.connect_download_started(move |_ctx, download| {
            let tab_id = download.web_view().and_then(|view| {
                webview_to_tab
                    .borrow()
                    .get(&(view.as_ptr() as usize))
                    .copied()
            });
            let Some(tab_id) = tab_id else {
                download.cancel();
                return;
            };

            let fallback_url = download
                .request()
                .and_then(|req| req.uri())
                .and_then(|uri| parse_page_url_safe(&uri));

            let events_cb = events.clone();
            let dir_cb = downloads_dir.clone();
            let emitted = Rc::new(Cell::new(false));
            let emitted_cb = emitted.clone();
            let fallback_url_cb = fallback_url.clone();
            download.connect_decide_destination(move |dl, suggested_filename| {
                let resolved_url = dl
                    .response()
                    .and_then(|resp| resp.uri())
                    .and_then(|uri| parse_page_url_safe(&uri))
                    .or_else(|| {
                        dl.request()
                            .and_then(|req| req.uri())
                            .and_then(|uri| parse_page_url_safe(&uri))
                    })
                    .or_else(|| fallback_url_cb.clone());

                if !emitted_cb.replace(true)
                    && let Some(url) = resolved_url
                    && let Ok(mut guard) = events_cb.lock()
                {
                    guard.push_back(EngineEvent::DownloadRequested {
                        tab: tab_id,
                        url,
                        suggested_name: Some(suggested_filename.to_owned()),
                    });
                }

                let dir = dir_cb.borrow().clone();
                if std::fs::create_dir_all(&dir).is_err() {
                    dl.cancel();
                    return true;
                }
                let existing_names: Vec<String> = std::fs::read_dir(&dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect();
                let existing_refs: Vec<&str> = existing_names.iter().map(String::as_str).collect();
                let unique_name = number_duplicate_filename(suggested_filename, &existing_refs);
                let candidate = dir.join(unique_name);
                let gfile = gio::File::for_path(&candidate);
                dl.set_allow_overwrite(false);
                dl.set_destination(&gfile.uri());
                true
            });
        });
    }

    pub(crate) fn create_tab(&mut self, options: TabOptions) -> EngineResult<TabId> {
        let ctx_ref = self.ensure_context(options.space, options.profile, options.private)?;
        let id = self.ids.allocate();

        let ucm = UserContentManager::new();
        let _ = ucm.register_script_message_handler(PAGE_HANDLER_NAME);
        let _ = ucm
            .register_script_message_handler_in_world(ISOLATED_HANDLER_NAME, ISOLATED_WORLD_NAME);

        let mut applied_filters = Vec::new();
        for (filter_id, filter) in &self.active_filters {
            filter.add_to(&ucm);
            applied_filters.push(filter_id.clone());
        }

        let webview = WebView::new_with_context_and_user_content_manager(&ctx_ref.context, &ucm);
        if let Some(settings) = WebViewExt::settings(&webview) {
            settings.set_enable_javascript(true);
            settings.set_enable_webgl(true);
            settings.set_enable_media(true);
            settings.set_enable_media_stream(true);
            settings.set_enable_mediasource(true);
            settings.set_enable_webaudio(true);
            settings.set_enable_developer_extras(true);
            settings.set_enable_smooth_scrolling(true);
            if let Some(ua) = options.user_agent.as_deref() {
                settings.set_user_agent(Some(ua));
            }
        }

        // House new WebViews inside an offscreen window until a GTK shell mounts
        // them, so headless contract tests get realized allocations and snapshots
        // without opening visible desktop windows.
        let offscreen = gtk::OffscreenWindow::new();
        offscreen.set_default_size(1180, 780);
        offscreen.add(&webview);
        offscreen.show_all();
        if options.background {
            webview.set_visible(false);
        }

        let find_tracker = Rc::new(RefCell::new(FindTracker::default()));
        self.webview_to_tab
            .borrow_mut()
            .insert(webview.as_ptr() as usize, id);

        connect_webview_signals(
            id,
            &webview,
            &ucm,
            &find_tracker,
            &self.events,
            options.private,
        );

        let initial_url = options.url.clone();
        self.tabs.insert(
            id,
            LiveWebTab {
                id,
                space: options.space,
                profile: options.profile,
                private: options.private,
                webview: webview.clone(),
                user_content_manager: ucm,
                offscreen: Some(offscreen),
                visible: !options.background,
                bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1180.0,
                    height: 780.0,
                },
                find_tracker,
                applied_filters,
                user_stylesheets: Vec::new(),
            },
        );

        if let Some(url) = initial_url {
            webview.load_uri(url.as_str());
        }

        Ok(id)
    }

    pub(crate) fn close_tab(&mut self, id: TabId) -> EngineResult<()> {
        let mut tab = self.tabs.remove(&id).ok_or(EngineError::NoSuchTab(id))?;
        self.webview_to_tab
            .borrow_mut()
            .remove(&(tab.webview.as_ptr() as usize));

        tab.webview.stop_loading();
        if let Some(parent) = tab.webview.parent()
            && let Ok(container) = parent.downcast::<gtk::Container>()
        {
            container.remove(&tab.webview);
        }
        if let Some(offscreen) = tab.offscreen.take() {
            offscreen.close();
        }

        if tab.private {
            let key = (tab.space, tab.profile, true);
            let still_open = self
                .tabs
                .values()
                .any(|t| t.private && t.space == tab.space && t.profile == tab.profile);
            if !still_open {
                self.contexts.remove(&key);
                self.stores.remove(&key);
            }
        }
        Ok(())
    }

    pub(crate) fn navigate(&mut self, id: TabId, url: PageUrl) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        tab.webview.load_uri(url.as_str());
        Ok(())
    }

    pub(crate) fn back(&mut self, id: TabId) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        if tab.webview.can_go_back() {
            tab.webview.go_back();
        }
        Ok(())
    }

    pub(crate) fn forward(&mut self, id: TabId) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        if tab.webview.can_go_forward() {
            tab.webview.go_forward();
        }
        Ok(())
    }

    pub(crate) fn reload(&mut self, id: TabId, from_origin: bool) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        if from_origin {
            tab.webview.reload_bypass_cache();
        } else {
            tab.webview.reload();
        }
        Ok(())
    }

    pub(crate) fn stop(&mut self, id: TabId) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        tab.webview.stop_loading();
        Ok(())
    }

    pub(crate) fn eval(&mut self, id: TabId, world: ScriptWorld, js: &str) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        let world_name = match world {
            ScriptWorld::Page => None,
            ScriptWorld::Isolated => Some(ISOLATED_WORLD_NAME),
        };
        tab.webview.evaluate_javascript(
            js,
            world_name,
            None,
            None::<&gio::Cancellable>,
            |_result| {},
        );
        Ok(())
    }

    pub(crate) fn inject(
        &mut self,
        id: TabId,
        world: ScriptWorld,
        at: InjectAt,
        js: &str,
    ) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        let time = match at {
            InjectAt::DocumentStart => UserScriptInjectionTime::Start,
            InjectAt::DocumentEnd => UserScriptInjectionTime::End,
        };
        let script = match world {
            ScriptWorld::Page => {
                UserScript::new(js, UserContentInjectedFrames::AllFrames, time, &[], &[])
            }
            ScriptWorld::Isolated => UserScript::for_world(
                js,
                UserContentInjectedFrames::AllFrames,
                time,
                ISOLATED_WORLD_NAME,
                &[],
                &[],
            ),
        };
        tab.user_content_manager.add_script(&script);
        Ok(())
    }

    pub(crate) fn set_bounds(&mut self, id: TabId, rect: Rect) -> EngineResult<()> {
        if !(rect.width.is_finite()
            && rect.height.is_finite()
            && rect.width >= 0.0
            && rect.height >= 0.0)
        {
            return Err(EngineError::Backend(format!(
                "invalid bounds: {}x{}",
                rect.width, rect.height
            )));
        }
        let tab = self.tabs.get_mut(&id).ok_or(EngineError::NoSuchTab(id))?;
        tab.bounds = rect;
        let width = (rect.width.round() as i32).max(1);
        let height = (rect.height.round() as i32).max(1);
        if let Some(offscreen) = tab.offscreen.as_ref()
            && tab.webview.parent().as_ref() == Some(offscreen.upcast_ref::<gtk::Widget>())
        {
            offscreen.resize(width, height);
            tab.webview.set_size_request(width, height);
        }
        Ok(())
    }

    pub(crate) fn set_visible(&mut self, id: TabId, visible: bool) -> EngineResult<()> {
        let tab = self.tabs.get_mut(&id).ok_or(EngineError::NoSuchTab(id))?;
        tab.visible = visible;
        tab.webview.set_visible(visible);
        Ok(())
    }

    pub(crate) fn focus(&mut self, id: TabId) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        tab.webview.grab_focus();
        Ok(())
    }

    pub(crate) fn snapshot_cell(cell: &Rc<RefCell<Self>>, id: TabId) -> EngineResult<Image> {
        let webview = {
            let engine = cell
                .try_borrow()
                .map_err(|_| EngineError::Backend("re-entrant borrow during snapshot".into()))?;
            engine
                .tabs
                .get(&id)
                .ok_or(EngineError::NoSuchTab(id))?
                .webview
                .clone()
        };

        let cancellable = gio::Cancellable::new();
        let outcome: Rc<RefCell<Option<Result<Image, String>>>> = Rc::new(RefCell::new(None));
        let outcome_cb = outcome.clone();

        webview.snapshot(
            SnapshotRegion::Visible,
            SnapshotOptions::NONE,
            Some(&cancellable),
            move |result| {
                let converted = match result {
                    Ok(surface) => {
                        let mut png = Vec::new();
                        match surface.write_to_png(&mut png) {
                            Ok(()) => {
                                let (width, height) = png_dimensions(&png).unwrap_or((1, 1));
                                Ok(Image { width, height, png })
                            }
                            Err(err) => Err(format!("PNG encode failed: {err}")),
                        }
                    }
                    Err(err) => Err(format!("WebKitGTK snapshot failed: {err}")),
                };
                *outcome_cb.borrow_mut() = Some(converted);
            },
        );

        let finished = wait_until(ASYNC_OP_TIMEOUT, || outcome.borrow().is_some());
        if !finished {
            gio::prelude::CancellableExt::cancel(&cancellable);
            return Err(EngineError::Backend(format!(
                "snapshot timed out for tab {id}"
            )));
        }

        match outcome.borrow_mut().take() {
            Some(Ok(image)) => Ok(image),
            Some(Err(msg)) => Err(EngineError::Backend(msg)),
            None => Err(EngineError::Backend(
                "snapshot completed without result".into(),
            )),
        }
    }

    pub(crate) fn find_cell(
        cell: &Rc<RefCell<Self>>,
        id: TabId,
        query: &str,
        options: FindOptions,
    ) -> EngineResult<FindState> {
        let (fc, tracker_rc) = {
            let engine = cell
                .try_borrow()
                .map_err(|_| EngineError::Backend("re-entrant borrow during find".into()))?;
            let tab = engine.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
            let fc = tab
                .webview
                .find_controller()
                .ok_or_else(|| EngineError::Backend("WebView has no FindController".into()))?;
            (fc, tab.find_tracker.clone())
        };

        if query.is_empty() {
            fc.search_finish();
            let mut tracker = tracker_rc.borrow_mut();
            *tracker = FindTracker::default();
            return Ok(FindState::default());
        }

        let mut wk_opts = WkFindOptions::NONE;
        if !options.case_sensitive {
            wk_opts |= WkFindOptions::CASE_INSENSITIVE;
        }
        if options.whole_words {
            wk_opts |= WkFindOptions::AT_WORD_STARTS;
        }
        if options.backwards {
            wk_opts |= WkFindOptions::BACKWARDS;
        }
        if options.wrap {
            wk_opts |= WkFindOptions::WRAP_AROUND;
        }

        let key = FindQueryKey {
            case_sensitive: options.case_sensitive,
            whole_words: options.whole_words,
        };

        let is_step = {
            let t = tracker_rc.borrow();
            t.query == query && t.key == Some(key) && t.matches > 0
        };

        if is_step {
            {
                let mut t = tracker_rc.borrow_mut();
                t.search_done = false;
                t.found = false;
                if options.backwards {
                    t.active = if t.active <= 1 {
                        if options.wrap { t.matches } else { 1 }
                    } else {
                        t.active - 1
                    };
                } else {
                    t.active = if t.active >= t.matches {
                        if options.wrap { 1 } else { t.matches }
                    } else {
                        t.active + 1
                    };
                }
            }
            if options.backwards {
                fc.search_previous();
            } else {
                fc.search_next();
            }
            let _ = wait_until(FIND_OP_TIMEOUT, || tracker_rc.borrow().search_done);
            let t = tracker_rc.borrow();
            return Ok(FindState {
                query: query.to_owned(),
                matches: t.matches,
                active: if t.found { t.active } else { 0 },
                finished: t.search_done,
            });
        }

        {
            let mut t = tracker_rc.borrow_mut();
            t.query = query.to_owned();
            t.key = Some(key);
            t.matches = 0;
            t.active = 0;
            t.count_done = false;
            t.search_done = false;
            t.found = false;
        }

        let count_opts = (wk_opts & !WkFindOptions::BACKWARDS).bits();
        fc.count_matches(query, count_opts, 1_000);
        fc.search(query, wk_opts.bits(), 1_000);

        let _ = wait_until(FIND_OP_TIMEOUT, || {
            let t = tracker_rc.borrow();
            t.count_done && t.search_done
        });

        let mut t = tracker_rc.borrow_mut();
        if t.found && t.matches == 0 {
            t.matches = 1;
        }
        t.active = if t.found && t.matches > 0 {
            if options.backwards { t.matches } else { 1 }
        } else {
            0
        };

        Ok(FindState {
            query: query.to_owned(),
            matches: t.matches,
            active: t.active,
            finished: t.count_done && t.search_done,
        })
    }

    pub(crate) fn set_zoom(&mut self, id: TabId, zoom: f64) -> EngineResult<()> {
        if !(zoom.is_finite() && (0.20..=6.0).contains(&zoom)) {
            return Err(EngineError::Backend(format!("zoom {zoom} out of range")));
        }
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        tab.webview.set_zoom_level(zoom);
        Ok(())
    }

    pub(crate) fn set_muted(&mut self, id: TabId, muted: bool) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        let was_muted = tab.webview.is_muted();
        tab.webview.set_is_muted(muted);
        if was_muted == muted {
            self.push_event(EngineEvent::MediaStateChanged {
                tab: id,
                audible: tab.webview.is_playing_audio(),
                muted,
            });
        }
        Ok(())
    }

    pub(crate) fn data_store(
        &mut self,
        space: SpaceId,
        profile: ProfileId,
        private: bool,
    ) -> EngineResult<DataStoreHandle> {
        let ctx_ref = self.ensure_context(space, profile, private)?;
        Ok(ctx_ref.handle)
    }

    pub(crate) fn clear_data_cell(
        cell: &Rc<RefCell<Self>>,
        store: DataStoreHandle,
        kinds: DataKinds,
    ) -> EngineResult<()> {
        let (data_manager, favicons) = {
            let engine = cell
                .try_borrow()
                .map_err(|_| EngineError::Backend("re-entrant borrow during clear_data".into()))?;
            let key = (store.space, store.profile, store.ephemeral);
            let Some(entry) = engine.contexts.get(&key) else {
                if engine.stores.get(&key) == Some(&store) {
                    return Ok(());
                }
                return Err(EngineError::Backend(format!(
                    "unknown data store handle {}",
                    store.id
                )));
            };
            if entry.handle != store {
                return Err(EngineError::Backend(format!(
                    "stale data store handle {}",
                    store.id
                )));
            }
            (entry.data_manager.clone(), entry.context.favicon_database())
        };

        let mut types = WebsiteDataTypes::empty();
        if kinds.cookies {
            types |= WebsiteDataTypes::COOKIES;
        }
        if kinds.cache {
            types |= WebsiteDataTypes::MEMORY_CACHE
                | WebsiteDataTypes::DISK_CACHE
                | WebsiteDataTypes::OFFLINE_APPLICATION_CACHE;
        }
        if kinds.site_data {
            types |= WebsiteDataTypes::LOCAL_STORAGE
                | WebsiteDataTypes::SESSION_STORAGE
                | WebsiteDataTypes::INDEXEDDB_DATABASES
                | WebsiteDataTypes::SERVICE_WORKER_REGISTRATIONS
                | WebsiteDataTypes::DOM_CACHE
                | WebsiteDataTypes::HSTS_CACHE
                | WebsiteDataTypes::ITP;
        }

        if !types.is_empty()
            && let Some(dm) = data_manager.as_ref()
        {
            let cancellable = gio::Cancellable::new();
            let outcome: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
            let outcome_cb = outcome.clone();
            dm.clear(
                types,
                glib::TimeSpan::from_seconds(0),
                Some(&cancellable),
                move |res| {
                    if let Ok(mut guard) = outcome_cb.lock() {
                        *guard = Some(res.map_err(|err| err.message().to_owned()));
                    }
                },
            );
            let finished = wait_until(ASYNC_OP_TIMEOUT, || {
                outcome.lock().is_ok_and(|guard| guard.is_some())
            });
            if !finished {
                gio::prelude::CancellableExt::cancel(&cancellable);
                return Err(EngineError::Backend("clear_data timed out".into()));
            }
            let result = outcome
                .lock()
                .map_err(|_| EngineError::Backend("clear_data result lock poisoned".into()))?
                .take();
            if let Some(Err(msg)) = result {
                return Err(EngineError::Backend(format!("clear_data failed: {msg}")));
            }
        }

        if kinds.favicons
            && let Some(favicons) = favicons
        {
            favicons.clear();
        }
        Ok(())
    }

    pub(crate) fn webview_widget(&self, id: TabId) -> Option<gtk::Widget> {
        self.tabs
            .get(&id)
            .map(|tab| tab.webview.clone().upcast::<gtk::Widget>())
    }

    pub(crate) fn apply_content_rules_cell(
        cell: &Rc<RefCell<Self>>,
        identifier: &str,
        rules_json: &str,
    ) -> EngineResult<()> {
        let store = {
            let mut engine = cell.try_borrow_mut().map_err(|_| {
                EngineError::Backend("re-entrant borrow during apply_content_rules".into())
            })?;
            if engine.filter_store.is_none() {
                let store_dir = engine.data_root.join("content-filters");
                engine.filter_store = Some(UserContentFilterStore::new(&store_dir)?);
            }
            engine
                .filter_store
                .clone()
                .ok_or_else(|| EngineError::Backend("UserContentFilterStore unavailable".into()))?
        };

        if rules_json.trim().is_empty() || rules_json.trim() == "[]" {
            {
                let mut engine = cell.try_borrow_mut().map_err(|_| {
                    EngineError::Backend("re-entrant borrow clearing content rules".into())
                })?;
                engine.active_filters.remove(identifier);
                for tab in engine.tabs.values_mut() {
                    tab.user_content_manager.remove_filter_by_id(identifier);
                    tab.applied_filters.retain(|f| f != identifier);
                }
            }
            let _ = store.remove_sync(identifier, ASYNC_OP_TIMEOUT);
            return Ok(());
        }

        let filter = store.save_sync(identifier, rules_json, ASYNC_OP_TIMEOUT)?;
        let mut engine = cell.try_borrow_mut().map_err(|_| {
            EngineError::Backend("re-entrant borrow installing content rules".into())
        })?;
        for tab in engine.tabs.values_mut() {
            tab.user_content_manager.remove_filter_by_id(identifier);
            filter.add_to(&tab.user_content_manager);
            if !tab.applied_filters.iter().any(|f| f == identifier) {
                tab.applied_filters.push(identifier.to_owned());
            }
        }
        engine.active_filters.insert(identifier.to_owned(), filter);
        Ok(())
    }

    pub(crate) fn apply_user_stylesheet(
        &mut self,
        id: TabId,
        css: &str,
        all_frames: bool,
        user_level: bool,
    ) -> EngineResult<()> {
        let tab = self.tabs.get_mut(&id).ok_or(EngineError::NoSuchTab(id))?;
        let frames = if all_frames {
            UserContentInjectedFrames::AllFrames
        } else {
            UserContentInjectedFrames::TopFrame
        };
        let level = if user_level {
            UserStyleLevel::User
        } else {
            UserStyleLevel::Author
        };
        let sheet = UserStyleSheet::new(css, frames, level, &[], &[]);
        tab.user_content_manager.add_style_sheet(&sheet);
        tab.user_stylesheets.push(sheet);
        Ok(())
    }

    pub(crate) fn clear_user_stylesheets(&mut self, id: TabId) -> EngineResult<()> {
        let tab = self.tabs.get_mut(&id).ok_or(EngineError::NoSuchTab(id))?;
        tab.user_content_manager.remove_all_style_sheets();
        tab.user_stylesheets.clear();
        Ok(())
    }

    pub(crate) fn set_downloads_dir(&mut self, dir: PathBuf) {
        *self.downloads_dir.borrow_mut() = dir;
    }

    pub(crate) fn terminate_tab_process(&mut self, id: TabId) -> EngineResult<()> {
        let tab = self.tabs.get(&id).ok_or(EngineError::NoSuchTab(id))?;
        tab.webview.terminate_web_process();
        Ok(())
    }

    pub(crate) fn tab_diagnostics(&self, id: TabId) -> Option<TabDiagnostics> {
        self.tabs.get(&id).map(|tab| TabDiagnostics {
            id: tab.id,
            visible: tab.visible,
            bounds: tab.bounds,
            zoom: tab.webview.zoom_level(),
            muted: tab.webview.is_muted(),
            ephemeral: tab.webview.is_ephemeral(),
            filter_count: tab.applied_filters.len(),
            stylesheet_count: tab.user_stylesheets.len(),
        })
    }
}

/// Snapshot of runtime properties for a live WebKitGTK tab (used by Linux shell
/// diagnostics and contract tests).
#[derive(Clone, Debug, PartialEq)]
pub struct TabDiagnostics {
    /// Tab ID.
    pub id: TabId,
    /// Widget visibility state.
    pub visible: bool,
    /// Last configured bounds.
    pub bounds: Rect,
    /// Current zoom level.
    pub zoom: f64,
    /// Current mute state.
    pub muted: bool,
    /// Whether the underlying WebView uses an ephemeral session.
    pub ephemeral: bool,
    /// Number of active compiled content filters.
    pub filter_count: usize,
    /// Number of active user stylesheets.
    pub stylesheet_count: usize,
}

struct ContextEntryRef {
    handle: DataStoreHandle,
    context: WebContext,
}

#[allow(clippy::too_many_lines)]
fn connect_webview_signals(
    tab_id: TabId,
    webview: &WebView,
    ucm: &UserContentManager,
    find_tracker: &Rc<RefCell<FindTracker>>,
    events: &Arc<Mutex<VecDeque<EngineEvent>>>,
    is_private: bool,
) {
    let current_url: Rc<RefCell<Option<PageUrl>>> = Rc::new(RefCell::new(None));
    let failed_current_load = Rc::new(Cell::new(false));
    let last_media_state = Rc::new(Cell::new((false, false)));

    let emit = {
        let events = events.clone();
        move |event: EngineEvent| {
            if let Ok(mut guard) = events.lock() {
                guard.push_back(event);
            }
        }
    };

    // Load lifecycle
    {
        let emit = emit.clone();
        let current_url = current_url.clone();
        let failed = failed_current_load.clone();
        webview.connect_load_changed(move |view, load_event| match load_event {
            LoadEvent::Started => {
                failed.set(false);
                if let Some(url) = view.uri().and_then(|u| parse_page_url_safe(&u)) {
                    *current_url.borrow_mut() = Some(url.clone());
                    emit(EngineEvent::NavigationStarted { tab: tab_id, url });
                }
            }
            LoadEvent::Redirected => {
                if let Some(url) = view.uri().and_then(|u| parse_page_url_safe(&u)) {
                    *current_url.borrow_mut() = Some(url.clone());
                    emit(EngineEvent::UrlChanged { tab: tab_id, url });
                }
            }
            LoadEvent::Committed => {
                if let Some(url) = view.uri().and_then(|u| parse_page_url_safe(&u)) {
                    *current_url.borrow_mut() = Some(url.clone());
                    emit(EngineEvent::NavigationCommitted { tab: tab_id, url });
                }
                emit(EngineEvent::HistoryChanged {
                    tab: tab_id,
                    can_go_back: view.can_go_back(),
                    can_go_forward: view.can_go_forward(),
                });
            }
            LoadEvent::Finished => {
                if failed.get() {
                    return;
                }
                if let Some(url) = view.uri().and_then(|u| parse_page_url_safe(&u)) {
                    *current_url.borrow_mut() = Some(url.clone());
                    emit(EngineEvent::NavigationFinished { tab: tab_id, url });
                }
                emit(EngineEvent::HistoryChanged {
                    tab: tab_id,
                    can_go_back: view.can_go_back(),
                    can_go_forward: view.can_go_forward(),
                });
            }
            _ => {}
        });
    }

    // Load failures
    {
        let emit = emit.clone();
        let failed = failed_current_load.clone();
        webview.connect_load_failed(move |_view, _event, uri, err| {
            failed.set(true);
            if let Some(url) = parse_page_url_safe(uri).or_else(about_blank_url) {
                emit(EngineEvent::NavigationFailed {
                    tab: tab_id,
                    url,
                    reason: err.message().to_owned(),
                });
            }
            true
        });
    }

    // TLS certificate failures
    {
        let emit = emit.clone();
        let failed = failed_current_load;
        webview.connect_load_failed_with_tls_errors(move |_view, failing_uri, _cert, errors| {
            failed.set(true);
            if let Some(url) = parse_page_url_safe(failing_uri).or_else(about_blank_url) {
                emit(EngineEvent::CertificateError {
                    tab: tab_id,
                    url,
                    reason: format!("{errors:?}"),
                });
            }
            true
        });
    }

    // Title
    {
        let emit = emit.clone();
        webview.connect_title_notify(move |view| {
            let title = view.title().map(|t| t.to_string()).unwrap_or_default();
            emit(EngineEvent::TitleChanged { tab: tab_id, title });
        });
    }

    // URI (History API / pushState / anchor changes)
    {
        let emit = emit.clone();
        webview.connect_uri_notify(move |view| {
            let Some(url) = view.uri().and_then(|u| parse_page_url_safe(&u)) else {
                return;
            };
            let mut slot = current_url.borrow_mut();
            if slot.as_ref() != Some(&url) {
                *slot = Some(url.clone());
                emit(EngineEvent::UrlChanged { tab: tab_id, url });
            }
        });
    }

    // Loading state
    {
        let emit = emit.clone();
        webview.connect_is_loading_notify(move |view| {
            emit(EngineEvent::LoadingChanged {
                tab: tab_id,
                loading: view.is_loading(),
            });
        });
    }

    // Favicon
    {
        let emit = emit.clone();
        webview.connect_favicon_notify(move |view| {
            let icon_url = view
                .context()
                .and_then(|ctx| ctx.favicon_database())
                .and_then(|db| view.uri().and_then(|page| db.favicon_uri(&page)))
                .and_then(|icon| parse_page_url_safe(&icon));
            emit(EngineEvent::FaviconChanged {
                tab: tab_id,
                url: icon_url,
            });
        });
    }

    // Media / audio state
    {
        let emit = emit.clone();
        let last_media = last_media_state.clone();
        webview.connect_is_playing_audio_notify(move |view| {
            let state = (view.is_playing_audio(), view.is_muted());
            if last_media.replace(state) != state {
                emit(EngineEvent::MediaStateChanged {
                    tab: tab_id,
                    audible: state.0,
                    muted: state.1,
                });
            }
        });
    }
    {
        let emit = emit.clone();
        let last_media = last_media_state;
        webview.connect_is_muted_notify(move |view| {
            let state = (view.is_playing_audio(), view.is_muted());
            if last_media.replace(state) != state {
                emit(EngineEvent::MediaStateChanged {
                    tab: tab_id,
                    audible: state.0,
                    muted: state.1,
                });
            }
        });
    }

    // Fullscreen
    {
        let emit = emit.clone();
        webview.connect_enter_fullscreen(move |_view| {
            emit(EngineEvent::FullscreenChanged {
                tab: tab_id,
                fullscreen: true,
            });
            false
        });
    }
    {
        let emit = emit.clone();
        webview.connect_leave_fullscreen(move |_view| {
            emit(EngineEvent::FullscreenChanged {
                tab: tab_id,
                fullscreen: false,
            });
            false
        });
    }

    // WebProcess crash / termination
    {
        let emit = emit.clone();
        webview.connect_web_process_terminated(move |_view, reason| {
            let reason_text = match reason {
                WebProcessTerminationReason::Crashed => "web process crashed",
                WebProcessTerminationReason::ExceededMemoryLimit => {
                    "web process exceeded memory limit"
                }
                WebProcessTerminationReason::TerminatedByApi => "web process terminated by API",
                _ => "web process terminated",
            };
            emit(EngineEvent::ProcessCrashed {
                tab: tab_id,
                reason: reason_text.to_owned(),
            });
        });
    }

    // Permissions (fail-closed, especially in private tabs per FEATURES.md §5.9)
    {
        let emit = emit.clone();
        webview.connect_permission_request(move |view, req| {
            let origin = view
                .uri()
                .and_then(|u| parse_page_url_safe(&u))
                .and_then(|p| p.origin())
                .unwrap_or_default();
            let kind = classify_permission_request(req);
            if let Some(kind) = kind {
                emit(EngineEvent::PermissionRequested {
                    tab: tab_id,
                    origin,
                    kind,
                });
            }
            // Private tabs and unconfirmed requests always deny safely until an explicit
            // permission response flow is wired on top of the Engine trait.
            let _ = is_private;
            req.deny();
            true
        });
    }

    // JS dialogs
    {
        let emit = emit.clone();
        webview.connect_script_dialog(move |_view, dialog| {
            let kind = match dialog.dialog_type() {
                ScriptDialogType::Alert => DialogKind::Alert,
                ScriptDialogType::Confirm => {
                    dialog.confirm_set_confirmed(false);
                    DialogKind::Confirm
                }
                ScriptDialogType::Prompt => DialogKind::Prompt,
                ScriptDialogType::BeforeUnloadConfirm => {
                    dialog.confirm_set_confirmed(true);
                    DialogKind::BeforeUnload
                }
                _ => DialogKind::Alert,
            };
            let message = dialog.message().map(|m| m.to_string()).unwrap_or_default();
            emit(EngineEvent::ScriptDialog {
                tab: tab_id,
                kind,
                message,
            });
            true
        });
    }

    // File picker requests
    {
        let emit = emit.clone();
        webview.connect_run_file_chooser(move |_view, req| {
            emit(EngineEvent::FilePickerRequested {
                tab: tab_id,
                multiple: req.selects_multiple(),
            });
            req.cancel();
            true
        });
    }

    // HTTP authentication requests
    {
        let emit = emit.clone();
        webview.connect_authenticate(move |_view, req| {
            let host = req.host().map(|h| h.to_string()).unwrap_or_default();
            let port = req.port();
            let origin = if port > 0 {
                format!("{host}:{port}")
            } else {
                host
            };
            emit(EngineEvent::AuthRequired {
                tab: tab_id,
                origin,
            });
            req.cancel();
            true
        });
    }

    // New window / popup requests (`create` + `decide-policy`)
    {
        let emit = emit.clone();
        webview.connect_create(move |_view, action| {
            if let Some(url) = action
                .request()
                .and_then(|req| req.uri())
                .and_then(|u| parse_page_url_safe(&u))
            {
                emit(EngineEvent::NewWindowRequested {
                    tab: tab_id,
                    url,
                    disposition: disposition_from_action(action),
                });
            }
            None
        });
    }
    {
        let emit = emit.clone();
        webview.connect_decide_policy(move |_view, decision, kind| match kind {
            PolicyDecisionType::NewWindowAction => {
                if let Ok(nav) = decision.clone().downcast::<NavigationPolicyDecision>()
                    && let Some(action) = nav.navigation_action()
                    && let Some(url) = action
                        .request()
                        .and_then(|req| req.uri())
                        .and_then(|u| parse_page_url_safe(&u))
                {
                    let disposition = disposition_from_action(&action);
                    emit(EngineEvent::NewWindowRequested {
                        tab: tab_id,
                        url,
                        disposition,
                    });
                }
                decision.ignore();
                true
            }
            PolicyDecisionType::Response => {
                if let Ok(resp) = decision.clone().downcast::<ResponsePolicyDecision>()
                    && !resp.is_mime_type_supported()
                    && resp.is_main_frame_main_resource()
                {
                    decision.download();
                    return true;
                }
                false
            }
            _ => false,
        });
    }

    // Script message handlers (isolated world vs page world)
    {
        let emit = emit.clone();
        ucm.connect_script_message_received(Some(ISOLATED_HANDLER_NAME), move |_ucm, msg| {
            let body = msg.js_value().map(|v| v.to_string()).unwrap_or_default();
            emit(EngineEvent::ScriptMessage {
                tab: tab_id,
                world: ScriptWorld::Isolated,
                body,
            });
        });
    }
    {
        ucm.connect_script_message_received(Some(PAGE_HANDLER_NAME), move |_ucm, msg| {
            let body = msg.js_value().map(|v| v.to_string()).unwrap_or_default();
            emit(EngineEvent::ScriptMessage {
                tab: tab_id,
                world: ScriptWorld::Page,
                body,
            });
        });
    }

    // FindController signals
    if let Some(fc) = webview.find_controller() {
        {
            let tracker = find_tracker.clone();
            fc.connect_counted_matches(move |_fc, count| {
                let mut t = tracker.borrow_mut();
                t.matches = count.min(1_000);
                t.count_done = true;
            });
        }
        {
            let tracker = find_tracker.clone();
            fc.connect_found_text(move |_fc, count| {
                let mut t = tracker.borrow_mut();
                if t.matches == 0 && count > 0 {
                    t.matches = count.min(1_000);
                }
                t.found = true;
                t.search_done = true;
            });
        }
        {
            let tracker = find_tracker.clone();
            fc.connect_failed_to_find_text(move |_fc| {
                let mut t = tracker.borrow_mut();
                t.found = false;
                t.search_done = true;
            });
        }
    }
}

fn classify_permission_request(req: &webkit2gtk::PermissionRequest) -> Option<PermissionKind> {
    if req.is::<webkit2gtk::GeolocationPermissionRequest>() {
        Some(PermissionKind::Geolocation)
    } else if req.is::<webkit2gtk::NotificationPermissionRequest>() {
        Some(PermissionKind::Notifications)
    } else if let Some(media) = req.downcast_ref::<webkit2gtk::UserMediaPermissionRequest>() {
        if filters::is_display_capture_permission(media) {
            Some(PermissionKind::ScreenCapture)
        } else if media.is_for_video_device() {
            Some(PermissionKind::Camera)
        } else if media.is_for_audio_device() {
            Some(PermissionKind::Microphone)
        } else {
            None
        }
    } else {
        None
    }
}

fn disposition_from_action(action: &NavigationAction) -> NewWindowDisposition {
    if action.modifiers() & gtk::gdk::ModifierType::SHIFT_MASK.bits() != 0 {
        NewWindowDisposition::BackgroundTab
    } else {
        NewWindowDisposition::ForegroundTab
    }
}

fn parse_page_url_safe(raw: &str) -> Option<PageUrl> {
    PageUrl::parse(raw).ok()
}

fn about_blank_url() -> Option<PageUrl> {
    PageUrl::parse("about:blank").ok()
}

fn png_dimensions(png: &[u8]) -> Option<(u32, u32)> {
    // PNG signature (8) + IHDR length (4) + "IHDR" (4) + width (4) + height (4) = 24 bytes.
    if png.len() < 24 || &png[0..8] != b"\x89PNG\r\n\x1a\n" || &png[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    Some((width, height))
}

fn default_internal_page(uri: &str) -> InternalPageResponse {
    let title = uri.strip_prefix("search://").unwrap_or("searlx");
    let escaped_title = html_escape(title);
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{INTERNAL_PAGE_CSP}\">\
         <title>{escaped_title}</title>\
         <style>body{{background:#181816;color:#ecece8;font-family:system-ui,sans-serif;\
         max-width:720px;margin:48px auto;padding:0 24px;line-height:1.6}}\
         code{{background:#282824;padding:2px 6px;border-radius:4px}}</style></head>\
         <body><h1>{escaped_title}</h1><p>searlx internal page <code>{escaped_title}</code></p></body></html>"
    );
    InternalPageResponse::html(html)
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
