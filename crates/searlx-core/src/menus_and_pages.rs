//! Site card model (`P2-13`), context menus (`P2-16`), and internal `search://`
//! pages router (`P2-17`) (`FEATURES.md` §5.8, §5.9, §24, §26).

use serde::{Deserialize, Serialize};

use crate::address::inspect_host_lookalike;
use crate::blocking::normalize_site_key;
use crate::domain::{PermissionDecision, PermissionKind};
use crate::error::{CoreError, Result};
use crate::ids::TabId;
use crate::privacy::PermissionStore;
use crate::settings::Settings;
use crate::tabs::TabModel;
use crate::url::PageUrl;

// ============================================================================
// P2-13 — Site card (`FEATURES.md` §5.8)
// ============================================================================

/// Connection security state shown in the site card (`FEATURES.md` §5.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// HTTPS with a valid certificate.
    Secure,
    /// HTTPS page that loaded plain-HTTP subresources.
    MixedContent,
    /// Plain `http://` page.
    NotSecure,
    /// Invalid or untrusted TLS certificate.
    UntrustedCertificate,
    /// Internal `search://`, `file://`, `about:`, or `data:` page.
    LocalOrInternal,
}

impl ConnectionState {
    /// Certificate button label inside the Connection sub-view (`FEATURES.md` §5.8).
    #[must_use]
    pub const fn certificate_button_label(self) -> Option<&'static str> {
        match self {
            Self::Secure | Self::MixedContent => Some("Show Certificate…"),
            Self::UntrustedCertificate => Some("Show Certificate (Not Valid)…"),
            Self::NotSecure | Self::LocalOrInternal => None,
        }
    }
}

/// Known video sites eligible for `"Don't Float Videos Here"` (`FEATURES.md` §5.8).
pub const KNOWN_VIDEO_SITES: &[&str] = &[
    "youtube.com",
    "vimeo.com",
    "twitch.tv",
    "netflix.com",
    "plex.tv",
    "nebula.tv",
    "dailymotion.com",
    "bilibili.com",
];

/// Input state for constructing a [`SiteCardModel`] (`FEATURES.md` §5.8).
#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct SiteCardInput<'a> {
    /// Active page URL.
    pub url: &'a PageUrl,
    /// Connection security classification.
    pub connection: ConnectionState,
    /// Whether this tab is a private tab.
    pub private_tab: bool,
    /// Current page zoom percentage (`100` = Actual Size).
    pub zoom_percent: u16,
    /// Whether the engine supports printing.
    pub engine_supports_print: bool,
    /// Whether the engine supports floating video.
    pub engine_supports_floating_video: bool,
    /// Whether the ad/tracker shield encountered a compile error.
    pub shield_in_trouble: bool,
    /// Whether the site's ad blocker is currently enabled.
    pub site_blocker_on: bool,
    /// Whether Dusk darkening is globally paused for the session (`⌥⇧⌘D`).
    pub dusk_paused: bool,
    /// Whether the browser window frame is currently dark.
    pub frame_is_dark: bool,
    /// Whether Dusk is active for this site.
    pub site_dusk_on: bool,
    /// Whether `"Play Sound by Itself"` is enabled for this site.
    pub site_autoplay_sound: bool,
    /// Whether `"Don't Float Videos Here"` is enabled for this site.
    pub site_dont_float_videos: bool,
    /// Whether `"Keep This Site Awake"` is enabled for this site/tab.
    pub site_keep_awake: bool,
    /// Browser settings.
    pub settings: &'a Settings,
    /// Permission store.
    pub permissions: &'a PermissionStore,
}

/// Per-site permission decisions displayed in the Site Card (`FEATURES.md` §5.8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePermissionsCardState {
    /// Camera decision for the site.
    pub camera: PermissionDecision,
    /// Microphone decision for the site.
    pub microphone: PermissionDecision,
    /// Location decision for the site.
    pub location: PermissionDecision,
    /// Notifications decision for the site.
    pub notifications: PermissionDecision,
    /// Conflicting origin spellings differing only by explicit default port.
    pub conflicting_origins: Vec<String>,
}

/// Custom shortcut labels displayed inside the Site Card (`FEATURES.md` §5.8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteCardShortcuts {
    /// Shortcut for Copy Address (`⇧⌘C` by default).
    pub copy_address: String,
    /// Shortcut for Darken This Site (`⇧⌘D` by default).
    pub darken_site: String,
}

