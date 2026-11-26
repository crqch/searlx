//! Search sidebar & top-tabs chrome (`FEATURES.md` §1-§3, §6-§8):
//! - Left / Right resizable column (`176..=440 pt`, default `232 pt`), Top tabs, Icons-only (`48 pt`), Folded (`Ctrl+S` + edge-peek).
//! - No permanent toolbar: quiet nav buttons, clickable site chip (opens Site Card), optional inline address line.
//! - Pinned grid (up to 4 squares/row) or pinned rows/folders, collapsible Tab Groups, loose tabs, split indicators, audio/mute, sleeping dimmed rows, hover close/put-down.
//! - Unified drag-to-reorder & drag-to-split (`BeginDrag`/`UpdateDrag`/`CommitDrag`/`CancelDrag`), plus right-click context menus.

use std::cell::{Cell, RefCell};

use gtk::gdk;
use gtk::prelude::*;
use searlx_core::{
    AddressBarWidth, AppCommand, DropTarget, InternalPageKind, PinFolderNode, SpaceId,
    SubmitDisposition, TabEntry, TabGroupItem, TabId, TabLayoutMode, TabModel, UiSnapshot,
    UiTabItem, build_tab_context_menu, compute_pin_grid_rows,
};

use super::actions::{ActionSink, ShellAction};
use super::theme::{
    ADDRESS_WIDTH_MEDIUM, ADDRESS_WIDTH_SMALL, SIDEBAR_ICONS, reveal_widget, space_badge_hex,
};

const DND_TARGET_TAB: &str = "application/x-searlx-tab";

#[derive(Clone, Debug, PartialEq)]
struct TabRowSig {
    id: TabId,
    title: String,
    display_url: String,
    letter: char,
    pinned: bool,
    group: Option<searlx_core::GroupId>,
    put_away: bool,
    loading: bool,
    audible: bool,
    muted: bool,
    sleeping: bool,
    private: bool,
    active: bool,
    in_split: bool,
    split_focused: bool,
    reading_pct: u32,
    crashed: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct SidebarSignature {
    layout: TabLayoutMode,
    folded: bool,
    dark: bool,
    ctrl_held: bool,
    active_space: SpaceId,
    active_tab: Option<TabId>,
    spaces: Vec<(SpaceId, String, searlx_core::SpaceColor, bool, usize)>,
    tabs: Vec<TabRowSig>,
    groups: Vec<(
        searlx_core::GroupId,
        String,
        searlx_core::SpaceColor,
        bool,
        usize,
    )>,
    pin_folders_len: usize,
    downloads_show: bool,
    downloads_active: usize,
    address_shown: bool,
    address_width: AddressBarWidth,
    toolbar_left: bool,
    doors_up: bool,
    player_on: bool,
    pins_list: bool,
    pins_folders: bool,
    sleep_dim: bool,
    reading_bar: bool,
    tabs_shortcuts: bool,
    tabs_address: bool,
    is_lookalike: bool,
    safe_host: String,
    active_url_display: String,
    active_insecure: bool,
}

/// Manages the vertical sidebar / icons-only column and the horizontal top tab strip.
pub struct TabSidebar {
    /// Vertical column container (used in LeftColumn, RightColumn, IconsOnly, and Folded peek).
    pub column_box: gtk::Box,
    /// Horizontal top strip container (used in TopTabs).
    pub top_strip_box: gtk::Box,

    column_header: gtk::Box,
    column_inline_entry: gtk::Entry,
    column_site_chip: gtk::Button,
    column_site_chip_label: gtk::Label,
    column_items: gtk::Box,
    column_player_box: gtk::Box,
    column_footer: gtk::Box,
    column_downloads_btn: gtk::Button,

    top_nav_box: gtk::Box,
    top_tabs_box: gtk::Box,
    top_site_chip: gtk::Button,
    top_site_chip_label: gtk::Label,
    top_inline_entry: gtk::Entry,
    top_doors_box: gtk::Box,
    top_downloads_btn: gtk::Button,

    last_sig: RefCell<Option<SidebarSignature>>,
    syncing_entry: Cell<bool>,
    current_active_layout: Cell<TabLayoutMode>,
    actions: ActionSink,
}

impl TabSidebar {
    /// Build sidebar and top-strip chrome widgets.
    #[must_use]
    pub fn new(actions: ActionSink) -> Self {
        let column_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column_box.style_context().add_class("searlx-sidebar");
        column_box.set_vexpand(true);

        let column_header = gtk::Box::new(gtk::Orientation::Horizontal, 2);

        let column_inline_entry = gtk::Entry::new();
        column_inline_entry
            .style_context()
            .add_class("searlx-inline-address");
        column_inline_entry.set_placeholder_text(Some("Address or search"));
        column_inline_entry.set_no_show_all(true);

        let column_site_chip = gtk::Button::new();
        column_site_chip
            .style_context()
            .add_class("searlx-site-chip");
        let column_site_chip_label = gtk::Label::new(Some("New Tab"));
        column_site_chip_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        column_site_chip_label.set_halign(gtk::Align::Start);
        column_site_chip.add(&column_site_chip_label);
        column_site_chip.set_tooltip_text(Some("Site Information & Permissions (Site Card)"));
        {
            let sink = actions.clone();
            column_site_chip.connect_clicked(move |_| {
                sink(ShellAction::ToggleSiteCard);
            });
        }

        let column_scroll =
            gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        column_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        column_scroll.set_vexpand(true);
        column_scroll.show();

        let column_items = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column_items.show();
        column_scroll.add(&column_items);

        let column_player_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        column_player_box.set_no_show_all(true);

        let column_footer = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let column_downloads_btn = gtk::Button::with_label("↓");
        column_downloads_btn
            .style_context()
            .add_class("searlx-nav-btn");
        column_downloads_btn.set_tooltip_text(Some("Downloads"));
        column_downloads_btn.set_no_show_all(true);
        {
            let sink = actions.clone();
            column_downloads_btn.connect_clicked(move |_| {
                sink(ShellAction::ToggleDownloadsPopover);
            });
        }

        column_box.pack_start(&column_header, false, false, 0);
        column_box.pack_start(&column_inline_entry, false, false, 0);
        column_box.pack_start(&column_site_chip, false, false, 0);
        column_box.pack_start(&column_scroll, true, true, 0);
        column_box.pack_start(&column_player_box, false, false, 0);
        column_box.pack_start(&column_footer, false, false, 0);

        // Top horizontal tab strip (`TabLayoutMode::TopTabs`)
        let top_strip_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top_strip_box.style_context().add_class("searlx-topstrip");
        top_strip_box.set_hexpand(true);
        top_strip_box.set_no_show_all(true);

        let top_nav_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);

        let top_inline_entry = gtk::Entry::new();
        top_inline_entry
            .style_context()
            .add_class("searlx-inline-address");
        top_inline_entry.set_placeholder_text(Some("Address or search"));
        top_inline_entry.set_no_show_all(true);

