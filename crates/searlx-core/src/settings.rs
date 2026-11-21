//! Typed settings, 11 settings pages, persistence, and synonym search (`FEATURES.md` §21, §26, P1-09).
//!
//! Settings is itself an internal tab (`search://settings` or
//! `search://settings/<page>`), with 11 pages:
//! 1. General
//! 2. Appearance
//! 3. Tabs
//! 4. Browsing
//! 5. Spaces & Pins
//! 6. Passwords
//! 7. Privacy
//! 8. AI (dormant unless `ai_shipped` is enabled in a test build)
//! 9. Extensions
//! 10. Keyboard & Mouse
//! 11. About
//!
//! Searching on `search://settings` filters every setting row and the
//! always-on notes by title, key, description, and synonyms. Rows whose parent
//! switch is off stay in search results but are marked `dimmed_by_parent = true`
//! (`FEATURES.md` §21).

use serde::{Deserialize, Serialize};

use crate::address::SearchEngine;
use crate::data::{DataStore, Extra};
use crate::error::Result;
use crate::url::PageUrl;

/// The 11 settings pages (`FEATURES.md` §21).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsPage {
    /// Default browser, webmail, sync, downloads, quit confirmation.
    #[default]
    General,
    /// Theme, dusk, tab layout, sidebar, cards, glass, glyphs, bookmarks bar, motion.
    Appearance,
    /// New-tab placement, close/reopen, switcher, sleep, auto-close, twins.
    Tabs,
    /// Search engines, keywords, small windows, reader, floating video, zoom, capture, site CSS, 120 Hz.
    Browsing,
    /// Spaces, profiles, space routes, pins, pin folders, floating new-tab, command bar, fresh start.
    SpacesAndPins,
    /// Password save/fill, passkeys, Apple Passwords, contact fill.
    Passwords,
    /// Shield ad/tracker blocker, cross-site tracking, click IDs, GPC, notifications, permissions, proxy.
    Privacy,
    /// On-device and cloud AI providers (dormant when `ai_shipped = false`).
    Ai,
    /// Installed web extensions and site access.
    Extensions,
    /// Link peek, option-split, autoscroll, swipe history, global launcher, keyboard shortcuts.
    KeyboardAndMouse,
    /// Updates, release notes, bench, coding agents.
    About,
}

impl SettingsPage {
    /// All 11 settings pages in sidebar order.
    pub const ALL: [Self; 11] = [
        Self::General,
        Self::Appearance,
        Self::Tabs,
        Self::Browsing,
        Self::SpacesAndPins,
        Self::Passwords,
        Self::Privacy,
        Self::Ai,
        Self::Extensions,
        Self::KeyboardAndMouse,
        Self::About,
    ];

    /// Visible pages (hides `Ai` when `ai_shipped` is false, per `FEATURES.md` §17.1).
    #[must_use]
    pub fn visible_pages(ai_shipped: bool) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|page| ai_shipped || *page != Self::Ai)
            .collect()
    }

    /// Display title of the page.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Tabs => "Tabs",
            Self::Browsing => "Browsing",
            Self::SpacesAndPins => "Spaces & Pins",
            Self::Passwords => "Passwords",
            Self::Privacy => "Privacy",
            Self::Ai => "AI",
            Self::Extensions => "Extensions",
            Self::KeyboardAndMouse => "Keyboard & Mouse",
            Self::About => "About",
        }
    }

    /// URL slug under `search://settings/<slug>`.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Tabs => "tabs",
            Self::Browsing => "browsing",
            Self::SpacesAndPins => "spaces",
            Self::Passwords => "passwords",
            Self::Privacy => "privacy",
            Self::Ai => "ai",
            Self::Extensions => "extensions",
            Self::KeyboardAndMouse => "keyboard",
            Self::About => "about",
        }
    }

    /// Parse a `search://settings` URL or section slug into a [`SettingsPage`].
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug.trim().trim_matches('/').to_ascii_lowercase().as_str() {
            "" | "general" => Some(Self::General),
            "appearance" | "look" | "theme" => Some(Self::Appearance),
            "tabs" => Some(Self::Tabs),
            "browsing" | "search" => Some(Self::Browsing),
            "spaces" | "spaces-and-pins" | "pins" | "profiles" => Some(Self::SpacesAndPins),
            "passwords" | "passkeys" | "vault" => Some(Self::Passwords),
            "privacy" | "shield" | "security" => Some(Self::Privacy),
            "ai" => Some(Self::Ai),
            "extensions" => Some(Self::Extensions),
            "keyboard" | "keyboard-and-mouse" | "shortcuts" | "mouse" => {
                Some(Self::KeyboardAndMouse)
            }
            "about" | "updates" => Some(Self::About),
            _ => None,
        }
    }

    /// Canonical `search://settings/<slug>` URL for this page.
    pub fn to_url(self) -> Result<PageUrl> {
        if self == Self::General {
            PageUrl::parse("search://settings")
        } else {
            PageUrl::parse(&format!("search://settings/{}", self.slug()))
        }
    }
}

