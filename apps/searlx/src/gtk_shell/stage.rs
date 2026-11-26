//! Content stage: rounded page card (`8 pt` gap, `10 pt` radius), 2–4 pane Split View,
//! optional Bookmarks bar, resizable Side Panel, and offscreen WebView parking.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use gtk::prelude::*;
use searlx_core::sync_panel_launcher_routes::SidePanelContent;
use searlx_core::{AppCommand, BookmarkItem, DownloadState, TabId, TabLayoutMode, UiSnapshot};
use searlx_engine_linux::WebKitGtkEngine;

use super::actions::{ActionSink, ShellAction};
use super::engine_bridge::EngineBridge;
use super::theme::{SIDE_PANEL_MAX, SIDE_PANEL_MIN, reveal_widget};

#[derive(Clone, Debug, PartialEq)]
struct StageSignature {
    panes: Vec<(TabId, Option<usize>, bool, bool)>,
    show_bookmarks: bool,
    bookmarks_count: usize,
    side_panel_kind: Option<(String, bool, bool)>,
    side_panel_left: bool,
}

/// Manages the center page card, split panes, side panel, and bookmarks bar.
pub struct StageSurface {
    pub root_box: gtk::Box,
    pub bookmarks_bar: gtk::Box,
    pub stage_area: gtk::Box,
    pub page_card_frame: gtk::Box,
    pub page_overlay: gtk::Overlay,
    panes_box: gtk::Box,
    side_panel_box: gtk::Box,
    pub split_drop_left: gtk::Box,
    pub split_drop_right: gtk::Box,
    parking_offscreen: gtk::OffscreenWindow,
    parking_box: gtk::Box,
    last_signature: RefCell<Option<StageSignature>>,
    last_stage_width: Cell<i32>,
    connected_pane_clicks: RefCell<BTreeSet<usize>>,
    suppress_paned_notify: Cell<bool>,
    actions: ActionSink,
}

impl StageSurface {
    /// Construct the stage surface and its offscreen WebView parking container.
    #[must_use]
    pub fn new(actions: ActionSink) -> Self {
        let root_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root_box.set_hexpand(true);
        root_box.set_vexpand(true);

        let bookmarks_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        bookmarks_bar
            .style_context()
            .add_class("searlx-bookmarks-bar");
        bookmarks_bar.set_no_show_all(true);

        let stage_area = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        stage_area.set_hexpand(true);
        stage_area.set_vexpand(true);

        let page_card_frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page_card_frame
            .style_context()
            .add_class("searlx-page-card");
        page_card_frame.set_hexpand(true);
        page_card_frame.set_vexpand(true);

        let page_overlay = gtk::Overlay::new();
        page_overlay.set_hexpand(true);
        page_overlay.set_vexpand(true);

        let panes_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        panes_box.set_hexpand(true);
        panes_box.set_vexpand(true);
        page_overlay.add(&panes_box);

        // Split drop edge preview overlays (`max(120 pt, 24%)`) shown only during drag-to-split.
        let split_drop_left = make_split_drop_band("Split Left", gtk::Align::Start);
        let split_drop_right = make_split_drop_band("Split Right", gtk::Align::End);
        page_overlay.add_overlay(&split_drop_left);
        page_overlay.add_overlay(&split_drop_right);
        page_overlay.set_overlay_pass_through(&split_drop_left, true);
        page_overlay.set_overlay_pass_through(&split_drop_right, true);

        page_card_frame.pack_start(&page_overlay, true, true, 0);

        let side_panel_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        side_panel_box
            .style_context()
            .add_class("searlx-side-panel");
        side_panel_box.set_no_show_all(true);

        stage_area.pack_start(&page_card_frame, true, true, 0);

        root_box.pack_start(&bookmarks_bar, false, false, 0);
        root_box.pack_start(&stage_area, true, true, 0);

        let parking_offscreen = gtk::OffscreenWindow::new();
        parking_offscreen.set_default_size(1180, 780);
        let parking_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        parking_offscreen.add(&parking_box);
        parking_offscreen.show_all();

        let last_stage_width = Rc::new(Cell::new(0_i32));
        let last_stage_width_cb = last_stage_width.clone();
        let actions_alloc = actions.clone();
        panes_box.connect_size_allocate(move |_w, alloc| {
            let w = alloc.width();
            if w > 100 && (w - last_stage_width_cb.get()).abs() >= 4 {
                last_stage_width_cb.set(w);
                actions_alloc(ShellAction::Command(AppCommand::SetViewportStageWidth {
                    width_pt: f64::from(w),
                }));
            }
        });

        Self {
            root_box,
            bookmarks_bar,
            stage_area,
            page_card_frame,
            page_overlay,
            panes_box,
            side_panel_box,
            split_drop_left,
            split_drop_right,
            parking_offscreen,
            parking_box,
            last_signature: RefCell::new(None),
            last_stage_width: Cell::new(0),
            connected_pane_clicks: RefCell::new(BTreeSet::new()),
            suppress_paned_notify: Cell::new(false),
            actions,
        }
    }