impl Default for SiteCardShortcuts {
    fn default() -> Self {
        Self {
            copy_address: "⇧⌘C".into(),
            darken_site: "⇧⌘D".into(),
        }
    }
}

/// Complete, capability-gated site card model (`FEATURES.md` §5.8, P2-13).
///
/// Explicitly excludes mute, site CSS, and reader mode (`FEATURES.md` §5.8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct SiteCardModel {
    /// Header label: host without `www.` (punycode preserved when IDN lookalike), `"File"` for `file://`, or scheme name.
    pub header: String,
    /// Whether the host triggered IDN homograph/punycode lookalike protection (`FEATURES.md` §4.2, §5.8).
    pub is_lookalike: bool,
    /// Connection state row.
    pub connection: ConnectionState,
    /// Certificate action label, when applicable.
    pub certificate_action: Option<String>,
    /// Copy Address row with shortcut.
    pub copy_address_shortcut: String,
    /// Whether the `Print…` row is shown.
    pub show_print: bool,
    /// Current zoom percentage (`40..=300` for web pages, `20..=600` for PDFs).
    pub zoom_percent: u16,
    /// Minimum allowed zoom percentage (`20` for PDF, `40` for web).
    pub zoom_min: u16,
    /// Maximum allowed zoom percentage (`600` for PDF, `300` for web).
    pub zoom_max: u16,
    /// `"Block Ads and Trackers"` row (`Some(enabled)` when `!private && http(s) && shield && !trouble`).
    pub block_ads_and_trackers: Option<bool>,
    /// Whether `"Site Permissions"` row is shown (`http`/`https` only).
    pub show_site_permissions: bool,
    /// Per-site permissions summary when `show_site_permissions` is true.
    pub site_permissions: Option<SitePermissionsCardState>,
    /// Whether Notifications `Allow` is disabled because notifications are off in Settings › Privacy.
    pub notifications_allow_disabled: bool,
    /// Notice shown inside Site Permissions (`"Permissions aren't saved in a private tab"`, etc.).
    pub permissions_notice: Option<String>,
    /// `"Play Sound by Itself"` row (`Some(enabled)` when `!private && http(s) && !media.click`, labeled `"from the next page"`).
    pub play_sound_by_itself: Option<bool>,
    /// `"Don't Float Videos Here"` row (`Some(enabled)` on known video sites when float-on-leave/away is on and non-private).
    pub dont_float_videos_here: Option<bool>,
    /// `"Darken This Site"` row (`Some(enabled)` when Dusk is on, not paused, frame is dark, and page has a host).
    pub darken_this_site: Option<bool>,
    /// Shortcut label for Darken This Site (`⇧⌘D` by default).
    pub darken_shortcut: String,
    /// `"Keep This Site Awake"` row (`Some(enabled)` on `http`/`https`).
    pub keep_this_site_awake: Option<bool>,
}

impl SiteCardModel {
    /// Build and gate every row of the site card from [`SiteCardInput`] (`FEATURES.md` §5.8).
    #[must_use]
    pub fn build(input: &SiteCardInput<'_>) -> Self {
        Self::build_with_shortcuts(input, &SiteCardShortcuts::default())
    }

