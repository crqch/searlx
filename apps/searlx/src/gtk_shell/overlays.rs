//! Floating overlays and popovers over the Search page card (`FEATURES.md` §3-§5, §11, §22):
//! - Floating overlay address bar (`Ctrl+L` / `Ctrl+T`) with live `SuggestionRow` list & shake feedback
//! - Site Card popover (`UiSnapshot.site_card`: connection/cert, zoom, blocker, permissions, dusk, keep awake, print)
//! - Floating Find pill (`Ctrl+F`)
//! - Downloads popover, Hover status line, Status banner, Crash/Error card, Prompt bars, Peek, Switcher

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use searlx_core::{
    AddressInvalidReason, AppCommand, ConnectionState, DownloadState, EngineDialogKind,
    EnginePermissionKind, InternalPageKind, PageUrl, PermissionDecision, SubmitDisposition,
    SuggestionRow, UiSnapshot,
};

use super::actions::{ActionSink, ShellAction};
use super::theme::{FIELD_WIDTH, motion_duration_ms, reveal_widget};

#[derive(Clone, Debug)]
enum SuggestionAction {
    ActivateTab(searlx_core::TabId),
    LockSite(searlx_core::SiteSearch),
    Url(PageUrl),
    Command(searlx_core::AddressCommand),
}

/// Manages all floating overlays and popovers mounted over `stage.page_overlay`.
pub struct ShellOverlays {
    // Floating address overlay (`Ctrl+L` / `Ctrl+T`)
    address_scrim: gtk::EventBox,
    address_card: gtk::Box,
    address_chip_box: gtk::Box,
    address_entry: gtk::Entry,
    address_hint_lbl: gtk::Label,
    address_shake_lbl: gtk::Label,
    address_suggestions_scroll: gtk::ScrolledWindow,
    address_suggestions_list: gtk::ListBox,
    suggestion_actions: RefCell<Vec<SuggestionAction>>,
    address_open: Cell<bool>,
    address_for_new_tab: Cell<bool>,
    syncing_address: Cell<bool>,

    // Floating Find pill (`Ctrl+F`)
    find_pill: gtk::Box,
    find_entry: gtk::Entry,
    find_status_lbl: gtk::Label,
    find_case_btn: gtk::ToggleButton,
    find_whole_btn: gtk::ToggleButton,
    syncing_find: Cell<bool>,

    // Hover status line & transient status banner
    status_line_box: gtk::Box,
    status_line_lbl: gtk::Label,
    status_banner_box: gtk::Box,
    status_banner_lbl: gtk::Label,
    status_banner_undo: gtk::Button,

    // Hide Elements active picker bar
    hide_picker_bar: gtk::Box,

    // Crash / in-page error centered card
    crash_card: gtk::Box,
    crash_title_lbl: gtk::Label,
    crash_reason_lbl: gtk::Label,

    // Top-center pending prompt (permission / JS dialog / auth / cert / password save)
    prompt_card: gtk::Box,
    last_prompt_sig: RefCell<String>,

    // Link Peek & Ctrl+Tab Switcher overlays
    peek_card: gtk::Box,
    switcher_card: gtk::Box,

    // Popovers (Site Card + Downloads)
    site_card_popover: gtk::Popover,
    site_card_box: gtk::Box,
    site_cert_expanded: Rc<Cell<bool>>,
    downloads_popover: gtk::Popover,
    downloads_box: gtk::Box,

    actions: ActionSink,
}

impl ShellOverlays {
    /// Create and mount all overlay layers onto `page_overlay`.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn new(page_overlay: &gtk::Overlay, actions: ActionSink) -> Self {
        // 1. Floating Address Bar overlay (`Ctrl+L` / `Ctrl+T`)
        let address_scrim = gtk::EventBox::new();
        address_scrim
            .style_context()
            .add_class("searlx-overlay-scrim");
        address_scrim.set_hexpand(true);
        address_scrim.set_vexpand(true);
        address_scrim.set_no_show_all(true);

        let scrim_layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scrim_layout.set_hexpand(true);
        scrim_layout.set_vexpand(true);

        let address_card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        address_card
            .style_context()
            .add_class("searlx-overlay-card");
        address_card.set_size_request(FIELD_WIDTH, -1);

        let input_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let address_chip_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let address_entry = gtk::Entry::new();
        address_entry
            .style_context()
            .add_class("searlx-overlay-entry");
        address_entry.set_placeholder_text(Some("Enter an address or search…"));
        address_entry.set_hexpand(true);

        input_row.pack_start(&address_chip_box, false, false, 0);
        input_row.pack_start(&address_entry, true, true, 0);

        let meta_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let address_hint_lbl = gtk::Label::new(None);
        address_hint_lbl
            .style_context()
            .add_class("searlx-completion-hint");
        address_hint_lbl.set_halign(gtk::Align::Start);
        address_hint_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let address_shake_lbl = gtk::Label::new(None);
        address_shake_lbl
            .style_context()
            .add_class("searlx-shake-label");
        address_shake_lbl.set_halign(gtk::Align::End);
        meta_row.pack_start(&address_hint_lbl, true, true, 0);
        meta_row.pack_end(&address_shake_lbl, false, false, 0);

        let address_suggestions_scroll =
            gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        address_suggestions_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        address_suggestions_scroll.set_max_content_height(300);
        address_suggestions_scroll.set_propagate_natural_height(true);

        let address_suggestions_list = gtk::ListBox::new();
        address_suggestions_list.set_selection_mode(gtk::SelectionMode::Single);
        address_suggestions_scroll.add(&address_suggestions_list);

        // Wrap card inside an invisible EventBox sized strictly to the card so outside scrim clicks dismiss.
        let card_guard = gtk::EventBox::new();
        card_guard.set_visible_window(false);
        card_guard.set_halign(gtk::Align::Center);
        card_guard.set_valign(gtk::Align::Start);
        card_guard.set_margin_top(68);
        card_guard.add(&address_card);
        card_guard.connect_button_press_event(|_, _| glib::Propagation::Stop);

        address_card.pack_start(&input_row, false, false, 0);
        address_card.pack_start(&meta_row, false, false, 0);
        address_card.pack_start(&address_suggestions_scroll, false, false, 0);

        scrim_layout.pack_start(&card_guard, false, false, 0);
        address_scrim.add(&scrim_layout);

        {
            let sink = actions.clone();
            address_scrim.connect_button_press_event(move |_, _| {
                sink(ShellAction::CloseAddressOverlay);
                glib::Propagation::Stop
            });
        }

        page_overlay.add_overlay(&address_scrim);

        // 2. Floating Find Pill (`Ctrl+F`)
        let find_pill = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        find_pill.style_context().add_class("searlx-find-pill");
        find_pill.set_halign(gtk::Align::End);
        find_pill.set_valign(gtk::Align::Start);
        find_pill.set_margin_top(12);
        find_pill.set_margin_end(14);
        find_pill.set_no_show_all(true);

        let find_entry = gtk::Entry::new();
        find_entry.style_context().add_class("searlx-find-entry");
        find_entry.set_placeholder_text(Some("Find on page…"));
        let find_status_lbl = gtk::Label::new(Some(""));
        find_status_lbl
            .style_context()
            .add_class("searlx-section-label");
        let find_case_btn = gtk::ToggleButton::with_label("Aa");
        find_case_btn.style_context().add_class("searlx-nav-btn");
        find_case_btn.set_tooltip_text(Some("Match Case"));
        let find_whole_btn = gtk::ToggleButton::with_label("W");
        find_whole_btn.style_context().add_class("searlx-nav-btn");
        find_whole_btn.set_tooltip_text(Some("Whole Words"));
        let find_prev_btn = gtk::Button::with_label("↑");
        find_prev_btn.style_context().add_class("searlx-nav-btn");
        let find_next_btn = gtk::Button::with_label("↓");
        find_next_btn.style_context().add_class("searlx-nav-btn");
        let find_close_btn = gtk::Button::with_label("×");
        find_close_btn.style_context().add_class("searlx-nav-btn");