        let top_site_chip = gtk::Button::new();
        top_site_chip.style_context().add_class("searlx-site-chip");
        let top_site_chip_label = gtk::Label::new(Some("New Tab"));
        top_site_chip_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        top_site_chip.add(&top_site_chip_label);
        {
            let sink = actions.clone();
            top_site_chip.connect_clicked(move |_| {
                sink(ShellAction::ToggleSiteCard);
            });
        }

        let top_tabs_scroll =
            gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        top_tabs_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
        top_tabs_scroll.set_hexpand(true);
        top_tabs_scroll.show();
        let top_tabs_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        top_tabs_box.show();
        top_tabs_scroll.add(&top_tabs_box);

        let top_doors_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let top_downloads_btn = gtk::Button::with_label("↓");
        top_downloads_btn
            .style_context()
            .add_class("searlx-nav-btn");
        top_downloads_btn.set_tooltip_text(Some("Downloads"));
        top_downloads_btn.set_no_show_all(true);
        {
            let sink = actions.clone();
            top_downloads_btn.connect_clicked(move |_| {
                sink(ShellAction::ToggleDownloadsPopover);
            });
        }

        top_strip_box.pack_start(&top_nav_box, false, false, 0);
        top_strip_box.pack_start(&top_inline_entry, false, false, 0);
        top_strip_box.pack_start(&top_site_chip, false, false, 0);
        top_strip_box.pack_start(&top_tabs_scroll, true, true, 0);
        top_strip_box.pack_start(&top_doors_box, false, false, 0);

        let sidebar = Self {
            column_box,
            top_strip_box,
            column_header,
            column_inline_entry,
            column_site_chip,
            column_site_chip_label,
            column_items,
            column_player_box,
            column_footer,
            column_downloads_btn,
            top_nav_box,
            top_tabs_box,
            top_site_chip,
            top_site_chip_label,
            top_inline_entry,
            top_doors_box,
            top_downloads_btn,
            last_sig: RefCell::new(None),
            syncing_entry: Cell::new(false),
            current_active_layout: Cell::new(TabLayoutMode::LeftColumn),
            actions,
        };

        sidebar.wire_inline_entry(&sidebar.column_inline_entry);
        sidebar.wire_inline_entry(&sidebar.top_inline_entry);
        sidebar
    }

    /// Widget anchor used when opening the Site Card popover.
    #[must_use]
    pub fn site_chip_anchor(&self) -> gtk::Widget {
        if self.current_active_layout.get() == TabLayoutMode::TopTabs {
            self.top_site_chip.clone().upcast::<gtk::Widget>()
        } else {
            self.column_site_chip.clone().upcast::<gtk::Widget>()
        }
    }

    /// Widget anchor used when opening the Downloads popover.
    #[must_use]
    pub fn downloads_anchor(&self) -> gtk::Widget {
        if self.current_active_layout.get() == TabLayoutMode::TopTabs {
            self.top_downloads_btn.clone().upcast::<gtk::Widget>()
        } else {
            self.column_downloads_btn.clone().upcast::<gtk::Widget>()
        }
    }

    /// Whether the inline address line (`settings.address_shown`) currently holds focus.
    #[must_use]
    pub fn inline_entry_has_focus(&self) -> bool {
        self.column_inline_entry.has_focus() || self.top_inline_entry.has_focus()
    }

    fn wire_inline_entry(&self, entry: &gtk::Entry) {
        let sink = self.actions.clone();
        entry.connect_activate(move |e| {
            let text = e.text().to_string();
            sink(ShellAction::Command(AppCommand::SubmitAddress {
                input: text,
                disposition: SubmitDisposition::CurrentTab,
                shift_pressed: false,
            }));
        });
    }