    /// Return current stage allocation for drag-to-split coordinate checks.
    #[must_use]
    #[allow(dead_code)]
    pub fn page_allocation(&self) -> gtk::Allocation {
        self.page_overlay.allocation()
    }

    /// Show or hide the split-edge drop preview bands (and make them interactive drop targets while dragging).
    pub fn set_split_drop_bands_visible(
        &self,
        visible: bool,
        active_side: Option<searlx_core::SplitDropSide>,
    ) {
        let alloc = self.page_overlay.allocation();
        let band_w = ((f64::from(alloc.width()) * 0.24).max(120.0)).round() as i32;
        self.split_drop_left.set_size_request(band_w, -1);
        self.split_drop_right.set_size_request(band_w, -1);
        if visible {
            self.page_overlay
                .set_overlay_pass_through(&self.split_drop_left, false);
            self.page_overlay
                .set_overlay_pass_through(&self.split_drop_right, false);
            self.split_drop_left.set_opacity(
                if active_side == Some(searlx_core::SplitDropSide::Left) {
                    0.95
                } else {
                    0.30
                },
            );
            self.split_drop_right.set_opacity(
                if active_side == Some(searlx_core::SplitDropSide::Right) {
                    0.95
                } else {
                    0.30
                },
            );
            reveal_widget(&self.split_drop_left);
            reveal_widget(&self.split_drop_right);
        } else {
            self.page_overlay
                .set_overlay_pass_through(&self.split_drop_left, true);
            self.page_overlay
                .set_overlay_pass_through(&self.split_drop_right, true);
            self.split_drop_left.hide();
            self.split_drop_right.hide();
        }
    }

    /// Park all live WebViews currently inside `panes_box` or `side_panel_box` cleanly
    /// before destroying wrapper widgets.
    pub fn park_widget(&self, widget: &gtk::Widget) {
        reparent_widget(
            widget,
            self.parking_box.upcast_ref::<gtk::Container>(),
            false,
        );
        widget.hide();
    }