    /// Build the site card using user-configured shortcut labels (`FEATURES.md` §5.8).
    #[must_use]
    pub fn build_with_shortcuts(input: &SiteCardInput<'_>, shortcuts: &SiteCardShortcuts) -> Self {
        let is_web = matches!(input.url.scheme(), "http" | "https");
        let has_host = input.url.host().is_some();
        let is_pdf = input
            .url
            .as_url()
            .path()
            .to_ascii_lowercase()
            .ends_with(".pdf");
        let (zoom_min, zoom_max) = if is_pdf { (20, 600) } else { (40, 300) };

        let lookalike_info = input.url.host().map(inspect_host_lookalike);
        let is_lookalike = lookalike_info.as_ref().is_some_and(|i| i.is_lookalike);
        let header = match input.url.scheme() {
            "file" => "File".to_owned(),
            _ => match lookalike_info {
                Some(info) if info.is_lookalike => info.safe_display_host,
                _ => input.url.host().map_or_else(
                    || input.url.scheme().to_owned(),
                    |h| h.trim_start_matches("www.").to_owned(),
                ),
            },
        };

        let block_ads_and_trackers =
            (!input.private_tab && is_web && input.settings.shield && !input.shield_in_trouble)
                .then_some(input.site_blocker_on);

        let notifications_allow_disabled = !input.permissions.notifications_globally_enabled;
        let conflicting_origins = if is_web && !input.private_tab {
            input.permissions.conflicting_origins_for_url(input.url)
        } else {
            Vec::new()
        };
        let permissions_notice = if !is_web {
            None
        } else if input.private_tab {
            Some("Permissions aren't saved in a private tab".to_owned())
        } else if !conflicting_origins.is_empty() {
            Some("Saved two ways for this site. Pick one to keep.".to_owned())
        } else if notifications_allow_disabled {
            Some("Notifications are off in Settings › Privacy".to_owned())
        } else {
            None
        };

        let site_permissions = is_web.then(|| SitePermissionsCardState {
            camera: input.permissions.evaluate_request(
                input.url,
                PermissionKind::Camera,
                input.private_tab,
                false,
            ),
            microphone: input.permissions.evaluate_request(
                input.url,
                PermissionKind::Microphone,
                input.private_tab,
                false,
            ),
            location: input.permissions.evaluate_request(
                input.url,
                PermissionKind::Location,
                input.private_tab,
                false,
            ),
            notifications: input.permissions.evaluate_request(
                input.url,
                PermissionKind::Notifications,
                input.private_tab,
                false,
            ),
            conflicting_origins,
        });

        let play_sound_by_itself = (!input.private_tab && is_web && !input.settings.media_click)
            .then_some(input.site_autoplay_sound);

        let is_video_site = input.url.host().is_some_and(|h| {
            let clean = normalize_site_key(h);
            KNOWN_VIDEO_SITES
                .iter()
                .any(|v| clean == *v || clean.ends_with(&format!(".{v}")))
        });
        let dont_float_videos_here = (!input.private_tab
            && is_web
            && is_video_site
            && input.engine_supports_floating_video
            && (input.settings.float_leave || input.settings.float_away))
            .then_some(input.site_dont_float_videos);

        let darken_this_site =
            (input.settings.pages_dusk && !input.dusk_paused && input.frame_is_dark && has_host)
                .then_some(input.site_dusk_on);

        let keep_this_site_awake = is_web.then_some(input.site_keep_awake);

        Self {
            header,
            is_lookalike,
            connection: input.connection,
            certificate_action: input
                .connection
                .certificate_button_label()
                .map(ToOwned::to_owned),
            copy_address_shortcut: shortcuts.copy_address.clone(),
            show_print: input.engine_supports_print,
            zoom_percent: input.zoom_percent.clamp(zoom_min, zoom_max),
            zoom_min,
            zoom_max,
            block_ads_and_trackers,
            show_site_permissions: is_web,
            site_permissions,
            notifications_allow_disabled,
            permissions_notice,
            play_sound_by_itself,
            dont_float_videos_here,
            darken_this_site,
            darken_shortcut: shortcuts.darken_site.clone(),
            keep_this_site_awake,
        }
    }
}

/// Step a zoom percentage up or down by 5% within the document's allowed range (`FEATURES.md` §5.9).
#[must_use]
pub fn step_zoom_percent(current: u16, delta_steps: i16, is_pdf: bool) -> u16 {
    let (min_z, max_z) = if is_pdf {
        (20_i32, 600_i32)
    } else {
        (40_i32, 300_i32)
    };
    let next = i32::from(current) + i32::from(delta_steps) * 5;
    next.clamp(min_z, max_z) as u16
}

// ============================================================================
// P2-16 — Context menus (`FEATURES.md` §5.9, §24)
// ============================================================================

/// One entry in a context menu (`FEATURES.md` §24, P2-16).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextMenuItem {
    /// Action identifier.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Whether the item is enabled.
    pub enabled: bool,
}