    /// Synchronize sidebar/top-tabs widgets to `snapshot`.
    #[allow(clippy::too_many_lines)]
    pub fn sync(&self, snapshot: &UiSnapshot, dark: bool, ctrl_held: bool) {
        self.current_active_layout.set(snapshot.sidebar_layout);

        let active_tab_entry = snapshot
            .active_tab
            .and_then(|aid| snapshot.tabs.iter().find(|t| t.id == aid));
        let active_insecure = active_tab_entry.is_some_and(|t| t.url.is_insecure());

        let sig = SidebarSignature {
            layout: snapshot.sidebar_layout,
            folded: snapshot.sidebar_folded,
            dark,
            ctrl_held,
            active_space: snapshot.active_space,
            active_tab: snapshot.active_tab,
            spaces: snapshot
                .spaces
                .iter()
                .map(|s| (s.id, s.name.clone(), s.color, s.active, s.tab_count))
                .collect(),
            tabs: snapshot
                .tabs
                .iter()
                .map(|t| TabRowSig {
                    id: t.id,
                    title: t.title.clone(),
                    display_url: t.display_url.clone(),
                    letter: t.letter,
                    pinned: t.pinned,
                    group: t.group,
                    put_away: t.put_away,
                    loading: t.loading,
                    audible: t.audible,
                    muted: t.muted,
                    sleeping: t.sleeping,
                    private: t.private,
                    active: t.active,
                    in_split: t.in_split,
                    split_focused: t.split_focused,
                    reading_pct: (t.reading_progress * 100.0).round() as u32,
                    crashed: t.crash_retry_message.is_some(),
                })
                .collect(),
            groups: snapshot
                .groups
                .iter()
                .map(|g| (g.id, g.name.clone(), g.color, g.collapsed, g.tabs.len()))
                .collect(),
            pin_folders_len: snapshot.pin_folders.len(),
            downloads_show: snapshot.downloads.show_button,
            downloads_active: snapshot.downloads.active_count,
            address_shown: snapshot.settings.address_shown,
            address_width: snapshot.settings.address_width,
            toolbar_left: snapshot.settings.toolbar_left,
            doors_up: snapshot.settings.sidebar_doors_up,
            player_on: snapshot.settings.sidebar_player,
            pins_list: snapshot.settings.pins_list,
            pins_folders: snapshot.settings.pins_folders,
            sleep_dim: snapshot.settings.tabs_sleep_dim,
            reading_bar: snapshot.settings.tabs_reading,
            tabs_shortcuts: snapshot.settings.tabs_shortcuts,
            tabs_address: snapshot.settings.tabs_address,
            is_lookalike: snapshot.address.is_lookalike,
            safe_host: snapshot.address.safe_host.clone(),
            active_url_display: snapshot.address.current_url_display.clone(),
            active_insecure,
        };

        // Always update inline address line text if unfocused.
        if snapshot.settings.address_shown && !self.inline_entry_has_focus() {
            self.syncing_entry.set(true);
            let txt = &snapshot.address.editable_address;
            if self.column_inline_entry.text().as_str() != txt {
                self.column_inline_entry.set_text(txt);
            }
            if self.top_inline_entry.text().as_str() != txt {
                self.top_inline_entry.set_text(txt);
            }
            self.syncing_entry.set(false);
        }

        if self.last_sig.borrow().as_ref() == Some(&sig) {
            return;
        }
        *self.last_sig.borrow_mut() = Some(sig);

        let icons_only = snapshot.sidebar_layout == TabLayoutMode::IconsOnly;
        if icons_only {
            self.column_box.set_size_request(SIDEBAR_ICONS, -1);
        } else {
            self.column_box.set_size_request(176, -1);
        }

        // Update site chip text & warning style.
        let chip_label = format_site_chip_label(snapshot, active_insecure, icons_only);
        self.column_site_chip_label.set_text(&chip_label);
        self.top_site_chip_label.set_text(&chip_label);
        for chip in [&self.column_site_chip, &self.top_site_chip] {
            if active_insecure || snapshot.address.is_lookalike {
                chip.style_context().add_class("insecure");
            } else {
                chip.style_context().remove_class("insecure");
            }
        }

        // Address line visibility & width (`FEATURES.md` §2.1).
        let addr_w = match snapshot.settings.address_width {
            AddressBarWidth::Small => ADDRESS_WIDTH_SMALL,
            AddressBarWidth::Medium => ADDRESS_WIDTH_MEDIUM,
        };
        if snapshot.settings.address_shown && !icons_only {
            self.column_inline_entry.set_width_request(addr_w.min(210));
            reveal_widget(&self.column_inline_entry);
            self.top_inline_entry.set_width_request(addr_w);
            reveal_widget(&self.top_inline_entry);
        } else {
            self.column_inline_entry.hide();
            self.top_inline_entry.hide();
        }

        if snapshot.sidebar_layout == TabLayoutMode::TopTabs {
            self.column_box.hide();
            reveal_widget(&self.top_strip_box);
            self.rebuild_top_strip(snapshot, dark, ctrl_held);
        } else {
            self.top_strip_box.hide();
            self.rebuild_column(snapshot, dark, ctrl_held, icons_only);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn rebuild_column(&self, snapshot: &UiSnapshot, dark: bool, ctrl_held: bool, icons_only: bool) {
        clear_box(&self.column_header);
        clear_box(&self.column_items);
        clear_box(&self.column_player_box);
        clear_box(&self.column_footer);

        // 1. Header navigation controls (`<`, `>`, `↻`/`✕`, fold, `+`, optional doors up).
        let active_tab = snapshot
            .active_tab
            .and_then(|id| snapshot.tabs.iter().find(|t| t.id == id));
        let nav_cluster = build_nav_cluster(active_tab, &self.actions, icons_only);
        let fold_btn = make_icon_btn(
            if snapshot.sidebar_folded {
                "▸"
            } else {
                "◧"
            },
            "Fold / Unfold Sidebar (Ctrl+S)",
            &self.actions,
            ShellAction::Command(AppCommand::ToggleSidebarFold),
        );
        let new_tab_btn = make_icon_btn(
            "+",
            if snapshot.settings.newtab_floating {
                "New Tab (Floating Field, Ctrl+T)"
            } else {
                "New Tab (Ctrl+T)"
            },
            &self.actions,
            if snapshot.settings.newtab_floating {
                ShellAction::OpenAddressOverlay {
                    prefill: Some(String::new()),
                    for_new_tab: true,
                }
            } else {
                ShellAction::Command(AppCommand::NewTab {
                    url: None,
                    activate: true,
                })
            },
        );

        if icons_only {
            self.column_header.pack_start(&new_tab_btn, true, true, 0);
            self.column_site_chip.hide();
        } else {
            self.column_site_chip.show_all();
            if snapshot.settings.toolbar_left {
                self.column_header.pack_start(&nav_cluster, false, false, 0);
                let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                self.column_header.pack_start(&spacer, true, true, 0);
                self.column_header.pack_end(&new_tab_btn, false, false, 0);
                self.column_header.pack_end(&fold_btn, false, false, 0);
            } else {
                self.column_header.pack_start(&fold_btn, false, false, 0);
                self.column_header.pack_start(&nav_cluster, false, false, 0);
                let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                self.column_header.pack_start(&spacer, true, true, 0);
                self.column_header.pack_end(&new_tab_btn, false, false, 0);
            }
            if snapshot.settings.sidebar_doors_up {
                let doors = self.build_doors_row(snapshot, true);
                self.column_header.pack_end(&doors, false, false, 2);
            }
        }
        self.column_header.show_all();

        // 2. Pinned tabs section (`Pinned grid` or `Pinned rows + folders`).
        let pinned: Vec<&UiTabItem> = snapshot.tabs.iter().filter(|t| t.pinned).collect();
        if !pinned.is_empty() {
            if icons_only || !snapshot.settings.pins_list {
                self.build_pin_grid(&pinned, snapshot, icons_only);
            } else {
                self.build_pin_rows(&pinned, &snapshot.pin_folders, snapshot, dark);
            }
        }

        // 3. Collapsible Tab Groups (`UiSnapshot.groups`).
        for group in &snapshot.groups {
            self.build_tab_group_section(group, snapshot, dark, icons_only, ctrl_held);
        }

        // 4. Loose tabs (non-pinned, ungrouped).
        let loose: Vec<(usize, &UiTabItem)> = snapshot
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.pinned && t.group.is_none())
            .collect();
        for (idx, (row_index, tab)) in loose.iter().enumerate() {
            let shortcut_num = (idx < 9).then_some(idx + 1);
            let row = self.build_tab_row(
                tab,
                snapshot,
                *row_index,
                shortcut_num,
                icons_only,
                ctrl_held,
                false,
                dark,
            );
            self.column_items.pack_start(&row, false, false, 0);
        }

        self.column_items.show_all();

        // 5. "What's playing" mini-player (`settings.sidebar_player`).
        if snapshot.settings.sidebar_player
            && !icons_only
            && let Some(playing) = snapshot.tabs.iter().find(|t| t.audible && !t.active)
        {
            self.column_player_box
                .style_context()
                .add_class("searlx-tab-row");
            let go_btn = gtk::Button::with_label(&format!("♪ {}", playing.title));
            go_btn.style_context().add_class("searlx-nav-btn");
            go_btn.set_hexpand(true);
            go_btn.set_halign(gtk::Align::Start);
            let tid = playing.id;
            let sink = self.actions.clone();
            go_btn.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::ActivateTab { tab: tid }));
            });
            let mute_btn = gtk::Button::with_label(if playing.muted { "🔇" } else { "🔊" });
            mute_btn.style_context().add_class("searlx-tab-close");
            let sink_m = self.actions.clone();
            mute_btn.connect_clicked(move |_| {
                sink_m(ShellAction::Command(AppCommand::ToggleMute {
                    tab: Some(tid),
                }));
            });
            self.column_player_box.pack_start(&go_btn, true, true, 0);
            self.column_player_box.pack_end(&mute_btn, false, false, 0);
            reveal_widget(&self.column_player_box);
        } else {
            self.column_player_box.hide();
        }

        // 6. Footer: Space switcher + Doors (unless doors_up).
        if !icons_only {
            let space_row = self.build_space_switcher(snapshot, dark);
            self.column_footer.pack_start(&space_row, false, false, 0);
        }
        if !snapshot.settings.sidebar_doors_up || icons_only {
            let doors = self.build_doors_row(snapshot, icons_only);
            self.column_footer.pack_start(&doors, false, false, 0);
        }
        self.column_footer.show_all();
        if !snapshot.downloads.show_button {
            self.column_downloads_btn.hide();
        }
    }

    fn rebuild_top_strip(&self, snapshot: &UiSnapshot, dark: bool, ctrl_held: bool) {
        clear_box(&self.top_nav_box);
        clear_box(&self.top_tabs_box);
        clear_box(&self.top_doors_box);

        let active_tab = snapshot
            .active_tab
            .and_then(|id| snapshot.tabs.iter().find(|t| t.id == id));
        let nav = build_nav_cluster(active_tab, &self.actions, false);
        self.top_nav_box.pack_start(&nav, false, false, 0);
        self.top_nav_box.show_all();

        for (idx, tab) in snapshot.tabs.iter().enumerate() {
            let shortcut_num = (idx < 9).then_some(idx + 1);
            let pill = self.build_top_tab_pill(tab, snapshot, idx, shortcut_num, ctrl_held, dark);
            self.top_tabs_box.pack_start(&pill, false, false, 0);
        }
        let plus = make_icon_btn(
            "+",
            "New Tab (Ctrl+T)",
            &self.actions,
            ShellAction::Command(AppCommand::NewTab {
                url: None,
                activate: true,
            }),
        );
        self.top_tabs_box.pack_start(&plus, false, false, 0);
        self.top_tabs_box.show_all();

        let fold_btn = make_icon_btn(
            "◧",
            "Toggle Sidebar Layout / Fold (Ctrl+S)",
            &self.actions,
            ShellAction::Command(AppCommand::ToggleSidebarFold),
        );
        self.top_doors_box.pack_start(&fold_btn, false, false, 0);
        let doors = self.build_doors_row(snapshot, true);
        self.top_doors_box.pack_start(&doors, false, false, 0);
        self.top_doors_box.show_all();
    }

    fn build_pin_grid(&self, pinned: &[&UiTabItem], snapshot: &UiSnapshot, icons_only: bool) {
        let grid_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let row_counts = if icons_only {
            vec![1; pinned.len()]
        } else {
            compute_pin_grid_rows(pinned.len())
        };

        let mut offset = 0_usize;
        for count in row_counts {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            row.set_homogeneous(!icons_only);
            for slot_idx in 0..count {
                if let Some(tab) = pinned.get(offset + slot_idx) {
                    let tile = self.build_pin_tile(tab, snapshot, offset + slot_idx);
                    row.pack_start(&tile, !icons_only, !icons_only, 0);
                }
            }
            offset += count;
            grid_box.pack_start(&row, false, false, 0);
        }

        // Register pin section drop target so loose tabs dragged onto pins become pinned!
        wire_drop_target(
            &grid_box.clone().upcast::<gtk::Widget>(),
            DropTarget::PinSlot(pinned.len()),
            &self.actions,
        );
        self.column_items.pack_start(&grid_box, false, false, 2);
    }

    fn build_pin_tile(
        &self,
        tab: &UiTabItem,
        snapshot: &UiSnapshot,
        pin_index: usize,
    ) -> gtk::EventBox {
        let ev = gtk::EventBox::new();
        let tile = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        tile.style_context().add_class("searlx-pin-tile");
        if tab.active {
            tile.style_context().add_class("active");
        }
        if tab.sleeping {
            tile.style_context().add_class("sleeping");
        }

        let letter_lbl = gtk::Label::new(Some(&tab.letter.to_string()));
        letter_lbl.set_hexpand(true);
        letter_lbl.set_halign(gtk::Align::Center);
        tile.pack_start(&letter_lbl, true, true, 0);

        if tab.audible || tab.muted {
            let spk = gtk::Label::new(Some(if tab.muted { "🔇" } else { "🔊" }));
            tile.pack_end(&spk, false, false, 1);
        }

        ev.add(&tile);
        ev.set_tooltip_text(Some(&format!("{} — {}", tab.title, tab.display_url)));

        let tid = tab.id;
        let is_active = tab.active;
        let snapshot_tabs = snapshot.tabs.clone();
        let spaces = snapshot.spaces.clone();
        let active_space = snapshot.active_space;
        let sink = self.actions.clone();
        ev.connect_button_press_event(move |widget, event| match event.button() {
            1 => {
                if is_active {
                    sink(ShellAction::ToggleSiteCard);
                } else {
                    sink(ShellAction::Command(AppCommand::ActivateTab { tab: tid }));
                }
                glib::Propagation::Proceed
            }
            2 => {
                sink(ShellAction::Command(AppCommand::CloseTab {
                    tab: Some(tid),
                }));
                glib::Propagation::Stop
            }
            3 => {
                show_tab_context_menu(
                    widget.upcast_ref::<gtk::Widget>(),
                    event,
                    tid,
                    &snapshot_tabs,
                    &spaces,
                    active_space,
                    &sink,
                );
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });

        wire_tab_drag(
            &ev.clone().upcast::<gtk::Widget>(),
            tid,
            DropTarget::PinSlot(pin_index),
            &self.actions,
        );
        ev
    }

    fn build_pin_rows(
        &self,
        pinned: &[&UiTabItem],
        folders: &[PinFolderNode],
        snapshot: &UiSnapshot,
        dark: bool,
    ) {
        if !folders.is_empty() && snapshot.settings.pins_folders {
            for folder in folders {
                let lbl = gtk::Label::new(Some(&format!("▸ {}", folder.name)));
                lbl.style_context().add_class("searlx-section-label");
                lbl.set_halign(gtk::Align::Start);
                self.column_items.pack_start(&lbl, false, false, 0);
            }
        }
        for (i, tab) in pinned.iter().enumerate() {
            let row = self.build_tab_row(tab, snapshot, i, None, false, false, false, dark);
            self.column_items.pack_start(&row, false, false, 0);
        }
    }

    fn build_tab_group_section(
        &self,
        group: &TabGroupItem,
        snapshot: &UiSnapshot,
        dark: bool,
        icons_only: bool,
        ctrl_held: bool,
    ) {
        if !icons_only {
            let hdr_ev = gtk::EventBox::new();
            let hdr = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            hdr.style_context().add_class("searlx-group-header");
            let arrow = if group.collapsed { "▸" } else { "▾" };
            let hex = space_badge_hex(group.color, dark);
            let dot = gtk::Label::new(None);
            dot.set_markup(&format!("<span foreground=\"{hex}\">●</span>"));
            let name = gtk::Label::new(Some(&format!(
                "{arrow} {} ({})",
                group.name,
                group.tabs.len()
            )));
            name.set_halign(gtk::Align::Start);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            hdr.pack_start(&dot, false, false, 0);
            hdr.pack_start(&name, true, true, 0);
            hdr_ev.add(&hdr);

            let gid = group.id;
            let collapsed = group.collapsed;
            let sink = self.actions.clone();
            hdr_ev.connect_button_press_event(move |widget, event| {
                if event.button() == 3 {
                    let menu = gtk::Menu::new();
                    let toggle_mi = gtk::MenuItem::with_label(if collapsed {
                        "Expand All Groups"
                    } else {
                        "Collapse All Groups"
                    });
                    let s1 = sink.clone();
                    toggle_mi.connect_activate(move |_| {
                        s1(ShellAction::Command(AppCommand::CollapseAllGroups {
                            collapsed: !collapsed,
                        }));
                    });
                    menu.append(&toggle_mi);

                    let close_mi = gtk::MenuItem::with_label("Close Group");
                    let s2 = sink.clone();
                    close_mi.connect_activate(move |_| {
                        s2(ShellAction::Command(AppCommand::CloseGroup { group: gid }));
                    });
                    menu.append(&close_mi);
                    menu.show_all();
                    menu.popup_at_pointer(Some(event));
                    return glib::Propagation::Stop;
                }
                if event.button() == 1 {
                    sink(ShellAction::Command(AppCommand::CollapseAllGroups {
                        collapsed: !collapsed,
                    }));
                }
                let _ = widget;
                glib::Propagation::Proceed
            });
            wire_drop_target(
                &hdr_ev.clone().upcast::<gtk::Widget>(),
                DropTarget::GroupSlot {
                    group: group.id,
                    index: 0,
                },
                &self.actions,
            );
            self.column_items.pack_start(&hdr_ev, false, false, 0);
        }

        if !group.collapsed || icons_only {
            for (idx, (row_index, tab)) in snapshot
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, t)| t.group == Some(group.id))
                .enumerate()
            {
                let row = self.build_tab_row(
                    tab, snapshot, row_index, None, icons_only, ctrl_held, true, dark,
                );
                if !icons_only {
                    row.set_margin_start(10);
                }
                wire_drop_target(
                    &row.clone().upcast::<gtk::Widget>(),
                    DropTarget::GroupSlot {
                        group: group.id,
                        index: idx,
                    },
                    &self.actions,
                );
                self.column_items.pack_start(&row, false, false, 0);
            }
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn build_tab_row(
        &self,
        tab: &UiTabItem,
        snapshot: &UiSnapshot,
        loose_or_tab_index: usize,
        shortcut_num: Option<usize>,
        icons_only: bool,
        ctrl_held: bool,
        in_group: bool,
        _dark: bool,
    ) -> gtk::EventBox {
        let ev = gtk::EventBox::new();
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 1);
        outer.style_context().add_class("searlx-tab-row");
        if tab.active {
            outer.style_context().add_class("active");
        }
        if tab.sleeping && snapshot.settings.tabs_sleep_dim {
            outer.style_context().add_class("sleeping");
        }
        if tab.put_away {
            outer.style_context().add_class("put-away");
        }

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);

        if ctrl_held
            && snapshot.settings.tabs_shortcuts
            && let Some(n) = shortcut_num
        {
            let num_lbl = gtk::Label::new(Some(&n.to_string()));
            num_lbl.style_context().add_class("searlx-tab-badge");
            row.pack_start(&num_lbl, false, false, 0);
        }

        let glyph = gtk::Label::new(Some(&if tab.loading {
            "◌".to_owned()
        } else {
            tab.letter.to_string()
        }));
        glyph.style_context().add_class("searlx-tab-glyph");
        glyph.set_halign(gtk::Align::Center);
        glyph.set_valign(gtk::Align::Center);
        row.pack_start(&glyph, false, false, 0);

        if icons_only {
            ev.add(&row);
            ev.set_tooltip_text(Some(&format!("{} — {}", tab.title, tab.display_url)));
        } else {
            let text_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let title_lbl = gtk::Label::new(Some(&tab.title));
            title_lbl.style_context().add_class("searlx-tab-title");
            title_lbl.set_halign(gtk::Align::Start);
            title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title_lbl.set_hexpand(true);
            text_col.pack_start(&title_lbl, false, false, 0);

            if tab.active
                && snapshot.settings.tabs_address
                && !tab.url.is_internal()
                && !tab.display_url.is_empty()
            {
                let sub = if tab.url.is_insecure() {
                    format!("Not secure · {}", tab.display_url)
                } else {
                    tab.display_url.clone()
                };
                let url_lbl = gtk::Label::new(Some(&sub));
                url_lbl.style_context().add_class("searlx-section-label");
                url_lbl.set_halign(gtk::Align::Start);
                url_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                text_col.pack_start(&url_lbl, false, false, 0);
            }
            row.pack_start(&text_col, true, true, 0);

            if tab.private {
                let priv_lbl = gtk::Label::new(Some("Private"));
                priv_lbl.style_context().add_class("searlx-private-badge");
                row.pack_start(&priv_lbl, false, false, 0);
            }
            if tab.in_split {
                let sp_lbl = gtk::Label::new(Some("⊞"));
                sp_lbl.style_context().add_class("searlx-tab-badge");
                sp_lbl.set_tooltip_text(Some("In Split View"));
                row.pack_start(&sp_lbl, false, false, 0);
            }
            if tab.crash_retry_message.is_some() {
                let crash_lbl = gtk::Label::new(Some("!"));
                crash_lbl.style_context().add_class("searlx-crash-badge");
                crash_lbl.set_tooltip_text(Some("Page crashed — click to retry"));
                row.pack_start(&crash_lbl, false, false, 0);
            }
            if tab.audible || tab.muted {
                let mute_btn = gtk::Button::with_label(if tab.muted { "🔇" } else { "🔊" });
                mute_btn.style_context().add_class("searlx-tab-close");
                mute_btn.set_tooltip_text(Some(if tab.muted { "Unmute" } else { "Mute" }));
                let tid = tab.id;
                let sink = self.actions.clone();
                mute_btn.connect_clicked(move |_| {
                    sink(ShellAction::Command(AppCommand::ToggleMute {
                        tab: Some(tid),
                    }));
                });
                row.pack_start(&mute_btn, false, false, 0);
            }

            let close_symbol = if tab.pinned || (in_group && !tab.put_away) {
                "−"
            } else {
                "×"
            };
            let close_btn = gtk::Button::with_label(close_symbol);
            close_btn.style_context().add_class("searlx-tab-close");
            close_btn.set_tooltip_text(Some(if tab.pinned {
                "Put Down Pin (Ctrl+W)"
            } else if in_group && !tab.put_away {
                "Put Away (Ctrl+W)"
            } else {
                "Close Tab (Ctrl+W)"
            }));
            let tid = tab.id;
            let sink = self.actions.clone();
            close_btn.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::CloseTab {
                    tab: Some(tid),
                }));
            });
            row.pack_end(&close_btn, false, false, 0);

            outer.pack_start(&row, false, false, 0);
            if tab.active && snapshot.settings.tabs_reading && tab.reading_progress > 0.01 {
                let bar = gtk::ProgressBar::new();
                bar.style_context().add_class("searlx-reading-bar");
                bar.set_fraction(f64::from(tab.reading_progress.clamp(0.0, 1.0)));
                outer.pack_start(&bar, false, false, 0);
            }
            ev.add(&outer);
        }

        let tid = tab.id;
        let is_active = tab.active;
        let tabs_snap = snapshot.tabs.clone();
        let spaces_snap = snapshot.spaces.clone();
        let active_space = snapshot.active_space;
        let sink = self.actions.clone();
        ev.connect_button_press_event(move |widget, event| match event.button() {
            1 => {
                if is_active {
                    sink(ShellAction::ToggleSiteCard);
                } else {
                    sink(ShellAction::Command(AppCommand::ActivateTab { tab: tid }));
                }
                glib::Propagation::Proceed
            }
            2 => {
                sink(ShellAction::Command(AppCommand::CloseTab {
                    tab: Some(tid),
                }));
                glib::Propagation::Stop
            }
            3 => {
                show_tab_context_menu(
                    widget.upcast_ref::<gtk::Widget>(),
                    event,
                    tid,
                    &tabs_snap,
                    &spaces_snap,
                    active_space,
                    &sink,
                );
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });

        let drop_target = if tab.pinned {
            DropTarget::PinSlot(loose_or_tab_index)
        } else {
            DropTarget::LooseTabSlot(loose_or_tab_index)
        };
        wire_tab_drag(
            &ev.clone().upcast::<gtk::Widget>(),
            tid,
            drop_target,
            &self.actions,
        );
        ev
    }

    fn build_top_tab_pill(
        &self,
        tab: &UiTabItem,
        snapshot: &UiSnapshot,
        index: usize,
        shortcut_num: Option<usize>,
        ctrl_held: bool,
        _dark: bool,
    ) -> gtk::EventBox {
        let ev = gtk::EventBox::new();
        let box_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        box_row.style_context().add_class("searlx-tab-row");
        if tab.active {
            box_row.style_context().add_class("active");
        }
        if tab.sleeping && snapshot.settings.tabs_sleep_dim {
            box_row.style_context().add_class("sleeping");
        }

        if ctrl_held
            && snapshot.settings.tabs_shortcuts
            && let Some(n) = shortcut_num
        {
            let num = gtk::Label::new(Some(&n.to_string()));
            num.style_context().add_class("searlx-tab-badge");
            box_row.pack_start(&num, false, false, 0);
        }

        let glyph = gtk::Label::new(Some(&tab.letter.to_string()));
        glyph.style_context().add_class("searlx-tab-glyph");
        box_row.pack_start(&glyph, false, false, 0);

        let title = gtk::Label::new(Some(&tab.title));
        title.style_context().add_class("searlx-tab-title");
        title.set_max_width_chars(18);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        box_row.pack_start(&title, false, false, 0);

        if tab.in_split {
            let sp = gtk::Label::new(Some("⊞"));
            sp.style_context().add_class("searlx-tab-badge");
            box_row.pack_start(&sp, false, false, 0);
        }
        if tab.private {
            let priv_lbl = gtk::Label::new(Some("Priv"));
            priv_lbl.style_context().add_class("searlx-private-badge");
            box_row.pack_start(&priv_lbl, false, false, 0);
        }

        let close_btn = gtk::Button::with_label(if tab.pinned { "−" } else { "×" });
        close_btn.style_context().add_class("searlx-tab-close");
        let tid = tab.id;
        let sink = self.actions.clone();
        close_btn.connect_clicked(move |_| {
            sink(ShellAction::Command(AppCommand::CloseTab {
                tab: Some(tid),
            }));
        });
        box_row.pack_end(&close_btn, false, false, 0);
        ev.add(&box_row);

        let is_active = tab.active;
        let tabs_snap = snapshot.tabs.clone();
        let spaces_snap = snapshot.spaces.clone();
        let active_space = snapshot.active_space;
        let sink_click = self.actions.clone();
        // Top-tabs open on button release so drags can begin (`FEATURES.md` §3.6).
        ev.connect_button_release_event(move |widget, event| match event.button() {
            1 => {
                if is_active {
                    sink_click(ShellAction::ToggleSiteCard);
                } else {
                    sink_click(ShellAction::Command(AppCommand::ActivateTab { tab: tid }));
                }
                glib::Propagation::Proceed
            }
            2 => {
                sink_click(ShellAction::Command(AppCommand::CloseTab {
                    tab: Some(tid),
                }));
                glib::Propagation::Stop
            }
            3 => {
                show_tab_context_menu(
                    widget.upcast_ref::<gtk::Widget>(),
                    event,
                    tid,
                    &tabs_snap,
                    &spaces_snap,
                    active_space,
                    &sink_click,
                );
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });

        wire_tab_drag(
            &ev.clone().upcast::<gtk::Widget>(),
            tid,
            DropTarget::LooseTabSlot(index),
            &self.actions,
        );
        ev
    }

    fn build_space_switcher(&self, snapshot: &UiSnapshot, dark: bool) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
        scroll.set_hexpand(true);
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 4);

        for sp in &snapshot.spaces {
            let hex = space_badge_hex(sp.color, dark);
            let btn = gtk::Button::new();
            btn.style_context().add_class("searlx-space-pill");
            if sp.active {
                btn.style_context().add_class("active");
            }
            let lbl = gtk::Label::new(None);
            lbl.set_markup(&format!(
                "<span foreground=\"{hex}\">●</span> {}",
                glib::markup_escape_text(&sp.name)
            ));
            btn.add(&lbl);
            btn.set_tooltip_text(Some(&format!("{} ({} tabs)", sp.name, sp.tab_count)));

            let sid = sp.id;
            let sink = self.actions.clone();
            btn.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::SwitchSpace { space: sid }));
            });
            let spaces_count = snapshot.spaces.len();
            let sink_menu = self.actions.clone();
            btn.connect_button_press_event(move |_w, ev| {
                if ev.button() == 3 {
                    let menu = gtk::Menu::new();
                    let rename = gtk::MenuItem::with_label("Rename Space…");
                    let s1 = sink_menu.clone();
                    rename.connect_activate(move |_| {
                        s1(ShellAction::PromptRenameSpace(sid));
                    });
                    menu.append(&rename);

                    if spaces_count > 1 {
                        let del = gtk::MenuItem::with_label("Delete Space");
                        let s2 = sink_menu.clone();
                        del.connect_activate(move |_| {
                            s2(ShellAction::Command(AppCommand::DeleteSpace { space: sid }));
                        });
                        menu.append(&del);
                    }
                    menu.show_all();
                    menu.popup_at_pointer(Some(ev));
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            strip.pack_start(&btn, false, false, 0);
        }

        let add_space = make_icon_btn(
            "+",
            "New Space…",
            &self.actions,
            ShellAction::OpenNewSpaceDialog,
        );
        scroll.add(&strip);
        row.pack_start(&scroll, true, true, 0);
        row.pack_end(&add_space, false, false, 0);
        row
    }

    fn build_doors_row(&self, snapshot: &UiSnapshot, compact: bool) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let dl_btn = if snapshot.sidebar_layout == TabLayoutMode::TopTabs {
            &self.top_downloads_btn
        } else {
            &self.column_downloads_btn
        };
        if snapshot.downloads.active_count > 0 {
            dl_btn.set_label(&format!("↓{}", snapshot.downloads.active_count));
        } else {
            dl_btn.set_label("↓");
        }
        if snapshot.downloads.show_button {
            reveal_widget(dl_btn);
        } else {
            dl_btn.hide();
        }

        let hist_btn = make_icon_btn(
            "◷",
            "History (Ctrl+Y)",
            &self.actions,
            ShellAction::OpenInternal(InternalPageKind::History, None),
        );
        let bm_btn = make_icon_btn(
            "★",
            "Bookmarks",
            &self.actions,
            ShellAction::OpenInternal(InternalPageKind::Bookmarks, None),
        );
        let priv_btn = make_icon_btn(
            "🕶",
            "New Private Tab (Shift+Ctrl+N)",
            &self.actions,
            ShellAction::Command(AppCommand::NewPrivateTab {
                url: None,
                activate: true,
            }),
        );
        let settings_btn = make_icon_btn(
            "⚙",
            "Settings (Ctrl+,)",
            &self.actions,
            ShellAction::OpenInternal(InternalPageKind::Settings, None),
        );

        if let Some(parent) = dl_btn
            .parent()
            .and_then(|p| p.downcast::<gtk::Container>().ok())
        {
            parent.remove(dl_btn);
        }
        row.pack_start(&bm_btn, !compact, !compact, 0);
        row.pack_start(&hist_btn, !compact, !compact, 0);
        row.pack_start(dl_btn, !compact, !compact, 0);
        row.pack_start(&priv_btn, !compact, !compact, 0);
        row.pack_end(&settings_btn, !compact, !compact, 0);
        row
    }
}