    /// Synchronize visible stage panes, bookmarks bar, and side panel to `snapshot`.
    pub fn sync(&self, snapshot: &UiSnapshot, engine: &WebKitGtkEngine, bridge: &EngineBridge) {
        let _ = &self.parking_offscreen;
        let _ = self.last_stage_width.get();

        // Compute visible panes list (`split_panes` if active, else `active_tab`).
        let pane_entries: Vec<(TabId, String, bool)> = if !snapshot.split_panes.is_empty() {
            snapshot
                .split_panes
                .iter()
                .map(|p| {
                    let label = p.url.host().map_or_else(
                        || p.title.clone(),
                        |h| h.trim_start_matches("www.").to_owned(),
                    );
                    (p.tab, label, p.focused)
                })
                .collect()
        } else if let Some(aid) = snapshot.active_tab {
            let label = snapshot
                .tabs
                .iter()
                .find(|t| t.id == aid)
                .map_or_else(String::new, |t| t.title.clone());
            vec![(aid, label, true)]
        } else {
            Vec::new()
        };

        let pane_widgets: Vec<(TabId, String, bool, Option<gtk::Widget>)> = pane_entries
            .into_iter()
            .map(|(tid, label, focused)| {
                let w = bridge.widget_for_core_tab(engine, tid);
                (tid, label, focused, w)
            })
            .collect();

        let sp_visible = snapshot.side_panel.open && !snapshot.side_panel.held_back_for_private;
        let side_panel_left = snapshot.sidebar_layout == TabLayoutMode::RightColumn;
        let sp_sig = if sp_visible {
            let desc = match &snapshot.side_panel.content {
                Some(SidePanelContent::Website { url, space_id, .. }) => {
                    format!("web:{}:{}", space_id, url.identity())
                }
                Some(SidePanelContent::Downloads) => "downloads".to_owned(),
                Some(SidePanelContent::Extension { extension_id, .. }) => {
                    format!("ext:{extension_id}")
                }
                None => "none".to_owned(),
            };
            let sp_has_widget = bridge.side_panel_widget(engine).is_some();
            Some((desc, sp_has_widget, side_panel_left))
        } else {
            None
        };

        let sig = StageSignature {
            panes: pane_widgets
                .iter()
                .map(|(tid, _, focused, w)| {
                    (
                        *tid,
                        w.as_ref().map(|wid| wid.as_ptr() as usize),
                        *focused,
                        pane_widgets_len_gt_1(&pane_widgets),
                    )
                })
                .collect(),
            show_bookmarks: snapshot.settings.bookmarks_bar,
            bookmarks_count: snapshot.bookmarks_bar.len(),
            side_panel_kind: sp_sig,
            side_panel_left,
        };

        let needs_rebuild = self.last_signature.borrow().as_ref() != Some(&sig);
        if !needs_rebuild {
            // Still refresh dynamic Downloads side panel rows if open.
            if sp_visible
                && matches!(
                    snapshot.side_panel.content,
                    Some(SidePanelContent::Downloads)
                )
            {
                self.refresh_downloads_side_panel(snapshot);
            }
            return;
        }
        *self.last_signature.borrow_mut() = Some(sig);

        // 1. Sync Bookmarks Bar (`settings.bookmarks_bar`).
        self.sync_bookmarks_bar(snapshot);

        // 2. Park all existing live WebViews safely before clearing containers.
        for (_, _, _, w_opt) in &pane_widgets {
            if let Some(w) = w_opt {
                self.park_widget(w);
            }
        }
        if let Some(sp_w) = bridge.side_panel_widget(engine) {
            self.park_widget(&sp_w);
        }
        clear_non_webview_children(&self.panes_box, &self.parking_box);

        // 3. Build single pane or 2..=4 pane Split View layout.
        if pane_widgets.len() == 1 {
            if let Some((tid, _label, _focused, Some(w))) = pane_widgets.into_iter().next() {
                self.connect_pane_click(&w, tid);
                reparent_widget(&w, self.panes_box.upcast_ref::<gtk::Container>(), true);
            } else {
                let placeholder = gtk::Label::new(Some("Loading…"));
                placeholder.set_hexpand(true);
                placeholder.set_vexpand(true);
                self.panes_box.pack_start(&placeholder, true, true, 0);
                placeholder.show();
            }
        } else if pane_widgets.len() >= 2 {
            self.build_split_panes(snapshot, pane_widgets);
        }

        // 4. Sync Side Panel (`300..=640 pt`, opposite side from sidebar).
        self.sync_side_panel(snapshot, engine, bridge, sp_visible, side_panel_left);
    }

    fn sync_bookmarks_bar(&self, snapshot: &UiSnapshot) {
        for child in self.bookmarks_bar.children() {
            self.bookmarks_bar.remove(&child);
        }
        if !snapshot.settings.bookmarks_bar {
            self.bookmarks_bar.hide();
            return;
        }
        if snapshot.bookmarks_bar.is_empty() {
            let empty = gtk::Label::new(Some("Bookmarks bar (Shift+Ctrl+B to add current page)"));
            empty.style_context().add_class("searlx-section-label");
            self.bookmarks_bar.pack_start(&empty, false, false, 4);
        } else {
            for item in &snapshot.bookmarks_bar {
                match item {
                    BookmarkItem::Link { title, url, .. } => {
                        let btn = gtk::Button::with_label(title);
                        btn.style_context().add_class("searlx-bookmark-chip");
                        btn.set_tooltip_text(Some(&url.display()));
                        let u = url.clone();
                        let sink = self.actions.clone();
                        btn.connect_clicked(move |_| {
                            sink(ShellAction::OpenUrlInCurrentTab(u.clone()));
                        });
                        self.bookmarks_bar.pack_start(&btn, false, false, 0);
                    }
                    BookmarkItem::Folder {
                        title, children, ..
                    } => {
                        let mb = gtk::MenuButton::new();
                        mb.set_label(&format!("{title} ▾"));
                        mb.style_context().add_class("searlx-bookmark-chip");
                        let menu = gtk::Menu::new();
                        populate_bookmark_submenu(&menu, children, &self.actions);
                        menu.show_all();
                        mb.set_popup(Some(&menu));
                        self.bookmarks_bar.pack_start(&mb, false, false, 0);
                    }
                }
            }
        }
        reveal_widget(&self.bookmarks_bar);
    }