/// Strip known click-tracking query parameters (`gclid`, `fbclid`, `msclkid`,
/// `twclid`, `dclid`, `mc_eid`, `igshid`, `yclid`) while keeping `utm_*` or
/// non-tracker parameters (`Copy Clean Link`, `FEATURES.md` §5.9, §12.2).
#[must_use]
pub fn clean_link_url(url: &PageUrl) -> PageUrl {
    const CLICK_TRACKERS: &[&str] = &[
        "gclid", "fbclid", "msclkid", "twclid", "dclid", "mc_eid", "igshid", "yclid",
    ];
    let mut parsed = url.as_url().clone();
    let Some(query) = parsed.query() else {
        return url.clone();
    };
    let kept: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .filter(|(k, _)| !CLICK_TRACKERS.iter().any(|t| k.eq_ignore_ascii_case(t)))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in kept {
            serializer.append_pair(&k, &v);
        }
        let new_q = serializer.finish();
        parsed.set_query(Some(&new_q));
    }
    PageUrl::from_url(&parsed).unwrap_or_else(|_| url.clone())
}

/// Build the modified page/link/image/selection context menu (`FEATURES.md` §5.9, §24, P2-16):
/// - Generic `"Open Link"` and `"Open Link in New Window"` are removed/replaced by
///   `"Open Link in New Tab"` and `"Open Link in New Tab and Go to It"`.
/// - `"Open Link in Split View"` is included when Split View is enabled.
/// - `"Copy Clean Link"` is included under `"Copy Link"` and when the page URL has trackers.
/// - Non-linked images receive: `"Open Image in New Tab"`, `"Copy Image"`,
///   `"Download Image"`, `"Copy Image Address"`.
/// - Selected text receives `"Search with <engine>"`.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn build_page_context_menu(
    page_url: &PageUrl,
    hovered_link: Option<&PageUrl>,
    is_non_linked_image: bool,
    selected_text: Option<&str>,
    search_engine_name: &str,
    split_view_enabled: bool,
    is_translated: bool,
    target_language: &str,
) -> Vec<ContextMenuItem> {
    let mut items = Vec::new();

    if let Some(link) = hovered_link {
        items.push(ContextMenuItem {
            id: "link.open_new_tab".into(),
            label: "Open Link in New Tab".into(),
            enabled: true,
        });
        items.push(ContextMenuItem {
            id: "link.open_new_tab_foreground".into(),
            label: "Open Link in New Tab and Go to It".into(),
            enabled: true,
        });
        if split_view_enabled {
            items.push(ContextMenuItem {
                id: "link.open_split".into(),
                label: "Open Link in Split View".into(),
                enabled: true,
            });
        }
        items.push(ContextMenuItem {
            id: "link.copy".into(),
            label: "Copy Link".into(),
            enabled: true,
        });
        let _ = link;
        items.push(ContextMenuItem {
            id: "link.copy_clean".into(),
            label: "Copy Clean Link".into(),
            enabled: true,
        });
    }

    if is_non_linked_image {
        for (id, label) in [
            ("image.open_new_tab", "Open Image in New Tab"),
            ("image.copy", "Copy Image"),
            ("image.download", "Download Image"),
            ("image.copy_address", "Copy Image Address"),
        ] {
            items.push(ContextMenuItem {
                id: id.into(),
                label: label.into(),
                enabled: true,
            });
        }
    }

    if let Some(sel) = selected_text
        && !sel.trim().is_empty()
    {
        items.push(ContextMenuItem {
            id: "selection.search".into(),
            label: format!("Search with {search_engine_name}"),
            enabled: true,
        });
    }

    if clean_link_url(page_url).as_str() != page_url.as_str() {
        items.push(ContextMenuItem {
            id: "page.copy_clean".into(),
            label: "Copy Clean Link".into(),
            enabled: true,
        });
    }

    items.push(ContextMenuItem {
        id: "page.translate".into(),
        label: if is_translated {
            "Show Original".into()
        } else {
            format!("Translate Page to {target_language}")
        },
        enabled: matches!(page_url.scheme(), "http" | "https"),
    });

    items.push(ContextMenuItem {
        id: "page.inspect".into(),
        label: "Inspect Element".into(),
        enabled: true,
    });

    items
}