/// Appearance look preference (`look`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LookPreference {
    /// Follow the OS appearance.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// Tab bar / column layout (`sidebar`, `sidebar.position`, `sidebar.icons`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabLayoutMode {
    /// Left sidebar column (default for new installs, `FEATURES.md` §2.2).
    #[default]
    LeftColumn,
    /// Right sidebar column.
    RightColumn,
    /// Top horizontal tab strip.
    TopTabs,
    /// Narrow fixed column of favicons/letters (`48 pt`).
    IconsOnly,
}

/// Always-visible address bar width (`address.width`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddressBarWidth {
    /// 220 pt.
    #[default]
    Small,
    /// 340 pt.
    Medium,
}

/// Tab glyph style (`glyph`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabGlyphMode {
    /// Single letter mark (default).
    #[default]
    Letters,
    /// Site favicon.
    Icons,
}

/// Where new tabs are inserted (`tabs.newPlace`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NewTabPlace {
    /// Immediately after the active tab (default).
    #[default]
    Next,
    /// At the start of loose tabs.
    Top,
    /// At the end of loose tabs.
    End,
}

/// Inactive tab sleep pace (`tabs.sleep.pace`, `FEATURES.md` §3.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SleepPace {
    /// Never put tabs to sleep automatically.
    Never,
    /// After 10 minutes away.
    Sooner,
    /// After 30 minutes away (default).
    #[default]
    Normal,
    /// After 2 hours away.
    Later,
}

impl SleepPace {
    /// Timeout in seconds before an eligible background tab sleeps.
    #[must_use]
    pub const fn timeout_seconds(self) -> Option<u64> {
        match self {
            Self::Never => None,
            Self::Sooner => Some(10 * 60),
            Self::Normal => Some(30 * 60),
            Self::Later => Some(120 * 60),
        }
    }
}

/// Auto-close threshold for unkept tabs (`tabs.close.after`, `FEATURES.md` §3.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AutoCloseAfter {
    /// Never auto-close (default).
    #[default]
    Never,
    /// After 12 hours away.
    Hours12,
    /// After 1 day away.
    Day1,
    /// After 1 week away.
    Week1,
    /// After 1 month (30 days) away.
    Month1,
}

impl AutoCloseAfter {
    /// Threshold in seconds before an unkept tab is archived.
    #[must_use]
    pub const fn threshold_seconds(self) -> Option<u64> {
        match self {
            Self::Never => None,
            Self::Hours12 => Some(12 * 3600),
            Self::Day1 => Some(24 * 3600),
            Self::Week1 => Some(7 * 24 * 3600),
            Self::Month1 => Some(30 * 24 * 3600),
        }
    }
}

/// Where the Downloads UI opens (`downloads.open`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadsOpenMode {
    /// `search://downloads` tab (default).
    #[default]
    Tab,
    /// Floating popover card.
    Popover,
    /// Side panel.
    Panel,
}

