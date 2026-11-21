//! Production GTK-native + WebKitGTK browser shell for Linux (`ADR-0002`, `P5-01`).
//!
//! Wires `searlx-core` (`resolve_address`, `DataStore`, `HistoryManager`,
//! `BookmarksManager`, `Settings`, `StoreCompositor`, `INTERNAL_PAGE_CSP`,
//! `DUSK_FILTER_FALLBACK_CSS`) to real `webkit2gtk::WebView` instances inside a
//! GTK shell with:
//! - Left sidebar tab column (foldable via `Ctrl+S`, with pinned tabs, loose tabs,
//!   space switcher, private-tab badge, and `+` new tab button),
//! - Single unified address & search field (`Ctrl+L`) with `searlx-core` parsing,
//!   keyword site searches (`yt`, `gh`, `wiki`, `ddg`, etc.), and internal `search://` pages,
//! - Live multi-tab `webkit2gtk::WebView` stack (switching tabs never reloads),
//! - Ephemeral `WebContext` for Private Tabs (`Ctrl+Shift+N`) enforced by `StoreCompositor`,
//! - Resizable 2-pane Split View (`Ctrl+Alt+N` or Split button),
//! - Floating Find-on-Page pill (`Ctrl+F`) backed by `WebKitFindController`,
//! - Dusk page darkening (`Ctrl+Shift+D`), Reader view (`Ctrl+Shift+R`), Bookmarks (`Ctrl+Shift+B`),
//!   and page zoom (`Ctrl++`, `Ctrl+-`, `Ctrl+0`).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use webkit2gtk::{
    FindControllerExt, FindOptions as WkFindOptions, LoadEvent, SettingsExt, WebContext, WebView,
    WebViewExt,
};

use searlx_core::{
    AddressCommand, AddressResolution, BookmarkItem, BookmarksManager, DUSK_FILTER_FALLBACK_CSS,
    HistoryManager, PageUrl, ParseOptions, Settings, StoreCompositor, TabId, TabIdAllocator,
    data::DataStore, fuzz_and_hardening::INTERNAL_PAGE_CSP, inspect_host_lookalike,
    resolve_address, settings::SettingsPage,
};

const BROWSER_CSS: &str = r#"
window {
    background-color: #141413;
    color: #ECECE8;
}
.searlx-sidebar {
    background-color: #141413;
    border-right: 1px solid #282825;
    padding: 6px;
}
.searlx-topbar {
    background-color: #141413;
    border-bottom: 1px solid #282825;
    padding: 6px 10px;
}
.searlx-address {
    background-color: #1E1E1C;
    color: #ECECE8;
    border: 1px solid #32322E;
    border-radius: 8px;
    padding: 4px 10px;
    min-height: 28px;
}
.searlx-address:focus {
    border-color: #2A6EF2;
}
.searlx-tab-btn {
    background-image: none;
    background-color: transparent;
    color: #A6A69E;
    border: none;
    border-radius: 7px;
    padding: 5px 8px;
}
.searlx-tab-btn:hover {
    background-color: #20201E;
    color: #ECECE8;
}
.searlx-tab-active {
    background-image: none;
    background-color: #232320;
    color: #F4F4F0;
    border: 1px solid #34342F;
    border-radius: 7px;
    padding: 5px 8px;
}
.searlx-chrome-btn {
    background-image: none;
    background-color: #1E1E1C;
    color: #D8D8D2;
    border: 1px solid #2E2E2A;
    border-radius: 7px;
    padding: 4px 9px;
}
.searlx-chrome-btn:hover {
    background-color: #282824;
    color: #FFFFFF;
}
.searlx-findbar {
    background-color: #1E1E1C;
    border-bottom: 1px solid #2E2E2A;
    padding: 5px 10px;
}
.searlx-status-pill {
    background-color: #232320;
    color: #E06C75;
    border-radius: 6px;
    padding: 3px 8px;
}
"#;

struct LiveTab {
    id: TabId,
    title: String,
    address: String,
    is_private: bool,
    pinned: bool,
    dusk_active: bool,
    reader_active: bool,
    zoom: f64,
    space_index: usize,
    webview: WebView,
}

struct BrowserState {
    id_alloc: TabIdAllocator,
    store: DataStore,
    settings: Settings,
    history: HistoryManager,
    bookmarks: BookmarksManager,
    compositor: StoreCompositor,
    ephemeral_context: WebContext,
    spaces: Vec<String>,
    active_space: usize,
    tabs: Vec<LiveTab>,
    active_tab: usize,
    split_partner: Option<usize>,
    sidebar_folded: bool,
    find_visible: bool,

    // GTK widgets
    window: gtk::Window,
    sidebar_box: gtk::Box,
    tabs_list_box: gtk::Box,
    space_button: gtk::Button,
    footer_label: gtk::Label,
    address_entry: gtk::Entry,
    status_label: gtk::Label,
    find_revealer: gtk::Revealer,
    find_entry: gtk::Entry,
    find_status_label: gtk::Label,
    stage_container: gtk::Box,
}

/// Resolve the user's data directory (`$XDG_DATA_HOME/searlx` or `~/.local/share/searlx`).
#[must_use]
pub fn default_data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg).join("searlx");
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("searlx");
    }
    std::env::temp_dir().join("searlx-data")
}