fn build_nav_cluster(
    active_tab: Option<&UiTabItem>,
    actions: &ActionSink,
    icons_only: bool,
) -> gtk::Box {
    let nav = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    if icons_only {
        return nav;
    }
    let back = make_icon_btn(
        "‹",
        "Back (Ctrl+[)",
        actions,
        ShellAction::Command(AppCommand::GoBack { tab: None }),
    );
    back.set_sensitive(active_tab.is_some_and(|t| t.can_go_back));
    let fwd = make_icon_btn(
        "›",
        "Forward (Ctrl+])",
        actions,
        ShellAction::Command(AppCommand::GoForward { tab: None }),
    );
    fwd.set_sensitive(active_tab.is_some_and(|t| t.can_go_forward));
    let is_loading = active_tab.is_some_and(|t| t.loading);
    let reload_or_stop = make_icon_btn(
        if is_loading { "✕" } else { "↻" },
        if is_loading {
            "Stop"
        } else {
            "Reload (Ctrl+R)"
        },
        actions,
        if is_loading {
            ShellAction::Command(AppCommand::Stop { tab: None })
        } else {
            ShellAction::Command(AppCommand::Reload {
                tab: None,
                from_origin: false,
            })
        },
    );
    nav.pack_start(&back, false, false, 0);
    nav.pack_start(&fwd, false, false, 0);
    nav.pack_start(&reload_or_stop, false, false, 0);
    nav
}