/// Webmail compose provider (`mail.webmail`, `FEATURES.md` §18.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebmailProvider {
    /// Delegate `mailto:` to the OS mail application.
    #[default]
    None,
    /// Gmail.
    Gmail,
    /// Outlook.com.
    Outlook,
    /// Fastmail.
    Fastmail,
    /// Proton Mail.
    ProtonMail,
}

/// Floating video style (`float.style`, `FEATURES.md` §19).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FloatStyle {
    /// Search's own floating window (default).
    #[default]
    Search,
    /// OS / WebKit native Picture-in-Picture (where supported by engine capability).
    System,
}

/// 120 Hz frame-rate preference (`pages.120.mode`, `FEATURES.md` §21).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HighRefreshMode {
    /// Cap at 60 Hz.
    Never,
    /// 120 Hz while on AC power (default).
    #[default]
    OnPower,
    /// Always 120 Hz.
    Always,
}

/// Browser-wide HTTP or SOCKS5 proxy configuration (`network.proxy`, `FEATURES.md` §12.2).
/// Passwords are never stored in `settings.json` (`AGENTS.md` §2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// `"http"` or `"socks5"`.
    pub protocol: String,
    /// Proxy hostname or IP.
    pub host: String,
    /// Proxy port.
    pub port: u16,
    /// Optional username (password lives in the OS credential store).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