        find_pill.pack_start(&find_entry, false, false, 0);
        find_pill.pack_start(&find_status_lbl, false, false, 2);
        find_pill.pack_start(&find_case_btn, false, false, 0);
        find_pill.pack_start(&find_whole_btn, false, false, 0);
        find_pill.pack_start(&find_prev_btn, false, false, 0);
        find_pill.pack_start(&find_next_btn, false, false, 0);
        find_pill.pack_start(&find_close_btn, false, false, 0);
        page_overlay.add_overlay(&find_pill);

        // 3. Hover Status Line (bottom-left)
        let status_line_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        status_line_box
            .style_context()
            .add_class("searlx-status-line");
        status_line_box.set_halign(gtk::Align::Start);
        status_line_box.set_valign(gtk::Align::End);
        status_line_box.set_margin_start(10);
        status_line_box.set_margin_bottom(10);
        status_line_box.set_no_show_all(true);
        let status_line_lbl = gtk::Label::new(None);
        status_line_lbl.set_max_width_chars(72);
        status_line_lbl.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        status_line_box.pack_start(&status_line_lbl, false, false, 0);
        page_overlay.add_overlay(&status_line_box);
        page_overlay.set_overlay_pass_through(&status_line_box, true);

        // 4. Transient status banner (bottom-center)
        let status_banner_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        status_banner_box
            .style_context()
            .add_class("searlx-status-banner");
        status_banner_box.set_halign(gtk::Align::Center);
        status_banner_box.set_valign(gtk::Align::End);
        status_banner_box.set_margin_bottom(16);
        status_banner_box.set_no_show_all(true);
        let status_banner_lbl = gtk::Label::new(None);
        let status_banner_undo = gtk::Button::with_label("Undo (Ctrl+Z)");
        status_banner_undo
            .style_context()
            .add_class("searlx-site-card-btn");
        {
            let sink = actions.clone();
            status_banner_undo.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::UndoHideElement));
            });
        }
        status_banner_box.pack_start(&status_banner_lbl, false, false, 0);
        status_banner_box.pack_start(&status_banner_undo, false, false, 0);
        page_overlay.add_overlay(&status_banner_box);

        // 5. Hide Elements active picker bar (top-center)
        let hide_picker_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        hide_picker_bar
            .style_context()
            .add_class("searlx-banner-prompt");
        hide_picker_bar.set_halign(gtk::Align::Center);
        hide_picker_bar.set_valign(gtk::Align::Start);
        hide_picker_bar.set_margin_top(12);
        hide_picker_bar.set_no_show_all(true);
        let hide_lbl = gtk::Label::new(Some(
            "Hide Elements: click any page element to hide · Esc to finish",
        ));
        let hide_done = gtk::Button::with_label("Done");
        hide_done.style_context().add_class("searlx-primary-btn");
        {
            let sink = actions.clone();
            hide_done.connect_clicked(move |_| {
                sink(ShellAction::ToggleHideElementsPicker);
            });
        }
        hide_picker_bar.pack_start(&hide_lbl, false, false, 0);
        hide_picker_bar.pack_start(&hide_done, false, false, 0);
        page_overlay.add_overlay(&hide_picker_bar);

        // 6. Crash / In-page error centered card
        let crash_card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        crash_card.style_context().add_class("searlx-crash-card");
        crash_card.set_halign(gtk::Align::Center);
        crash_card.set_valign(gtk::Align::Center);
        crash_card.set_no_show_all(true);
        let crash_title_lbl = gtk::Label::new(None);
        crash_title_lbl
            .style_context()
            .add_class("searlx-site-card-title");
        let crash_reason_lbl = gtk::Label::new(None);
        crash_reason_lbl.set_line_wrap(true);
        crash_reason_lbl.set_max_width_chars(56);
        let crash_retry_btn = gtk::Button::with_label("Reload Page");
        crash_retry_btn
            .style_context()
            .add_class("searlx-primary-btn");
        crash_retry_btn.set_halign(gtk::Align::Center);
        {
            let sink = actions.clone();
            crash_retry_btn.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::RetryCrashedTab {
                    tab: None,
                }));
            });
        }
        crash_card.pack_start(&crash_title_lbl, false, false, 0);
        crash_card.pack_start(&crash_reason_lbl, false, false, 0);
        crash_card.pack_start(&crash_retry_btn, false, false, 4);
        page_overlay.add_overlay(&crash_card);

        // 7. Pending Prompt bar (permissions, JS dialogs, auth, cert, password save)
        let prompt_card = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        prompt_card
            .style_context()
            .add_class("searlx-banner-prompt");
        prompt_card.set_halign(gtk::Align::Center);
        prompt_card.set_valign(gtk::Align::Start);
        prompt_card.set_margin_top(14);
        prompt_card.set_no_show_all(true);
        page_overlay.add_overlay(&prompt_card);

        // 8. Peek preview & Ctrl+Tab switcher overlays
        let peek_card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        peek_card.style_context().add_class("searlx-overlay-card");
        peek_card.set_halign(gtk::Align::Center);
        peek_card.set_valign(gtk::Align::Center);
        peek_card.set_size_request(540, -1);
        peek_card.set_no_show_all(true);
        page_overlay.add_overlay(&peek_card);

        let switcher_card = gtk::Box::new(gtk::Orientation::Vertical, 10);
        switcher_card
            .style_context()
            .add_class("searlx-switcher-card");
        switcher_card.set_halign(gtk::Align::Center);
        switcher_card.set_valign(gtk::Align::Center);
        switcher_card.set_no_show_all(true);
        page_overlay.add_overlay(&switcher_card);

        // 9. Popovers (Site Card & Downloads)
        let site_card_popover = gtk::Popover::new(None::<&gtk::Widget>);
        site_card_popover
            .style_context()
            .add_class("searlx-site-card");
        let site_card_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        site_card_box.set_size_request(310, -1);
        site_card_popover.add(&site_card_box);

        let downloads_popover = gtk::Popover::new(None::<&gtk::Widget>);
        downloads_popover
            .style_context()
            .add_class("searlx-site-card");
        let downloads_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        downloads_box.set_size_request(340, -1);
        downloads_popover.add(&downloads_box);

        let overlays = Self {
            address_scrim,
            address_card,
            address_chip_box,
            address_entry,
            address_hint_lbl,
            address_shake_lbl,
            address_suggestions_scroll,
            address_suggestions_list,
            suggestion_actions: RefCell::new(Vec::new()),
            address_open: Cell::new(false),
            address_for_new_tab: Cell::new(false),
            syncing_address: Cell::new(false),
            find_pill,
            find_entry,
            find_status_lbl,
            find_case_btn,
            find_whole_btn,
            syncing_find: Cell::new(false),
            status_line_box,
            status_line_lbl,
            status_banner_box,
            status_banner_lbl,
            status_banner_undo,
            hide_picker_bar,
            crash_card,
            crash_title_lbl,
            crash_reason_lbl,
            prompt_card,
            last_prompt_sig: RefCell::new(String::new()),
            peek_card,
            switcher_card,
            site_card_popover,
            site_card_box,
            site_cert_expanded: Rc::new(Cell::new(false)),
            downloads_popover,
            downloads_box,
            actions,
        };

        overlays.wire_find_events(&find_prev_btn, &find_next_btn, &find_close_btn);
        overlays
    }

    /// Connect address overlay keyboard/entry signals (called once after `Rc<ShellOverlays>` exists).
    pub fn wire_address_signals(this: &std::rc::Rc<Self>) {
        // Entry text changed -> update BrowserController address suggestions
        {
            let weak = std::rc::Rc::downgrade(this);
            this.address_entry.connect_changed(move |entry| {
                if let Some(me) = weak.upgrade()
                    && !me.syncing_address.get()
                {
                    let text = entry.text().to_string();
                    (me.actions)(ShellAction::Command(AppCommand::SetAddressText { text }));
                }
            });
        }

        // Suggestion row activated by click
        {
            let weak = std::rc::Rc::downgrade(this);
            this.address_suggestions_list
                .connect_row_activated(move |_lb, row| {
                    if let Some(me) = weak.upgrade()
                        && let Ok(idx) = usize::try_from(row.index())
                    {
                        me.activate_suggestion_at(idx, SubmitDisposition::CurrentTab);
                    }
                });
        }

        // Entry key navigation (`Up`, `Down`, `Tab`, `Return`, `Escape`, `BackSpace`)
        {
            let weak = std::rc::Rc::downgrade(this);
            this.address_entry
                .connect_key_press_event(move |entry, ev| {
                    let Some(me) = weak.upgrade() else {
                        return glib::Propagation::Proceed;
                    };
                    let state = ev.state();
                    let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
                    let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
                    let key = ev.keyval();

                    if key == gdk::keys::constants::Escape {
                        (me.actions)(ShellAction::CloseAddressOverlay);
                        return glib::Propagation::Stop;
                    }
                    if key == gdk::keys::constants::Down {
                        me.move_suggestion_selection(1);
                        return glib::Propagation::Stop;
                    }
                    if key == gdk::keys::constants::Up {
                        me.move_suggestion_selection(-1);
                        return glib::Propagation::Stop;
                    }
                    if key == gdk::keys::constants::Tab || key == gdk::keys::constants::ISO_Left_Tab
                    {
                        // If the current selected row or top row is a SiteSearchOffer, `Tab` locks the chip (`FEATURES.md` §4.3).
                        if me.try_lock_selected_site_chip() {
                            me.syncing_address.set(true);
                            entry.set_text("");
                            me.syncing_address.set(false);
                            return glib::Propagation::Stop;
                        }
                        me.move_suggestion_selection(if shift { -1 } else { 1 });
                        return glib::Propagation::Stop;
                    }
                    if key == gdk::keys::constants::BackSpace && entry.text().is_empty() {
                        (me.actions)(ShellAction::Command(AppCommand::LockSiteSearchChip {
                            site: None,
                        }));
                        return glib::Propagation::Proceed;
                    }
                    if key == gdk::keys::constants::Return || key == gdk::keys::constants::KP_Enter
                    {
                        let disposition = if me.address_for_new_tab.get() || (ctrl && shift) {
                            SubmitDisposition::NewTabInFront
                        } else if ctrl {
                            SubmitDisposition::NewTabBehind
                        } else {
                            SubmitDisposition::CurrentTab
                        };

                        if let Some(row) = me.address_suggestions_list.selected_row()
                            && let Ok(idx) = usize::try_from(row.index())
                        {
                            me.activate_suggestion_at(idx, disposition);
                            return glib::Propagation::Stop;
                        }

                        let input = entry.text().to_string();
                        (me.actions)(ShellAction::Command(AppCommand::SubmitAddress {
                            input,
                            disposition,
                            shift_pressed: shift,
                        }));
                        return glib::Propagation::Stop;
                    }
                    glib::Propagation::Proceed
                });
        }
    }

    fn wire_find_events(
        &self,
        prev_btn: &gtk::Button,
        next_btn: &gtk::Button,
        close_btn: &gtk::Button,
    ) {
        let emit_find = {
            let entry = self.find_entry.clone();
            let case_btn = self.find_case_btn.clone();
            let whole_btn = self.find_whole_btn.clone();
            let sink = self.actions.clone();
            move || {
                sink(ShellAction::Command(AppCommand::FindInPage {
                    query: entry.text().to_string(),
                    case_sensitive: case_btn.is_active(),
                    whole_words: whole_btn.is_active(),
                    text_hint: None,
                }));
            }
        };

        {
            let syncing = &self.syncing_find;
            let syncing_flag = syncing.clone();
            let emit = emit_find.clone();
            self.find_entry.connect_changed(move |_| {
                if !syncing_flag.get() {
                    emit();
                }
            });
        }
        {
            let sink = self.actions.clone();
            self.find_entry.connect_key_press_event(move |_, ev| {
                if ev.keyval() == gdk::keys::constants::Escape {
                    sink(ShellAction::Command(AppCommand::CloseFindBar));
                    return glib::Propagation::Stop;
                }
                if (ev.keyval() == gdk::keys::constants::Return
                    || ev.keyval() == gdk::keys::constants::KP_Enter)
                    && ev.state().contains(gdk::ModifierType::SHIFT_MASK)
                {
                    sink(ShellAction::Command(AppCommand::FindPrevious));
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
        }
        {
            let sink = self.actions.clone();
            self.find_entry.connect_activate(move |_| {
                sink(ShellAction::Command(AppCommand::FindNext));
            });
        }
        {
            let emit = emit_find.clone();
            self.find_case_btn.connect_toggled(move |_| emit());
        }
        {
            let emit = emit_find;
            self.find_whole_btn.connect_toggled(move |_| emit());
        }
        {
            let sink = self.actions.clone();
            prev_btn.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::FindPrevious));
            });
        }
        {
            let sink = self.actions.clone();
            next_btn.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::FindNext));
            });
        }
        {
            let sink = self.actions.clone();
            close_btn.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::CloseFindBar));
            });
        }
    }

    /// Whether the floating address overlay or Find pill currently owns keyboard focus.
    #[must_use]
    pub fn holds_text_focus(&self) -> bool {
        self.address_open.get() || self.find_entry.has_focus()
    }

    /// Whether the floating address overlay is currently open.
    #[must_use]
    pub fn is_address_open(&self) -> bool {
        self.address_open.get()
    }

    /// Open the floating address bar overlay (`Ctrl+L` or `Ctrl+T`).
    pub fn open_address_overlay(&self, prefill: &str, for_new_tab: bool) {
        self.address_open.set(true);
        self.address_for_new_tab.set(for_new_tab);
        self.address_card.style_context().remove_class("shake");
        self.address_shake_lbl.set_text("");

        self.syncing_address.set(true);
        self.address_entry.set_text(prefill);
        self.syncing_address.set(false);

        reveal_widget(&self.address_scrim);
        self.address_entry.grab_focus();
        self.address_entry.select_region(0, -1);
    }

    /// Dismiss the floating address bar overlay.
    pub fn close_address_overlay(&self) {
        self.address_open.set(false);
        self.address_for_new_tab.set(false);
        self.address_scrim.hide();
    }

    /// Trigger invalid-input shake feedback on the floating address card (`FEATURES.md` §1, §3.6).
    pub fn show_address_shake(&self, reason: &AddressInvalidReason, reduce_motion: bool) {
        self.address_card.style_context().add_class("shake");
        self.address_shake_lbl
            .set_text(&format_invalid_reason(reason));
        let card = self.address_card.clone();
        let ms = motion_duration_ms(260, reduce_motion);
        if ms > 0 {
            glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || {
                card.style_context().remove_class("shake");
            });
        }
    }

    /// Focus the floating Find bar entry (`Ctrl+F`).
    pub fn focus_find_entry(&self) {
        reveal_widget(&self.find_pill);
        self.find_entry.grab_focus();
        self.find_entry.select_region(0, -1);
    }

    /// Update hovered link destination status line (`FEATURES.md` §2.1).
    pub fn set_hovered_link(&self, href: &str) {
        let trimmed = href.trim();
        if trimmed.is_empty() {
            self.status_line_box.hide();
        } else {
            self.status_line_lbl.set_text(trimmed);
            reveal_widget(&self.status_line_box);
        }
    }

    /// Toggle the Site Card popover anchored to `anchor`.
    pub fn toggle_site_card(&self, anchor: &gtk::Widget, snapshot: &UiSnapshot) {
        if self.site_card_popover.is_visible() {
            self.site_card_popover.popdown();
            return;
        }
        self.site_cert_expanded.set(false);
        self.site_card_popover.set_relative_to(Some(anchor));
        self.rebuild_site_card(snapshot);
        self.site_card_popover.popup();
    }

    /// Refresh Site Card contents when open.
    pub fn refresh_site_card_if_open(&self, snapshot: &UiSnapshot) {
        if self.site_card_popover.is_visible() {
            self.rebuild_site_card(snapshot);
        }
    }

    /// Toggle the Downloads popover anchored to `anchor`.
    pub fn toggle_downloads_popover(&self, anchor: &gtk::Widget, snapshot: &UiSnapshot) {
        if self.downloads_popover.is_visible() {
            self.downloads_popover.popdown();
            return;
        }
        self.downloads_popover.set_relative_to(Some(anchor));
        self.rebuild_downloads_popover(snapshot);
        self.downloads_popover.popup();
    }

    /// Synchronize all overlay states from `snapshot`.
    pub fn sync(&self, snapshot: &UiSnapshot, hide_picker_active: bool) {
        // 1. Address bar overlay suggestions, chip, inline completion, shake
        if self.address_open.get() {
            self.sync_address_overlay(snapshot);
        }

        // 2. Find pill
        if let Some(find) = &snapshot.find {
            if !self.find_pill.is_visible() {
                reveal_widget(&self.find_pill);
            }
            self.find_status_lbl.set_text(&find.status_text);
            self.find_whole_btn
                .set_sensitive(find.whole_words_available);
            if self.find_case_btn.is_active() != find.case_sensitive {
                self.find_case_btn.set_active(find.case_sensitive);
            }
            if self.find_whole_btn.is_active() != find.whole_words {
                self.find_whole_btn.set_active(find.whole_words);
            }
        } else if self.find_pill.is_visible() {
            self.find_pill.hide();
        }

        // 3. Hide Elements active picker bar
        if hide_picker_active {
            reveal_widget(&self.hide_picker_bar);
        } else {
            self.hide_picker_bar.hide();
        }

        // 4. Status banner (`"Hidden — ⌘Z puts it back"`, etc.)
        if let Some(msg) = &snapshot.status_banner {
            self.status_banner_lbl.set_text(msg);
            reveal_widget(&self.status_banner_box);
            if msg.contains("Hidden") {
                self.status_banner_undo.show();
            } else {
                self.status_banner_undo.hide();
            }
        } else {
            self.status_banner_box.hide();
        }

        // 5. Crash / In-page error banner on active tab
        let active_tab = snapshot
            .active_tab
            .and_then(|aid| snapshot.tabs.iter().find(|t| t.id == aid));
        if let Some(tab) = active_tab
            && let Some(crash_msg) = &tab.crash_retry_message
        {
            self.crash_title_lbl
                .set_text("This page ended unexpectedly");
            self.crash_reason_lbl.set_text(crash_msg);
            reveal_widget(&self.crash_card);
        } else if let Some(tab) = active_tab
            && let Some(err) = &tab.in_page_error
        {
            self.crash_title_lbl
                .set_text("This page could not be loaded");
            self.crash_reason_lbl
                .set_text(&format!("{} — {}", err.url.display(), err.reason));
            reveal_widget(&self.crash_card);
        } else {
            self.crash_card.hide();
        }

        // 6. Pending prompts (`pending_permission`, `pending_dialog`, `pending_auth`, `pending_cert_error`, `pending_password_offer`)
        self.sync_pending_prompt(snapshot);

        // 7. Link Peek & Ctrl+Tab switcher
        self.sync_peek_and_switcher(snapshot);

        // 8. Refresh open popovers
        self.refresh_site_card_if_open(snapshot);
        if self.downloads_popover.is_visible() {
            self.rebuild_downloads_popover(snapshot);
        }
    }

    fn sync_address_overlay(&self, snapshot: &UiSnapshot) {
        // Chip
        for c in self.address_chip_box.children() {
            self.address_chip_box.remove(&c);
        }
        if let Some(site) = &snapshot.address.site_search_chip {
            let chip_btn = gtk::Button::with_label(&format!("Search {} ×", site.name));
            chip_btn.style_context().add_class("searlx-site-card-btn");
            let sink = self.actions.clone();
            chip_btn.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::LockSiteSearchChip {
                    site: None,
                }));
            });
            self.address_chip_box.pack_start(&chip_btn, false, false, 0);
            self.address_chip_box.show_all();
        } else {
            self.address_chip_box.hide();
        }

        // Completion & shake hints
        if let Some(reason) = &snapshot.address.shake_reason {
            self.address_shake_lbl
                .set_text(&format_invalid_reason(reason));
            self.address_card.style_context().add_class("shake");
        } else {
            self.address_shake_lbl.set_text("");
            self.address_card.style_context().remove_class("shake");
        }

        let hint = if let Some(comp) = &snapshot.address.inline_completion {
            format!(
                "Completion: {}{}  (Return accepts)",
                snapshot.address.text, comp
            )
        } else if snapshot.address.is_lookalike {
            format!("Lookalike host protection: {}", snapshot.address.safe_host)
        } else {
            "Enter opens URL or searches · Tab locks site search · Ctrl+Return opens behind"
                .to_owned()
        };
        self.address_hint_lbl.set_text(&hint);

        // Live suggestions list
        for row in self.address_suggestions_list.children() {
            self.address_suggestions_list.remove(&row);
        }
        let mut actions_list = Vec::new();
        for sug in snapshot.address.suggestions.iter().take(10) {
            let (badge, primary, secondary, act) = describe_suggestion(sug);
            actions_list.push(act);

            let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row_box.style_context().add_class("searlx-suggestion-row");

            let badge_lbl = gtk::Label::new(Some(badge));
            badge_lbl.style_context().add_class("searlx-tab-badge");
            row_box.pack_start(&badge_lbl, false, false, 0);

            let title_lbl = gtk::Label::new(Some(&primary));
            title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title_lbl.set_halign(gtk::Align::Start);
            row_box.pack_start(&title_lbl, false, false, 0);

            if !secondary.is_empty() {
                let sub_lbl = gtk::Label::new(Some(&secondary));
                sub_lbl.style_context().add_class("searlx-section-label");
                sub_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                sub_lbl.set_halign(gtk::Align::End);
                sub_lbl.set_hexpand(true);
                row_box.pack_end(&sub_lbl, true, true, 0);
            }

            let lb_row = gtk::ListBoxRow::new();
            lb_row.add(&row_box);
            self.address_suggestions_list.add(&lb_row);
        }
        *self.suggestion_actions.borrow_mut() = actions_list;
        if snapshot.address.suggestions.is_empty() {
            self.address_suggestions_scroll.hide();
        } else {
            self.address_suggestions_scroll.show_all();
        }
    }

    fn move_suggestion_selection(&self, delta: i32) {
        let count = self.suggestion_actions.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let current = self
            .address_suggestions_list
            .selected_row()
            .map(|r| r.index());
        match (current, delta) {
            (None, 1) => {
                if let Some(first) = self.address_suggestions_list.row_at_index(0) {
                    self.address_suggestions_list.select_row(Some(&first));
                }
            }
            (Some(0), -1) => {
                self.address_suggestions_list.unselect_all();
            }
            (Some(idx), d) => {
                let next = (idx + d).clamp(0, count - 1);
                if let Some(row) = self.address_suggestions_list.row_at_index(next) {
                    self.address_suggestions_list.select_row(Some(&row));
                }
            }
            _ => {}
        }
    }

    fn try_lock_selected_site_chip(&self) -> bool {
        let actions = self.suggestion_actions.borrow();
        let idx = self
            .address_suggestions_list
            .selected_row()
            .and_then(|r| usize::try_from(r.index()).ok())
            .unwrap_or(0);
        if let Some(SuggestionAction::LockSite(site)) = actions.get(idx) {
            (self.actions)(ShellAction::Command(AppCommand::LockSiteSearchChip {
                site: Some(site.clone()),
            }));
            return true;
        }
        false
    }

    fn activate_suggestion_at(&self, idx: usize, disposition: SubmitDisposition) {
        let act = self.suggestion_actions.borrow().get(idx).cloned();
        let Some(act) = act else {
            return;
        };
        match act {
            SuggestionAction::ActivateTab(tab) => {
                self.close_address_overlay();
                (self.actions)(ShellAction::Command(AppCommand::ActivateTab { tab }));
            }
            SuggestionAction::LockSite(site) => {
                (self.actions)(ShellAction::Command(AppCommand::LockSiteSearchChip {
                    site: Some(site),
                }));
                self.syncing_address.set(true);
                self.address_entry.set_text("");
                self.syncing_address.set(false);
            }
            SuggestionAction::Url(url) => {
                self.close_address_overlay();
                (self.actions)(ShellAction::Command(AppCommand::SubmitAddress {
                    input: url.as_str().to_owned(),
                    disposition,
                    shift_pressed: false,
                }));
            }
            SuggestionAction::Command(cmd) => {
                self.close_address_overlay();
                (self.actions)(ShellAction::Command(AppCommand::ExecuteAddressCommand(cmd)));
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn rebuild_site_card(&self, snapshot: &UiSnapshot) {
        for child in self.site_card_box.children() {
            self.site_card_box.remove(&child);
        }
        let Some(card) = &snapshot.site_card else {
            let lbl = gtk::Label::new(Some("Blank / Search internal page"));
            lbl.style_context().add_class("searlx-section-label");
            self.site_card_box.pack_start(&lbl, false, false, 4);
            self.site_card_box.show_all();
            return;
        };

        // Header
        let hdr_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let hdr = gtk::Label::new(Some(&card.header));
        hdr.style_context().add_class("searlx-site-card-title");
        hdr.set_halign(gtk::Align::Start);
        hdr.set_ellipsize(gtk::pango::EllipsizeMode::End);
        hdr_row.pack_start(&hdr, true, true, 0);
        if card.is_lookalike {
            let warn = gtk::Label::new(Some("Lookalike IDN"));
            warn.style_context().add_class("searlx-crash-badge");
            hdr_row.pack_end(&warn, false, false, 0);
        }
        self.site_card_box.pack_start(&hdr_row, false, false, 0);

        // Connection row + Certificate inspection
        let conn_label = match card.connection {
            ConnectionState::Secure => "● Connection is secure",
            ConnectionState::MixedContent => "⚠ Mixed content (partially insecure)",
            ConnectionState::NotSecure => "⚠ Not secure (plain HTTP)",
            ConnectionState::UntrustedCertificate => "✕ Untrusted certificate",
            ConnectionState::LocalOrInternal => "● Local or internal page",
        };
        let conn_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let conn_lbl = gtk::Label::new(Some(conn_label));
        conn_lbl.style_context().add_class("searlx-section-label");
        conn_lbl.set_halign(gtk::Align::Start);
        conn_row.pack_start(&conn_lbl, true, true, 0);
        if let Some(cert_label) = &card.certificate_action {
            let cert_btn = gtk::Button::with_label(cert_label);
            cert_btn.style_context().add_class("searlx-site-card-btn");
            let box_clone = self.site_card_box.clone();
            let card_clone = card.clone();
            let flag = self.site_cert_expanded.clone();
            cert_btn.connect_clicked(move |_| {
                let next = !flag.get();
                flag.set(next);
                if next {
                    let info = gtk::Label::new(Some(&format!(
                        "Host: {} · State: {:?}",
                        card_clone.header, card_clone.connection
                    )));
                    info.style_context().add_class("searlx-section-label");
                    info.set_halign(gtk::Align::Start);
                    box_clone.pack_start(&info, false, false, 2);
                    info.show();
                }
            });
            conn_row.pack_end(&cert_btn, false, false, 0);
        }
        self.site_card_box.pack_start(&conn_row, false, false, 0);

        // Copy Address + Print
        let actions_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let copy_btn =
            gtk::Button::with_label(&format!("Copy Address ({})", card.copy_address_shortcut));
        copy_btn.style_context().add_class("searlx-site-card-btn");
        copy_btn.set_hexpand(true);
        {
            let sink = self.actions.clone();
            let pop = self.site_card_popover.clone();
            copy_btn.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::CopyAddress { tab: None }));
                pop.popdown();
            });
        }
        actions_row.pack_start(&copy_btn, true, true, 0);
        if card.show_print {
            let print_btn = gtk::Button::with_label("Print…");
            print_btn.style_context().add_class("searlx-site-card-btn");
            let sink = self.actions.clone();
            let pop = self.site_card_popover.clone();
            print_btn.connect_clicked(move |_| {
                pop.popdown();
                sink(ShellAction::PrintActiveTab);
            });
            actions_row.pack_end(&print_btn, false, false, 0);
        }
        self.site_card_box.pack_start(&actions_row, false, false, 0);

        // Zoom row (`zoom_min..=zoom_max`, 40-300% or 20-600% PDF)
        let zoom_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let z_lbl = gtk::Label::new(Some("Zoom"));
        z_lbl.set_halign(gtk::Align::Start);
        let z_out = gtk::Button::with_label("−");
        z_out.style_context().add_class("searlx-site-card-btn");
        z_out.set_tooltip_text(Some("Ctrl+-"));
        z_out.set_sensitive(card.zoom_percent > card.zoom_min);
        {
            let sink = self.actions.clone();
            z_out.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::ZoomStep {
                    delta_steps: -1,
                }));
            });
        }
        let z_reset = gtk::Button::with_label(&format!("{}%", card.zoom_percent));
        z_reset.style_context().add_class("searlx-site-card-btn");
        z_reset.set_tooltip_text(Some("Actual Size (Ctrl+0)"));
        {
            let sink = self.actions.clone();
            z_reset.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::ZoomReset));
            });
        }
        let z_in = gtk::Button::with_label("+");
        z_in.style_context().add_class("searlx-site-card-btn");
        z_in.set_tooltip_text(Some("Ctrl++"));
        z_in.set_sensitive(card.zoom_percent < card.zoom_max);
        {
            let sink = self.actions.clone();
            z_in.connect_clicked(move |_| {
                sink(ShellAction::Command(AppCommand::ZoomStep {
                    delta_steps: 1,
                }));
            });
        }
        zoom_row.pack_start(&z_lbl, true, true, 0);
        zoom_row.pack_end(&z_in, false, false, 0);
        zoom_row.pack_end(&z_reset, false, false, 0);
        zoom_row.pack_end(&z_out, false, false, 0);
        self.site_card_box.pack_start(&zoom_row, false, false, 0);

        // Ad/tracker blocker toggle
        if let Some(enabled) = card.block_ads_and_trackers {
            self.site_card_box.pack_start(
                &make_card_switch_row(
                    "Block Ads and Trackers",
                    enabled,
                    &self.actions,
                    ShellAction::Command(AppCommand::ToggleSiteBlocker { site_host: None }),
                ),
                false,
                false,
                0,
            );
        }

        // Darken This Site
        if let Some(dusk_on) = card.darken_this_site {
            self.site_card_box.pack_start(
                &make_card_switch_row(
                    &format!("Darken This Site ({})", card.darken_shortcut),
                    dusk_on,
                    &self.actions,
                    ShellAction::Command(AppCommand::ToggleDuskForSite),
                ),
                false,
                false,
                0,
            );
        }

        // Play Sound by Itself
        if let Some(autoplay_on) = card.play_sound_by_itself {
            self.site_card_box.pack_start(
                &make_card_switch_row(
                    "Play Sound by Itself (from next page)",
                    autoplay_on,
                    &self.actions,
                    ShellAction::Command(AppCommand::ToggleAutoplaySound),
                ),
                false,
                false,
                0,
            );
        }

        // Don't Float Videos Here
        if let Some(no_float) = card.dont_float_videos_here {
            self.site_card_box.pack_start(
                &make_card_switch_row(
                    "Don't Float Videos Here",
                    no_float,
                    &self.actions,
                    ShellAction::Command(AppCommand::ToggleDontFloatVideos),
                ),
                false,
                false,
                0,
            );
        }

        // Keep This Site Awake
        if let Some(awake) = card.keep_this_site_awake {
            self.site_card_box.pack_start(
                &make_card_switch_row(
                    "Keep This Site Awake",
                    awake,
                    &self.actions,
                    ShellAction::Command(AppCommand::ToggleKeepAwake),
                ),
                false,
                false,
                0,
            );
        }

        // Site Permissions (`http`/`https`)
        if card.show_site_permissions
            && let Some(perms) = &card.site_permissions
            && let Some(active_tab) = snapshot
                .active_tab
                .and_then(|aid| snapshot.tabs.iter().find(|t| t.id == aid))
            && let Some(origin) = active_tab.url.origin()
        {
            self.site_card_box.pack_start(
                &gtk::Separator::new(gtk::Orientation::Horizontal),
                false,
                false,
                2,
            );
            let p_hdr = gtk::Label::new(Some("Site Permissions"));
            p_hdr.style_context().add_class("searlx-section-label");
            p_hdr.set_halign(gtk::Align::Start);
            self.site_card_box.pack_start(&p_hdr, false, false, 0);

            if let Some(notice) = &card.permissions_notice {
                let n_lbl = gtk::Label::new(Some(notice));
                n_lbl.style_context().add_class("searlx-section-label");
                n_lbl.set_line_wrap(true);
                n_lbl.set_halign(gtk::Align::Start);
                self.site_card_box.pack_start(&n_lbl, false, false, 0);
            }

            if !perms.conflicting_origins.is_empty() {
                let conflict_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                for keep in &perms.conflicting_origins {
                    let b = gtk::Button::with_label(&format!("Keep {keep}"));
                    b.style_context().add_class("searlx-site-card-btn");
                    let u = active_tab.url.clone();
                    let k = keep.clone();
                    let sink = self.actions.clone();
                    b.connect_clicked(move |_| {
                        sink(ShellAction::Command(
                            AppCommand::ResolvePermissionPortConflict {
                                url: u.clone(),
                                keep_origin: k.clone(),
                            },
                        ));
                    });
                    conflict_row.pack_start(&b, false, false, 0);
                }
                self.site_card_box
                    .pack_start(&conflict_row, false, false, 0);
            }

            for (label, kind, current, disable_allow) in [
                ("Camera", EnginePermissionKind::Camera, perms.camera, false),
                (
                    "Microphone",
                    EnginePermissionKind::Microphone,
                    perms.microphone,
                    false,
                ),
                (
                    "Location",
                    EnginePermissionKind::Geolocation,
                    perms.location,
                    false,
                ),
                (
                    "Notifications",
                    EnginePermissionKind::Notifications,
                    perms.notifications,
                    card.notifications_allow_disabled,
                ),
            ] {
                self.site_card_box.pack_start(
                    &make_permission_row(
                        label,
                        active_tab.id,
                        &origin,
                        kind,
                        current,
                        disable_allow,
                        &self.actions,
                    ),
                    false,
                    false,
                    0,
                );
            }
        }

        // Subtle extra page tools row at bottom (Hide Elements + Site Style + Reader)
        self.site_card_box.pack_start(
            &gtk::Separator::new(gtk::Orientation::Horizontal),
            false,
            false,
            2,
        );
        let tools_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let reader_btn = gtk::Button::with_label("Reader");
        reader_btn.style_context().add_class("searlx-site-card-btn");
        {
            let sink = self.actions.clone();
            let pop = self.site_card_popover.clone();
            reader_btn.connect_clicked(move |_| {
                pop.popdown();
                sink(ShellAction::ToggleReaderMode);
            });
        }
        let hide_btn = gtk::Button::with_label("Hide Element");
        hide_btn.style_context().add_class("searlx-site-card-btn");
        {
            let sink = self.actions.clone();
            let pop = self.site_card_popover.clone();
            hide_btn.connect_clicked(move |_| {
                pop.popdown();
                sink(ShellAction::ToggleHideElementsPicker);
            });
        }
        tools_row.pack_start(&reader_btn, true, true, 0);
        tools_row.pack_start(&hide_btn, true, true, 0);
        if snapshot.settings.sites_styles {
            let css_btn = gtk::Button::with_label("Site CSS");
            css_btn.style_context().add_class("searlx-site-card-btn");
            let sink = self.actions.clone();
            let pop = self.site_card_popover.clone();
            css_btn.connect_clicked(move |_| {
                pop.popdown();
                sink(ShellAction::OpenSiteCssDialog);
            });
            tools_row.pack_start(&css_btn, true, true, 0);
        }
        self.site_card_box.pack_start(&tools_row, false, false, 0);

        self.site_card_box.show_all();
    }

    fn rebuild_downloads_popover(&self, snapshot: &UiSnapshot) {
        for child in self.downloads_box.children() {
            self.downloads_box.remove(&child);
        }
        let hdr = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let title = gtk::Label::new(Some("Downloads"));
        title.style_context().add_class("searlx-site-card-title");
        title.set_halign(gtk::Align::Start);
        let open_all = gtk::Button::with_label("View All");
        open_all.style_context().add_class("searlx-site-card-btn");
        {
            let sink = self.actions.clone();
            let pop = self.downloads_popover.clone();
            open_all.connect_clicked(move |_| {
                pop.popdown();
                sink(ShellAction::OpenInternal(InternalPageKind::Downloads, None));
            });
        }
        hdr.pack_start(&title, true, true, 0);
        hdr.pack_end(&open_all, false, false, 0);
        self.downloads_box.pack_start(&hdr, false, false, 0);

        if snapshot.downloads.entries.is_empty() {
            let empty = gtk::Label::new(Some("No downloads yet."));
            empty.style_context().add_class("searlx-section-label");
            self.downloads_box.pack_start(&empty, false, false, 6);
        } else {
            for entry in snapshot.downloads.entries.iter().take(6) {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let name = gtk::Label::new(Some(&entry.file_name));
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                name.set_halign(gtk::Align::Start);
                let state = match &entry.state {
                    DownloadState::InProgress => format!("{} B", entry.bytes_received),
                    DownloadState::Paused => "Paused".into(),
                    DownloadState::Completed => "Done".into(),
                    DownloadState::Failed { .. } => "Failed".into(),
                    DownloadState::Cancelled => "Cancelled".into(),
                };
                let st_lbl = gtk::Label::new(Some(&state));
                st_lbl.style_context().add_class("searlx-section-label");
                row.pack_start(&name, true, true, 0);
                row.pack_end(&st_lbl, false, false, 0);
                self.downloads_box.pack_start(&row, false, false, 0);
            }
        }
        self.downloads_box.show_all();
    }

    fn sync_pending_prompt(&self, snapshot: &UiSnapshot) {
        let sig = format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}",
            snapshot.pending_permission,
            snapshot.pending_dialog,
            snapshot.pending_auth,
            snapshot.pending_cert_error,
            snapshot.pending_password_offer,
        );
        if *self.last_prompt_sig.borrow() == sig {
            return;
        }
        *self.last_prompt_sig.borrow_mut() = sig;

        for c in self.prompt_card.children() {
            self.prompt_card.remove(&c);
        }

        if let Some(perm) = &snapshot.pending_permission {
            let lbl = gtk::Label::new(Some(&format!(
                "{} requests {:?} access",
                perm.origin, perm.kind
            )));
            let allow = gtk::Button::with_label("Allow");
            allow.style_context().add_class("searlx-primary-btn");
            let refuse = gtk::Button::with_label("Don't Allow");
            refuse.style_context().add_class("searlx-site-card-btn");
            let (t, o, k) = (perm.tab, perm.origin.clone(), perm.kind);
            let s1 = self.actions.clone();
            allow.connect_clicked(move |_| {
                s1(ShellAction::Command(AppCommand::DecidePermission {
                    tab: t,
                    origin: o.clone(),
                    kind: k,
                    decision: PermissionDecision::Allow,
                }));
            });
            let (t2, o2, k2) = (perm.tab, perm.origin.clone(), perm.kind);
            let s2 = self.actions.clone();
            refuse.connect_clicked(move |_| {
                s2(ShellAction::Command(AppCommand::DecidePermission {
                    tab: t2,
                    origin: o2.clone(),
                    kind: k2,
                    decision: PermissionDecision::Refuse,
                }));
            });
            self.prompt_card.pack_start(&lbl, false, false, 0);
            self.prompt_card.pack_start(&refuse, false, false, 0);
            self.prompt_card.pack_start(&allow, false, false, 0);
            reveal_widget(&self.prompt_card);
            return;
        }

        if let Some(pw) = &snapshot.pending_password_offer {
            let verb = if pw.is_update { "Update" } else { "Save" };
            let lbl = gtk::Label::new(Some(&format!(
                "{verb} password for {} on {}?",
                pw.username, pw.origin
            )));
            let save_b = gtk::Button::with_label(verb);
            save_b.style_context().add_class("searlx-primary-btn");
            let never_b = gtk::Button::with_label("Never Here");
            never_b.style_context().add_class("searlx-site-card-btn");
            let dismiss_b = gtk::Button::with_label("Not Now");
            dismiss_b.style_context().add_class("searlx-site-card-btn");
            let t = pw.tab;
            let s1 = self.actions.clone();
            save_b.connect_clicked(move |_| {
                s1(ShellAction::Command(AppCommand::SavePasswordOffer {
                    tab: t,
                    accept: true,
                    never_for_host: false,
                }));
            });
            let s2 = self.actions.clone();
            never_b.connect_clicked(move |_| {
                s2(ShellAction::Command(AppCommand::SavePasswordOffer {
                    tab: t,
                    accept: false,
                    never_for_host: true,
                }));
            });
            let s3 = self.actions.clone();
            dismiss_b.connect_clicked(move |_| {
                s3(ShellAction::Command(AppCommand::SavePasswordOffer {
                    tab: t,
                    accept: false,
                    never_for_host: false,
                }));
            });
            self.prompt_card.pack_start(&lbl, false, false, 0);
            self.prompt_card.pack_start(&never_b, false, false, 0);
            self.prompt_card.pack_start(&dismiss_b, false, false, 0);
            self.prompt_card.pack_start(&save_b, false, false, 0);
            reveal_widget(&self.prompt_card);
            return;
        }

        if let Some(cert) = &snapshot.pending_cert_error {
            let lbl = gtk::Label::new(Some(&format!(
                "Untrusted certificate on {}: {}",
                cert.url.display(),
                cert.reason
            )));
            let back_b = gtk::Button::with_label("Go Back");
            back_b.style_context().add_class("searlx-primary-btn");
            let t = cert.tab;
            let s1 = self.actions.clone();
            back_b.connect_clicked(move |_| {
                s1(ShellAction::Command(
                    AppCommand::ProceedWithUntrustedCertificate {
                        tab: t,
                        proceed: false,
                    },
                ));
            });
            self.prompt_card.pack_start(&lbl, false, false, 0);
            self.prompt_card.pack_start(&back_b, false, false, 0);
            reveal_widget(&self.prompt_card);
            return;
        }

        if let Some(dlg) = &snapshot.pending_dialog {
            let lbl = gtk::Label::new(Some(&dlg.message));
            let prompt_entry = gtk::Entry::new();
            let ok_b = gtk::Button::with_label("OK");
            ok_b.style_context().add_class("searlx-primary-btn");
            let cancel_b = gtk::Button::with_label("Cancel");
            cancel_b.style_context().add_class("searlx-site-card-btn");
            let t = dlg.tab;
            let is_prompt = dlg.kind == EngineDialogKind::Prompt;
            let pe = prompt_entry.clone();
            let s1 = self.actions.clone();
            ok_b.connect_clicked(move |_| {
                s1(ShellAction::Command(AppCommand::RespondJsDialog {
                    tab: t,
                    accept: true,
                    prompt_text: is_prompt.then(|| pe.text().to_string()),
                }));
            });
            let s2 = self.actions.clone();
            cancel_b.connect_clicked(move |_| {
                s2(ShellAction::Command(AppCommand::RespondJsDialog {
                    tab: t,
                    accept: false,
                    prompt_text: None,
                }));
            });
            self.prompt_card.pack_start(&lbl, false, false, 0);
            if is_prompt {
                self.prompt_card.pack_start(&prompt_entry, false, false, 0);
            }
            if dlg.kind != EngineDialogKind::Alert {
                self.prompt_card.pack_start(&cancel_b, false, false, 0);
            }
            self.prompt_card.pack_start(&ok_b, false, false, 0);
            reveal_widget(&self.prompt_card);
            return;
        }

        self.prompt_card.hide();
    }

    fn sync_peek_and_switcher(&self, snapshot: &UiSnapshot) {
        for c in self.peek_card.children() {
            self.peek_card.remove(&c);
        }
        if let Some(peek) = &snapshot.peek {
            let hdr = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let lbl = gtk::Label::new(Some(&format!("Peek · {}", peek.peek_url.display())));
            lbl.set_hexpand(true);
            lbl.set_halign(gtk::Align::Start);
            let keep_tab = gtk::Button::with_label("Keep as Tab");
            keep_tab.style_context().add_class("searlx-primary-btn");
            let s1 = self.actions.clone();
            keep_tab.connect_clicked(move |_| {
                s1(ShellAction::Command(AppCommand::KeepPeekAsTab));
            });
            let keep_split = gtk::Button::with_label("Keep in Split");
            keep_split.style_context().add_class("searlx-site-card-btn");
            let s2 = self.actions.clone();
            keep_split.connect_clicked(move |_| {
                s2(ShellAction::Command(AppCommand::KeepPeekInSplit));
            });
            let close_b = gtk::Button::with_label("×");
            close_b.style_context().add_class("searlx-nav-btn");
            let s3 = self.actions.clone();
            close_b.connect_clicked(move |_| {
                s3(ShellAction::Command(AppCommand::DismissPeek));
            });
            hdr.pack_start(&lbl, true, true, 0);
            hdr.pack_end(&close_b, false, false, 0);
            hdr.pack_end(&keep_split, false, false, 0);
            hdr.pack_end(&keep_tab, false, false, 0);
            self.peek_card.pack_start(&hdr, false, false, 0);
            reveal_widget(&self.peek_card);
        } else {
            self.peek_card.hide();
        }

        for c in self.switcher_card.children() {
            self.switcher_card.remove(&c);
        }
        if let Some(sw) = &snapshot.tab_switcher
            && sw.grid_visible
        {
            let title = gtk::Label::new(Some("Recent Tabs (Ctrl+Tab)"));
            title.style_context().add_class("searlx-site-card-title");
            self.switcher_card.pack_start(&title, false, false, 0);

            let grid = gtk::Grid::new();
            grid.set_row_spacing(8);
            grid.set_column_spacing(8);
            for (i, tid) in sw.cards.iter().enumerate() {
                if let Some(tab) = snapshot.tabs.iter().find(|t| t.id == *tid) {
                    let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
                    tile.style_context().add_class("searlx-switcher-tile");
                    if i == sw.selected_index {
                        tile.style_context().add_class("selected");
                    }
                    let t_lbl = gtk::Label::new(Some(&tab.title));
                    t_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    let u_lbl = gtk::Label::new(Some(&tab.display_url));
                    u_lbl.style_context().add_class("searlx-section-label");
                    u_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    tile.pack_start(&t_lbl, false, false, 0);
                    tile.pack_start(&u_lbl, false, false, 0);
                    grid.attach(&tile, (i % 5) as i32, (i / 5) as i32, 1, 1);
                }
            }
            self.switcher_card.pack_start(&grid, false, false, 0);
            reveal_widget(&self.switcher_card);
        } else {
            self.switcher_card.hide();
        }
    }
}