fn make_icon_btn(
    label: &str,
    tooltip: &str,
    actions: &ActionSink,
    action: ShellAction,
) -> gtk::Button {
    let btn = gtk::Button::with_label(label);
    btn.style_context().add_class("searlx-nav-btn");
    btn.set_tooltip_text(Some(tooltip));
    let sink = actions.clone();
    btn.connect_clicked(move |_| {
        sink(action.clone());
    });
    btn
}

fn format_site_chip_label(
    snapshot: &UiSnapshot,
    active_insecure: bool,
    icons_only: bool,
) -> String {
    if icons_only {
        return "●".to_owned();
    }
    if snapshot.address.is_lookalike {
        return format!("⚠ {}", snapshot.address.safe_host);
    }
    let Some(tab) = snapshot
        .active_tab
        .and_then(|aid| snapshot.tabs.iter().find(|t| t.id == aid))
    else {
        return "New Tab".to_owned();
    };
    if tab.url.is_internal() {
        return tab.title.clone();
    }
    let host = tab.url.host().map_or_else(
        || tab.title.clone(),
        |h| h.trim_start_matches("www.").to_owned(),
    );
    if active_insecure {
        format!("Not secure · {host}")
    } else {
        host
    }
}

fn show_tab_context_menu(
    _anchor: &gtk::Widget,
    event: &gdk::EventButton,
    target_tab: TabId,
    tabs: &[UiTabItem],
    spaces: &[searlx_core::UiSpaceItem],
    active_space: SpaceId,
    actions: &ActionSink,
) {
    let Some(target) = tabs.iter().find(|t| t.id == target_tab) else {
        return;
    };
    let active_tab_id = tabs.iter().find(|t| t.active).map(|t| t.id);

    // Build core menu entries from `searlx_core::build_tab_context_menu` via a temporary TabModel-compatible view.
    let mut temp_alloc = searlx_core::TabIdAllocator::new();
    let core_entries: Vec<TabEntry> = tabs
        .iter()
        .map(|t| {
            let mut e = TabEntry::new(t.id, t.url.clone(), &t.title, t.private);
            e.pinned = t.pinned;
            e.muted = t.muted;
            e.audible = t.audible;
            e.loading = t.loading;
            e.sleeping = t.sleeping;
            e
        })
        .collect();
    let active_idx = tabs.iter().position(|t| t.active).unwrap_or(0);
    let core_items = TabModel::from_tabs(core_entries, active_idx, &mut temp_alloc)
        .map(|m| build_tab_context_menu(&m, target_tab, true))
        .unwrap_or_default();

    let menu = gtk::Menu::new();
    for item in core_items {
        let mi = gtk::MenuItem::with_label(&item.label);
        mi.set_sensitive(item.enabled);
        let sink = actions.clone();
        let cmd = match item.id.as_str() {
            "pin.back_home" => Some(AppCommand::RehomePin {
                tab: target_tab,
                replace_home: false,
            }),
            "pin.rehome" => Some(AppCommand::RehomePin {
                tab: target_tab,
                replace_home: true,
            }),
            "pin.unpin" => Some(AppCommand::UnpinTab { tab: target_tab }),
            "tab.pin" => Some(AppCommand::PinTab { tab: target_tab }),
            "tab.duplicate" => Some(AppCommand::DuplicateTab {
                tab: Some(target_tab),
            }),
            "tab.mute" => Some(AppCommand::ToggleMute {
                tab: Some(target_tab),
            }),
            "tab.sleep" => Some(AppCommand::SleepTab { tab: target_tab }),
            "tab.close" => Some(AppCommand::CloseTab {
                tab: Some(target_tab),
            }),
            "tab.reopen" => Some(AppCommand::ReopenClosedTab),
            _ => None,
        };
        if let Some(cmd) = cmd {
            mi.connect_activate(move |_| sink(ShellAction::Command(cmd.clone())));
        }
        menu.append(&mi);
    }

    menu.append(&gtk::SeparatorMenuItem::new());

    // Split View submenu
    let split_mi = gtk::MenuItem::with_label("Split View");
    let split_sub = gtk::Menu::new();
    if target.in_split {
        let rm = gtk::MenuItem::with_label("Remove from Split View");
        let s1 = actions.clone();
        rm.connect_activate(move |_| {
            s1(ShellAction::Command(AppCommand::RemoveSplitPane {
                tab: target_tab,
            }));
        });
        split_sub.append(&rm);
        let swap = gtk::MenuItem::with_label("Swap Pages");
        let s2 = actions.clone();
        swap.connect_activate(move |_| {
            s2(ShellAction::Command(AppCommand::SwapSplitPanes));
        });
        split_sub.append(&swap);
        let even = gtk::MenuItem::with_label("Even Out Panes");
        let s3 = actions.clone();
        even.connect_activate(move |_| {
            s3(ShellAction::Command(AppCommand::EvenSplitPanes));
        });
        split_sub.append(&even);
    } else {
        let open_split = gtk::MenuItem::with_label("Open in Split View");
        let s1 = actions.clone();
        let partner = active_tab_id.filter(|&aid| aid != target_tab);
        open_split.connect_activate(move |_| {
            s1(ShellAction::Command(AppCommand::ToggleSplitView {
                target_tab: Some(target_tab),
                partner_tab: partner,
            }));
        });
        split_sub.append(&open_split);

        if active_tab_id.is_some_and(|aid| aid != target_tab) {
            let add_split = gtk::MenuItem::with_label("Add to Active Split View");
            let s2 = actions.clone();
            add_split.connect_activate(move |_| {
                s2(ShellAction::Command(AppCommand::AddSplitPane {
                    tab: target_tab,
                }));
            });
            split_sub.append(&add_split);
        }
    }
    split_mi.set_submenu(Some(&split_sub));
    menu.append(&split_mi);

    // Move to Space submenu
    if spaces.len() > 1 {
        let move_mi = gtk::MenuItem::with_label("Move to Space");
        let move_sub = gtk::Menu::new();
        for sp in spaces {
            if sp.id == active_space {
                continue;
            }
            let sp_mi = gtk::MenuItem::with_label(&sp.name);
            let s = actions.clone();
            let sid = sp.id;
            let url = target.url.clone();
            sp_mi.connect_activate(move |_| {
                let target_url = url.clone();
                s(ShellAction::Command(AppCommand::CloseTab {
                    tab: Some(target_tab),
                }));
                s(ShellAction::Command(AppCommand::SwitchSpace { space: sid }));
                s(ShellAction::Command(AppCommand::NewTab {
                    url: Some(target_url),
                    activate: true,
                }));
            });
            move_sub.append(&sp_mi);
        }
        move_mi.set_submenu(Some(&move_sub));
        menu.append(&move_mi);
    }

    // Group, Rename, Side Panel, Site Info, Copy Address
    if !target.pinned && !target.private {
        let group_mi = gtk::MenuItem::with_label("New Tab Group…");
        let s = actions.clone();
        group_mi.connect_activate(move |_| s(ShellAction::PromptGroupTab(target_tab)));
        menu.append(&group_mi);
    }

    let rename_mi = gtk::MenuItem::with_label("Rename Tab…");
    let s_ren = actions.clone();
    rename_mi.connect_activate(move |_| s_ren(ShellAction::PromptRenameTab(target_tab)));
    menu.append(&rename_mi);

    let copy_mi = gtk::MenuItem::with_label("Copy Address");
    let s_copy = actions.clone();
    copy_mi.connect_activate(move |_| {
        s_copy(ShellAction::Command(AppCommand::CopyAddress {
            tab: Some(target_tab),
        }));
    });
    menu.append(&copy_mi);

    if matches!(target.url.scheme(), "http" | "https") {
        let panel_mi = gtk::MenuItem::with_label("Open in Side Panel");
        let s_pan = actions.clone();
        let u = target.url.clone();
        panel_mi.connect_activate(move |_| {
            s_pan(ShellAction::Command(AppCommand::ToggleSidePanelWebsite {
                url: u.clone(),
            }));
        });
        menu.append(&panel_mi);
    }

    let site_mi = gtk::MenuItem::with_label("Site Information…");
    let s_site = actions.clone();
    site_mi.connect_activate(move |_| {
        s_site(ShellAction::Command(AppCommand::ActivateTab {
            tab: target_tab,
        }));
        s_site(ShellAction::ToggleSiteCard);
    });
    menu.append(&site_mi);

    menu.show_all();
    menu.popup_at_pointer(Some(event));
}

