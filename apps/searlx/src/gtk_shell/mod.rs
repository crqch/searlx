//! Authentic Search-faithful GTK3 + WebKitGTK Linux browser shell (`ADR-0002`, `P5-01`).
//!
//! Composition root wiring [`searlx_core::BrowserController`] directly to
//! [`searlx_engine_linux::WebKitGtkEngine`] across modular GTK3 views:
//! - `theme.rs`: Light/Dark/System + High Contrast + Reduce Motion + Space washes
//! - `sidebar.rs`: No permanent toolbar; left/right/top-tabs/icons-only/folded edge-peek
//! - `stage.rs`: Rounded 10 pt page card (8 pt gap), 2-4 pane Split View, Side Panel, Bookmarks bar
//! - `overlays.rs`: `Ctrl+L` floating field, Site Card popover, `Ctrl+F` Find pill, Prompts, Peek, Switcher
//! - `internal_pages.rs`: CSP-compliant interactive `search://` pages over isolated `ScriptMessage`
//! - `engine_bridge.rs`: `EngineDirective` execution + `WebKitGtkEngine::pump()` reconciliation

mod actions;
mod engine_bridge;
mod internal_pages;
mod overlays;
mod sidebar;
mod stage;
mod theme;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtk::gdk;
use gtk::prelude::*;
use searlx_core::controller::EngineEvent as CtrlEvent;
use searlx_core::{
    AddressResolution, AppCommand, BrowserController, DropTarget, EngineDirective,
    ExternalDialogRequest, InternalPageKind, PageUrl, ParseOptions, RestoreMode, SpaceColor,
    SpaceId, SpaceStorePolicy, SplitDropSide, SubmitDisposition, TabId, TabLayoutMode, UiSnapshot,
    resolve_address,
};
use searlx_engine::Engine;
use searlx_engine_linux::{InternalPageResponse, WebKitGtkEngine};

use actions::{ActionSink, ShellAction};
use engine_bridge::{EngineBridge, expand_downloads_dir, resolve_data_roots};
use internal_pages::{InternalAction, InternalPageContext, render_internal_uri};
use overlays::ShellOverlays;
use sidebar::{TabSidebar, wire_drop_target};
use stage::StageSurface;
use theme::{
    FOLDED_DWELL_MS, FOLDED_EDGE_ZONE_PX, FOLDED_GRACE_MS, SIDEBAR_DEFAULT, SIDEBAR_MAX,
    SIDEBAR_MIN, ShellPalette, SpaceChromeTint, WINDOW_DEFAULT_HEIGHT, WINDOW_DEFAULT_WIDTH,
    WINDOW_MIN_HEIGHT, WINDOW_MIN_WIDTH, apply_gtk_dark_preference, build_gtk_css,
    detect_system_dark, reveal_widget,
};

struct ShellCore {
    controller: BrowserController,
    bridge: EngineBridge,
    internal_ctx: Arc<RwLock<Option<InternalPageContext>>>,
    css_provider: gtk::CssProvider,
    last_css: String,
    last_dark: bool,
    sidebar_width_pt: i32,
    folded_peek_open: bool,
    hide_picker_active: bool,
    ctrl_held: bool,
    dragging_tab: bool,
    last_layout_key: Option<(TabLayoutMode, bool, bool)>,
    window: gtk::Window,
    main_vbox: gtk::Box,
    body_overlay: gtk::Overlay,
    body_box: gtk::Box,
    folded_peek_box: gtk::Box,
    edge_zone: gtk::EventBox,
    sidebar: TabSidebar,
    stage: StageSurface,
    overlays: Rc<ShellOverlays>,
    actions: ActionSink,
}