/// Launch the interactive GTK + WebKitGTK browser on Linux (`ADR-0002`, `P5-01`).
///
/// # Errors
/// Returns an error string if GTK cannot connect to an X11/Wayland display.
pub fn run_gtk_browser(initial_urls: &[String]) -> Result<(), String> {
    // Per ADR-0002 and Phase 0 measurements, WebKitGTK on GTK3 has reliable keyboard
    // focus and overlay composition on X11/XWayland, whereas native Wayland leaves
    // WebView focus ungrabbed. Prefer X11/XWayland when DISPLAY is present unless
    // the user explicitly set GDK_BACKEND.
    if std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("DISPLAY").is_some() {
        gdk::set_allowed_backends("x11,*");
    }
    if let Err(err) = gtk::init() {
        return Err(format!(
            "GTK could not initialize display (ensure DISPLAY or WAYLAND_DISPLAY is set): {err}"
        ));
    }

    install_css();

    let data_dir = default_data_dir();
    let store = DataStore::new(&data_dir);
    let settings = Settings::load_from(&store).unwrap_or_default();
    let history = HistoryManager::load_from(&store, None).unwrap_or_default();
    let bookmarks = BookmarksManager::load_from(&store, None).unwrap_or_default();

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("searlx");
    window.set_default_size(1180, 780);
    window.set_size_request(640, 420);

    let root_hbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    // --- Left Sidebar (Tabs + Spaces + Quick actions) ---
    let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    sidebar_box.set_width_request(230);
    sidebar_box.style_context().add_class("searlx-sidebar");

    let sidebar_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let space_button = gtk::Button::with_label("● Personal");
    space_button.style_context().add_class("searlx-chrome-btn");
    space_button.set_tooltip_text(Some("Switch Space (Personal / Work)"));

    let new_tab_btn = gtk::Button::with_label("+");
    new_tab_btn.style_context().add_class("searlx-chrome-btn");
    new_tab_btn.set_tooltip_text(Some("New Tab (Ctrl+T)"));

    let private_tab_btn = gtk::Button::with_label("Private");
    private_tab_btn
        .style_context()
        .add_class("searlx-chrome-btn");
    private_tab_btn.set_tooltip_text(Some("New Private Tab (Ctrl+Shift+N)"));

    sidebar_header.pack_start(&space_button, true, true, 0);
    sidebar_header.pack_start(&private_tab_btn, false, false, 0);
    sidebar_header.pack_start(&new_tab_btn, false, false, 0);

    let tabs_scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    tabs_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    let tabs_list_box = gtk::Box::new(gtk::Orientation::Vertical, 3);
    tabs_scroll.add(&tabs_list_box);

    let sidebar_footer = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let footer_label = gtk::Label::new(Some("1 tab"));
    footer_label.set_halign(gtk::Align::Start);
    let settings_btn = gtk::Button::with_label("Settings");
    settings_btn.style_context().add_class("searlx-chrome-btn");
    let history_btn = gtk::Button::with_label("History");
    history_btn.style_context().add_class("searlx-chrome-btn");
    sidebar_footer.pack_start(&footer_label, true, true, 2);
    sidebar_footer.pack_start(&history_btn, false, false, 0);
    sidebar_footer.pack_start(&settings_btn, false, false, 0);

    sidebar_box.pack_start(&sidebar_header, false, false, 0);
    sidebar_box.pack_start(&tabs_scroll, true, true, 0);
    sidebar_box.pack_start(&sidebar_footer, false, false, 0);

    // --- Main Right Column (Top bar + Find bar + WebView Stage) ---
    let right_vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let topbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    topbar.style_context().add_class("searlx-topbar");

    let fold_btn = gtk::Button::with_label("☰");
    fold_btn.style_context().add_class("searlx-chrome-btn");
    fold_btn.set_tooltip_text(Some("Toggle Sidebar (Ctrl+S)"));

    let back_btn = gtk::Button::with_label("←");
    back_btn.style_context().add_class("searlx-chrome-btn");
    back_btn.set_tooltip_text(Some("Back (Alt+Left)"));

    let fwd_btn = gtk::Button::with_label("→");
    fwd_btn.style_context().add_class("searlx-chrome-btn");
    fwd_btn.set_tooltip_text(Some("Forward (Alt+Right)"));

    let reload_btn = gtk::Button::with_label("↻");
    reload_btn.style_context().add_class("searlx-chrome-btn");
    reload_btn.set_tooltip_text(Some("Reload (Ctrl+R)"));

    let address_entry = gtk::Entry::new();
    address_entry.set_placeholder_text(Some(
        "Address, search, or command (try example.com, yt cats, search://settings)…",
    ));
    address_entry.set_hexpand(true);
    address_entry.style_context().add_class("searlx-address");

    let status_label = gtk::Label::new(None);
    status_label.style_context().add_class("searlx-status-pill");
    status_label.set_no_show_all(true);

    let bookmark_btn = gtk::Button::with_label("☆");
    bookmark_btn.style_context().add_class("searlx-chrome-btn");
    bookmark_btn.set_tooltip_text(Some("Bookmark This Page (Ctrl+Shift+B)"));

    let split_btn = gtk::Button::with_label("Split");
    split_btn.style_context().add_class("searlx-chrome-btn");
    split_btn.set_tooltip_text(Some("Toggle Split View (Ctrl+Alt+N)"));

    let reader_btn = gtk::Button::with_label("Reader");
    reader_btn.style_context().add_class("searlx-chrome-btn");
    reader_btn.set_tooltip_text(Some("Toggle Reader Mode (Ctrl+Shift+R)"));

    let dusk_btn = gtk::Button::with_label("Dusk");
    dusk_btn.style_context().add_class("searlx-chrome-btn");
    dusk_btn.set_tooltip_text(Some("Darken Page (Ctrl+Shift+D)"));

    let find_btn = gtk::Button::with_label("Find");
    find_btn.style_context().add_class("searlx-chrome-btn");
    find_btn.set_tooltip_text(Some("Find on Page (Ctrl+F)"));

    topbar.pack_start(&fold_btn, false, false, 0);
    topbar.pack_start(&back_btn, false, false, 0);
    topbar.pack_start(&fwd_btn, false, false, 0);
    topbar.pack_start(&reload_btn, false, false, 0);
    topbar.pack_start(&address_entry, true, true, 0);
    topbar.pack_start(&status_label, false, false, 0);
    topbar.pack_start(&bookmark_btn, false, false, 0);
    topbar.pack_start(&split_btn, false, false, 0);
    topbar.pack_start(&reader_btn, false, false, 0);
    topbar.pack_start(&dusk_btn, false, false, 0);
    topbar.pack_start(&find_btn, false, false, 0);

    // --- Floating Find Bar (`Ctrl+F`) ---
    let find_revealer = gtk::Revealer::new();
    find_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    let find_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    find_box.style_context().add_class("searlx-findbar");
    let find_entry = gtk::Entry::new();
    find_entry.set_placeholder_text(Some("Find on page…"));
    find_entry.style_context().add_class("searlx-address");
    let find_prev_btn = gtk::Button::with_label("↑");
    find_prev_btn.style_context().add_class("searlx-chrome-btn");
    let find_next_btn = gtk::Button::with_label("↓");
    find_next_btn.style_context().add_class("searlx-chrome-btn");
    let find_status_label = gtk::Label::new(Some(""));
    let find_close_btn = gtk::Button::with_label("×");
    find_close_btn
        .style_context()
        .add_class("searlx-chrome-btn");
    find_box.pack_start(&find_entry, false, false, 0);
    find_box.pack_start(&find_prev_btn, false, false, 0);
    find_box.pack_start(&find_next_btn, false, false, 0);
    find_box.pack_start(&find_status_label, false, false, 6);
    find_box.pack_end(&find_close_btn, false, false, 0);
    find_revealer.add(&find_box);

    // --- WebView Stage ---
    let stage_container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stage_container.set_hexpand(true);
    stage_container.set_vexpand(true);

    right_vbox.pack_start(&topbar, false, false, 0);
    right_vbox.pack_start(&find_revealer, false, false, 0);
    right_vbox.pack_start(&stage_container, true, true, 0);

    root_hbox.pack_start(&sidebar_box, false, false, 0);
    root_hbox.pack_start(&right_vbox, true, true, 0);
    window.add(&root_hbox);

    let ephemeral_context = WebContext::new_ephemeral();

    let state = Rc::new(RefCell::new(BrowserState {
        id_alloc: TabIdAllocator::new(),
        store,
        settings,
        history,
        bookmarks,
        compositor: StoreCompositor::new(),
        ephemeral_context,
        spaces: vec!["Personal".to_owned(), "Work".to_owned()],
        active_space: 0,
        tabs: Vec::new(),
        active_tab: 0,
        split_partner: None,
        sidebar_folded: false,
        find_visible: false,
        window: window.clone(),
        sidebar_box: sidebar_box.clone(),
        tabs_list_box: tabs_list_box.clone(),
        space_button: space_button.clone(),
        footer_label: footer_label.clone(),
        address_entry: address_entry.clone(),
        status_label: status_label.clone(),
        find_revealer: find_revealer.clone(),
        find_entry: find_entry.clone(),
        find_status_label: find_status_label.clone(),
        stage_container: stage_container.clone(),
    }));

    // Create initial tabs
    if initial_urls.is_empty() {
        create_tab(&state, "search://newtab", false);
        create_tab(&state, "https://example.com", false);
        activate_tab(&state, 0);
    } else {
        for url in initial_urls {
            create_tab(&state, url, false);
        }
        activate_tab(&state, 0);
    }

    // Wire address entry submission
    {
        let st = state.clone();
        address_entry.connect_activate(move |entry| {
            let text = entry.text().to_string();
            submit_address_input(&st, &text);
        });
    }

    // Wire topbar & sidebar buttons
    {
        let st = state.clone();
        new_tab_btn.connect_clicked(move |_| {
            let idx = create_tab(&st, "search://newtab", false);
            activate_tab(&st, idx);
            st.borrow().address_entry.grab_focus();
        });
    }
    {
        let st = state.clone();
        private_tab_btn.connect_clicked(move |_| {
            let idx = create_tab(&st, "search://newtab", true);
            activate_tab(&st, idx);
            st.borrow().address_entry.grab_focus();
        });
    }
    {
        let st = state.clone();
        space_button.connect_clicked(move |_| {
            switch_next_space(&st);
        });
    }
    {
        let st = state.clone();
        fold_btn.connect_clicked(move |_| {
            toggle_sidebar(&st);
        });
    }
    {
        let st = state.clone();
        back_btn.connect_clicked(move |_| {
            let b = st.borrow();
            if let Some(tab) = b.tabs.get(b.active_tab) {
                tab.webview.go_back();
            }
        });
    }
    {
        let st = state.clone();
        fwd_btn.connect_clicked(move |_| {
            let b = st.borrow();
            if let Some(tab) = b.tabs.get(b.active_tab) {
                tab.webview.go_forward();
            }
        });
    }
    {
        let st = state.clone();
        reload_btn.connect_clicked(move |_| {
            let b = st.borrow();
            if let Some(tab) = b.tabs.get(b.active_tab) {
                tab.webview.reload();
            }
        });
    }
    {
        let st = state.clone();
        bookmark_btn.connect_clicked(move |_| {
            bookmark_active_tab(&st);
        });
    }
    {
        let st = state.clone();
        split_btn.connect_clicked(move |_| {
            toggle_split_view(&st);
        });
    }
    {
        let st = state.clone();
        reader_btn.connect_clicked(move |_| {
            toggle_reader_on_active(&st);
        });
    }
    {
        let st = state.clone();
        dusk_btn.connect_clicked(move |_| {
            toggle_dusk_on_active(&st);
        });
    }
    {
        let st = state.clone();
        find_btn.connect_clicked(move |_| {
            toggle_find_bar(&st);
        });
    }
    {
        let st = state.clone();
        settings_btn.connect_clicked(move |_| {
            navigate_active_or_open(&st, "search://settings");
        });
    }
    {
        let st = state.clone();
        history_btn.connect_clicked(move |_| {
            navigate_active_or_open(&st, "search://history");
        });
    }

    // Wire Find bar
    {
        let st = state.clone();
        find_entry.connect_changed(move |entry| {
            run_find_query(&st, &entry.text());
        });
    }
    {
        let st = state.clone();
        find_entry.connect_activate(move |_| {
            find_step(&st, true);
        });
    }
    {
        let st = state.clone();
        find_next_btn.connect_clicked(move |_| {
            find_step(&st, true);
        });
    }
    {
        let st = state.clone();
        find_prev_btn.connect_clicked(move |_| {
            find_step(&st, false);
        });
    }
    {
        let st = state.clone();
        find_close_btn.connect_clicked(move |_| {
            close_find_bar(&st);
        });
    }

    // Wire global keyboard shortcuts (`FEATURES.md` §22)
    {
        let st = state.clone();
        window.connect_key_press_event(move |_win, event| handle_key_shortcut(&st, event));
    }

    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Proceed
    });

    window.show_all();
    status_label.hide();
    address_entry.grab_focus();

    gtk::main();
    Ok(())
}