/// Complete typed settings model (`FEATURES.md` §21).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Settings {
    // --- General ---
    pub webmail: WebmailProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_folder: Option<String>,
    pub handoff: bool,
    pub icloud_sync: bool,
    pub quit_confirm: bool,
    pub downloads_dir: String,
    pub downloads_ask: bool,
    pub downloads_button: bool,
    pub downloads_open: DownloadsOpenMode,

    // --- Appearance ---
    pub look: LookPreference,
    pub pages_dusk: bool,
    pub inspector_chrome: bool,
    pub inspector_dark: bool,
    pub tab_layout: TabLayoutMode,
    pub sidebar_hides: bool,
    pub sidebar_slide: bool,
    pub sidebar_fold_icons: bool,
    pub layout_cards: bool,
    pub layout_cards_fullscreen: bool,
    pub toolbar_left: bool,
    pub tabs_compact: bool,
    pub address_shown: bool,
    pub address_width: AddressBarWidth,
    pub window_glass: bool,
    pub sidebar_roomy: bool,
    pub sidebar_doors_up: bool,
    pub sidebar_player: bool,
    pub glyph: TabGlyphMode,
    pub tabs_high_contrast: bool,
    pub tabs_reading: bool,
    pub tabs_give: bool,
    pub bookmarks_bar: bool,
    pub low_power_save: bool,
    pub reduce_motion: bool,
    pub text_scale: f32,

    // --- Tabs ---
    pub tabs_new_place: NewTabPlace,
    pub tabs_front_beside: bool,
    pub tabs_new_after_last: bool,
    pub tabs_close_goes_back: bool,
    pub tabs_undo_reopens: bool,
    pub tabs_switcher_behind_first: bool,
    pub tabs_switcher_leaves_out_pins: bool,
    pub tabs_lazy: bool,
    pub tabs_popup_windows: bool,
    pub tabs_sleep: bool,
    pub tabs_sleep_pace: SleepPace,
    pub tabs_sleep_dim: bool,
    pub tabs_close_after: AutoCloseAfter,
    pub tabs_shortcuts: bool,
    pub tabs_address: bool,
    pub tabs_mirror: bool,
    pub profiles_one_window: bool,

    // --- Browsing ---
    pub search_engine: SearchEngine,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_shift_engine: Option<SearchEngine>,
    pub search_suggest: bool,
    pub search_keywords: bool,
    pub links_little: bool,
    pub links_little_tabs: bool,
    pub reader_twins: bool,
    pub reader_follows: bool,
    pub float_style: FloatStyle,
    pub float_leave: bool,
    pub float_away: bool,
    pub float_minimize: bool,
    pub media_click: bool,
    pub fullscreen_escape_twice: bool,
    pub fullscreen_keeps_window: bool,
    pub translate_target: String,
    pub page_zoom: u16,
    pub pages_capture: bool,
    pub sites_styles: bool,
    pub pages_120_mode: HighRefreshMode,

    // --- Spaces & Pins ---
    pub spaces: bool,
    pub spaces_command_numbers: bool,
    pub spaces_links: bool,
    pub links_ask: bool,
    pub pins_home: bool,
    pub pins_peek: bool,
    pub pins_list: bool,
    pub pins_folders: bool,
    pub pins_command_new_tab: bool,
    pub newtab_floating: bool,
    pub newtab_recent: bool,
    pub commandbar: bool,
    pub start_fresh: bool,

    // --- Passwords ---
    pub passwords_save: bool,
    pub passwords_fill: bool,
    pub passkeys: bool,
    pub passwords_apple: bool,
    pub contacts_fill: bool,

    // --- Privacy ---
    pub shield: bool,
    /// `sites.keep` is stored inverted in the original app (`sites.keep = true`
    /// means cross-site tracking prevention is OFF; `FEATURES.md` §21, §29).
    pub sites_keep: bool,
    pub privacy_click_ids: bool,
    pub privacy_gpc: bool,
    pub notifications_ask: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_proxy: Option<ProxyConfig>,

    // --- Keyboard & Mouse ---
    pub links_peek: bool,
    pub links_peek_stays: bool,
    pub links_option_split: bool,
    pub autoscroll: bool,
    pub pages_bounce: bool,
    pub swipe_history: bool,
    pub launcher_on: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launcher_key: Option<String>,
    pub launcher_little: bool,

    // --- About / Dormant ---
    pub update_auto_install: bool,
    pub bench: bool,
    pub agents: bool,
    pub ai_shipped: bool,

    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            webmail: WebmailProvider::None,
            sync_folder: None,
            handoff: false,
            icloud_sync: false,
            quit_confirm: false,
            downloads_dir: "~/Downloads".into(),
            downloads_ask: false,
            downloads_button: false,
            downloads_open: DownloadsOpenMode::Tab,

            look: LookPreference::System,
            pages_dusk: false,
            inspector_chrome: false,
            inspector_dark: false,
            tab_layout: TabLayoutMode::LeftColumn,
            sidebar_hides: false,
            sidebar_slide: false,
            sidebar_fold_icons: false,
            layout_cards: true,
            layout_cards_fullscreen: false,
            toolbar_left: false,
            tabs_compact: false,
            address_shown: false,
            address_width: AddressBarWidth::Small,
            window_glass: false,
            sidebar_roomy: false,
            sidebar_doors_up: false,
            sidebar_player: false,
            glyph: TabGlyphMode::Letters,
            tabs_high_contrast: false,
            tabs_reading: false,
            tabs_give: false,
            bookmarks_bar: false,
            low_power_save: false,
            reduce_motion: false,
            text_scale: 1.0,

            tabs_new_place: NewTabPlace::Next,
            tabs_front_beside: false,
            tabs_new_after_last: false,
            tabs_close_goes_back: false,
            tabs_undo_reopens: false,
            tabs_switcher_behind_first: false,
            tabs_switcher_leaves_out_pins: false,
            tabs_lazy: false,
            tabs_popup_windows: false,
            tabs_sleep: true,
            tabs_sleep_pace: SleepPace::Normal,
            tabs_sleep_dim: false,
            tabs_close_after: AutoCloseAfter::Never,
            tabs_shortcuts: false,
            tabs_address: true,
            tabs_mirror: false,
            profiles_one_window: false,

            search_engine: SearchEngine::Google,
            search_shift_engine: None,
            search_suggest: false,
            search_keywords: false,
            links_little: true,
            links_little_tabs: false,
            reader_twins: false,
            reader_follows: false,
            float_style: FloatStyle::Search,
            float_leave: true,
            float_away: false,
            float_minimize: false,
            media_click: false,
            fullscreen_escape_twice: false,
            fullscreen_keeps_window: false,
            translate_target: "en".into(),
            page_zoom: 100,
            pages_capture: false,
            sites_styles: false,
            pages_120_mode: HighRefreshMode::OnPower,

            spaces: false,
            spaces_command_numbers: false,
            spaces_links: false,
            links_ask: false,
            pins_home: true,
            pins_peek: true,
            pins_list: false,
            pins_folders: false,
            pins_command_new_tab: false,
            newtab_floating: false,
            newtab_recent: false,
            commandbar: false,
            start_fresh: false,

            passwords_save: true,
            passwords_fill: true,
            passkeys: true,
            passwords_apple: false,
            contacts_fill: false,

            shield: true,
            sites_keep: false,
            privacy_click_ids: true,
            privacy_gpc: true,
            notifications_ask: true,
            network_proxy: None,

            links_peek: true,
            links_peek_stays: false,
            links_option_split: false,
            autoscroll: false,
            pages_bounce: false,
            swipe_history: true,
            launcher_on: false,
            launcher_key: None,
            launcher_little: false,

            update_auto_install: true,
            bench: false,
            agents: false,
            ai_shipped: false,

            extra: serde_json::Map::new(),
        }
    }
}