/// Build the tab row context menu with accurate enabled/disabled states (`FEATURES.md` §3.2, §24, P2-16).
#[must_use]
pub fn build_tab_context_menu(
    model: &TabModel,
    target_tab: TabId,
    can_reopen_closed: bool,
) -> Vec<ContextMenuItem> {
    let Some(tab) = model.tab(target_tab) else {
        return Vec::new();
    };
    let is_active = model.active_tab().id == target_tab;
    let can_sleep = !is_active && !tab.audible && !tab.loading && !tab.sleeping;

    let mut items = Vec::new();
    if tab.pinned {
        if tab
            .home
            .as_ref()
            .is_some_and(|h| h.identity() != tab.url.identity())
        {
            items.push(ContextMenuItem {
                id: "pin.back_home".into(),
                label: "Back to Pinned Page".into(),
                enabled: true,
            });
        }
        items.push(ContextMenuItem {
            id: "pin.rehome".into(),
            label: "Replace Pinned Page with Current URL".into(),
            enabled: true,
        });
        items.push(ContextMenuItem {
            id: "pin.unpin".into(),
            label: "Unpin".into(),
            enabled: true,
        });
    } else {
        items.push(ContextMenuItem {
            id: "tab.pin".into(),
            label: "Pin".into(),
            enabled: !tab.private,
        });
    }

    items.push(ContextMenuItem {
        id: "tab.duplicate".into(),
        label: "Duplicate".into(),
        enabled: true,
    });
    items.push(ContextMenuItem {
        id: "tab.mute".into(),
        label: if tab.muted { "Unmute" } else { "Mute" }.into(),
        enabled: true,
    });
    items.push(ContextMenuItem {
        id: "tab.sleep".into(),
        label: "Put to Sleep".into(),
        enabled: can_sleep,
    });
    items.push(ContextMenuItem {
        id: "tab.close".into(),
        label: "Close Tab".into(),
        enabled: true,
    });
    items.push(ContextMenuItem {
        id: "tab.reopen".into(),
        label: "Reopen Closed Tab".into(),
        enabled: can_reopen_closed,
    });
    items
}

// ============================================================================
// P2-17 — Internal pages (`search://`, `FEATURES.md` §26)
// ============================================================================

/// The 8 internal `search://` page kinds (`FEATURES.md` §26, P2-17).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InternalPageKind {
    /// `search://settings` (and `search://settings/<section>`).
    Settings,
    /// `search://history`.
    History,
    /// `search://downloads`.
    Downloads,
    /// `search://bookmarks`.
    Bookmarks,
    /// `search://passwords`.
    Passwords,
    /// `search://archive`.
    Archive,
    /// `search://import` (and `search://import/<source>`).
    Import,
    /// `search://release-notes`.
    ReleaseNotes,
}

impl InternalPageKind {
    /// Classify a [`PageUrl`] into an [`InternalPageKind`], if it is one of the 8 internal pages.
    #[must_use]
    pub fn from_url(url: &PageUrl) -> Option<Self> {
        if !url.is_internal() {
            return None;
        }
        let display = url.display();
        let after = display.strip_prefix("search://")?;
        let head = after.split('/').next().unwrap_or_default();
        match head {
            "settings" => Some(Self::Settings),
            "history" => Some(Self::History),
            "downloads" => Some(Self::Downloads),
            "bookmarks" => Some(Self::Bookmarks),
            "passwords" => Some(Self::Passwords),
            "archive" => Some(Self::Archive),
            "import" => Some(Self::Import),
            "release-notes" => Some(Self::ReleaseNotes),
            _ => None,
        }
    }

    /// Canonical `search://` URL for this internal page with an optional detail sub-route.
    pub fn to_url(self, detail: Option<&str>) -> Result<PageUrl> {
        let base = match self {
            Self::Settings => "search://settings",
            Self::History => "search://history",
            Self::Downloads => "search://downloads",
            Self::Bookmarks => "search://bookmarks",
            Self::Passwords => "search://passwords",
            Self::Archive => "search://archive",
            Self::Import => "search://import",
            Self::ReleaseNotes => "search://release-notes",
        };
        let suffix = detail
            .map(|d| d.trim_matches('/'))
            .filter(|d| !d.is_empty());
        match suffix {
            Some(sub) => PageUrl::parse(&format!("{base}/{sub}")),
            None => PageUrl::parse(base),
        }
    }

    /// Default title for the internal page tab.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::History => "History",
            Self::Downloads => "Downloads",
            Self::Bookmarks => "Bookmarks",
            Self::Passwords => "Passwords",
            Self::Archive => "Archived Tabs",
            Self::Import => "Bring Things Over",
            Self::ReleaseNotes => "What's New",
        }
    }
}