    fn build_split_panes(
        &self,
        snapshot: &UiSnapshot,
        pane_widgets: Vec<(TabId, String, bool, Option<gtk::Widget>)>,
    ) {
        let count = pane_widgets.len();
        let pane_boxes: Vec<gtk::Widget> = pane_widgets
            .into_iter()
            .map(|(tid, title, focused, widget_opt)| {
                let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
                outer.style_context().add_class("searlx-split-pane");
                if focused {
                    outer.style_context().add_class("focused");
                }
                outer.set_hexpand(true);
                outer.set_vexpand(true);

                let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                header.style_context().add_class("searlx-split-pane-bar");

                let focus_btn = gtk::Button::with_label(&title);
                focus_btn.style_context().add_class("searlx-nav-btn");
                focus_btn.set_hexpand(true);
                focus_btn.set_halign(gtk::Align::Start);
                let sink = self.actions.clone();
                focus_btn.connect_clicked(move |_| {
                    sink(ShellAction::Command(AppCommand::ActivateTab { tab: tid }));
                });
                header.pack_start(&focus_btn, true, true, 0);

                let swap_btn = gtk::Button::with_label("⇄");
                swap_btn.style_context().add_class("searlx-tab-close");
                swap_btn.set_tooltip_text(Some("Swap split panes"));
                let sink_swap = self.actions.clone();
                swap_btn.connect_clicked(move |_| {
                    sink_swap(ShellAction::Command(AppCommand::SwapSplitPanes));
                });
                header.pack_end(&swap_btn, false, false, 0);

                let close_btn = gtk::Button::with_label("×");
                close_btn.style_context().add_class("searlx-tab-close");
                close_btn.set_tooltip_text(Some("Remove from Split View"));
                let sink_rm = self.actions.clone();
                close_btn.connect_clicked(move |_| {
                    sink_rm(ShellAction::Command(AppCommand::RemoveSplitPane {
                        tab: tid,
                    }));
                });
                header.pack_end(&close_btn, false, false, 0);

                outer.pack_start(&header, false, false, 0);
                if let Some(w) = widget_opt {
                    self.connect_pane_click(&w, tid);
                    reparent_widget(&w, outer.upcast_ref::<gtk::Container>(), true);
                } else {
                    let lbl = gtk::Label::new(Some("Loading split pane…"));
                    lbl.set_hexpand(true);
                    lbl.set_vexpand(true);
                    outer.pack_start(&lbl, true, true, 0);
                }
                outer.show_all();
                outer.upcast::<gtk::Widget>()
            })
            .collect();

        let stage_width = self.panes_box.allocation().width().max(800);
        let root_split = build_nested_paned(
            &pane_boxes,
            &snapshot.split_panes,
            stage_width,
            count,
            &self.actions,
        );
        self.panes_box.pack_start(&root_split, true, true, 0);
        root_split.show();
    }