fn install_css() {
    let provider = gtk::CssProvider::new();
    if provider.load_from_data(BROWSER_CSS.as_bytes()).is_ok()
        && let Some(screen) = gdk::Screen::default()
    {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn configure_webview(view: &WebView, settings: &Settings) {
    if let Some(wk) = WebViewExt::settings(view) {
        wk.set_enable_javascript(true);
        wk.set_enable_webgl(true);
        wk.set_enable_media(true);
        wk.set_enable_media_stream(true);
        wk.set_enable_mediasource(true);
        wk.set_enable_webaudio(true);
        wk.set_enable_developer_extras(true);
        wk.set_enable_smooth_scrolling(true);
        wk.set_media_playback_requires_user_gesture(settings.media_click);
    }
}

fn create_tab(state: &Rc<RefCell<BrowserState>>, initial_input: &str, is_private: bool) -> usize {
    let (tab_id, webview, active_space) = {
        let mut b = state.borrow_mut();
        let tab_id = b.id_alloc.allocate();
        b.compositor.register_tab(tab_id, is_private, false);
        let view = if is_private {
            WebView::with_context(&b.ephemeral_context)
        } else {
            WebView::new()
        };
        configure_webview(&view, &b.settings);
        let active_space = b.active_space;
        b.tabs.push(LiveTab {
            id: tab_id,
            title: if is_private {
                "Private Tab".to_owned()
            } else {
                "New Tab".to_owned()
            },
            address: initial_input.to_owned(),
            is_private,
            pinned: false,
            dusk_active: false,
            reader_active: false,
            zoom: 1.0,
            space_index: active_space,
            webview: view.clone(),
        });
        (tab_id, view, active_space)
    };
    let _ = active_space;

    // Listen to title changes from WebKitGTK
    {
        let st = state.clone();
        webview.connect_title_notify(move |view| {
            let new_title = view.title().map(|s| s.to_string()).unwrap_or_default();
            if new_title.is_empty() {
                return;
            }
            let mut b = st.borrow_mut();
            if let Some(tab) = b.tabs.iter_mut().find(|t| t.id == tab_id) {
                tab.title = new_title;
            }
            drop(b);
            rebuild_sidebar_tabs(&st);
        });
    }

    // Listen to load commits/finishes to sync URI and record history (via StoreCompositor)
    {
        let st = state.clone();
        webview.connect_load_changed(move |view, event| {
            if event != LoadEvent::Committed && event != LoadEvent::Finished {
                return;
            }
            let Some(uri_gstr) = view.uri() else {
                return;
            };
            let uri = uri_gstr.to_string();
            if uri.is_empty() || uri == "about:blank" {
                return;
            }

            let mut b = st.borrow_mut();
            let active_id = b.tabs.get(b.active_tab).map(|t| t.id);
            let mut should_record = None;

            if let Some(tab) = b.tabs.iter_mut().find(|t| t.id == tab_id) {
                if !tab.address.starts_with("search://") || !uri.starts_with("about:") {
                    tab.address.clone_from(&uri);
                }
                let title = view
                    .title()
                    .map(|t| t.to_string())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| tab.title.clone());
                tab.title.clone_from(&title);

                if event == LoadEvent::Finished
                    && let Ok(parsed_url) = PageUrl::parse(&uri)
                {
                    should_record = Some((parsed_url, title, tab.is_private));
                }
            }

            if let Some((parsed_url, title, is_priv)) = should_record
                && !is_priv
                && b.compositor.allows_durable_writes(tab_id)
            {
                let b_mut = &mut *b;
                let _ = b_mut.history.record_visit(
                    parsed_url,
                    title,
                    "2026-11-21T12:00:00Z",
                    false,
                    None,
                );
                let _ = b_mut.history.save_to(&b_mut.store, None);
            }

            if active_id == Some(tab_id) {
                let addr = b
                    .tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .map(|t| t.address.clone())
                    .unwrap_or_default();
                b.address_entry.set_text(&addr);
            }
            drop(b);
            rebuild_sidebar_tabs(&st);
        });
    }

    let idx = state.borrow().tabs.len() - 1;
    load_input_into_tab(state, idx, initial_input);
    rebuild_sidebar_tabs(state);
    idx
}

fn activate_tab(state: &Rc<RefCell<BrowserState>>, index: usize) {
    {
        let mut b = state.borrow_mut();
        if index >= b.tabs.len() {
            return;
        }
        b.active_tab = index;
        if b.split_partner == Some(index) {
            b.split_partner = None;
        }
        let addr = b.tabs[index].address.clone();
        let title = b.tabs[index].title.clone();
        b.address_entry.set_text(&addr);
        b.window.set_title(&format!("{title} — searlx"));
        b.status_label.hide();
    }
    rebuild_stage(state);
    rebuild_sidebar_tabs(state);
}

fn close_tab_at(state: &Rc<RefCell<BrowserState>>, index: usize) {
    let mut b = state.borrow_mut();
    if b.tabs.len() <= 1 {
        // Closing the last tab resets it to a fresh blank tab (`FEATURES.md` §3.2)
        b.tabs[0].address = "search://newtab".to_owned();
        b.tabs[0].title = "New Tab".to_owned();
        drop(b);
        load_input_into_tab(state, 0, "search://newtab");
        activate_tab(state, 0);
        return;
    }
    let removed = b.tabs.remove(index);
    let _ = b.compositor.unregister_tab(removed.id);
    if b.split_partner == Some(index) {
        b.split_partner = None;
    } else if let Some(p) = b.split_partner
        && p > index
    {
        b.split_partner = Some(p - 1);
    }
    if b.active_tab >= b.tabs.len() {
        b.active_tab = b.tabs.len() - 1;
    }
    let next_active = b.active_tab;
    drop(b);
    activate_tab(state, next_active);
}

fn rebuild_stage(state: &Rc<RefCell<BrowserState>>) {
    let b = state.borrow();
    for child in b.stage_container.children() {
        b.stage_container.remove(&child);
    }

    let Some(primary) = b.tabs.get(b.active_tab) else {
        return;
    };
    if let Some(parent) = primary.webview.parent()
        && let Ok(container) = parent.downcast::<gtk::Container>()
    {
        container.remove(&primary.webview);
    }

    if let Some(partner_idx) = b.split_partner
        && partner_idx != b.active_tab
        && let Some(partner) = b.tabs.get(partner_idx)
    {
        if let Some(parent) = partner.webview.parent()
            && let Ok(container) = parent.downcast::<gtk::Container>()
        {
            container.remove(&partner.webview);
        }
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_wide_handle(true);
        paned.pack1(&primary.webview, true, true);
        paned.pack2(&partner.webview, true, true);
        let alloc_w = b.stage_container.allocation().width().max(900);
        paned.set_position(alloc_w / 2);
        b.stage_container.pack_start(&paned, true, true, 0);
    } else {
        b.stage_container
            .pack_start(&primary.webview, true, true, 0);
    }
    b.stage_container.show_all();
}

fn rebuild_sidebar_tabs(state: &Rc<RefCell<BrowserState>>) {
    let b = state.borrow();
    for child in b.tabs_list_box.children() {
        b.tabs_list_box.remove(&child);
    }

    let active_space = b.active_space;
    let space_name = b
        .spaces
        .get(active_space)
        .cloned()
        .unwrap_or_else(|| "Personal".to_owned());
    b.space_button.set_label(&format!("● {space_name}"));

    let mut visible_count = 0usize;
    for (idx, tab) in b.tabs.iter().enumerate() {
        if tab.space_index != active_space && !tab.pinned {
            continue;
        }
        visible_count += 1;

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let glyph = tab
            .title
            .chars()
            .find(|c| c.is_alphanumeric())
            .map_or('·', |c| c.to_ascii_uppercase());
        let badge = if tab.is_private {
            " [Private]"
        } else if tab.pinned {
            " 📌"
        } else if b.split_partner == Some(idx) {
            " [Split]"
        } else {
            ""
        };
        let label_text = format!("[{glyph}]  {}{badge}", tab.title);
        let tab_btn = gtk::Button::with_label(&label_text);
        tab_btn.set_hexpand(true);
        if let Some(lbl) = tab_btn
            .child()
            .and_then(|w| w.downcast::<gtk::Label>().ok())
        {
            lbl.set_xalign(0.0);
            lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        }
        tab_btn.style_context().add_class(if idx == b.active_tab {
            "searlx-tab-active"
        } else {
            "searlx-tab-btn"
        });

        let close_btn = gtk::Button::with_label(if tab.pinned { "−" } else { "×" });
        close_btn.style_context().add_class("searlx-tab-btn");

        {
            let st = state.clone();
            tab_btn.connect_clicked(move |_| {
                activate_tab(&st, idx);
            });
        }
        {
            let st = state.clone();
            close_btn.connect_clicked(move |_| {
                close_tab_at(&st, idx);
            });
        }

        row_box.pack_start(&tab_btn, true, true, 0);
        row_box.pack_start(&close_btn, false, false, 0);
        b.tabs_list_box.pack_start(&row_box, false, false, 0);
    }

    b.footer_label.set_text(&format!(
        "{visible_count} tab{}",
        if visible_count == 1 { "" } else { "s" }
    ));
    b.tabs_list_box.show_all();
}

fn submit_address_input(state: &Rc<RefCell<BrowserState>>, raw: &str) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let (opts, active_idx) = {
        let b = state.borrow();
        (
            ParseOptions {
                engine: b.settings.search_engine.clone(),
                commands_enabled: true,
                keywords_enabled: true,
                ..ParseOptions::default()
            },
            b.active_tab,
        )
    };

    match resolve_address(trimmed, &opts) {
        AddressResolution::Navigate(url) => {
            state.borrow().status_label.hide();
            load_page_url_into_tab(state, active_idx, &url);
        }
        AddressResolution::Search { url, query, source } => {
            state.borrow().status_label.hide();
            {
                let mut b = state.borrow_mut();
                if let Some(tab) = b.tabs.get_mut(active_idx) {
                    tab.title = format!("{query} — {source}");
                }
            }
            load_page_url_into_tab(state, active_idx, &url);
        }
        AddressResolution::Command(cmd) => {
            state.borrow().status_label.hide();
            match cmd {
                AddressCommand::NewTab => {
                    let idx = create_tab(state, "search://newtab", false);
                    activate_tab(state, idx);
                }
                AddressCommand::NewPrivateTab => {
                    let idx = create_tab(state, "search://newtab", true);
                    activate_tab(state, idx);
                }
                AddressCommand::Settings => navigate_active_or_open(state, "search://settings"),
                AddressCommand::History => navigate_active_or_open(state, "search://history"),
                AddressCommand::Bookmarks => navigate_active_or_open(state, "search://bookmarks"),
                AddressCommand::Downloads => navigate_active_or_open(state, "search://downloads"),
                AddressCommand::Passwords => navigate_active_or_open(state, "search://passwords"),
                AddressCommand::ToggleSidebar => toggle_sidebar(state),
                AddressCommand::NewSpace => switch_next_space(state),
            }
        }
        AddressResolution::Invalid(_) => {
            let b = state.borrow();
            b.status_label.set_text("Invalid address");
            b.status_label.show();
        }
    }
}