impl Settings {
    /// Whether cross-site tracking prevention is active (`!sites_keep`, `FEATURES.md` §12.2, §29).
    #[must_use]
    pub const fn prevent_cross_site_tracking(&self) -> bool {
        !self.sites_keep
    }

    /// Set cross-site tracking prevention, updating the inverted `sites_keep` field.
    pub fn set_prevent_cross_site_tracking(&mut self, enabled: bool) {
        self.sites_keep = !enabled;
    }

    /// Load settings from `settings.json` in `store`, falling back to defaults if absent or corrupt.
    pub fn load_from(store: &DataStore) -> Result<Self> {
        Ok(store.read_or_default::<Self>("settings.json")?.value)
    }

    /// Persist settings atomically to `settings.json` in `store`.
    pub fn save_to(&self, store: &DataStore) -> Result<()> {
        store.write_file("settings.json", self)
    }
}

/// A searchable entry in the Settings index (`FEATURES.md` §21).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsSearchHit {
    /// Settings page where the row lives.
    pub page: SettingsPage,
    /// Setting key (or `"note"` for an always-on note).
    pub key: &'static str,
    /// User-visible row label.
    pub label: &'static str,
    /// Whether this row is an informational "always-on note" with no switch.
    pub always_on_note: bool,
    /// Whether the row's parent switch is currently off (dimmed in search results, `FEATURES.md` §21).
    pub dimmed_by_parent: bool,
}

struct CatalogEntry {
    page: SettingsPage,
    key: &'static str,
    label: &'static str,
    synonyms: &'static [&'static str],
    always_on_note: bool,
    parent_enabled: fn(&Settings) -> bool,
}

const fn always_enabled(_: &Settings) -> bool {
    true
}