fn make_card_switch_row(
    label: &str,
    active: bool,
    actions: &ActionSink,
    action: ShellAction,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_halign(gtk::Align::Start);
    lbl.set_hexpand(true);
    let sw = gtk::Switch::new();
    sw.set_active(active);
    let sink = actions.clone();
    sw.connect_state_set(move |_sw, _state| {
        sink(action.clone());
        glib::Propagation::Proceed
    });
    row.pack_start(&lbl, true, true, 0);
    row.pack_end(&sw, false, false, 0);
    row
}

fn make_permission_row(
    label: &str,
    tab: searlx_core::TabId,
    origin: &str,
    kind: EnginePermissionKind,
    current: PermissionDecision,
    disable_allow: bool,
    actions: &ActionSink,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_halign(gtk::Align::Start);
    lbl.set_hexpand(true);

    let combo = gtk::ComboBoxText::new();
    combo.append(Some("ask"), "Ask");
    if !disable_allow {
        combo.append(Some("allow"), "Allow");
    }
    combo.append(Some("refuse"), "Refuse");
    let active_id = match current {
        PermissionDecision::Ask => "ask",
        PermissionDecision::Allow | PermissionDecision::AllowOnce => {
            if disable_allow {
                "ask"
            } else {
                "allow"
            }
        }
        PermissionDecision::Refuse => "refuse",
    };
    combo.set_active_id(Some(active_id));

    let o = origin.to_owned();
    let sink = actions.clone();
    combo.connect_changed(move |c| {
        let decision = match c.active_id().as_deref() {
            Some("allow") => PermissionDecision::Allow,
            Some("refuse") => PermissionDecision::Refuse,
            _ => PermissionDecision::Ask,
        };
        sink(ShellAction::Command(AppCommand::DecidePermission {
            tab,
            origin: o.clone(),
            kind,
            decision,
        }));
    });
    row.pack_start(&lbl, true, true, 0);
    row.pack_end(&combo, false, false, 0);
    row
}