fn navigate_active_or_open(state: &Rc<RefCell<BrowserState>>, target: &str) {
    let active_idx = state.borrow().active_tab;
    load_input_into_tab(state, active_idx, target);
    activate_tab(state, active_idx);
}

fn load_input_into_tab(state: &Rc<RefCell<BrowserState>>, index: usize, raw: &str) {
    if let Ok(url) = PageUrl::parse(raw) {
        load_page_url_into_tab(state, index, &url);
    } else if let Ok(url) = PageUrl::parse("search://newtab") {
        load_page_url_into_tab(state, index, &url);
    }
}

fn load_page_url_into_tab(state: &Rc<RefCell<BrowserState>>, index: usize, url: &PageUrl) {
    let internal_html = if url.is_internal() {
        Some(render_internal_page_html(&state.borrow(), url))
    } else {
        None
    };

    let mut b = state.borrow_mut();
    let is_active = index == b.active_tab;
    let Some(tab) = b.tabs.get_mut(index) else {
        return;
    };
    tab.address = url.as_str().to_owned();
    tab.reader_active = false;

    if let Some((title, html)) = internal_html {
        tab.title = title;
        tab.webview.load_html(&html, Some("about:blank"));
    } else {
        if let Some(host) = url.host() {
            tab.title = inspect_host_lookalike(host).safe_display_host;
        }
        tab.webview.load_uri(url.as_str());
    }

    let addr = tab.address.clone();
    if is_active {
        b.address_entry.set_text(&addr);
    }
    drop(b);
    rebuild_sidebar_tabs(state);
}