/// Open or focus an internal `search://` page in `model` (`FEATURES.md` §26, P2-17):
/// - One tab of each [`InternalPageKind`] per window: asking a second time brings
///   the existing tab forward (updating its detail sub-route if needed).
/// - An empty non-private tab in front is reused.
/// - Private tabs **never** become internal pages (if the active tab is private,
///   a new non-private tab is opened instead).
pub fn open_internal_page(
    model: &mut TabModel,
    allocator: &mut crate::ids::TabIdAllocator,
    url: PageUrl,
) -> Result<TabId> {
    let Some(kind) = InternalPageKind::from_url(&url) else {
        return Err(CoreError::Invalid(format!(
            "{} is not an internal page",
            url.redacted()
        )));
    };

    // 1. Check if a non-private tab of this kind already exists in the window.
    if let Some(existing_idx) = model
        .tabs()
        .iter()
        .position(|t| !t.private && InternalPageKind::from_url(&t.url) == Some(kind))
    {
        model.select_index(existing_idx);
        model.active_tab_mut().url = url;
        model.active_tab_mut().title = kind.title().to_owned();
        return Ok(model.active_tab().id);
    }

    // 2. If the active tab is a reusable non-private blank tab, reuse it in place.
    if model.active_tab().is_reusable_blank() {
        let active = model.active_tab_mut();
        active.url = url;
        active.title = kind.title().to_owned();
        return Ok(active.id);
    }

    // 3. Otherwise open a new non-private foreground tab (never turn a private tab into an internal page).
    model.create_tab(allocator, Some(url), kind.title(), false, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::TabIdAllocator;

    #[test]
    fn site_card_gates_rows_accurately_for_normal_private_and_video_sites() {
        let settings = Settings::default();
        let perms = PermissionStore::new();
        let yt_url = PageUrl::parse("https://www.youtube.com/watch?v=abc").expect("yt");

        let card = SiteCardModel::build(&SiteCardInput {
            url: &yt_url,
            connection: ConnectionState::Secure,
            private_tab: false,
            zoom_percent: 110,
            engine_supports_print: true,
            engine_supports_floating_video: true,
            shield_in_trouble: false,
            site_blocker_on: true,
            dusk_paused: false,
            frame_is_dark: true,
            site_dusk_on: false,
            site_autoplay_sound: false,
            site_dont_float_videos: false,
            site_keep_awake: true,
            settings: &settings,
            permissions: &perms,
        });
        assert_eq!(card.header, "youtube.com");
        assert_eq!(
            card.certificate_action.as_deref(),
            Some("Show Certificate…")
        );
        assert_eq!(card.block_ads_and_trackers, Some(true));
        assert!(card.show_site_permissions);
        assert_eq!(card.play_sound_by_itself, Some(false));
        assert_eq!(card.dont_float_videos_here, Some(false));
        assert_eq!(card.keep_this_site_awake, Some(true));

        // In a private tab, blocker, autoplay, and dont_float_videos are hidden,
        // and permissions shows the private-tab notice.
        let priv_card = SiteCardModel::build(&SiteCardInput {
            private_tab: true,
            ..SiteCardInput {
                url: &yt_url,
                connection: ConnectionState::Secure,
                private_tab: true,
                zoom_percent: 100,
                engine_supports_print: true,
                engine_supports_floating_video: true,
                shield_in_trouble: false,
                site_blocker_on: true,
                dusk_paused: false,
                frame_is_dark: true,
                site_dusk_on: false,
                site_autoplay_sound: false,
                site_dont_float_videos: false,
                site_keep_awake: false,
                settings: &settings,
                permissions: &perms,
            }
        });
        assert!(priv_card.block_ads_and_trackers.is_none());
        assert!(priv_card.play_sound_by_itself.is_none());
        assert!(priv_card.dont_float_videos_here.is_none());
        assert_eq!(
            priv_card.permissions_notice.as_deref(),
            Some("Permissions aren't saved in a private tab")
        );
    }

    #[test]
    fn internal_pages_deduplicate_per_kind_and_never_convert_private_tabs() {
        let mut alloc = TabIdAllocator::new();
        let mut model = TabModel::new(&mut alloc).expect("model");

        // Active blank tab is reused for search://settings.
        let first_settings = open_internal_page(
            &mut model,
            &mut alloc,
            PageUrl::parse("search://settings").expect("url"),
        )
        .expect("open settings");
        assert_eq!(model.tabs().len(), 1);

        // Open a private tab in front.
        let priv_id = model
            .create_tab(
                &mut alloc,
                Some(PageUrl::parse("https://example.com/private").expect("url")),
                "Private",
                true,
                true,
            )
            .expect("private tab");
        assert_eq!(model.active_tab().id, priv_id);

        // Asking for search://settings/privacy brings the existing settings tab forward
        // rather than opening a second one or converting the private tab!
        let second_settings = open_internal_page(
            &mut model,
            &mut alloc,
            PageUrl::parse("search://settings/privacy").expect("url"),
        )
        .expect("focus existing settings");
        assert_eq!(first_settings, second_settings);
        assert_eq!(model.active_tab().id, first_settings);
        assert_eq!(model.active_tab().url.as_str(), "search://settings/privacy");
        assert!(model.tab(priv_id).expect("priv tab still intact").private);
    }

    #[test]
    fn context_menu_strips_click_trackers_and_disables_sleep_on_active_tab() {
        let dirty = PageUrl::parse(
            "https://example.com/story?utm_source=newsletter&fbclid=secret123&gclid=abc",
        )
        .expect("dirty");
        let cleaned = clean_link_url(&dirty);
        assert_eq!(
            cleaned.as_str(),
            "https://example.com/story?utm_source=newsletter"
        );

        let page_menu = build_page_context_menu(
            &dirty,
            Some(&dirty),
            true,
            Some("rust"),
            "Google",
            true,
            false,
            "English",
        );
        assert!(page_menu.iter().any(|i| i.label == "Open Link in New Tab"));
        assert!(
            page_menu
                .iter()
                .any(|i| i.label == "Open Link in Split View")
        );
        assert!(page_menu.iter().any(|i| i.label == "Copy Clean Link"));
        assert!(page_menu.iter().any(|i| i.label == "Open Image in New Tab"));
        assert!(page_menu.iter().any(|i| i.label == "Search with Google"));

        let mut alloc = TabIdAllocator::new();
        let model = TabModel::new(&mut alloc).expect("model");
        let active_id = model.active_tab().id;
        let tab_menu = build_tab_context_menu(&model, active_id, false);
        let sleep_item = tab_menu
            .iter()
            .find(|i| i.id == "tab.sleep")
            .expect("sleep item");
        assert!(!sleep_item.enabled, "active tab cannot be put to sleep");
    }

    #[test]
    fn site_card_surfaces_lookalike_and_permissions_summary() {
        let settings = Settings::default();
        let mut perms = PermissionStore::new();
        let url = PageUrl::parse("https://example.com/app").expect("url");
        perms
            .insert_raw_storage_key(
                "capture.https://example.com|camera",
                PermissionDecision::Allow,
            )
            .expect("allow");
        perms
            .insert_raw_storage_key(
                "capture.https://example.com:443|camera",
                PermissionDecision::Refuse,
            )
            .expect("conflict");

        let card = SiteCardModel::build_with_shortcuts(
            &SiteCardInput {
                url: &url,
                connection: ConnectionState::Secure,
                private_tab: false,
                zoom_percent: 100,
                engine_supports_print: true,
                engine_supports_floating_video: true,
                shield_in_trouble: false,
                site_blocker_on: true,
                dusk_paused: false,
                frame_is_dark: true,
                site_dusk_on: false,
                site_autoplay_sound: false,
                site_dont_float_videos: false,
                site_keep_awake: false,
                settings: &settings,
                permissions: &perms,
            },
            &SiteCardShortcuts {
                copy_address: "Ctrl+Shift+C".into(),
                darken_site: "Ctrl+Shift+D".into(),
            },
        );
        assert_eq!(card.copy_address_shortcut, "Ctrl+Shift+C");
        assert_eq!(card.darken_shortcut, "Ctrl+Shift+D");
        let sp = card.site_permissions.expect("permissions summary");
        assert_eq!(sp.conflicting_origins.len(), 2);
        assert_eq!(
            InternalPageKind::Settings
                .to_url(Some("privacy"))
                .expect("to_url")
                .as_str(),
            "search://settings/privacy"
        );
    }
}