    fn sync_side_panel(
        &self,
        snapshot: &UiSnapshot,
        engine: &WebKitGtkEngine,
        bridge: &EngineBridge,
        sp_visible: bool,
        side_panel_left: bool,
    ) {
        // Detach page_card_frame and side_panel_box from stage_area without destroying page_card_frame.
        for child in self.stage_area.children() {
            if let Ok(paned) = child.clone().downcast::<gtk::Paned>() {
                for pchild in paned.children() {
                    paned.remove(&pchild);
                }
            }
            self.stage_area.remove(&child);
        }

        if !sp_visible {
            self.side_panel_box.hide();
            self.stage_area
                .pack_start(&self.page_card_frame, true, true, 0);
            self.page_card_frame.show();
            return;
        }

        for child in self.side_panel_box.children() {
            if is_webview_child(&child, bridge.side_panel_widget(engine).as_ref()) {
                self.park_widget(&child);
            } else {
                self.side_panel_box.remove(&child);
            }
        }

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.style_context().add_class("searlx-side-panel-header");

        let title_text = match &snapshot.side_panel.content {
            Some(SidePanelContent::Website { url, .. }) => {
                if url.is_insecure() {
                    format!("{} · Not secure", url.display())
                } else {
                    url.host()
                        .unwrap_or("Side Panel")
                        .trim_start_matches("www.")
                        .to_owned()
                }
            }
            Some(SidePanelContent::Downloads) => "Downloads".to_owned(),
            Some(SidePanelContent::Extension { extension_id, .. }) => extension_id.clone(),
            None => "Side Panel".to_owned(),
        };

        let title_lbl = gtk::Label::new(Some(&title_text));
        title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_lbl.set_halign(gtk::Align::Start);
        header.pack_start(&title_lbl, true, true, 0);

        if let Some(SidePanelContent::Website { url, .. }) = &snapshot.side_panel.content {
            let open_tab = gtk::Button::with_label("↗");
            open_tab.style_context().add_class("searlx-nav-btn");
            open_tab.set_tooltip_text(Some("Open in Tab"));
            let u = url.clone();
            let sink = self.actions.clone();
            open_tab.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::NewTab {
                    url: Some(u.clone()),
                    activate: true,
                }));
            });
            header.pack_end(&open_tab, false, false, 0);
        }

        let close_btn = gtk::Button::with_label("×");
        close_btn.style_context().add_class("searlx-nav-btn");
        close_btn.set_tooltip_text(Some("Close Side Panel"));
        let sink_close = self.actions.clone();
        close_btn.connect_clicked(move |_| {
            sink_close(ShellAction::Command(AppCommand::CloseSidePanel));
        });
        header.pack_end(&close_btn, false, false, 0);

        self.side_panel_box.pack_start(&header, false, false, 0);

        match &snapshot.side_panel.content {
            Some(SidePanelContent::Website { .. }) => {
                if let Some(sp_w) = bridge.side_panel_widget(engine) {
                    reparent_widget(
                        &sp_w,
                        self.side_panel_box.upcast_ref::<gtk::Container>(),
                        true,
                    );
                } else {
                    let lbl = gtk::Label::new(Some("Loading companion site…"));
                    lbl.set_vexpand(true);
                    self.side_panel_box.pack_start(&lbl, true, true, 0);
                }
            }
            Some(SidePanelContent::Downloads) => {
                let dl_scroll =
                    gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
                dl_scroll.set_vexpand(true);
                let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
                list.set_margin_top(8);
                list.set_margin_bottom(8);
                list.set_margin_start(8);
                list.set_margin_end(8);
                populate_compact_downloads(&list, snapshot, &self.actions);
                dl_scroll.add(&list);
                self.side_panel_box.pack_start(&dl_scroll, true, true, 0);
            }
            _ => {}
        }

        reveal_widget(&self.side_panel_box);
        self.side_panel_box.set_size_request(SIDE_PANEL_MIN, -1);

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_hexpand(true);
        paned.set_vexpand(true);
        let target_w = snapshot
            .side_panel
            .width_pt
            .round()
            .clamp(f64::from(SIDE_PANEL_MIN), f64::from(SIDE_PANEL_MAX))
            as i32;
        let total_w = self.stage_area.allocation().width().max(900);

        self.suppress_paned_notify.set(true);
        if side_panel_left {
            paned.pack1(&self.side_panel_box, false, true);
            paned.pack2(&self.page_card_frame, true, true);
            paned.set_position(target_w);
        } else {
            paned.pack1(&self.page_card_frame, true, true);
            paned.pack2(&self.side_panel_box, false, true);
            paned.set_position((total_w - target_w).max(280));
        }
        self.suppress_paned_notify.set(false);

        let sink_resize = self.actions.clone();
        let stage_area_ref = self.stage_area.clone();
        paned.connect_button_release_event(move |p, ev| {
            if ev.event_type() == gtk::gdk::EventType::DoubleButtonPress {
                sink_resize(ShellAction::Command(AppCommand::ResetSidePanelWidth));
                return glib::Propagation::Proceed;
            }
            let alloc_w = stage_area_ref.allocation().width();
            if alloc_w > 400 {
                let width = if side_panel_left {
                    p.position()
                } else {
                    alloc_w - p.position()
                };
                sink_resize(ShellAction::Command(AppCommand::SetSidePanelWidth {
                    width_pt: f64::from(width),
                }));
            }
            glib::Propagation::Proceed
        });

        self.stage_area.pack_start(&paned, true, true, 0);
        paned.show();
    }

    fn refresh_downloads_side_panel(&self, snapshot: &UiSnapshot) {
        let children = self.side_panel_box.children();
        if let Some(scroll) = children
            .get(1)
            .and_then(|w| w.downcast_ref::<gtk::ScrolledWindow>())
            && let Some(vp) = scroll.child().and_then(|c| c.downcast::<gtk::Bin>().ok())
            && let Some(list) = vp.child().and_then(|c| c.downcast::<gtk::Box>().ok())
        {
            for row in list.children() {
                list.remove(&row);
            }
            populate_compact_downloads(&list, snapshot, &self.actions);
            list.show_all();
        }
    }

    fn connect_pane_click(&self, widget: &gtk::Widget, tab_id: TabId) {
        let ptr = widget.as_ptr() as usize;
        if !self.connected_pane_clicks.borrow_mut().insert(ptr) {
            return;
        }
        let sink = self.actions.clone();
        widget.connect_button_press_event(move |_, _| {
            sink(ShellAction::Command(AppCommand::ActivateTab {
                tab: tab_id,
            }));
            glib::Propagation::Proceed
        });
    }
}