fn render_internal_page_html(b: &BrowserState, url: &PageUrl) -> (String, String) {
    let host = url.host().unwrap_or("newtab");
    let style = r#"
        body {
            background: #181816;
            color: #ECECE8;
            font-family: system-ui, -apple-system, sans-serif;
            max-width: 760px;
            margin: 40px auto;
            padding: 0 24px;
            line-height: 1.6;
        }
        h1 { font-size: 24px; font-weight: 600; margin-bottom: 8px; }
        h2 { font-size: 16px; color: #A6A69E; margin-top: 24px; }
        .card {
            background: #21211E;
            border: 1px solid #30302C;
            border-radius: 10px;
            padding: 14px 18px;
            margin-bottom: 10px;
        }
        a { color: #5C9DFF; text-decoration: none; }
        a:hover { text-decoration: underline; }
        code { background: #282824; padding: 2px 6px; border-radius: 4px; }
        .muted { color: #9A9A92; font-size: 13px; }
    "#;

    match host {
        "settings" => {
            let pages_nav: String = SettingsPage::visible_pages(b.settings.ai_shipped)
                .into_iter()
                .map(|p| format!("<code>{}</code>", p.title()))
                .collect::<Vec<_>>()
                .join(" · ");
            let engine_name = format!("{:?}", b.settings.search_engine);
            (
                "Settings".to_owned(),
                format!(
                    "<!doctype html><html><head><meta charset=\"utf-8\">\
                     <meta http-equiv=\"Content-Security-Policy\" content=\"{INTERNAL_PAGE_CSP}\">\
                     <title>Settings</title><style>{style}</style></head><body>\
                     <h1>Settings</h1>\
                     <p class=\"muted\">{pages_nav}</p>\
                     <div class=\"card\"><strong>Search Engine:</strong> {engine_name}<br>\
                     <strong>Ad &amp; Tracker Shield:</strong> {}<br>\
                     <strong>Global Privacy Control (Sec-GPC):</strong> {}<br>\
                     <strong>Strip Click Trackers (gclid/fbclid):</strong> {}<br>\
                     <strong>Sleep Inactive Tabs:</strong> {:?}</div>\
                     <div class=\"card\"><strong>Linux Engine (ADR-0002):</strong> WebKitGTK (GTK-native shell)<br>\
                     <strong>Zero Telemetry / Zero Account:</strong> Enforced</div>\
                     </body></html>",
                    if b.settings.shield { "On" } else { "Off" },
                    if b.settings.privacy_gpc { "On" } else { "Off" },
                    if b.settings.privacy_click_ids {
                        "On"
                    } else {
                        "Off"
                    },
                    b.settings.tabs_sleep_pace,
                ),
            )
        }
        "history" => {
            let mut rows = String::new();
            for visit in b.history.visits().iter().take(40) {
                rows.push_str(&format!(
                    "<div class=\"card\"><a href=\"{u}\">{t}</a><br><span class=\"muted\">{u} · {c} visit(s)</span></div>",
                    u = html_escape(visit.url.as_str()),
                    t = html_escape(&visit.title),
                    c = visit.visit_count
                ));
            }
            if rows.is_empty() {
                rows.push_str("<div class=\"card muted\">No browsing history recorded yet. Private tabs never write history.</div>");
            }
            (
                "History".to_owned(),
                format!(
                    "<!doctype html><html><head><meta charset=\"utf-8\">\
                     <title>History</title><style>{style}</style></head><body>\
                     <h1>History</h1><p class=\"muted\">{} recorded place(s) (2,000 disk cap)</p>{rows}</body></html>",
                    b.history.visits().len()
                ),
            )
        }
        "bookmarks" => {
            let mut rows = String::new();
            for item in b.bookmarks.bar_items() {
                if let BookmarkItem::Link { title, url, .. } = item {
                    rows.push_str(&format!(
                        "<div class=\"card\"><a href=\"{u}\">{t}</a><br><span class=\"muted\">{u}</span></div>",
                        u = html_escape(url.as_str()),
                        t = html_escape(title),
                    ));
                }
            }
            if rows.is_empty() {
                rows.push_str("<div class=\"card muted\">No bookmarks yet. Press ☆ or <code>Ctrl+Shift+B</code> on any page to bookmark it.</div>");
            }
            (
                "Bookmarks".to_owned(),
                format!(
                    "<!doctype html><html><head><meta charset=\"utf-8\">\
                     <title>Bookmarks</title><style>{style}</style></head><body>\
                     <h1>Bookmarks</h1>{rows}</body></html>"
                ),
            )
        }
        "downloads" => (
            "Downloads".to_owned(),
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\">\
                 <meta http-equiv=\"Content-Security-Policy\" content=\"{INTERNAL_PAGE_CSP}\">\
                 <title>Downloads</title><style>{style}</style></head><body>\
                 <h1>Downloads</h1>\
                 <div class=\"card muted\">Destination folder: <code>{}</code><br>Private downloads are never listed here.</div>\
                 </body></html>",
                html_escape(&b.settings.downloads_dir)
            ),
        ),
        "passwords" => (
            "Passwords".to_owned(),
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\">\
                 <meta http-equiv=\"Content-Security-Policy\" content=\"{INTERNAL_PAGE_CSP}\">\
                 <title>Passwords</title><style>{style}</style></head><body>\
                 <h1>Passwords</h1>\
                 <div class=\"card muted\">Backed by OS credential store (<code>org.freedesktop.secrets</code> on Linux).<br>\
                 HTTPS passwords are never offered or filled on plain HTTP origins.</div>\
                 </body></html>"
            ),
        ),
        _ => (
            "New Tab".to_owned(),
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\">\
                 <title>New Tab — searlx</title><style>{style}</style></head><body>\
                 <h1>searlx</h1>\
                 <p class=\"muted\">Fast, quiet browser · WebKitGTK native engine · Zero telemetry</p>\
                 <h2>Quick Places</h2>\
                 <div class=\"card\">\
                 <a href=\"https://example.com\">example.com</a> · \
                 <a href=\"https://en.wikipedia.org\">Wikipedia</a> · \
                 <a href=\"https://news.ycombinator.com\">Hacker News</a> · \
                 <a href=\"https://www.rust-lang.org\">Rust</a> · \
                 <a href=\"https://duckduckgo.com\">DuckDuckGo</a>\
                 </div>\
                 <h2>Keyboard Shortcuts</h2>\
                 <div class=\"card muted\">\
                 <code>Ctrl+L</code> Focus address &amp; search field · \
                 <code>Ctrl+T</code> New tab · \
                 <code>Ctrl+Shift+N</code> New private tab · \
                 <code>Ctrl+W</code> Close tab<br>\
                 <code>Ctrl+S</code> Fold/unfold sidebar · \
                 <code>Ctrl+Alt+N</code> Toggle 2-pane Split View · \
                 <code>Ctrl+F</code> Find on page<br>\
                 <code>Ctrl+Shift+R</code> Reader mode · \
                 <code>Ctrl+Shift+D</code> Darken page (Dusk) · \
                 <code>Ctrl+Shift+B</code> Bookmark page\
                 </div></body></html>"
            ),
        ),
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn switch_next_space(state: &Rc<RefCell<BrowserState>>) {
    let next_tab_idx = {
        let mut b = state.borrow_mut();
        b.active_space = (b.active_space + 1) % b.spaces.len();
        let target_space = b.active_space;
        b.tabs
            .iter()
            .position(|t| t.space_index == target_space || t.pinned)
    };
    if let Some(idx) = next_tab_idx {
        activate_tab(state, idx);
    } else {
        let idx = create_tab(state, "search://newtab", false);
        activate_tab(state, idx);
    }
}

fn toggle_sidebar(state: &Rc<RefCell<BrowserState>>) {
    let mut b = state.borrow_mut();
    b.sidebar_folded = !b.sidebar_folded;
    b.sidebar_box.set_visible(!b.sidebar_folded);
}

fn toggle_split_view(state: &Rc<RefCell<BrowserState>>) {
    {
        let mut b = state.borrow_mut();
        if b.split_partner.is_some() {
            b.split_partner = None;
        } else if b.tabs.len() >= 2 {
            let partner = if b.active_tab + 1 < b.tabs.len() {
                b.active_tab + 1
            } else {
                b.active_tab.saturating_sub(1)
            };
            b.split_partner = Some(partner);
        } else {
            drop(b);
            let partner = create_tab(state, "https://example.com", false);
            state.borrow_mut().split_partner = Some(partner);
        }
    }
    rebuild_stage(state);
    rebuild_sidebar_tabs(state);
}

fn bookmark_active_tab(state: &Rc<RefCell<BrowserState>>) {
    let mut b = state.borrow_mut();
    let active_idx = b.active_tab;
    let Some(tab) = b.tabs.get(active_idx) else {
        return;
    };
    let Ok(url) = PageUrl::parse(&tab.address) else {
        return;
    };
    if url.is_internal() {
        return;
    }
    let title = tab.title.clone();
    let b_mut = &mut *b;
    let _ = b_mut.bookmarks.add_bookmark(None, &title, url);
    let _ = b_mut.bookmarks.save_to(&b_mut.store, None);
    b_mut.status_label.set_text("Bookmarked ★");
    b_mut.status_label.show();
}

fn toggle_dusk_on_active(state: &Rc<RefCell<BrowserState>>) {
    let mut b = state.borrow_mut();
    let active_idx = b.active_tab;
    let Some(tab) = b.tabs.get_mut(active_idx) else {
        return;
    };
    tab.dusk_active = !tab.dusk_active;
    let js = if tab.dusk_active {
        format!(
            "(function(){{let s=document.getElementById('__searlx_dusk');if(!s){{s=document.createElement('style');s.id='__searlx_dusk';document.head.appendChild(s);}}s.textContent={:?};}})();",
            DUSK_FILTER_FALLBACK_CSS
        )
    } else {
        "(function(){let s=document.getElementById('__searlx_dusk');if(s)s.remove();})();"
            .to_owned()
    };
    #[allow(deprecated)]
    tab.webview
        .run_javascript(&js, None::<&gio::Cancellable>, |_| {});
}

fn toggle_reader_on_active(state: &Rc<RefCell<BrowserState>>) {
    let mut b = state.borrow_mut();
    let active_idx = b.active_tab;
    let Some(tab) = b.tabs.get_mut(active_idx) else {
        return;
    };
    tab.reader_active = !tab.reader_active;
    if tab.reader_active {
        let js = r#"
        (function() {
            const root = document.querySelector('article, main, [role="main"]') || document.body;
            const title = document.title || 'Reader';
            const clone = root.cloneNode(true);
            clone.querySelectorAll('script, style, nav, footer, aside, iframe').forEach(e => e.remove());
            document.head.innerHTML = '<meta charset="utf-8"><title>' + title + '</title><style>body{background:#191917;color:#ECECE6;font-family:Georgia,serif;font-size:18px;line-height:1.72;max-width:38em;margin:3rem auto;padding:0 1.5rem}a{color:#6CA6FF}pre,code{font-family:monospace;background:#242420;padding:4px;border-radius:4px;overflow-x:auto}img{max-width:100%;height:auto}</style>';
            document.body.innerHTML = '<h1>' + title + '</h1>' + clone.innerHTML;
        })();
        "#;
        #[allow(deprecated)]
        tab.webview
            .run_javascript(js, None::<&gio::Cancellable>, |_| {});
    } else {
        // Leaving Reader reloads the original page (`FEATURES.md` §5.2)
        tab.webview.reload();
    }
}

fn toggle_find_bar(state: &Rc<RefCell<BrowserState>>) {
    let mut b = state.borrow_mut();
    b.find_visible = !b.find_visible;
    b.find_revealer.set_reveal_child(b.find_visible);
    if b.find_visible {
        b.find_entry.grab_focus();
    } else if let Some(tab) = b.tabs.get(b.active_tab)
        && let Some(fc) = tab.webview.find_controller()
    {
        fc.search_finish();
    }
}

fn close_find_bar(state: &Rc<RefCell<BrowserState>>) {
    let mut b = state.borrow_mut();
    b.find_visible = false;
    b.find_revealer.set_reveal_child(false);
    b.find_status_label.set_text("");
    if let Some(tab) = b.tabs.get(b.active_tab)
        && let Some(fc) = tab.webview.find_controller()
    {
        fc.search_finish();
    }
}

fn run_find_query(state: &Rc<RefCell<BrowserState>>, query: &str) {
    let b = state.borrow();
    let Some(tab) = b.tabs.get(b.active_tab) else {
        return;
    };
    let Some(fc) = tab.webview.find_controller() else {
        return;
    };
    if query.trim().is_empty() {
        fc.search_finish();
        b.find_status_label.set_text("");
    } else {
        let opts = WkFindOptions::CASE_INSENSITIVE | WkFindOptions::WRAP_AROUND;
        fc.search(query, opts.bits(), 1_000);
        b.find_status_label.set_text("Searching…");
    }
}

fn find_step(state: &Rc<RefCell<BrowserState>>, forward: bool) {
    let b = state.borrow();
    let Some(tab) = b.tabs.get(b.active_tab) else {
        return;
    };
    let Some(fc) = tab.webview.find_controller() else {
        return;
    };
    if forward {
        fc.search_next();
    } else {
        fc.search_previous();
    }
}

fn adjust_zoom(state: &Rc<RefCell<BrowserState>>, delta: Option<f64>) {
    let mut b = state.borrow_mut();
    let active_idx = b.active_tab;
    let Some(tab) = b.tabs.get_mut(active_idx) else {
        return;
    };
    tab.zoom = match delta {
        Some(d) => (tab.zoom + d).clamp(0.4, 3.0),
        None => 1.0,
    };
    tab.webview.set_zoom_level(tab.zoom);
}

fn handle_key_shortcut(
    state: &Rc<RefCell<BrowserState>>,
    event: &gdk::EventKey,
) -> glib::Propagation {
    let mods = event.state();
    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
    let alt = mods.contains(gdk::ModifierType::MOD1_MASK);
    let key = event.keyval().to_unicode().map(|c| c.to_ascii_lowercase());

    if event.keyval() == gdk::keys::constants::Escape && state.borrow().find_visible {
        close_find_bar(state);
        return glib::Propagation::Stop;
    }

    if ctrl && !alt {
        match (shift, key) {
            (false, Some('t')) => {
                let idx = create_tab(state, "search://newtab", false);
                activate_tab(state, idx);
                state.borrow().address_entry.grab_focus();
                return glib::Propagation::Stop;
            }
            (true, Some('n')) => {
                let idx = create_tab(state, "search://newtab", true);
                activate_tab(state, idx);
                state.borrow().address_entry.grab_focus();
                return glib::Propagation::Stop;
            }
            (false, Some('w')) => {
                let active = state.borrow().active_tab;
                close_tab_at(state, active);
                return glib::Propagation::Stop;
            }
            (false, Some('l')) => {
                let b = state.borrow();
                b.address_entry.grab_focus();
                b.address_entry.select_region(0, -1);
                return glib::Propagation::Stop;
            }
            (false, Some('s')) => {
                toggle_sidebar(state);
                return glib::Propagation::Stop;
            }
            (false, Some('f')) => {
                toggle_find_bar(state);
                return glib::Propagation::Stop;
            }
            (false, Some('r')) => {
                let b = state.borrow();
                if let Some(tab) = b.tabs.get(b.active_tab) {
                    tab.webview.reload();
                }
                return glib::Propagation::Stop;
            }
            (true, Some('r')) => {
                toggle_reader_on_active(state);
                return glib::Propagation::Stop;
            }
            (true, Some('d')) => {
                toggle_dusk_on_active(state);
                return glib::Propagation::Stop;
            }
            (true, Some('b')) => {
                bookmark_active_tab(state);
                return glib::Propagation::Stop;
            }
            (false, Some(',')) => {
                navigate_active_or_open(state, "search://settings");
                return glib::Propagation::Stop;
            }
            (false, Some('y')) => {
                navigate_active_or_open(state, "search://history");
                return glib::Propagation::Stop;
            }
            (false, Some('+') | Some('=')) => {
                adjust_zoom(state, Some(0.1));
                return glib::Propagation::Stop;
            }
            (false, Some('-')) => {
                adjust_zoom(state, Some(-0.1));
                return glib::Propagation::Stop;
            }
            (false, Some('0')) => {
                adjust_zoom(state, None);
                return glib::Propagation::Stop;
            }
            (false, Some('q')) => {
                gtk::main_quit();
                return glib::Propagation::Stop;
            }
            _ => {}
        }
    }

    if ctrl && alt && matches!(key, Some('n')) {
        toggle_split_view(state);
        return glib::Propagation::Stop;
    }

    glib::Propagation::Proceed
}