/// Launch the Linux GTK3 + WebKitGTK Search browser shell (`ADR-0002`, `P5-01`).
pub fn run_gtk_browser(initial_urls: &[String]) -> Result<(), String> {
    let (data_root, webkit_root) = resolve_data_roots();
    let mut engine = WebKitGtkEngine::with_data_root_on_current_thread(webkit_root)
        .map_err(|err| err.to_string())?;

    let internal_ctx: Arc<RwLock<Option<InternalPageContext>>> = Arc::new(RwLock::new(None));
    let ctx_for_handler = internal_ctx.clone();
    engine
        .register_internal_handler(move |uri| {
            let guard = ctx_for_handler.read().ok();
            match guard.as_ref().and_then(|opt| opt.as_ref()) {
                Some(ctx) => render_internal_uri(uri, ctx),
                None => InternalPageResponse::html(
                    "<!doctype html><html><body><p>Initializing searlx…</p></body></html>",
                ),
            }
        })
        .map_err(|err| err.to_string())?;

    let caps = EngineBridge::convert_capabilities(&engine.capabilities());
    let (mut controller, init_directives) =
        BrowserController::open(&data_root, caps, RestoreMode::Normal)
            .map_err(|err| format!("failed to restore browser session: {err}"))?;

    let dl_dir = expand_downloads_dir(&controller.settings().downloads_dir);
    let _ = engine.set_downloads_dir(dl_dir);

    let system_dark = detect_system_dark();
    let initial_palette = ShellPalette::resolve(controller.settings(), system_dark);
    apply_gtk_dark_preference(initial_palette.dark);
    if let Ok(dirs) = controller.dispatch(AppCommand::SetFrameDark {
        dark: initial_palette.dark,
    }) {
        let _ = dirs;
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("searlx");
    window.set_default_size(WINDOW_DEFAULT_WIDTH, WINDOW_DEFAULT_HEIGHT);
    window.set_size_request(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT);
    window.style_context().add_class("searlx-window");

    let css_provider = gtk::CssProvider::new();
    if let Some(screen) = gtk::gdk::Screen::default() {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &css_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let engine_cell = Rc::new(RefCell::new(engine));
    let queue: Rc<RefCell<VecDeque<ShellAction>>> = Rc::new(RefCell::new(VecDeque::new()));
    let in_drain = Rc::new(Cell::new(false));
    let core_slot: Rc<RefCell<Option<Rc<RefCell<ShellCore>>>>> = Rc::new(RefCell::new(None));

    let sink: ActionSink = {
        let queue_cb = queue.clone();
        let in_drain_cb = in_drain.clone();
        let core_slot_cb = core_slot.clone();
        let engine_cb = engine_cell.clone();
        Rc::new(move |action: ShellAction| {
            queue_cb.borrow_mut().push_back(action);
            if in_drain_cb.get() {
                return;
            }
            if let Some(core_rc) = core_slot_cb.borrow().clone() {
                drain_action_queue(&core_rc, &engine_cb, &queue_cb, &in_drain_cb);
            }
        })
    };

    let sidebar = TabSidebar::new(sink.clone());
    let stage = StageSurface::new(sink.clone());
    let overlays = Rc::new(ShellOverlays::new(&stage.page_overlay, sink.clone()));
    ShellOverlays::wire_address_signals(&overlays);

    // Wire split-drop DND targets on page edge bands (`FEATURES.md` §3.3).
    wire_split_edge_dnd(
        &stage.split_drop_left,
        SplitDropSide::Left,
        &core_slot,
        &sink,
    );
    wire_split_edge_dnd(
        &stage.split_drop_right,
        SplitDropSide::Right,
        &core_slot,
        &sink,
    );

    let main_vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    main_vbox.style_context().add_class("searlx-ground");
    main_vbox.pack_start(&sidebar.top_strip_box, false, false, 0);

    let body_overlay = gtk::Overlay::new();
    body_overlay.set_hexpand(true);
    body_overlay.set_vexpand(true);
    let body_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body_box.set_hexpand(true);
    body_box.set_vexpand(true);
    body_overlay.add(&body_box);

    let edge_zone = gtk::EventBox::new();
    edge_zone.set_size_request(FOLDED_EDGE_ZONE_PX, -1);
    edge_zone.set_halign(gtk::Align::Start);
    edge_zone.set_valign(gtk::Align::Fill);
    edge_zone.set_visible_window(false);
    edge_zone.set_no_show_all(true);

    let folded_peek_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    folded_peek_box.set_halign(gtk::Align::Start);
    folded_peek_box.set_valign(gtk::Align::Fill);
    folded_peek_box.set_size_request(SIDEBAR_DEFAULT, -1);
    folded_peek_box.set_no_show_all(true);

    body_overlay.add_overlay(&edge_zone);
    body_overlay.add_overlay(&folded_peek_box);
    main_vbox.pack_start(&body_overlay, true, true, 0);
    window.add(&main_vbox);

    let mut bridge = EngineBridge::new();
    {
        let mut eng = engine_cell
            .try_borrow_mut()
            .map_err(|_| "engine borrowed at init".to_owned())?;
        execute_and_pump(
            &mut controller,
            &mut bridge,
            &mut eng,
            init_directives,
            &window,
            false,
        );

        for raw_url in initial_urls {
            let trimmed = raw_url.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resolved = match resolve_address(trimmed, &ParseOptions::default()) {
                AddressResolution::Navigate(u) | AddressResolution::Search { url: u, .. } => {
                    Some(u)
                }
                _ => PageUrl::parse(trimmed).ok(),
            };
            if let Some(url) = resolved
                && let Ok(dirs) = controller.dispatch(AppCommand::NewTab {
                    url: Some(url),
                    activate: true,
                })
            {
                execute_and_pump(&mut controller, &mut bridge, &mut eng, dirs, &window, false);
            }
        }
    }

    let core_rc = Rc::new(RefCell::new(ShellCore {
        controller,
        bridge,
        internal_ctx,
        css_provider,
        last_css: String::new(),
        last_dark: initial_palette.dark,
        sidebar_width_pt: SIDEBAR_DEFAULT,
        folded_peek_open: false,
        hide_picker_active: false,
        ctrl_held: false,
        dragging_tab: false,
        last_layout_key: None,
        window: window.clone(),
        main_vbox,
        body_overlay,
        body_box,
        folded_peek_box,
        edge_zone,
        sidebar,
        stage,
        overlays,
        actions: sink.clone(),
    }));
    *core_slot.borrow_mut() = Some(core_rc.clone());

    // Initial full UI sync + window show
    {
        let mut eng = engine_cell
            .try_borrow_mut()
            .map_err(|_| "engine borrowed during initial sync".to_owned())?;
        if let Ok(mut core) = core_rc.try_borrow_mut() {
            core.refresh_ui(&mut eng);
        }
    }
    window.show();
    core_rc.borrow().main_vbox.show();
    core_rc.borrow().body_overlay.show();
    core_rc.borrow().body_box.show();
    core_rc.borrow().stage.root_box.show_all();
    // Keep folded peek & hidden overlay bars explicitly hidden after initial show_all on stage.
    {
        let core = core_rc.borrow();
        let snap = core.controller.ui_snapshot();
        core.overlays.sync(&snap, core.hide_picker_active);
        core.stage.set_split_drop_bands_visible(false, None);
        if let Some(aid) = snap.active_tab
            && let Ok(eng) = engine_cell.try_borrow()
            && let Some(w) = core.bridge.widget_for_core_tab(&eng, aid)
            && w.is_realized()
        {
            w.grab_focus();
        }
    }

    wire_window_signals(&window, &core_rc, &sink);
    wire_folded_edge_peek(&core_rc, &sink);

    // Pump WebKitGTK events every 20 ms (`pump()` -> `handle_engine_event` -> execute directives).
    {
        let core_pump = core_rc.clone();
        let engine_pump = engine_cell.clone();
        let queue_pump = queue.clone();
        let in_drain_pump = in_drain.clone();
        glib::timeout_add_local(Duration::from_millis(20), move || {
            let raw_events = match engine_pump.try_borrow_mut() {
                Ok(mut eng) => eng.pump().unwrap_or_default(),
                Err(_) => return glib::ControlFlow::Continue,
            };
            if !raw_events.is_empty()
                && let Ok(mut core) = core_pump.try_borrow_mut()
                && let Ok(mut eng) = engine_pump.try_borrow_mut()
            {
                core.process_raw_events(&mut eng, raw_events);
                core.refresh_ui(&mut eng);
            }
            if !queue_pump.borrow().is_empty() && !in_drain_pump.get() {
                drain_action_queue(&core_pump, &engine_pump, &queue_pump, &in_drain_pump);
            }
            glib::ControlFlow::Continue
        });
    }

    // Background tab sleep / auto-close lifecycle sweep every 15 seconds.
    {
        let sink_tick = sink;
        glib::timeout_add_local(Duration::from_secs(15), move || {
            let secs = current_unix_secs();
            sink_tick(ShellAction::Command(AppCommand::TickLifecycle {
                now_secs: secs,
            }));
            glib::ControlFlow::Continue
        });
    }

    gtk::main();

    // Ensure session is persisted cleanly on exit.
    if let Ok(mut core) = core_rc.try_borrow_mut() {
        let _ = core.controller.dispatch(AppCommand::SaveSession);
    }

    Ok(())
}

impl ShellCore {
    fn refresh_ui(&mut self, engine: &mut WebKitGtkEngine) {
        let system_dark = detect_system_dark();
        let snapshot = self.controller.ui_snapshot();
        let palette = ShellPalette::resolve(&snapshot.settings, system_dark);

        if palette.dark != self.last_dark {
            self.last_dark = palette.dark;
            apply_gtk_dark_preference(palette.dark);
            let _ = self
                .controller
                .dispatch(AppCommand::SetFrameDark { dark: palette.dark });
        }

        // 1. Update CSS theme & space chrome wash
        let active_space_tint =
            snapshot
                .spaces
                .iter()
                .find(|s| s.active)
                .map(|s| SpaceChromeTint {
                    color: s.color,
                    gradient: s.gradient,
                    strength: s.strength,
                });
        let css = build_gtk_css(&palette, &snapshot.settings, active_space_tint);
        if css != self.last_css {
            if let Err(err) = self.css_provider.load_from_data(css.as_bytes()) {
                tracing::warn!(target: "search", error = %err, "GTK CSS load warning");
            }
            self.last_css = css;
            self.bridge.mark_internal_views_dirty(engine);
        }

        // 2. Refresh thread-safe internal page context before creating/navigating `search://` WebViews
        self.update_internal_context(&snapshot, palette.clone());

        // 3. Materialize shell-managed `search://` WebViews for visible internal tabs & Side Panel website
        let active_profile = snapshot.active_profile;
        let active_space = snapshot.active_space;
        if !snapshot.split_panes.is_empty() {
            for pane in &snapshot.split_panes {
                if pane.url.is_internal() {
                    let private = snapshot
                        .tabs
                        .iter()
                        .find(|t| t.id == pane.tab)
                        .is_some_and(|t| t.private);
                    self.bridge.ensure_internal_tab_view(
                        engine,
                        pane.tab,
                        active_space,
                        active_profile,
                        private,
                        &pane.url,
                    );
                }
            }
        } else if let Some(aid) = snapshot.active_tab
            && let Some(tab) = snapshot.tabs.iter().find(|t| t.id == aid)
            && tab.url.is_internal()
        {
            self.bridge.ensure_internal_tab_view(
                engine,
                tab.id,
                active_space,
                active_profile,
                tab.private,
                &tab.url,
            );
        }

        let sp_wanted = if snapshot.side_panel.open && !snapshot.side_panel.held_back_for_private {
            if let Some(searlx_core::sync_panel_launcher_routes::SidePanelContent::Website {
                url,
                space_id,
                ..
            }) = &snapshot.side_panel.content
            {
                Some((*space_id, active_profile, url))
            } else {
                None
            }
        } else {
            None
        };
        self.bridge.sync_side_panel_website(engine, sp_wanted);

        // 4. Rebuild outer layout (Left/Right/TopTabs/IconsOnly/Folded) if layout mode changed
        self.sync_outer_layout(&snapshot);

        // 5. Update Sidebar, Stage, and Overlays
        self.sidebar.sync(&snapshot, palette.dark, self.ctrl_held);
        self.stage.sync(&snapshot, engine, &self.bridge);
        self.overlays.sync(&snapshot, self.hide_picker_active);

        if let Some(tab) = snapshot
            .active_tab
            .and_then(|aid| snapshot.tabs.iter().find(|t| t.id == aid))
        {
            let title = if tab.title.trim().is_empty() {
                "searlx".to_owned()
            } else {
                format!("{} — searlx", tab.title)
            };
            self.window.set_title(&title);
        }
    }

    fn update_internal_context(&self, snapshot: &UiSnapshot, palette: ShellPalette) {
        let hist = self.controller.history_for_space(snapshot.active_space);
        let days = hist.grouped_by_day("2026-11-24", "");
        let recently_closed = hist.recently_closed().to_vec();
        let bookmarks = self
            .controller
            .bookmarks()
            .map(|b| b.tree().clone())
            .unwrap_or_default();
        let passwords = self.controller.password_vault().masked_entries();
        let archived_tabs = self.controller.compositor().history.archive().to_vec();

        let ctx = InternalPageContext::from_snapshot(
            snapshot,
            palette,
            days,
            recently_closed,
            bookmarks,
            passwords,
            archived_tabs,
        );
        if let Ok(mut guard) = self.internal_ctx.write() {
            *guard = Some(ctx);
        }
    }

    fn sync_outer_layout(&mut self, snapshot: &UiSnapshot) {
        let key = (
            snapshot.sidebar_layout,
            snapshot.sidebar_folded,
            self.folded_peek_open,
        );
        if self.last_layout_key == Some(key) {
            return;
        }
        self.last_layout_key = Some(key);

        // Park sidebar.column_box and stage.root_box safely before rearranging outer layout.
        if let Some(p) = self
            .sidebar
            .column_box
            .parent()
            .and_then(|w| w.downcast::<gtk::Container>().ok())
        {
            p.remove(&self.sidebar.column_box);
        }
        if let Some(p) = self
            .stage
            .root_box
            .parent()
            .and_then(|w| w.downcast::<gtk::Container>().ok())
        {
            p.remove(&self.stage.root_box);
        }
        for child in self.body_box.children() {
            self.body_box.remove(&child);
        }

        if snapshot.sidebar_layout == TabLayoutMode::TopTabs {
            self.edge_zone.hide();
            self.folded_peek_box.hide();
            self.sidebar.column_box.hide();
            if snapshot.sidebar_folded {
                self.sidebar.top_strip_box.hide();
            } else {
                reveal_widget(&self.sidebar.top_strip_box);
            }
            self.body_box
                .pack_start(&self.stage.root_box, true, true, 0);
            self.stage.root_box.show();
            return;
        }

        self.sidebar.top_strip_box.hide();

        if snapshot.sidebar_folded {
            let align = if snapshot.sidebar_layout == TabLayoutMode::RightColumn {
                gtk::Align::End
            } else {
                gtk::Align::Start
            };
            self.edge_zone.set_halign(align);
            self.folded_peek_box.set_halign(align);
            self.edge_zone.show();

            self.body_box
                .pack_start(&self.stage.root_box, true, true, 0);
            self.stage.root_box.show();

            if self.folded_peek_open {
                self.sidebar
                    .column_box
                    .style_context()
                    .add_class("folded-peek");
                self.folded_peek_box
                    .pack_start(&self.sidebar.column_box, true, true, 0);
                self.sidebar.column_box.show();
                reveal_widget(&self.folded_peek_box);
            } else {
                self.sidebar
                    .column_box
                    .style_context()
                    .remove_class("folded-peek");
                self.sidebar.column_box.hide();
                self.folded_peek_box.hide();
            }
            return;
        }

        // Unfolded LeftColumn / RightColumn / IconsOnly
        self.edge_zone.hide();
        self.folded_peek_box.hide();
        self.sidebar
            .column_box
            .style_context()
            .remove_class("folded-peek");
        self.sidebar.column_box.show();

        if snapshot.sidebar_layout == TabLayoutMode::IconsOnly {
            if snapshot.sidebar_layout == TabLayoutMode::RightColumn {
                self.body_box
                    .pack_start(&self.stage.root_box, true, true, 0);
                self.body_box
                    .pack_end(&self.sidebar.column_box, false, false, 0);
            } else {
                self.body_box
                    .pack_start(&self.sidebar.column_box, false, false, 0);
                self.body_box
                    .pack_start(&self.stage.root_box, true, true, 0);
            }
        } else {
            let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
            paned.set_hexpand(true);
            paned.set_vexpand(true);
            let sb_w = self.sidebar_width_pt.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
            if snapshot.sidebar_layout == TabLayoutMode::RightColumn {
                paned.pack1(&self.stage.root_box, true, true);
                paned.pack2(&self.sidebar.column_box, false, false);
                let total = self.window.allocation().width().max(WINDOW_DEFAULT_WIDTH);
                paned.set_position((total - sb_w).max(320));
            } else {
                paned.pack1(&self.sidebar.column_box, false, false);
                paned.pack2(&self.stage.root_box, true, true);
                paned.set_position(sb_w);
            }
            let win_clone = self.window.clone();
            let layout_dir = snapshot.sidebar_layout;
            paned.connect_position_notify(move |p| {
                let raw = if layout_dir == TabLayoutMode::RightColumn {
                    win_clone.allocation().width() - p.position()
                } else {
                    p.position()
                };
                if raw < SIDEBAR_MIN {
                    let target = if layout_dir == TabLayoutMode::RightColumn {
                        win_clone.allocation().width() - SIDEBAR_MIN
                    } else {
                        SIDEBAR_MIN
                    };
                    p.set_position(target);
                } else if raw > SIDEBAR_MAX {
                    let target = if layout_dir == TabLayoutMode::RightColumn {
                        win_clone.allocation().width() - SIDEBAR_MAX
                    } else {
                        SIDEBAR_MAX
                    };
                    p.set_position(target);
                }
            });
            self.body_box.pack_start(&paned, true, true, 0);
            paned.show();
        }
        self.stage.root_box.show();
    }

    fn process_raw_events(
        &mut self,
        engine: &mut WebKitGtkEngine,
        raw_events: Vec<searlx_engine::EngineEvent>,
    ) {
        let outcome = self.bridge.classify_raw_events(raw_events);

        if let Some((tab, href)) = outcome.hovered_link
            && self.controller.ui_snapshot().active_tab == Some(tab)
        {
            self.overlays.set_hovered_link(&href);
        }
        if outcome.hide_picker_cancelled {
            self.hide_picker_active = false;
        }
        if outcome.side_panel_crashed {
            let _ = self.controller.dispatch(AppCommand::CloseSidePanel);
        }
        for (tab, progress) in outcome.scroll_progress {
            let _ = progress;
            let _ = tab;
        }

        for ctrl_ev in outcome.controller_events {
            if let Ok(dirs) = self.controller.handle_engine_event(ctrl_ev) {
                execute_and_pump(
                    &mut self.controller,
                    &mut self.bridge,
                    engine,
                    dirs,
                    &self.window,
                    !self.overlays.holds_text_focus() && !self.sidebar.inline_entry_has_focus(),
                );
            }
        }

        for (core_tab, action) in outcome.internal_actions {
            self.handle_internal_page_action(engine, core_tab, action);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_internal_page_action(
        &mut self,
        engine: &mut WebKitGtkEngine,
        core_tab: TabId,
        action: InternalAction,
    ) {
        let mut needs_reload = false;
        let cmd = match action {
            InternalAction::Navigate(url) => Some(AppCommand::NavigateTab { tab: core_tab, url }),
            InternalAction::SubmitAddress(input) => Some(AppCommand::SubmitAddress {
                input,
                disposition: SubmitDisposition::CurrentTab,
                shift_pressed: false,
            }),
            InternalAction::OpenInternal { kind, detail } => {
                needs_reload = true;
                Some(AppCommand::OpenInternalPage { kind, detail })
            }
            InternalAction::SetSetting { key, value } => {
                let mut next = self.controller.settings().clone();
                if internal_pages::apply_setting_mutation(&mut next, &key, &value) {
                    if key == "downloads_dir" {
                        let _ = engine.set_downloads_dir(expand_downloads_dir(&next.downloads_dir));
                    }
                    Some(AppCommand::UpdateSettings {
                        settings: Box::new(next),
                    })
                } else {
                    None
                }
            }
            InternalAction::ClearHistory => {
                needs_reload = true;
                Some(AppCommand::ClearHistory {
                    scope: searlx_core::ClearHistoryScope::All,
                })
            }
            InternalAction::ReopenClosed => Some(AppCommand::ReopenClosedTab),
            InternalAction::AddBookmark { title, url } => {
                needs_reload = true;
                Some(AppCommand::AddBookmark {
                    parent_folder_id: None,
                    title,
                    url,
                })
            }
            InternalAction::AddBookmarkFolder { title } => {
                needs_reload = true;
                Some(AppCommand::AddBookmarkFolder {
                    parent_folder_id: None,
                    title,
                })
            }
            InternalAction::RemoveBookmark { id } => {
                needs_reload = true;
                Some(AppCommand::RemoveBookmark { id })
            }
            InternalAction::PauseDownload(id) => {
                needs_reload = true;
                Some(AppCommand::PauseDownload { id })
            }
            InternalAction::ResumeDownload(id) => {
                needs_reload = true;
                Some(AppCommand::ResumeDownload { id })
            }
            InternalAction::CancelDownload(id) => {
                needs_reload = true;
                Some(AppCommand::CancelDownload { id })
            }
            InternalAction::RetryDownload(id) => {
                needs_reload = true;
                Some(AppCommand::RetryDownload { id })
            }
            InternalAction::ForgetPermissions => {
                needs_reload = true;
                Some(AppCommand::ForgetAllPermissions)
            }
            InternalAction::ToggleDuskPause => Some(AppCommand::ToggleDuskPausedEverywhere),
            InternalAction::ToggleSidebarFold => Some(AppCommand::ToggleSidebarFold),
        };

        if let Some(cmd) = cmd
            && let Ok(dirs) = self.controller.dispatch(cmd)
        {
            execute_and_pump(
                &mut self.controller,
                &mut self.bridge,
                engine,
                dirs,
                &self.window,
                false,
            );
            let snap = self.controller.ui_snapshot();
            let palette = ShellPalette::resolve(&snap.settings, detect_system_dark());
            self.update_internal_context(&snap, palette);
            if needs_reload {
                self.bridge.refresh_internal_tab(engine, core_tab);
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_action(&mut self, engine: &mut WebKitGtkEngine, action: ShellAction) {
        match action {
            ShellAction::Command(cmd) => {
                let is_submit = matches!(cmd, AppCommand::SubmitAddress { .. });
                let is_find_open = matches!(cmd, AppCommand::OpenFindBar);
                let is_update_settings = matches!(cmd, AppCommand::UpdateSettings { .. });
                let is_fold = matches!(cmd, AppCommand::ToggleSidebarFold);

                if let Ok(dirs) = self.controller.dispatch(cmd) {
                    if is_update_settings {
                        let dl = expand_downloads_dir(&self.controller.settings().downloads_dir);
                        let _ = engine.set_downloads_dir(dl);
                    }
                    execute_and_pump(
                        &mut self.controller,
                        &mut self.bridge,
                        engine,
                        dirs,
                        &self.window,
                        !self.overlays.holds_text_focus() && !self.sidebar.inline_entry_has_focus(),
                    );
                }

                let snap = self.controller.ui_snapshot();
                if is_submit {
                    if let Some(reason) = &snap.address.shake_reason {
                        self.overlays
                            .show_address_shake(reason, snap.settings.reduce_motion);
                    } else {
                        self.overlays.close_address_overlay();
                    }
                }
                if is_find_open {
                    self.overlays.focus_find_entry();
                }
                if is_fold && !snap.sidebar_folded {
                    self.folded_peek_open = false;
                }
            }
            ShellAction::OpenAddressOverlay {
                prefill,
                for_new_tab,
            } => {
                let _ = self.controller.dispatch(AppCommand::FocusAddressBar);
                let snap = self.controller.ui_snapshot();
                let text = prefill.unwrap_or(snap.address.editable_address);
                self.overlays.open_address_overlay(&text, for_new_tab);
            }
            ShellAction::CloseAddressOverlay => {
                let _ = self.controller.dispatch(AppCommand::DismissAddressShake);
                self.overlays.close_address_overlay();
            }
            ShellAction::ToggleSiteCard => {
                let snap = self.controller.ui_snapshot();
                let anchor = self.sidebar.site_chip_anchor();
                self.overlays.toggle_site_card(&anchor, &snap);
            }
            ShellAction::ToggleDownloadsPopover => {
                let snap = self.controller.ui_snapshot();
                let anchor = self.sidebar.downloads_anchor();
                self.overlays.toggle_downloads_popover(&anchor, &snap);
            }
            ShellAction::ToggleHideElementsPicker => {
                if let Some(aid) = self.controller.ui_snapshot().active_tab {
                    self.hide_picker_active = !self.hide_picker_active;
                    self.bridge
                        .set_hide_picker(engine, aid, self.hide_picker_active);
                }
            }
            ShellAction::OpenHiddenElementsDialog => {
                self.show_hidden_elements_dialog();
            }
            ShellAction::OpenSiteCssDialog => {
                self.show_site_css_dialog();
            }
            ShellAction::PrintActiveTab => {
                if let Some(aid) = self.controller.ui_snapshot().active_tab {
                    self.bridge.print_tab(engine, aid);
                }
            }
            ShellAction::OpenNewSpaceDialog => {
                self.show_new_space_dialog();
            }
            ShellAction::CreateAndSwitchSpace {
                name,
                color,
                store_policy,
            } => {
                if let Ok(dirs) = self.controller.dispatch(AppCommand::CreateSpace {
                    profile: None,
                    name,
                    color,
                    icon: "sparkles".into(),
                    store_policy,
                }) {
                    execute_and_pump(
                        &mut self.controller,
                        &mut self.bridge,
                        engine,
                        dirs,
                        &self.window,
                        false,
                    );
                    if let Some(last_sp) = self.controller.spaces().spaces().last().map(|s| s.id)
                        && let Ok(dirs2) = self
                            .controller
                            .dispatch(AppCommand::SwitchSpace { space: last_sp })
                    {
                        execute_and_pump(
                            &mut self.controller,
                            &mut self.bridge,
                            engine,
                            dirs2,
                            &self.window,
                            false,
                        );
                    }
                }
            }
            ShellAction::PromptRenameSpace(sid) => {
                self.show_rename_space_dialog(sid);
            }
            ShellAction::PromptRenameTab(tid) => {
                self.show_rename_tab_dialog(tid);
            }
            ShellAction::PromptGroupTab(tid) => {
                self.show_create_group_dialog(tid);
            }
            ShellAction::StartTabDrag { tab, x, y } => {
                self.dragging_tab = true;
                let _ = self.controller.dispatch(AppCommand::BeginDrag {
                    payload: searlx_core::DragPayload::Tab(tab),
                    x,
                    y,
                });
                self.stage.set_split_drop_bands_visible(true, None);
            }
            ShellAction::UpdateTabDrag { x, y, target } => {
                let side = match &target {
                    Some(DropTarget::PageSplitEdge { side, .. }) => Some(*side),
                    _ => None,
                };
                if self.dragging_tab {
                    self.stage.set_split_drop_bands_visible(true, side);
                }
                let _ = self
                    .controller
                    .dispatch(AppCommand::UpdateDrag { x, y, target });
            }
            ShellAction::FinishTabDrag => {
                self.dragging_tab = false;
                self.stage.set_split_drop_bands_visible(false, None);
                if let Ok(dirs) = self.controller.dispatch(AppCommand::CommitDrag) {
                    execute_and_pump(
                        &mut self.controller,
                        &mut self.bridge,
                        engine,
                        dirs,
                        &self.window,
                        false,
                    );
                }
            }
            ShellAction::CancelTabDrag => {
                self.dragging_tab = false;
                self.stage.set_split_drop_bands_visible(false, None);
                let _ = self.controller.dispatch(AppCommand::CancelDrag);
            }
            ShellAction::OpenUrlInCurrentTab(url) => {
                if let Some(aid) = self.controller.ui_snapshot().active_tab
                    && let Ok(dirs) = self
                        .controller
                        .dispatch(AppCommand::NavigateTab { tab: aid, url })
                {
                    execute_and_pump(
                        &mut self.controller,
                        &mut self.bridge,
                        engine,
                        dirs,
                        &self.window,
                        true,
                    );
                }
            }
            ShellAction::OpenInternal(kind, detail) => {
                if let Ok(dirs) = self
                    .controller
                    .dispatch(AppCommand::OpenInternalPage { kind, detail })
                {
                    execute_and_pump(
                        &mut self.controller,
                        &mut self.bridge,
                        engine,
                        dirs,
                        &self.window,
                        false,
                    );
                }
            }
            ShellAction::ToggleReaderMode => {
                if let Ok(dirs) = self
                    .controller
                    .dispatch(AppCommand::ToggleReaderMode { html: None })
                {
                    execute_and_pump(
                        &mut self.controller,
                        &mut self.bridge,
                        engine,
                        dirs,
                        &self.window,
                        false,
                    );
                }
            }
            ShellAction::SetCtrlHeld(held) => {
                self.ctrl_held = held;
            }
            ShellAction::HideFoldedPeek => {
                self.folded_peek_open = false;
            }
            ShellAction::RequestQuit => {
                if self.controller.settings().quit_confirm {
                    let dlg = gtk::MessageDialog::new(
                        Some(&self.window),
                        gtk::DialogFlags::MODAL,
                        gtk::MessageType::Question,
                        gtk::ButtonsType::YesNo,
                        "Quit searlx?",
                    );
                    let resp = dlg.run();
                    dlg.close();
                    if resp != gtk::ResponseType::Yes {
                        return;
                    }
                }
                let _ = self.controller.dispatch(AppCommand::SaveSession);
                gtk::main_quit();
            }
        }
    }

    fn show_new_space_dialog(&self) {
        let dlg = gtk::Dialog::with_buttons(
            Some("New Space"),
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            &[
                ("Cancel", gtk::ResponseType::Cancel),
                ("Create Space", gtk::ResponseType::Accept),
            ],
        );
        let box_c = dlg.content_area();
        box_c.set_spacing(8);
        box_c.set_margin_top(10);
        box_c.set_margin_bottom(10);
        box_c.set_margin_start(12);
        box_c.set_margin_end(12);

        let name_entry = gtk::Entry::new();
        name_entry.set_placeholder_text(Some("Space name (e.g. Work)"));
        let color_combo = gtk::ComboBoxText::new();
        for (id, label) in [
            ("blue", "Blue"),
            ("green", "Green"),
            ("orange", "Orange"),
            ("red", "Red"),
            ("violet", "Violet"),
            ("none", "Neutral"),
        ] {
            color_combo.append(Some(id), label);
        }
        color_combo.set_active_id(Some("blue"));
        let isolated_chk = gtk::CheckButton::with_label("Separate sign-ins & history (OwnStore)");

        box_c.pack_start(&name_entry, false, false, 0);
        box_c.pack_start(&color_combo, false, false, 0);
        box_c.pack_start(&isolated_chk, false, false, 0);
        dlg.show_all();

        let resp = dlg.run();
        let name = name_entry.text().to_string();
        let color = SpaceColor::parse(color_combo.active_id().as_deref().unwrap_or("blue"));
        let policy = if isolated_chk.is_active() {
            SpaceStorePolicy::OwnStore
        } else {
            SpaceStorePolicy::SharedWithFirst
        };
        dlg.close();
        if resp == gtk::ResponseType::Accept && !name.trim().is_empty() {
            (self.actions)(ShellAction::CreateAndSwitchSpace {
                name: name.trim().to_owned(),
                color,
                store_policy: policy,
            });
        }
    }

    fn show_rename_space_dialog(&self, sid: SpaceId) {
        let cur = self
            .controller
            .spaces()
            .space(sid)
            .map_or_else(String::new, |s| s.name.clone());
        if let Some(next) = run_single_text_dialog(&self.window, "Rename Space", "Space name", &cur)
        {
            (self.actions)(ShellAction::Command(AppCommand::RenameSpace {
                space: sid,
                name: next,
            }));
        }
    }

    fn show_rename_tab_dialog(&self, tid: TabId) {
        let cur = self
            .controller
            .find_tab(tid)
            .map_or_else(String::new, |(_, t)| t.display_title().to_owned());
        if let Some(next) = run_single_text_dialog(&self.window, "Rename Tab", "Tab title", &cur) {
            (self.actions)(ShellAction::Command(AppCommand::RenameTab {
                tab: tid,
                name: Some(next),
            }));
        }
    }

    fn show_create_group_dialog(&self, tid: TabId) {
        if let Some(name) =
            run_single_text_dialog(&self.window, "New Tab Group", "Group name", "Research")
        {
            (self.actions)(ShellAction::Command(AppCommand::CreateTabGroup {
                name,
                color: SpaceColor::Violet,
                tabs: vec![tid],
            }));
        }
    }

    fn show_hidden_elements_dialog(&self) {
        let snap = self.controller.ui_snapshot();
        let Some(tab) = snap
            .active_tab
            .and_then(|aid| snap.tabs.iter().find(|t| t.id == aid))
        else {
            return;
        };
        let Some(host) = tab.url.host() else {
            return;
        };
        let selectors = self
            .controller
            .hide_elements()
            .selectors_for_site(host, tab.private);
        let dlg = gtk::Dialog::with_buttons(
            Some(&format!("Hidden on {host}")),
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            &[
                ("Restore All", gtk::ResponseType::Apply),
                ("Close", gtk::ResponseType::Close),
            ],
        );
        dlg.set_default_size(440, 260);
        let content = dlg.content_area();
        content.set_spacing(6);
        content.set_margin_top(10);
        content.set_margin_bottom(10);
        content.set_margin_start(12);
        content.set_margin_end(12);

        if selectors.is_empty() {
            content.pack_start(
                &gtk::Label::new(Some("No hidden elements on this site.")),
                false,
                false,
                8,
            );
        } else {
            for sel in selectors {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let lbl = gtk::Label::new(Some(&sel));
                lbl.set_halign(gtk::Align::Start);
                lbl.set_hexpand(true);
                let btn = gtk::Button::with_label("Restore");
                let s = self.actions.clone();
                let sel_owned = sel.clone();
                let d_clone = dlg.clone();
                btn.connect_clicked(move |_| {
                    s(ShellAction::Command(AppCommand::RestoreHiddenElement {
                        selector: sel_owned.clone(),
                    }));
                    d_clone.close();
                });
                row.pack_start(&lbl, true, true, 0);
                row.pack_end(&btn, false, false, 0);
                content.pack_start(&row, false, false, 0);
            }
        }
        dlg.show_all();
        let resp = dlg.run();
        dlg.close();
        if resp == gtk::ResponseType::Apply {
            (self.actions)(ShellAction::Command(AppCommand::RestoreAllHiddenElements));
        }
    }

    fn show_site_css_dialog(&self) {
        let snap = self.controller.ui_snapshot();
        let Some(tab) = snap
            .active_tab
            .and_then(|aid| snap.tabs.iter().find(|t| t.id == aid))
        else {
            return;
        };
        if tab.private {
            return;
        }
        let Some(host) = tab.url.host() else {
            return;
        };
        let existing = self
            .controller
            .site_css()
            .active_css_for_site(host)
            .unwrap_or("")
            .to_owned();
        let dlg = gtk::Dialog::with_buttons(
            Some(&format!("Edit Site Style — {host}")),
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            &[
                ("Clear", gtk::ResponseType::Reject),
                ("Cancel", gtk::ResponseType::Cancel),
                ("Done", gtk::ResponseType::Accept),
            ],
        );
        dlg.set_default_size(420, 220);
        dlg.set_size_request(340, 140);
        let tv = gtk::TextView::new();
        if let Some(buf) = tv.buffer() {
            buf.set_text(&existing);
        }
        let sc = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        sc.set_vexpand(true);
        sc.add(&tv);
        dlg.content_area().pack_start(&sc, true, true, 6);
        dlg.show_all();

        let resp = dlg.run();
        let text = tv
            .buffer()
            .and_then(|b| b.text(&b.start_iter(), &b.end_iter(), false))
            .map(|g| g.to_string())
            .unwrap_or_default();
        dlg.close();

        if resp == gtk::ResponseType::Accept {
            (self.actions)(ShellAction::Command(AppCommand::UpdateSiteCss {
                site_host: None,
                css: text,
            }));
        } else if resp == gtk::ResponseType::Reject {
            (self.actions)(ShellAction::Command(AppCommand::UpdateSiteCss {
                site_host: None,
                css: String::new(),
            }));
        }
    }
}

fn drain_action_queue(
    core_rc: &Rc<RefCell<ShellCore>>,
    engine_cell: &Rc<RefCell<WebKitGtkEngine>>,
    queue: &Rc<RefCell<VecDeque<ShellAction>>>,
    in_drain: &Rc<Cell<bool>>,
) {
    in_drain.set(true);
    loop {
        let next = queue.borrow_mut().pop_front();
        let Some(action) = next else {
            break;
        };
        if let Ok(mut core) = core_rc.try_borrow_mut()
            && let Ok(mut eng) = engine_cell.try_borrow_mut()
        {
            core.handle_action(&mut eng, action);
            core.refresh_ui(&mut eng);
        } else {
            queue.borrow_mut().push_front(action);
            break;
        }
    }
    in_drain.set(false);
}

#[allow(clippy::too_many_lines)]
fn execute_and_pump(
    controller: &mut BrowserController,
    bridge: &mut EngineBridge,
    engine: &mut WebKitGtkEngine,
    mut pending: Vec<EngineDirective>,
    window: &gtk::Window,
    allow_webview_focus: bool,
) {
    let mut synthetic_events: Vec<CtrlEvent> = Vec::new();
    let mut external_dialogs: Vec<ExternalDialogRequest> = Vec::new();
    let mut clipboard_texts: Vec<String> = Vec::new();

    for _cycle in 0..16 {
        if pending.is_empty() && synthetic_events.is_empty() {
            break;
        }
        for directive in pending.drain(..) {
            bridge.execute_directive(
                engine,
                controller,
                directive,
                allow_webview_focus,
                &mut synthetic_events,
                &mut external_dialogs,
                &mut clipboard_texts,
            );
        }

        for text in clipboard_texts.drain(..) {
            let cb = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
            cb.set_text(&text);
        }
        for req in external_dialogs.drain(..) {
            match req {
                ExternalDialogRequest::Print { tab } => {
                    bridge.print_tab(engine, tab);
                }
                ExternalDialogRequest::FilePicker { multiple, .. } => {
                    let chooser = gtk::FileChooserDialog::with_buttons(
                        Some("Select File"),
                        Some(window),
                        gtk::FileChooserAction::Open,
                        &[
                            ("Cancel", gtk::ResponseType::Cancel),
                            ("Open", gtk::ResponseType::Accept),
                        ],
                    );
                    chooser.set_select_multiple(multiple);
                    let _ = chooser.run();
                    chooser.close();
                }
            }
        }

        for ev in synthetic_events.drain(..) {
            if let Ok(follow_up) = controller.handle_engine_event(ev) {
                pending.extend(follow_up);
            }
        }
    }
}

fn wire_split_edge_dnd(
    band: &gtk::Box,
    side: SplitDropSide,
    core_slot: &Rc<RefCell<Option<Rc<RefCell<ShellCore>>>>>,
    sink: &ActionSink,
) {
    let slot = core_slot.clone();
    let target_lookup = move || -> Option<DropTarget> {
        let core_rc = slot.borrow().clone()?;
        let core = core_rc.try_borrow().ok()?;
        let active_id = core.controller.ui_snapshot().active_tab?;
        Some(DropTarget::PageSplitEdge {
            target_tab: active_id,
            side,
        })
    };

    let entries = [gtk::TargetEntry::new(
        "application/x-searlx-tab",
        gtk::TargetFlags::SAME_APP,
        0,
    )];
    band.drag_dest_set(gtk::DestDefaults::ALL, &entries, gdk::DragAction::MOVE);

    let lookup_m = target_lookup.clone();
    let sink_m = sink.clone();
    band.connect_drag_motion(move |_w, _ctx, x, y, _time| {
        if let Some(target) = lookup_m() {
            sink_m(ShellAction::UpdateTabDrag {
                x: f64::from(x),
                y: f64::from(y),
                target: Some(target),
            });
        }
        true
    });

    let sink_d = sink.clone();
    band.connect_drag_drop(move |_w, _ctx, x, y, _time| {
        if let Some(target) = target_lookup() {
            sink_d(ShellAction::UpdateTabDrag {
                x: f64::from(x),
                y: f64::from(y),
                target: Some(target),
            });
            sink_d(ShellAction::FinishTabDrag);
        }
        true
    });
}

fn wire_folded_edge_peek(core_rc: &Rc<RefCell<ShellCore>>, sink: &ActionSink) {
    let dwell_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let grace_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let edge = core_rc.borrow().edge_zone.clone();
    let peek_box = core_rc.borrow().folded_peek_box.clone();

    {
        let core_weak = Rc::downgrade(core_rc);
        let dwell_slot = dwell_timer.clone();
        let grace_slot = grace_timer.clone();
        edge.connect_enter_notify_event(move |_, _| {
            if let Some(id) = grace_slot.borrow_mut().take() {
                id.remove();
            }
            if dwell_slot.borrow().is_some() {
                return glib::Propagation::Proceed;
            }
            let weak = core_weak.clone();
            let dwell_clear = dwell_slot.clone();
            let id =
                glib::timeout_add_local_once(Duration::from_millis(FOLDED_DWELL_MS), move || {
                    *dwell_clear.borrow_mut() = None;
                    if let Some(core_r) = weak.upgrade()
                        && let Ok(mut core) = core_r.try_borrow_mut()
                    {
                        core.folded_peek_open = true;
                        let snap = core.controller.ui_snapshot();
                        core.sync_outer_layout(&snap);
                    }
                });
            *dwell_slot.borrow_mut() = Some(id);
            glib::Propagation::Proceed
        });
    }

    {
        let dwell_slot = dwell_timer;
        edge.connect_leave_notify_event(move |_, _| {
            if let Some(id) = dwell_slot.borrow_mut().take() {
                id.remove();
            }
            glib::Propagation::Proceed
        });
    }

    {
        let grace_slot = grace_timer.clone();
        peek_box.connect_enter_notify_event(move |_, _| {
            if let Some(id) = grace_slot.borrow_mut().take() {
                id.remove();
            }
            glib::Propagation::Proceed
        });
    }

    {
        let grace_slot = grace_timer;
        let sink_leave = sink.clone();
        peek_box.connect_leave_notify_event(move |_, ev| {
            if ev.detail() == gdk::NotifyType::Inferior {
                return glib::Propagation::Proceed;
            }
            if grace_slot.borrow().is_some() {
                return glib::Propagation::Proceed;
            }
            let grace_clear = grace_slot.clone();
            let s = sink_leave.clone();
            let id =
                glib::timeout_add_local_once(Duration::from_millis(FOLDED_GRACE_MS), move || {
                    *grace_clear.borrow_mut() = None;
                    s(ShellAction::HideFoldedPeek);
                });
            *grace_slot.borrow_mut() = Some(id);
            glib::Propagation::Proceed
        });
    }

    let _ = wire_drop_target;
}

fn wire_window_signals(window: &gtk::Window, core_rc: &Rc<RefCell<ShellCore>>, sink: &ActionSink) {
    let sink_del = sink.clone();
    window.connect_delete_event(move |_, _| {
        sink_del(ShellAction::RequestQuit);
        glib::Propagation::Stop
    });

    {
        let weak = Rc::downgrade(core_rc);
        let sink_key = sink.clone();
        window.connect_key_press_event(move |_, ev| {
            let key = ev.keyval();
            let state = ev.state();
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            let alt = state.contains(gdk::ModifierType::MOD1_MASK);

            if matches!(
                key,
                gdk::keys::constants::Control_L | gdk::keys::constants::Control_R
            ) && let Some(core_r) = weak.upgrade()
                && let Ok(core) = core_r.try_borrow()
                && !core.ctrl_held
            {
                let sink_ctrl = sink_key.clone();
                glib::idle_add_local_once(move || {
                    sink_ctrl(ShellAction::SetCtrlHeld(true));
                });
            }

            let Some(core_r) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let Ok(core) = core_r.try_borrow() else {
                return glib::Propagation::Proceed;
            };

            let lower = key.to_lower();

            if key == gdk::keys::constants::Escape {
                if core.overlays.is_address_open() {
                    drop(core);
                    sink_key(ShellAction::CloseAddressOverlay);
                    return glib::Propagation::Stop;
                }
                if core.hide_picker_active {
                    drop(core);
                    sink_key(ShellAction::ToggleHideElementsPicker);
                    return glib::Propagation::Stop;
                }
                let snap = core.controller.ui_snapshot();
                if snap.find.is_some() {
                    drop(core);
                    sink_key(ShellAction::Command(AppCommand::CloseFindBar));
                    return glib::Propagation::Stop;
                }
                if snap.peek.is_some() {
                    drop(core);
                    sink_key(ShellAction::Command(AppCommand::DismissPeek));
                    return glib::Propagation::Stop;
                }
                if snap.tab_switcher.is_some() {
                    drop(core);
                    sink_key(ShellAction::Command(AppCommand::SwitcherCancel));
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            }

            if !ctrl {
                return glib::Propagation::Proceed;
            }

            // `Ctrl+Tab` / `Ctrl+Shift+Tab`
            if key == gdk::keys::constants::Tab || key == gdk::keys::constants::ISO_Left_Tab {
                drop(core);
                sink_key(ShellAction::Command(AppCommand::SwitcherStep {
                    backwards: shift,
                    elapsed_ms: 200,
                }));
                return glib::Propagation::Stop;
            }

            // `Ctrl+1..9`
            if let Some(ch) = lower.to_unicode()
                && ('1'..='9').contains(&ch)
                && !shift
                && !alt
            {
                let idx = (ch as usize) - ('1' as usize);
                let snap = core.controller.ui_snapshot();
                if snap.settings.spaces_command_numbers {
                    if let Some(sp) = snap.spaces.get(idx) {
                        let sid = sp.id;
                        drop(core);
                        sink_key(ShellAction::Command(AppCommand::SwitchSpace { space: sid }));
                        return glib::Propagation::Stop;
                    }
                } else if let Some(tab) = snap.tabs.get(idx) {
                    let tid = tab.id;
                    drop(core);
                    sink_key(ShellAction::Command(AppCommand::ActivateTab { tab: tid }));
                    return glib::Propagation::Stop;
                }
            }

            let action = match (lower, shift, alt) {
                (k, false, false) if k == gdk::keys::constants::l => {
                    Some(ShellAction::OpenAddressOverlay {
                        prefill: None,
                        for_new_tab: false,
                    })
                }
                (k, false, false) if k == gdk::keys::constants::t => {
                    if core.controller.settings().newtab_floating {
                        Some(ShellAction::OpenAddressOverlay {
                            prefill: Some(String::new()),
                            for_new_tab: true,
                        })
                    } else {
                        Some(ShellAction::Command(AppCommand::NewTab {
                            url: None,
                            activate: true,
                        }))
                    }
                }
                (k, true, false) if k == gdk::keys::constants::t => {
                    Some(ShellAction::Command(AppCommand::ReopenClosedTab))
                }
                (k, true, false) if k == gdk::keys::constants::n => {
                    Some(ShellAction::Command(AppCommand::NewPrivateTab {
                        url: None,
                        activate: true,
                    }))
                }
                (k, false, true) if k == gdk::keys::constants::n => {
                    Some(ShellAction::Command(AppCommand::ToggleSplitView {
                        target_tab: None,
                        partner_tab: None,
                    }))
                }
                (k, false, false) if k == gdk::keys::constants::w => {
                    Some(ShellAction::Command(AppCommand::CloseTab { tab: None }))
                }
                (k, false, false) if k == gdk::keys::constants::s => {
                    Some(ShellAction::Command(AppCommand::ToggleSidebarFold))
                }
                (k, false, false) if k == gdk::keys::constants::f => {
                    Some(ShellAction::Command(AppCommand::OpenFindBar))
                }
                (k, false, false) if k == gdk::keys::constants::g => {
                    Some(ShellAction::Command(AppCommand::FindNext))
                }
                (k, true, false) if k == gdk::keys::constants::g => {
                    Some(ShellAction::Command(AppCommand::FindPrevious))
                }
                (k, false, false) if k == gdk::keys::constants::r => {
                    Some(ShellAction::Command(AppCommand::Reload {
                        tab: None,
                        from_origin: false,
                    }))
                }
                (k, false, true) if k == gdk::keys::constants::r => {
                    Some(ShellAction::Command(AppCommand::Reload {
                        tab: None,
                        from_origin: true,
                    }))
                }
                (k, true, false) if k == gdk::keys::constants::r => {
                    Some(ShellAction::ToggleReaderMode)
                }
                (k, false, false) if k == gdk::keys::constants::d => {
                    Some(ShellAction::Command(AppCommand::DuplicateTab { tab: None }))
                }
                (k, true, false) if k == gdk::keys::constants::d => {
                    Some(ShellAction::Command(AppCommand::ToggleDuskForSite))
                }
                (k, true, true) if k == gdk::keys::constants::d => {
                    Some(ShellAction::Command(AppCommand::ToggleDuskPausedEverywhere))
                }
                (k, true, false) if k == gdk::keys::constants::h => {
                    Some(ShellAction::ToggleHideElementsPicker)
                }
                (k, true, false) if k == gdk::keys::constants::u => {
                    Some(ShellAction::OpenHiddenElementsDialog)
                }
                (k, true, false) if k == gdk::keys::constants::b => {
                    Some(ShellAction::Command(AppCommand::ToggleBookmark {
                        parent_folder_id: None,
                        title: None,
                    }))
                }
                (k, true, false) if k == gdk::keys::constants::c => {
                    Some(ShellAction::Command(AppCommand::CopyAddress { tab: None }))
                }
                (k, false, false) if k == gdk::keys::constants::y => {
                    Some(ShellAction::OpenInternal(InternalPageKind::History, None))
                }
                (k, true, false) if k == gdk::keys::constants::j => {
                    Some(ShellAction::OpenInternal(InternalPageKind::Downloads, None))
                }
                (k, false, false) if k == gdk::keys::constants::comma => {
                    Some(ShellAction::OpenInternal(InternalPageKind::Settings, None))
                }
                (k, false, false) if k == gdk::keys::constants::q => Some(ShellAction::RequestQuit),
                (k, false, false) if k == gdk::keys::constants::bracketleft => {
                    Some(ShellAction::Command(AppCommand::GoBack { tab: None }))
                }
                (k, false, false) if k == gdk::keys::constants::bracketright => {
                    Some(ShellAction::Command(AppCommand::GoForward { tab: None }))
                }
                (k, _, false)
                    if k == gdk::keys::constants::equal
                        || k == gdk::keys::constants::plus
                        || k == gdk::keys::constants::KP_Add =>
                {
                    Some(ShellAction::Command(AppCommand::ZoomStep {
                        delta_steps: 1,
                    }))
                }
                (k, false, false)
                    if k == gdk::keys::constants::minus
                        || k == gdk::keys::constants::KP_Subtract =>
                {
                    Some(ShellAction::Command(AppCommand::ZoomStep {
                        delta_steps: -1,
                    }))
                }
                (k, false, false) if k == gdk::keys::constants::_0 => {
                    Some(ShellAction::Command(AppCommand::ZoomReset))
                }
                (k, false, true) if k == gdk::keys::constants::Left => {
                    Some(ShellAction::Command(AppCommand::MoveSplitFocus {
                        delta: -1,
                    }))
                }
                (k, false, true) if k == gdk::keys::constants::Right => {
                    Some(ShellAction::Command(AppCommand::MoveSplitFocus {
                        delta: 1,
                    }))
                }
                (k, false, false) if k == gdk::keys::constants::z => {
                    if core.hide_picker_active {
                        Some(ShellAction::Command(AppCommand::UndoHideElement))
                    } else if core.controller.settings().tabs_undo_reopens
                        && !core.overlays.holds_text_focus()
                        && !core.sidebar.inline_entry_has_focus()
                    {
                        Some(ShellAction::Command(AppCommand::ReopenClosedTab))
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(act) = action {
                drop(core);
                sink_key(act);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }

    {
        let weak = Rc::downgrade(core_rc);
        let sink_rel = sink.clone();
        window.connect_key_release_event(move |_, ev| {
            let key = ev.keyval();
            if matches!(
                key,
                gdk::keys::constants::Control_L | gdk::keys::constants::Control_R
            ) && let Some(core_r) = weak.upgrade()
                && let Ok(core) = core_r.try_borrow()
            {
                let snap = core.controller.ui_snapshot();
                if snap.tab_switcher.is_some() {
                    let s = sink_rel.clone();
                    glib::idle_add_local_once(move || {
                        s(ShellAction::SetCtrlHeld(false));
                        s(ShellAction::Command(AppCommand::SwitcherRelease));
                    });
                    return glib::Propagation::Proceed;
                }
                if core.ctrl_held {
                    let s = sink_rel.clone();
                    glib::idle_add_local_once(move || {
                        s(ShellAction::SetCtrlHeld(false));
                    });
                }
            }
            glib::Propagation::Proceed
        });
    }
}

fn run_single_text_dialog(
    parent: &gtk::Window,
    title: &str,
    placeholder: &str,
    initial: &str,
) -> Option<String> {
    let dlg = gtk::Dialog::with_buttons(
        Some(title),
        Some(parent),
        gtk::DialogFlags::MODAL,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("OK", gtk::ResponseType::Accept),
        ],
    );
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some(placeholder));
    entry.set_text(initial);
    entry.set_activates_default(true);
    dlg.set_default_response(gtk::ResponseType::Accept);
    let content = dlg.content_area();
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.pack_start(&entry, false, false, 0);
    dlg.show_all();
    let resp = dlg.run();
    let text = entry.text().to_string();
    dlg.close();
    if resp == gtk::ResponseType::Accept && !text.trim().is_empty() {
        Some(text.trim().to_owned())
    } else {
        None
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1_700_000_000, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use searlx_core::{EngineCapabilities, LookPreference, Settings};

    #[test]
    fn browser_controller_restores_asleep_and_handles_internal_settings_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");

        // 1. Seed a session with 2 tabs, save it, then reopen with RestoreMode::Normal.
        {
            let (mut ctrl, _) = BrowserController::open(
                dir.path(),
                EngineCapabilities::baseline("webkitgtk"),
                RestoreMode::FreshStart,
            )
            .expect("open fresh");
            ctrl.dispatch(AppCommand::NewTab {
                url: Some(PageUrl::parse("https://example.com/one").expect("u1")),
                activate: true,
            })
            .expect("tab1");
            ctrl.dispatch(AppCommand::NewTab {
                url: Some(PageUrl::parse("https://example.com/two").expect("u2")),
                activate: false,
            })
            .expect("tab2");
            ctrl.dispatch(AppCommand::SaveSession).expect("save");
        }

        let (mut restored, init_dirs) = BrowserController::open(
            dir.path(),
            EngineCapabilities::baseline("webkitgtk"),
            RestoreMode::Normal,
        )
        .expect("restore");
        let snap = restored.ui_snapshot();
        assert_eq!(snap.tabs.len(), 3);
        // Active tab materializes; all inactive restored tabs stay asleep (`engine_tab_id: None`).
        assert!(
            init_dirs
                .iter()
                .any(|d| matches!(d, EngineDirective::CreateTab { .. }))
        );
        let inactive: Vec<_> = snap.tabs.iter().filter(|t| !t.active).collect();
        assert_eq!(inactive.len(), 2);
        for t in inactive {
            assert!(t.sleeping);
            assert!(t.engine_tab_id.is_none());
        }

        // 2. Apply setting mutation from `search://settings` IPC bridge -> persisted Settings.
        let mut next = restored.settings().clone();
        assert!(internal_pages::apply_setting_mutation(
            &mut next, "look", "dark"
        ));
        assert!(internal_pages::apply_setting_mutation(
            &mut next,
            "address_shown",
            "true"
        ));
        restored
            .dispatch(AppCommand::UpdateSettings {
                settings: Box::new(next),
            })
            .expect("update settings");

        let reloaded = Settings::load_from(restored.store()).expect("load settings");
        assert_eq!(reloaded.look, LookPreference::Dark);
        assert!(reloaded.address_shown);
    }
}