fn wire_tab_drag(
    widget: &gtk::Widget,
    tab_id: TabId,
    drop_target: DropTarget,
    actions: &ActionSink,
) {
    let entries = [gtk::TargetEntry::new(
        DND_TARGET_TAB,
        gtk::TargetFlags::SAME_APP,
        0,
    )];
    widget.drag_source_set(
        gdk::ModifierType::BUTTON1_MASK,
        &entries,
        gdk::DragAction::MOVE,
    );
    let sink_begin = actions.clone();
    widget.connect_drag_begin(move |w, _ctx| {
        let alloc = w.allocation();
        sink_begin(ShellAction::StartTabDrag {
            tab: tab_id,
            x: f64::from(alloc.x()),
            y: f64::from(alloc.y()),
        });
    });
    let sink_end = actions.clone();
    widget.connect_drag_end(move |_w, _ctx| {
        sink_end(ShellAction::CancelTabDrag);
    });
    wire_drop_target(widget, drop_target, actions);
}

/// Register a GTK widget as a drop destination for tab drags (`BeginDrag`/`UpdateDrag`/`CommitDrag`).
pub fn wire_drop_target(widget: &gtk::Widget, target: DropTarget, actions: &ActionSink) {
    let entries = [gtk::TargetEntry::new(
        DND_TARGET_TAB,
        gtk::TargetFlags::SAME_APP,
        0,
    )];
    widget.drag_dest_set(gtk::DestDefaults::ALL, &entries, gdk::DragAction::MOVE);

    let t_motion = target.clone();
    let sink_motion = actions.clone();
    widget.connect_drag_motion(move |w, _ctx, x, y, _time| {
        w.style_context().add_class("drag-over");
        sink_motion(ShellAction::UpdateTabDrag {
            x: f64::from(x),
            y: f64::from(y),
            target: Some(t_motion.clone()),
        });
        true
    });

    widget.connect_drag_leave(move |w, _ctx, _time| {
        w.style_context().remove_class("drag-over");
    });

    let t_drop = target;
    let sink_drop = actions.clone();
    widget.connect_drag_drop(move |w, _ctx, x, y, _time| {
        w.style_context().remove_class("drag-over");
        sink_drop(ShellAction::UpdateTabDrag {
            x: f64::from(x),
            y: f64::from(y),
            target: Some(t_drop.clone()),
        });
        sink_drop(ShellAction::FinishTabDrag);
        true
    });
}

fn clear_box(container: &gtk::Box) {
    for child in container.children() {
        container.remove(&child);
    }
}