fn describe_suggestion(sug: &SuggestionRow) -> (&'static str, String, String, SuggestionAction) {
    match sug {
        SuggestionRow::OpenTab { tab, title, url } => (
            "Tab",
            title.clone(),
            url.display(),
            SuggestionAction::ActivateTab(*tab),
        ),
        SuggestionRow::SiteSearchOffer { site } => (
            "Tab ↹",
            format!("Search {}", site.name),
            site.host.clone(),
            SuggestionAction::LockSite(site.clone()),
        ),
        SuggestionRow::PastSiteSearch {
            site_name,
            query,
            url,
            ..
        } => (
            "Past",
            format!("{site_name}: {query}"),
            url.display(),
            SuggestionAction::Url(url.clone()),
        ),
        SuggestionRow::History { title, url, .. } => (
            "History",
            if title.is_empty() {
                url.display()
            } else {
                title.clone()
            },
            url.display(),
            SuggestionAction::Url(url.clone()),
        ),
        SuggestionRow::Bookmark { title, url } => (
            "Bookmark",
            title.clone(),
            url.as_ref()
                .map_or_else(|| "Folder".into(), PageUrl::display),
            url.as_ref().map_or_else(
                || SuggestionAction::Command(searlx_core::AddressCommand::Bookmarks),
                |u| SuggestionAction::Url(u.clone()),
            ),
        ),
        SuggestionRow::ICloudTab {
            device_name,
            title,
            url,
        } => (
            "Device",
            format!("{title} ({device_name})"),
            url.display(),
            SuggestionAction::Url(url.clone()),
        ),
        SuggestionRow::KnownSite { title, url } => (
            "Site",
            title.clone(),
            url.display(),
            SuggestionAction::Url(url.clone()),
        ),
        SuggestionRow::EngineSuggestion { query, url } => (
            "Suggest",
            query.clone(),
            String::new(),
            SuggestionAction::Url(url.clone()),
        ),
        SuggestionRow::InternalPage { label, url, .. } => (
            "Search",
            label.clone(),
            url.display(),
            SuggestionAction::Url(url.clone()),
        ),
        SuggestionRow::Command(cmd) => (
            "Command",
            cmd.label().to_owned(),
            String::new(),
            SuggestionAction::Command(*cmd),
        ),
        SuggestionRow::SearchWords { engine, query, url } => (
            "Search",
            format!("Search {engine} for \"{query}\""),
            String::new(),
            SuggestionAction::Url(url.clone()),
        ),
    }
}

fn format_invalid_reason(reason: &AddressInvalidReason) -> String {
    match reason {
        AddressInvalidReason::Empty => "Type an address or search term".into(),
        AddressInvalidReason::EmailOrUserinfo => "Addresses cannot contain '@' or userinfo".into(),
        AddressInvalidReason::UnsupportedScheme(s) => format!("Unsupported scheme: {s}"),
        AddressInvalidReason::MissingLocalFile(p) => format!("File not found: {}", p.display()),
        AddressInvalidReason::MalformedAddress(m) => format!("Invalid address: {m}"),
    }
}