fn pane_widgets_len_gt_1(items: &[(TabId, String, bool, Option<gtk::Widget>)]) -> bool {
    items.len() > 1
}

fn build_nested_paned(
    widgets: &[gtk::Widget],
    panes: &[searlx_core::UiSplitPaneItem],
    total_width: i32,
    total_count: usize,
    actions: &ActionSink,
) -> gtk::Widget {
    if widgets.len() == 1 {
        return widgets[0].clone();
    }
    if widgets.len() == 2 {
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.style_context().add_class("searlx-split-paned");
        paned.set_hexpand(true);
        paned.set_vexpand(true);
        paned.pack1(&widgets[0], true, true);
        paned.pack2(&widgets[1], true, true);
        let first_w = panes
            .first()
            .map_or(total_width / 2, |p| (p.width.round() as i32).max(160));
        paned.set_position(first_w);
        if total_count == 2 {
            let sink = actions.clone();
            let p_clone = paned.clone();
            paned.connect_button_release_event(move |_, ev| {
                let stage_w = f64::from(p_clone.allocation().width().max(520));
                if ev.event_type() == gtk::gdk::EventType::DoubleButtonPress {
                    sink(ShellAction::Command(AppCommand::EvenSplitPanes));
                } else {
                    sink(ShellAction::Command(AppCommand::ResizeSplit {
                        stage_width_pt: stage_w,
                        pointer_x: f64::from(p_clone.position()),
                    }));
                }
                glib::Propagation::Proceed
            });
        }
        return paned.upcast::<gtk::Widget>();
    }

    let tail = build_nested_paned(
        &widgets[1..],
        &panes[1..],
        total_width,
        total_count,
        actions,
    );
    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.style_context().add_class("searlx-split-paned");
    paned.set_hexpand(true);
    paned.set_vexpand(true);
    paned.pack1(&widgets[0], true, true);
    paned.pack2(&tail, true, true);
    let first_w = panes
        .first()
        .map_or(total_width / (widgets.len() as i32), |p| {
            (p.width.round() as i32).max(160)
        });
    paned.set_position(first_w);
    paned.upcast::<gtk::Widget>()
}

fn make_split_drop_band(title: &str, halign: gtk::Align) -> gtk::Box {
    let band = gtk::Box::new(gtk::Orientation::Vertical, 0);
    band.style_context().add_class("searlx-split-drop-band");
    band.set_halign(halign);
    band.set_valign(gtk::Align::Fill);
    band.set_no_show_all(true);
    let lbl = gtk::Label::new(Some(title));
    lbl.style_context().add_class("searlx-split-drop-label");
    lbl.set_valign(gtk::Align::Center);
    lbl.set_vexpand(true);
    band.pack_start(&lbl, true, true, 0);
    band
}

fn populate_bookmark_submenu(menu: &gtk::Menu, items: &[BookmarkItem], actions: &ActionSink) {
    for item in items {
        match item {
            BookmarkItem::Link { title, url, .. } => {
                let mi = gtk::MenuItem::with_label(title);
                let u = url.clone();
                let sink = actions.clone();
                mi.connect_activate(move |_| {
                    sink(ShellAction::OpenUrlInCurrentTab(u.clone()));
                });
                menu.append(&mi);
            }
            BookmarkItem::Folder {
                title, children, ..
            } => {
                let mi = gtk::MenuItem::with_label(title);
                let sub = gtk::Menu::new();
                populate_bookmark_submenu(&sub, children, actions);
                mi.set_submenu(Some(&sub));
                menu.append(&mi);
            }
        }
    }
}