const CATALOG: &[CatalogEntry] = &[
    // General
    CatalogEntry {
        page: SettingsPage::General,
        key: "default_browser",
        label: "Open links from other apps",
        synonyms: &["default browser", "http", "https", "system handler"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::General,
        key: "mail.webmail",
        label: "Email links open in",
        synonyms: &[
            "mailto", "webmail", "gmail", "outlook", "fastmail", "proton",
        ],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::General,
        key: "sync.folder",
        label: "Sync with your other computers",
        synonyms: &["folder sync", "dropbox", "icloud drive", "syncthing"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::General,
        key: "handoff",
        label: "Handoff to iPhone and iPad",
        synonyms: &["continuity", "apple handoff"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::General,
        key: "icloud.sync",
        label: "Sync with iCloud",
        synonyms: &["cloudkit", "icloud tabs", "apple sync"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::General,
        key: "quit.confirm",
        label: "Confirm before quitting",
        synonyms: &["cmd-q", "exit confirmation", "close app"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::General,
        key: "downloads",
        label: "Save downloads to",
        synonyms: &["downloads folder", "save path", "destination"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::General,
        key: "downloads.ask",
        label: "Ask where to save each file",
        synonyms: &["prompt download", "save dialog"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // Appearance
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "look",
        label: "Appearance",
        synonyms: &[
            "theme",
            "dark mode",
            "light mode",
            "system",
            "colour",
            "color",
        ],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "pages.dusk",
        label: "Darken pages that stay light",
        synonyms: &["dusk", "dark reader", "force dark", "night mode"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "sidebar",
        label: "Tabs layout",
        synonyms: &[
            "sidebar",
            "left column",
            "right column",
            "top tabs",
            "icons only",
            "vertical tabs",
        ],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "sidebar.hides",
        label: "Hide column until edge",
        synonyms: &["auto hide sidebar", "fold", "edge dwell"],
        always_on_note: false,
        parent_enabled: |s| s.tab_layout != TabLayoutMode::TopTabs,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "layout.cards",
        label: "Page as a card",
        synonyms: &["rounded corners", "border", "inset", "frame"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "address.shown",
        label: "Always show address bar",
        synonyms: &["url bar", "omnibox", "address line"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "window.glass",
        label: "Glass",
        synonyms: &[
            "vibrancy",
            "blur",
            "frosted",
            "liquid glass",
            "transparency",
        ],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "glyph",
        label: "Tabs show letters or icons",
        synonyms: &["favicons", "letters", "tab icon"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "tabs.highContrast",
        label: "High contrast text",
        synonyms: &["accessibility", "contrast", "legibility"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "bookmarks.bar",
        label: "Show bookmarks bar",
        synonyms: &["favourites bar", "favorites bar"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Appearance,
        key: "motion.reduce",
        label: "Reduce animations",
        synonyms: &["reduce motion", "disable animations", "accessibility"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // Tabs
    CatalogEntry {
        page: SettingsPage::Tabs,
        key: "tabs.newPlace",
        label: "New tab position",
        synonyms: &["next", "top", "end", "order"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Tabs,
        key: "tabs.sleep",
        label: "Sleep tabs",
        synonyms: &["suspend", "discard", "memory", "hibernate"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Tabs,
        key: "tabs.sleep.dim",
        label: "Dim sleeping tabs",
        synonyms: &["faded", "inactive tabs"],
        always_on_note: false,
        parent_enabled: |s| s.tabs_sleep,
    },
    CatalogEntry {
        page: SettingsPage::Tabs,
        key: "tabs.close.after",
        label: "Close tabs left alone",
        synonyms: &["auto close", "archive", "expire", "tidy"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // Browsing
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "search.engine",
        label: "Search with",
        synonyms: &[
            "google",
            "duckduckgo",
            "bing",
            "kagi",
            "brave",
            "ecosia",
            "startpage",
            "qwant",
        ],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "search.keywords",
        label: "Site shortcuts",
        synonyms: &["keywords", "bang", "yt", "quick search"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "links.little",
        label: "Open links from other apps in a small window",
        synonyms: &["little window", "mini window", "external links"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "reader.twins",
        label: "Read the site's own Markdown",
        synonyms: &["reading mode", "reader", "markdown twin"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "float.style",
        label: "Floating video",
        synonyms: &["picture in picture", "pip", "video player"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "pageZoom",
        label: "Page zoom",
        synonyms: &["text size", "magnification", "scale"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "pages.capture",
        label: "Capture pages",
        synonyms: &["screenshot", "snap", "whole page"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "sites.styles",
        label: "Your own style for a site",
        synonyms: &["custom css", "userstyle", "boosts", "site css"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // Spaces & Pins
    CatalogEntry {
        page: SettingsPage::SpacesAndPins,
        key: "spaces",
        label: "Spaces",
        synonyms: &["workspaces", "arc", "contexts", "containers"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::SpacesAndPins,
        key: "spaces.links",
        label: "Send links to a space",
        synonyms: &["space routes", "air traffic control", "url routing"],
        always_on_note: false,
        parent_enabled: |s| s.spaces,
    },
    CatalogEntry {
        page: SettingsPage::SpacesAndPins,
        key: "pins.list",
        label: "Pinned rows",
        synonyms: &["pins", "favourites", "arc sidebar"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::SpacesAndPins,
        key: "pins.folders",
        label: "Folders in pinned rows",
        synonyms: &["pin folders", "live folders", "rss", "atom"],
        always_on_note: false,
        parent_enabled: |s| s.pins_list,
    },
    CatalogEntry {
        page: SettingsPage::SpacesAndPins,
        key: "start.fresh",
        label: "Start with a fresh window",
        synonyms: &["clean start", "don't restore session"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // Passwords
    CatalogEntry {
        page: SettingsPage::Passwords,
        key: "passwords.save",
        label: "Offer to save passwords",
        synonyms: &["keychain", "vault", "credentials", "login"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Passwords,
        key: "passkeys",
        label: "Offer passkeys",
        synonyms: &[
            "webauthn",
            "fido2",
            "security key",
            "touch id",
            "windows hello",
        ],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // Privacy
    CatalogEntry {
        page: SettingsPage::Privacy,
        key: "shield",
        label: "Block ads and trackers",
        synonyms: &[
            "adblock",
            "ad blocker",
            "content blocker",
            "shield",
            "youtube ads",
        ],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Privacy,
        key: "sites.keep",
        label: "Prevent cross-site tracking",
        synonyms: &["third party cookies", "itp", "tracking prevention"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Privacy,
        key: "privacy.clickIDs",
        label: "Remove click trackers",
        synonyms: &["gclid", "fbclid", "clean links", "query stripping"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Privacy,
        key: "privacy.gpc",
        label: "Global Privacy Control",
        synonyms: &["sec-gpc", "do not sell", "gpc"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Privacy,
        key: "network.proxy",
        label: "Proxy",
        synonyms: &["socks5", "http proxy", "tunnel"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // Extensions
    CatalogEntry {
        page: SettingsPage::Extensions,
        key: "extensions",
        label: "Extensions",
        synonyms: &["chrome web store", "crx", "unpacked", "addons"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // Keyboard & Mouse
    CatalogEntry {
        page: SettingsPage::KeyboardAndMouse,
        key: "links.peek",
        label: "Peek at a link",
        synonyms: &["shift click", "little arc", "preview link"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::KeyboardAndMouse,
        key: "swipe.history",
        label: "Hold a swipe to pick from history",
        synonyms: &["gesture", "trackpad swipe", "back menu"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::KeyboardAndMouse,
        key: "launcher.on",
        label: "Open address field from any app",
        synonyms: &["global hotkey", "quick launcher", "spotlight"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // About
    CatalogEntry {
        page: SettingsPage::About,
        key: "update.install",
        label: "Install updates on its own",
        synonyms: &["auto update", "appcast", "version"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::About,
        key: "agents",
        label: "Let coding agents use Search",
        synonyms: &["mcp", "claude code", "codex", "automation"],
        always_on_note: false,
        parent_enabled: always_enabled,
    },
    // Always-on notes (`FEATURES.md` §21)
    CatalogEntry {
        page: SettingsPage::Tabs,
        key: "note.splitView",
        label: "Split View",
        synonyms: &["side by side", "two pages", "alt-cmd-n", "divider"],
        always_on_note: true,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Tabs,
        key: "note.tabGroups",
        label: "Tab groups",
        synonyms: &["fold groups", "group tabs", "sections"],
        always_on_note: true,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "note.sidePanel",
        label: "Websites in the side panel",
        synonyms: &["claude", "chatgpt", "gemini", "perplexity", "sidebar panel"],
        always_on_note: true,
        parent_enabled: always_enabled,
    },
    CatalogEntry {
        page: SettingsPage::Browsing,
        key: "note.siteSearch",
        label: "Search a site from the field",
        synonyms: &["tab to search", "opensearch", "site chip"],
        always_on_note: true,
        parent_enabled: always_enabled,
    },
];

/// Search settings rows and always-on notes by label, key, or synonym (`FEATURES.md` §21).
#[must_use]
pub fn search_settings(query: &str, settings: &Settings) -> Vec<SettingsSearchHit> {
    let trimmed = query.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    CATALOG
        .iter()
        .filter(|entry| settings.ai_shipped || entry.page != SettingsPage::Ai)
        .filter(|entry| {
            words.iter().all(|word| {
                entry.label.to_ascii_lowercase().contains(word)
                    || entry.key.to_ascii_lowercase().contains(word)
                    || entry.page.title().to_ascii_lowercase().contains(word)
                    || entry
                        .synonyms
                        .iter()
                        .any(|syn| syn.to_ascii_lowercase().contains(word))
            })
        })
        .map(|entry| SettingsSearchHit {
            page: entry.page,
            key: entry.key,
            label: entry.label,
            always_on_note: entry.always_on_note,
            dimmed_by_parent: !(entry.parent_enabled)(settings),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_preserve_inverted_sites_keep() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());

        let mut settings = Settings::default();
        assert!(settings.prevent_cross_site_tracking());
        assert!(!settings.sites_keep);

        settings.set_prevent_cross_site_tracking(false);
        assert!(settings.sites_keep);
        settings.look = LookPreference::Dark;
        settings.page_zoom = 125;

        settings.save_to(&store).expect("save settings");
        let loaded = Settings::load_from(&store).expect("load settings");
        assert_eq!(loaded.look, LookPreference::Dark);
        assert_eq!(loaded.page_zoom, 125);
        assert!(!loaded.prevent_cross_site_tracking());
    }

    #[test]
    fn settings_pages_hide_ai_when_not_shipped_and_parse_slugs() {
        assert_eq!(SettingsPage::visible_pages(false).len(), 10);
        assert_eq!(SettingsPage::visible_pages(true).len(), 11);
        assert_eq!(
            SettingsPage::from_slug("privacy"),
            Some(SettingsPage::Privacy)
        );
        assert_eq!(
            SettingsPage::from_slug("spaces"),
            Some(SettingsPage::SpacesAndPins)
        );
        assert_eq!(
            SettingsPage::Privacy.to_url().expect("url").as_str(),
            "search://settings/privacy"
        );
    }

    #[test]
    fn search_finds_rows_by_synonym_and_dims_children_of_disabled_parents() {
        let mut settings = Settings::default();
        let adblock_hits = search_settings("adblock", &settings);
        assert!(adblock_hits.iter().any(|hit| hit.key == "shield"));

        let split_hits = search_settings("side by side", &settings);
        assert!(
            split_hits
                .iter()
                .any(|hit| hit.key == "note.splitView" && hit.always_on_note)
        );

        // `pins.folders` requires `pins.list` to be true; when `pins_list` is false,
        // the row is still found in search but marked `dimmed_by_parent`.
        settings.pins_list = false;
        let folder_hits = search_settings("rss", &settings);
        let hit = folder_hits
            .iter()
            .find(|h| h.key == "pins.folders")
            .expect("find pins.folders by rss synonym");
        assert!(hit.dimmed_by_parent);

        settings.pins_list = true;
        let folder_hits_enabled = search_settings("rss", &settings);
        let hit_enabled = folder_hits_enabled
            .iter()
            .find(|h| h.key == "pins.folders")
            .expect("find pins.folders");
        assert!(!hit_enabled.dimmed_by_parent);
    }
}