fn populate_compact_downloads(list: &gtk::Box, snapshot: &UiSnapshot, actions: &ActionSink) {
    if snapshot.downloads.entries.is_empty() {
        let lbl = gtk::Label::new(Some("No downloads yet."));
        lbl.style_context().add_class("searlx-section-label");
        list.pack_start(&lbl, false, false, 8);
        return;
    }
    for entry in &snapshot.downloads.entries {
        let row = gtk::Box::new(gtk::Orientation::Vertical, 3);
        row.style_context().add_class("searlx-tab-row");
        let name = gtk::Label::new(Some(&entry.file_name));
        name.set_halign(gtk::Align::Start);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.pack_start(&name, false, false, 0);

        let status = match &entry.state {
            DownloadState::InProgress => format!("{} bytes…", entry.bytes_received),
            DownloadState::Paused => "Paused".to_owned(),
            DownloadState::Completed => "Completed".to_owned(),
            DownloadState::Failed { reason } => format!("Failed: {reason}"),
            DownloadState::Cancelled => "Cancelled".to_owned(),
        };
        let sub = gtk::Label::new(Some(&status));
        sub.style_context().add_class("searlx-section-label");
        sub.set_halign(gtk::Align::Start);
        row.pack_start(&sub, false, false, 0);

        let btns = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        if entry.can_pause() {
            let b = gtk::Button::with_label("Pause");
            b.style_context().add_class("searlx-site-card-btn");
            let id = entry.id.clone();
            let sink = actions.clone();
            b.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::PauseDownload {
                    id: id.clone(),
                }));
            });
            btns.pack_start(&b, false, false, 0);
        }
        if matches!(entry.state, DownloadState::Paused) {
            let b = gtk::Button::with_label("Resume");
            b.style_context().add_class("searlx-site-card-btn");
            let id = entry.id.clone();
            let sink = actions.clone();
            b.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::ResumeDownload {
                    id: id.clone(),
                }));
            });
            btns.pack_start(&b, false, false, 0);
        }
        if matches!(
            entry.state,
            DownloadState::InProgress | DownloadState::Paused
        ) {
            let b = gtk::Button::with_label("Cancel");
            b.style_context().add_class("searlx-site-card-btn");
            let id = entry.id.clone();
            let sink = actions.clone();
            b.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::CancelDownload {
                    id: id.clone(),
                }));
            });
            btns.pack_start(&b, false, false, 0);
        }
        row.pack_start(&btns, false, false, 0);
        list.pack_start(&row, false, false, 2);
    }
}

fn clear_non_webview_children(container: &gtk::Box, parking_box: &gtk::Box) {
    for child in container.children() {
        park_nested_webviews(&child, parking_box);
        container.remove(&child);
    }
}

fn park_nested_webviews(widget: &gtk::Widget, parking_box: &gtk::Box) {
    if widget.type_().name().contains("WebView") {
        reparent_widget(widget, parking_box.upcast_ref::<gtk::Container>(), false);
        widget.hide();
        return;
    }
    if let Ok(container) = widget.clone().downcast::<gtk::Container>() {
        for child in container.children() {
            park_nested_webviews(&child, parking_box);
        }
    }
}

fn is_webview_child(widget: &gtk::Widget, target: Option<&gtk::Widget>) -> bool {
    target.is_some_and(|t| t.as_ptr() == widget.as_ptr())
        || widget.type_().name().contains("WebView")
}

/// Safely move `widget` into `target` without dropping its GObject refcount.
pub fn reparent_widget(widget: &gtk::Widget, target: &gtk::Container, show: bool) {
    if let Some(current_parent) = widget.parent() {
        if current_parent.as_ptr() == target.upcast_ref::<gtk::Widget>().as_ptr() {
            if show {
                widget.show();
            }
            return;
        }
        if let Ok(container) = current_parent.downcast::<gtk::Container>() {
            container.remove(widget);
        }
    }
    widget.set_size_request(-1, -1);
    if let Ok(box_target) = target.clone().downcast::<gtk::Box>() {
        box_target.pack_start(widget, true, true, 0);
    } else {
        target.add(widget);
    }
    if show {
        widget.show();
    }
}
