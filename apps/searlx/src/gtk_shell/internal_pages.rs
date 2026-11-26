//! Interactive `search://` internal pages (`FEATURES.md` §21, §26) served via
//! `WebKitGtkEngine::register_internal_handler` and bridged back to
//! `BrowserController` via isolated-world `ScriptMessage` (`INTERNAL_PAGE_CSP`).

use std::collections::BTreeMap;

use searlx_core::fuzz_and_hardening::{INTERNAL_PAGE_CSP, validate_ipc_message};
use searlx_core::{
    AddressBarWidth, AddressResolution, BookmarkItem, BookmarkTree, DownloadEntry, DownloadState,
    HistoryDayGroup, InternalPageData, InternalPageKind, LookPreference, NewTabPlace, PageUrl,
    ParseOptions, RecentlyClosedItem, SearchEngine, Settings, SettingsPage, SleepPace,
    TabGlyphMode, TabLayoutMode, UiSnapshot, resolve_address, search_settings,
};
use searlx_engine_linux::InternalPageResponse;
use serde::Deserialize;

use super::theme::ShellPalette;

/// Isolated-world JavaScript bridge installed ONLY on shell-owned `search://` WebViews.
///
/// Obeys `INTERNAL_PAGE_CSP` (`script-src 'self'` blocks inline scripts, so this
/// runs in WebKitGTK's isolated `"searlx"` world) and relays `[data-action]` UI
/// controls back to the GTK shell over `window.webkit.messageHandlers.searlx`.
pub const INTERNAL_PAGE_BRIDGE_JS: &str = r#"(() => {
    if (window.__searlxInternalBridgeInstalled) return;
    window.__searlxInternalBridgeInstalled = true;

    const send = (payload) => {
        try {
            if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.searlx) {
                window.webkit.messageHandlers.searlx.postMessage(JSON.stringify(payload));
            }
        } catch (_err) {}
    };

    const filterRows = (inputId, rowSelector) => {
        const input = document.getElementById(inputId);
        if (!input) return;
        const q = (input.value || '').trim().toLowerCase();
        const rows = document.querySelectorAll(rowSelector);
        rows.forEach((el) => {
            const hay = (el.getAttribute('data-search') || el.textContent || '').toLowerCase();
            el.style.display = (!q || hay.includes(q)) ? '' : 'none';
        });
    };

    document.addEventListener('input', (ev) => {
        const t = ev.target;
        if (!t) return;
        if (t.id === 'settings-search') {
            filterRows('settings-search', '.setting-row');
        } else if (t.id === 'history-search') {
            filterRows('history-search', '.history-row');
        }
    });

    document.addEventListener('change', (ev) => {
        const t = ev.target;
        if (!t || !t.dataset) return;
        if (t.dataset.action === 'set_setting') {
            const kind = t.dataset.type || 'string';
            let value = t.value;
            if (kind === 'bool') {
                value = !!t.checked;
            } else if (kind === 'number') {
                value = Number(t.value);
            }
            send({
                kind: 'internal_action',
                action: 'set_setting',
                key: t.dataset.key || '',
                value_type: kind,
                value: String(value)
            });
        }
    });

    document.addEventListener('submit', (ev) => {
        const form = ev.target;
        if (!form || !form.dataset || !form.dataset.actionForm) return;
        ev.preventDefault();
        const action = form.dataset.actionForm;
        if (action === 'newtab_address') {
            const inp = form.querySelector('input[name="address"]');
            if (inp && inp.value.trim()) {
                send({ kind: 'internal_action', action: 'submit_address', value: inp.value.trim() });
            }
        } else if (action === 'add_bookmark') {
            const titleEl = form.querySelector('input[name="title"]');
            const urlEl = form.querySelector('input[name="url"]');
            send({
                kind: 'internal_action',
                action: 'add_bookmark',
                title: titleEl ? titleEl.value : '',
                url: urlEl ? urlEl.value : ''
            });
        } else if (action === 'add_bookmark_folder') {
            const titleEl = form.querySelector('input[name="folder_title"]');
            send({
                kind: 'internal_action',
                action: 'add_bookmark_folder',
                title: titleEl ? titleEl.value : ''
            });
        }
    });

    document.addEventListener('click', (ev) => {
        const target = ev.target && ev.target.closest ? ev.target.closest('[data-action], a[href]') : null;
        if (!target) return;

        if (target.dataset && target.dataset.action) {
            const action = target.dataset.action;
            if (action === 'set_setting') return; // handled by change
            ev.preventDefault();
            send({
                kind: 'internal_action',
                action: action,
                key: target.dataset.key || '',
                value: target.dataset.value || '',
                url: target.dataset.url || '',
                id: target.dataset.id || '',
                page: target.dataset.page || '',
                detail: target.dataset.detail || ''
            });
            return;
        }

        const href = target.getAttribute('href') || '';
        if (href.startsWith('http://') || href.startsWith('https://') || href.startsWith('file://')) {
            ev.preventDefault();
            send({ kind: 'internal_action', action: 'navigate', url: href });
        } else if (href.startsWith('search://')) {
            ev.preventDefault();
            send({ kind: 'internal_action', action: 'navigate', url: href });
        }
    });
})();"#;

/// Thread-safe snapshot used by `WebKitGtkEngine::register_internal_handler`.
#[derive(Clone, Debug)]
pub struct InternalPageContext {
    pub settings: Settings,
    pub palette: ShellPalette,
    pub active_space_name: String,
    pub active_tab_private: bool,
    pub engine_name: String,
    pub capabilities_passkeys: bool,
    pub capabilities_extensions: bool,
    pub saved_permissions_count: usize,
    pub dusk_sites: BTreeMap<String, bool>,
    pub history_days: Vec<HistoryDayGroup>,
    pub recently_closed: Vec<RecentlyClosedItem>,
    pub recent_visits: Vec<(String, String)>,
    pub downloads: Vec<DownloadEntry>,
    pub bookmarks: BookmarkTree,
    pub passwords: Vec<(String, String, String, bool)>,
    pub archived_tabs: Vec<searlx_core::data::ArchivedTab>,
}

impl InternalPageContext {
    /// Build a render context from a controller [`UiSnapshot`] and explicit managers.
    #[must_use]
    pub fn from_snapshot(
        snapshot: &UiSnapshot,
        palette: ShellPalette,
        history_days: Vec<HistoryDayGroup>,
        recently_closed: Vec<RecentlyClosedItem>,
        bookmarks: BookmarkTree,
        passwords: Vec<(String, String, String, bool)>,
        archived_tabs: Vec<searlx_core::data::ArchivedTab>,
    ) -> Self {
        let active_space_name = snapshot
            .spaces
            .iter()
            .find(|s| s.active)
            .map_or_else(|| "Personal".to_owned(), |s| s.name.clone());
        let active_tab_private = snapshot
            .tabs
            .iter()
            .find(|t| t.active)
            .is_some_and(|t| t.private);

        let recent_visits: Vec<(String, String)> = history_days
            .iter()
            .flat_map(|g| g.visits.iter())
            .take(8)
            .map(|v| {
                let title = if v.title.trim().is_empty() {
                    v.url.display()
                } else {
                    v.title.clone()
                };
                (title, v.url.as_str().to_owned())
            })
            .collect();

        let (saved_permissions_count, dusk_sites) = match &snapshot.active_internal_page {
            Some(InternalPageData::Settings {
                saved_permissions_count,
                dusk_sites,
                ..
            }) => (*saved_permissions_count, dusk_sites.clone()),
            _ => (0, BTreeMap::new()),
        };

        Self {
            settings: snapshot.settings.clone(),
            palette,
            active_space_name,
            active_tab_private,
            engine_name: snapshot.capabilities.engine.clone(),
            capabilities_passkeys: snapshot.capabilities.passkeys,
            capabilities_extensions: snapshot.capabilities.extensions,
            saved_permissions_count,
            dusk_sites,
            history_days,
            recently_closed,
            recent_visits,
            downloads: snapshot.downloads.entries.clone(),
            bookmarks,
            passwords,
            archived_tabs,
        }
    }
}

/// Validated interactive action triggered inside a `search://` internal page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InternalAction {
    Navigate(PageUrl),
    SubmitAddress(String),
    OpenInternal {
        kind: InternalPageKind,
        detail: Option<String>,
    },
    SetSetting {
        key: String,
        value: String,
    },
    ClearHistory,
    ReopenClosed,
    AddBookmark {
        title: String,
        url: PageUrl,
    },
    AddBookmarkFolder {
        title: String,
    },
    RemoveBookmark {
        id: String,
    },
    PauseDownload(String),
    ResumeDownload(String),
    CancelDownload(String),
    RetryDownload(String),
    ForgetPermissions,
    ToggleDuskPause,
    ToggleSidebarFold,
}

#[derive(Deserialize)]
struct RawInternalPayload {
    kind: String,
    action: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    page: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    title: String,
}

/// Validate and parse an IPC JSON message emitted by an internal `search://` page.
///
/// Enforces [`validate_ipc_message`] with `expected_origin_prefix = Some("search://")`.
pub fn parse_internal_action(
    raw_json: &str,
    committed_uri: &str,
) -> Result<Option<InternalAction>, String> {
    let _map = validate_ipc_message(raw_json, committed_uri, Some("search://"))
        .map_err(|e| e.to_string())?;
    let payload: RawInternalPayload =
        serde_json::from_str(raw_json).map_err(|e| format!("invalid internal payload: {e}"))?;
    if payload.kind != "internal_action" {
        return Ok(None);
    }
    let action = match payload.action.as_str() {
        "navigate" => {
            let url = PageUrl::parse(&payload.url).map_err(|e| e.to_string())?;
            InternalAction::Navigate(url)
        }
        "submit_address" => InternalAction::SubmitAddress(payload.value),
        "open_internal" => {
            let kind = parse_internal_kind(&payload.page)
                .ok_or_else(|| format!("unknown internal page: {}", payload.page))?;
            let detail = (!payload.detail.trim().is_empty()).then_some(payload.detail);
            InternalAction::OpenInternal { kind, detail }
        }
        "set_setting" => InternalAction::SetSetting {
            key: payload.key,
            value: payload.value,
        },
        "clear_history" => InternalAction::ClearHistory,
        "reopen_closed" => InternalAction::ReopenClosed,
        "add_bookmark" => {
            let resolved = match resolve_address(&payload.url, &ParseOptions::default()) {
                AddressResolution::Navigate(u) => u,
                _ => PageUrl::parse(&payload.url).map_err(|e| e.to_string())?,
            };
            let title = if payload.title.trim().is_empty() {
                resolved.display()
            } else {
                payload.title.trim().to_owned()
            };
            InternalAction::AddBookmark {
                title,
                url: resolved,
            }
        }
        "add_bookmark_folder" => InternalAction::AddBookmarkFolder {
            title: payload.title.trim().to_owned(),
        },
        "remove_bookmark" => InternalAction::RemoveBookmark { id: payload.id },
        "download_pause" => InternalAction::PauseDownload(payload.id),
        "download_resume" => InternalAction::ResumeDownload(payload.id),
        "download_cancel" => InternalAction::CancelDownload(payload.id),
        "download_retry" => InternalAction::RetryDownload(payload.id),
        "forget_permissions" => InternalAction::ForgetPermissions,
        "toggle_dusk_pause" => InternalAction::ToggleDuskPause,
        "toggle_sidebar_fold" => InternalAction::ToggleSidebarFold,
        _ => return Ok(None),
    };
    Ok(Some(action))
}

/// Apply an interactive `set_setting` mutation to a cloned [`Settings`] struct.
/// Returns `true` if a known setting changed.
pub fn apply_setting_mutation(settings: &mut Settings, key: &str, raw_value: &str) -> bool {
    let parse_bool = || matches!(raw_value.trim(), "true" | "1" | "on");
    match key {
        "look" => {
            settings.look = match raw_value {
                "light" => LookPreference::Light,
                "dark" => LookPreference::Dark,
                _ => LookPreference::System,
            };
        }
        "tab_layout" => {
            settings.tab_layout = match raw_value {
                "right" => TabLayoutMode::RightColumn,
                "top" => TabLayoutMode::TopTabs,
                "icons" => TabLayoutMode::IconsOnly,
                _ => TabLayoutMode::LeftColumn,
            };
        }
        "address_shown" => settings.address_shown = parse_bool(),
        "address_width" => {
            settings.address_width = match raw_value {
                "medium" => AddressBarWidth::Medium,
                _ => AddressBarWidth::Small,
            };
        }
        "bookmarks_bar" => settings.bookmarks_bar = parse_bool(),
        "tabs_high_contrast" => settings.tabs_high_contrast = parse_bool(),
        "reduce_motion" => settings.reduce_motion = parse_bool(),
        "low_power_save" => settings.low_power_save = parse_bool(),
        "layout_cards" => settings.layout_cards = parse_bool(),
        "layout_cards_fullscreen" => settings.layout_cards_fullscreen = parse_bool(),
        "toolbar_left" => settings.toolbar_left = parse_bool(),
        "tabs_compact" => settings.tabs_compact = parse_bool(),
        "sidebar_roomy" => settings.sidebar_roomy = parse_bool(),
        "sidebar_hides" => settings.sidebar_hides = parse_bool(),
        "sidebar_fold_icons" => settings.sidebar_fold_icons = parse_bool(),
        "sidebar_doors_up" => settings.sidebar_doors_up = parse_bool(),
        "sidebar_player" => settings.sidebar_player = parse_bool(),
        "glyph" => {
            settings.glyph = match raw_value {
                "icons" => TabGlyphMode::Icons,
                _ => TabGlyphMode::Letters,
            };
        }
        "tabs_reading" => settings.tabs_reading = parse_bool(),
        "tabs_give" => settings.tabs_give = parse_bool(),
        "tabs_shortcuts" => settings.tabs_shortcuts = parse_bool(),
        "tabs_address" => settings.tabs_address = parse_bool(),
        "tabs_new_place" => {
            settings.tabs_new_place = match raw_value {
                "top" => NewTabPlace::Top,
                "end" => NewTabPlace::End,
                _ => NewTabPlace::Next,
            };
        }
        "tabs_front_beside" => settings.tabs_front_beside = parse_bool(),
        "tabs_new_after_last" => settings.tabs_new_after_last = parse_bool(),
        "tabs_close_goes_back" => settings.tabs_close_goes_back = parse_bool(),
        "tabs_undo_reopens" => settings.tabs_undo_reopens = parse_bool(),
        "tabs_lazy" => settings.tabs_lazy = parse_bool(),
        "tabs_sleep" => settings.tabs_sleep = parse_bool(),
        "tabs_sleep_pace" => {
            settings.tabs_sleep_pace = match raw_value {
                "never" => SleepPace::Never,
                "sooner" => SleepPace::Sooner,
                "later" => SleepPace::Later,
                _ => SleepPace::Normal,
            };
        }
        "tabs_sleep_dim" => settings.tabs_sleep_dim = parse_bool(),
        "tabs_switcher_behind_first" => settings.tabs_switcher_behind_first = parse_bool(),
        "tabs_switcher_leaves_out_pins" => settings.tabs_switcher_leaves_out_pins = parse_bool(),
        "search_engine" => {
            settings.search_engine = match raw_value {
                "duckduckgo" => SearchEngine::DuckDuckGo,
                "bing" => SearchEngine::Bing,
                "ecosia" => SearchEngine::Ecosia,
                "startpage" => SearchEngine::Startpage,
                "kagi" => SearchEngine::Kagi,
                "brave" => SearchEngine::Brave,
                "qwant" => SearchEngine::Qwant,
                _ => SearchEngine::Google,
            };
        }
        "search_suggest" => settings.search_suggest = parse_bool(),
        "search_keywords" => settings.search_keywords = parse_bool(),
        "page_zoom" => {
            if let Ok(z) = raw_value.trim().parse::<u16>() {
                settings.page_zoom = z.clamp(40, 300);
            } else {
                return false;
            }
        }
        "pages_dusk" => settings.pages_dusk = parse_bool(),
        "sites_styles" => settings.sites_styles = parse_bool(),
        "reader_twins" => settings.reader_twins = parse_bool(),
        "reader_follows" => settings.reader_follows = parse_bool(),
        "media_click" => settings.media_click = parse_bool(),
        "float_leave" => settings.float_leave = parse_bool(),
        "float_away" => settings.float_away = parse_bool(),
        "spaces" => settings.spaces = parse_bool(),
        "spaces_command_numbers" => settings.spaces_command_numbers = parse_bool(),
        "pins_home" => settings.pins_home = parse_bool(),
        "pins_peek" => settings.pins_peek = parse_bool(),
        "pins_list" => settings.pins_list = parse_bool(),
        "pins_folders" => settings.pins_folders = parse_bool(),
        "newtab_floating" => settings.newtab_floating = parse_bool(),
        "newtab_recent" => settings.newtab_recent = parse_bool(),
        "commandbar" => settings.commandbar = parse_bool(),
        "start_fresh" => settings.start_fresh = parse_bool(),
        "passwords_save" => settings.passwords_save = parse_bool(),
        "passwords_fill" => settings.passwords_fill = parse_bool(),
        "shield" => settings.shield = parse_bool(),
        "prevent_tracking" => settings.set_prevent_cross_site_tracking(parse_bool()),
        "privacy_click_ids" => settings.privacy_click_ids = parse_bool(),
        "privacy_gpc" => settings.privacy_gpc = parse_bool(),
        "notifications_ask" => settings.notifications_ask = parse_bool(),
        "downloads_ask" => settings.downloads_ask = parse_bool(),
        "downloads_button" => settings.downloads_button = parse_bool(),
        "downloads_dir" => {
            if !raw_value.trim().is_empty() {
                settings.downloads_dir = raw_value.trim().to_owned();
            }
        }
        "quit_confirm" => settings.quit_confirm = parse_bool(),
        "links_peek" => settings.links_peek = parse_bool(),
        "links_peek_stays" => settings.links_peek_stays = parse_bool(),
        "links_option_split" => settings.links_option_split = parse_bool(),
        "autoscroll" => settings.autoscroll = parse_bool(),
        "swipe_history" => settings.swipe_history = parse_bool(),
        "bench" => settings.bench = parse_bool(),
        "agents" => settings.agents = parse_bool(),
        _ => return false,
    }
    true
}

fn parse_internal_kind(raw: &str) -> Option<InternalPageKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "settings" => Some(InternalPageKind::Settings),
        "history" => Some(InternalPageKind::History),
        "downloads" => Some(InternalPageKind::Downloads),
        "bookmarks" => Some(InternalPageKind::Bookmarks),
        "passwords" => Some(InternalPageKind::Passwords),
        "archive" => Some(InternalPageKind::Archive),
        "import" => Some(InternalPageKind::Import),
        "release-notes" => Some(InternalPageKind::ReleaseNotes),
        _ => None,
    }
}

/// Render an [`InternalPageResponse`] for any valid `search://` URI.
#[must_use]
pub fn render_internal_uri(uri: &str, ctx: &InternalPageContext) -> InternalPageResponse {
    let trimmed = uri.trim();
    let stripped = trimmed
        .strip_prefix("search://")
        .unwrap_or("newtab")
        .trim_matches('/');
    let (head, detail) = match stripped.split_once('/') {
        Some((h, d)) => (h, Some(d)),
        None => (stripped, None),
    };

    let (title, body) = match head {
        "" | "newtab" => ("New Tab", render_newtab(ctx)),
        "settings" => {
            let page = detail
                .and_then(SettingsPage::from_slug)
                .unwrap_or(SettingsPage::General);
            ("Settings", render_settings(ctx, page))
        }
        "history" => ("History", render_history(ctx)),
        "downloads" => ("Downloads", render_downloads(ctx)),
        "bookmarks" => ("Bookmarks", render_bookmarks(ctx)),
        "passwords" => ("Passwords", render_passwords(ctx)),
        "archive" => ("Archived Tabs", render_archive(ctx)),
        "import" => ("Bring Things Over", render_import(detail)),
        "release-notes" => ("What's New", render_release_notes()),
        _ => ("Search", render_newtab(ctx)),
    };

    InternalPageResponse::html(wrap_document(title, &body, &ctx.palette, &ctx.settings))
}

fn wrap_document(title: &str, body: &str, palette: &ShellPalette, settings: &Settings) -> String {
    let font_px = super::theme::scaled_font_px(14.0, settings.text_scale);
    let scheme = if palette.dark { "dark" } else { "light" };
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="{INTERNAL_PAGE_CSP}">
<title>{title}</title>
<style>
:root {{
  color-scheme: {scheme};
  --ground: {ground};
  --chrome: {chrome};
  --hover: {hover};
  --selected: {selected};
  --text: {text};
  --muted: {muted};
  --line: {line};
  --page: {page};
  --accent: {accent};
  --accent-soft: {accent_soft};
  --danger: {danger};
  --warning: {warning};
  --secure: {secure};
}}
* {{ box-sizing: border-box; }}
body {{
  margin: 0;
  background: var(--page);
  color: var(--text);
  font-family: -apple-system, BlinkMacSystemFont, "Inter", "Cantarell", system-ui, sans-serif;
  font-size: {font_px}px;
  line-height: 1.5;
}}
.wrap {{
  max-width: 920px;
  margin: 0 auto;
  padding: 28px 28px 56px 28px;
}}
.hero {{
  max-width: 620px;
  margin: 72px auto 0 auto;
  padding: 0 20px;
}}
h1 {{
  font-size: 24px;
  font-weight: 700;
  margin: 0 0 6px 0;
  letter-spacing: -0.02em;
}}
h2 {{
  font-size: 15px;
  font-weight: 600;
  margin: 22px 0 8px 0;
  color: var(--muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}}
.sub {{
  color: var(--muted);
  margin: 0 0 20px 0;
  font-size: 13px;
}}
.search-input {{
  width: 100%;
  padding: 10px 14px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--chrome);
  color: var(--text);
  font-size: 14px;
  outline: none;
}}
.search-input:focus {{
  border-color: var(--accent);
}}
.card {{
  background: var(--chrome);
  border: 1px solid var(--line);
  border-radius: 12px;
  overflow: hidden;
  margin-bottom: 16px;
}}
.row {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--line);
}}
.row:last-child {{ border-bottom: none; }}
.row.dimmed {{ opacity: 0.55; }}
.row-title {{ font-weight: 500; }}
.row-desc {{ font-size: 12px; color: var(--muted); margin-top: 2px; }}
.badge {{
  display: inline-block;
  font-size: 11px;
  font-weight: 600;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--hover);
  color: var(--muted);
  border: 1px solid var(--line);
}}
.badge.warn {{
  color: var(--warning);
  border-color: var(--warning);
}}
.badge.ok {{
  color: var(--secure);
  border-color: var(--secure);
}}
.btn {{
  padding: 6px 12px;
  border-radius: 8px;
  border: 1px solid var(--line);
  background: var(--selected);
  color: var(--text);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
}}
.btn:hover {{ border-color: var(--accent); }}
.btn.primary {{
  background: var(--accent);
  color: #fff;
  border-color: var(--accent);
}}
.btn.danger {{
  background: var(--danger);
  color: #fff;
  border-color: var(--danger);
}}
select, input[type="text"], input[type="number"] {{
  padding: 6px 10px;
  border-radius: 8px;
  border: 1px solid var(--line);
  background: var(--selected);
  color: var(--text);
  font-size: 13px;
}}
.settings-shell {{
  display: grid;
  grid-template-columns: 205px 1fr;
  gap: 24px;
  align-items: start;
}}
.settings-nav {{
  position: sticky;
  top: 20px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}}
.nav-item {{
  text-align: left;
  padding: 8px 12px;
  border-radius: 8px;
  border: none;
  background: transparent;
  color: var(--muted);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
}}
.nav-item:hover {{
  background: var(--hover);
  color: var(--text);
}}
.nav-item.active {{
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}}
.quick-grid {{
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 16px;
}}
.link {{
  color: var(--accent);
  text-decoration: none;
}}
.link:hover {{ text-decoration: underline; }}
.mono {{
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
}}
</style>
</head>
<body>
{body}
</body>
</html>"#,
        title = html_escape(title),
        ground = palette.ground,
        chrome = palette.chrome,
        hover = palette.hover,
        selected = palette.selected,
        text = palette.text,
        muted = palette.muted,
        line = palette.line,
        page = palette.page,
        accent = palette.accent,
        accent_soft = palette.accent_soft,
        danger = palette.danger,
        warning = palette.warning,
        secure = palette.secure,
    )
}

fn render_newtab(ctx: &InternalPageContext) -> String {
    let private_pill = if ctx.active_tab_private {
        r#"<span class="badge warn">Private Tab — Nothing saved</span>"#
    } else {
        ""
    };
    let mut recent_html = String::new();
    if ctx.settings.newtab_recent && !ctx.active_tab_private && !ctx.recent_visits.is_empty() {
        recent_html.push_str(r#"<h2>Recent Places</h2><div class="card">"#);
        for (title, url) in &ctx.recent_visits {
            recent_html.push_str(&format!(
                r#"<div class="row">
                    <div>
                      <div class="row-title"><a class="link" href="{u}">{t}</a></div>
                      <div class="row-desc">{u}</div>
                    </div>
                  </div>"#,
                t = html_escape(title),
                u = html_escape(url),
            ));
        }
        recent_html.push_str("</div>");
    }

    format!(
        r#"<div class="hero">
  <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:14px;">
    <div>
      <h1>Search</h1>
      <p class="sub">Space: <strong>{space}</strong> &middot; Press <code>Ctrl+L</code> anytime for the floating field</p>
    </div>
    {private_pill}
  </div>
  <form data-action-form="newtab_address">
    <input class="search-input" type="text" name="address" placeholder="Enter an address or search with {engine}…" autofocus>
  </form>
  <div class="quick-grid">
    <button class="btn" data-action="open_internal" data-page="settings">Settings</button>
    <button class="btn" data-action="open_internal" data-page="bookmarks">Bookmarks</button>
    <button class="btn" data-action="open_internal" data-page="history">History</button>
    <button class="btn" data-action="open_internal" data-page="downloads">Downloads</button>
    <button class="btn" data-action="open_internal" data-page="archive">Archived Tabs</button>
    <button class="btn" data-action="open_internal" data-page="passwords">Passwords</button>
  </div>
  {recent_html}
</div>"#,
        space = html_escape(&ctx.active_space_name),
        engine = html_escape(ctx.settings.search_engine.name()),
    )
}

#[allow(clippy::too_many_lines)]
fn render_settings(ctx: &InternalPageContext, active_page: SettingsPage) -> String {
    let s = &ctx.settings;
    let visible = SettingsPage::visible_pages(s.ai_shipped);

    let mut nav = String::new();
    for &page in &visible {
        let active_cls = if page == active_page { " active" } else { "" };
        nav.push_str(&format!(
            r#"<button class="nav-item{active_cls}" data-action="open_internal" data-page="settings" data-detail="{slug}">{title}</button>"#,
            slug = page.slug(),
            title = html_escape(page.title()),
        ));
    }

    let section_rows = match active_page {
        SettingsPage::General => format!(
            r#"<div class="card">
  {dl_dir}
  {dl_ask}
  {dl_btn}
  {quit}
</div>
<div class="card">
  <div class="row setting-row" data-search="import bring things over chrome firefox arc safari">
    <div><div class="row-title">Bring things over</div><div class="row-desc">Import bookmarks, history, and passwords from another browser</div></div>
    <button class="btn" data-action="open_internal" data-page="import">Open Importer…</button>
  </div>
</div>"#,
            dl_dir = setting_text_row(
                "Save downloads to",
                "Default destination directory for downloaded files",
                "downloads_dir",
                &s.downloads_dir
            ),
            dl_ask = setting_bool_row(
                "Ask where to save each file",
                "Show a file dialog before starting a download",
                "downloads_ask",
                s.downloads_ask,
                false
            ),
            dl_btn = setting_bool_row(
                "Always show downloads button",
                "Keep the Downloads door visible in the sidebar footer",
                "downloads_button",
                s.downloads_button,
                false
            ),
            quit = setting_bool_row(
                "Confirm before quitting",
                "Prompt before closing the browser",
                "quit_confirm",
                s.quit_confirm,
                false
            ),
        ),
        SettingsPage::Appearance => format!(
            r#"<div class="card">
  {look}
  {layout}
  {fold_icons}
  {sidebar_hides}
  {roomy}
  {doors_up}
  {player}
</div>
<div class="card">
  {cards}
  {toolbar_left}
  {compact}
  {address_shown}
  {address_width}
  {bookmarks_bar}
  {glyph}
  {reading}
  {tabs_hc}
  {dusk}
  {reduce_motion}
  {low_power}
</div>"#,
            look = setting_select_row(
                "Appearance",
                "Choose Light, Dark, or follow the OS theme",
                "look",
                match s.look {
                    LookPreference::System => "system",
                    LookPreference::Light => "light",
                    LookPreference::Dark => "dark",
                },
                &[("system", "System"), ("light", "Light"), ("dark", "Dark")],
            ),
            layout = setting_select_row(
                "Tabs layout",
                "Left column, right column, top horizontal strip, or icons only",
                "tab_layout",
                match s.tab_layout {
                    TabLayoutMode::LeftColumn => "left",
                    TabLayoutMode::RightColumn => "right",
                    TabLayoutMode::TopTabs => "top",
                    TabLayoutMode::IconsOnly => "icons",
                },
                &[
                    ("left", "Left Column"),
                    ("right", "Right Column"),
                    ("top", "Top Tabs"),
                    ("icons", "Icons Only"),
                ],
            ),
            fold_icons = setting_bool_row(
                "Ctrl+S makes column icons only",
                "Toggle compact icon rail instead of completely hiding the sidebar",
                "sidebar_fold_icons",
                s.sidebar_fold_icons,
                false
            ),
            sidebar_hides = setting_bool_row(
                "Hide column until edge",
                "Reveal folded sidebar when pointer dwells on window edge",
                "sidebar_hides",
                s.sidebar_hides,
                false
            ),
            roomy = setting_bool_row(
                "Roomier tabs",
                "Use taller 38 pt rows and larger tab titles",
                "sidebar_roomy",
                s.sidebar_roomy,
                false
            ),
            doors_up = setting_bool_row(
                "Buttons at the top",
                "Move footer doors next to navigation controls",
                "sidebar_doors_up",
                s.sidebar_doors_up,
                false
            ),
            player = setting_bool_row(
                "What's playing in other tabs",
                "Show a compact audio control card above the sidebar foot",
                "sidebar_player",
                s.sidebar_player,
                false
            ),
            cards = setting_bool_row(
                "Page as a card",
                "Frame web pages with an 8 pt inset and 10 pt corner radius",
                "layout_cards",
                s.layout_cards,
                false
            ),
            toolbar_left = setting_bool_row(
                "Back/forward/reload on the left",
                "Place navigation arrows on the leading edge of the sidebar header",
                "toolbar_left",
                s.toolbar_left,
                false
            ),
            compact = setting_bool_row(
                "Shorter tab bar",
                "Compact height when tabs are across the top",
                "tabs_compact",
                s.tabs_compact,
                false
            ),
            address_shown = setting_bool_row(
                "Always show address bar",
                "Keep a compact inline address line visible in chrome",
                "address_shown",
                s.address_shown,
                false
            ),
            address_width = setting_select_row(
                "Address width",
                "Width of the compact inline address bar",
                "address_width",
                match s.address_width {
                    AddressBarWidth::Small => "small",
                    AddressBarWidth::Medium => "medium",
                },
                &[("small", "Small (220 pt)"), ("medium", "Medium (340 pt)")],
            ),
            bookmarks_bar = setting_bool_row(
                "Show bookmarks bar",
                "Display profile bookmarks above the page card",
                "bookmarks_bar",
                s.bookmarks_bar,
                false
            ),
            glyph = setting_select_row(
                "Tabs show",
                "Single-letter marks or site favicons",
                "glyph",
                match s.glyph {
                    TabGlyphMode::Letters => "letters",
                    TabGlyphMode::Icons => "icons",
                },
                &[("letters", "Letters"), ("icons", "Icons")],
            ),
            reading = setting_bool_row(
                "Show how far you've read",
                "Display scroll reading progress along the active tab row",
                "tabs_reading",
                s.tabs_reading,
                false
            ),
            tabs_hc = setting_bool_row(
                "High contrast text",
                "Strengthen chrome contrast and borders for readability",
                "tabs_high_contrast",
                s.tabs_high_contrast,
                false
            ),
            dusk = setting_bool_row(
                "Darken pages that stay light",
                &format!(
                    "Enable Dusk page darkening when chrome is dark ({} site override(s))",
                    ctx.dusk_sites.len()
                ),
                "pages_dusk",
                s.pages_dusk,
                false
            ),
            reduce_motion = setting_bool_row(
                "Reduce animations",
                "Collapse chrome motion durations to 0 ms",
                "reduce_motion",
                s.reduce_motion,
                false
            ),
            low_power = setting_bool_row(
                "Save power in Low Power Mode",
                "Shorten background sleep threshold when power saving is active",
                "low_power_save",
                s.low_power_save,
                false
            ),
        ),
        SettingsPage::Tabs => format!(
            r#"<div class="card">
  {new_place}
  {front_beside}
  {new_after_last}
  {close_back}
  {undo_reopens}
  {lazy}
  {sleep}
  {sleep_pace}
  {sleep_dim}
  {shortcuts}
  {tabs_address}
</div>"#,
            new_place = setting_select_row(
                "New tab position",
                "Where newly opened tabs appear in the list",
                "tabs_new_place",
                match s.tabs_new_place {
                    NewTabPlace::Next => "next",
                    NewTabPlace::Top => "top",
                    NewTabPlace::End => "end",
                },
                &[("next", "Next to active"), ("top", "Top"), ("end", "End")],
            ),
            front_beside = setting_bool_row(
                "Links opened in front go beside their page",
                "Place foreground link opens immediately after their opener",
                "tabs_front_beside",
                s.tabs_front_beside,
                false
            ),
            new_after_last = setting_bool_row(
                "Leave a new tab when the last closes",
                "Keep a fresh blank tab when closing the last loose tab",
                "tabs_new_after_last",
                s.tabs_new_after_last,
                false
            ),
            close_back = setting_bool_row(
                "Closing a tab goes back",
                "Select the previously viewed tab instead of neighbor",
                "tabs_close_goes_back",
                s.tabs_close_goes_back,
                false
            ),
            undo_reopens = setting_bool_row(
                "Ctrl+Z reopens a closed tab",
                "Undo closing a tab when it was the last action",
                "tabs_undo_reopens",
                s.tabs_undo_reopens,
                false
            ),
            lazy = setting_bool_row(
                "Load background tabs when you go to them",
                "Keep background-opened tabs asleep until first visit",
                "tabs_lazy",
                s.tabs_lazy,
                false
            ),
            sleep = setting_bool_row(
                "Sleep inactive tabs",
                "Release background WebKit processes after inactivity",
                "tabs_sleep",
                s.tabs_sleep,
                false
            ),
            sleep_pace = setting_select_row(
                "Sleep pace",
                "How quickly inactive background tabs go to sleep",
                "tabs_sleep_pace",
                match s.tabs_sleep_pace {
                    SleepPace::Never => "never",
                    SleepPace::Sooner => "sooner",
                    SleepPace::Normal => "normal",
                    SleepPace::Later => "later",
                },
                &[
                    ("never", "Never"),
                    ("sooner", "Sooner (10 min)"),
                    ("normal", "Normal (30 min)"),
                    ("later", "Later (2 hours)"),
                ],
            ),
            sleep_dim = setting_bool_row(
                "Dim sleeping tabs",
                "Dim titles on sleeping tabs in the sidebar",
                "tabs_sleep_dim",
                s.tabs_sleep_dim,
                false
            ),
            shortcuts = setting_bool_row(
                "Show tab numbers while Ctrl held",
                "Reveal 1..9 badges while holding Control",
                "tabs_shortcuts",
                s.tabs_shortcuts,
                false
            ),
            tabs_address = setting_bool_row(
                "Show address on the active tab",
                "Display the site host beneath the active tab title",
                "tabs_address",
                s.tabs_address,
                false
            ),
        ),
        SettingsPage::Browsing => format!(
            r#"<div class="card">
  {engine}
  {suggest}
  {keywords}
  {zoom}
  {reader_twins}
  {reader_follows}
  {sites_styles}
  {media_click}
</div>"#,
            engine = setting_select_row(
                "Search with",
                "Default search engine used from the address field",
                "search_engine",
                match s.search_engine {
                    SearchEngine::Google => "google",
                    SearchEngine::DuckDuckGo => "duckduckgo",
                    SearchEngine::Bing => "bing",
                    SearchEngine::Ecosia => "ecosia",
                    SearchEngine::Startpage => "startpage",
                    SearchEngine::Kagi => "kagi",
                    SearchEngine::Brave => "brave",
                    SearchEngine::Qwant => "qwant",
                    SearchEngine::Custom(_) => "google",
                },
                &[
                    ("google", "Google"),
                    ("duckduckgo", "DuckDuckGo"),
                    ("bing", "Bing"),
                    ("ecosia", "Ecosia"),
                    ("startpage", "Startpage"),
                    ("kagi", "Kagi"),
                    ("brave", "Brave"),
                    ("qwant", "Qwant"),
                ],
            ),
            suggest = setting_bool_row(
                "Suggestions from search engine",
                "Show live search suggestions (disabled in private tabs)",
                "search_suggest",
                s.search_suggest,
                false
            ),
            keywords = setting_bool_row(
                "Site shortcuts",
                "Allow keyword prefixes such as 'yt cats' or 'gh searlx'",
                "search_keywords",
                s.search_keywords,
                false
            ),
            zoom = setting_number_row(
                "Default page zoom (%)",
                "Default zoom factor for pages (40% to 300%)",
                "page_zoom",
                s.page_zoom,
                40,
                300,
                5
            ),
            reader_twins = setting_bool_row(
                "Read the site's own Markdown",
                "Prefer published Markdown twin documents in Reading Mode",
                "reader_twins",
                s.reader_twins,
                false
            ),
            reader_follows = setting_bool_row(
                "Reading mode follows light/dark",
                "Match Reading Mode surface to current window appearance",
                "reader_follows",
                s.reader_follows,
                false
            ),
            sites_styles = setting_bool_row(
                "Your own style for a site",
                "Enable per-site custom CSS boosts",
                "sites_styles",
                s.sites_styles,
                false
            ),
            media_click = setting_bool_row(
                "Videos wait for a click",
                "Require user interaction before media plays automatically",
                "media_click",
                s.media_click,
                false
            ),
        ),
        SettingsPage::SpacesAndPins => format!(
            r#"<div class="card">
  {spaces}
  {space_nums}
  {pins_home}
  {pins_peek}
  {pins_list}
  {pins_folders}
  {newtab_floating}
  {newtab_recent}
  {commandbar}
  {start_fresh}
</div>"#,
            spaces = setting_bool_row(
                "Spaces",
                "Separate workspaces with independent tab rows and optional isolated cookies",
                "spaces",
                s.spaces,
                false
            ),
            space_nums = setting_bool_row(
                "Ctrl+1..9 switch spaces",
                "Use number shortcuts for spaces instead of tabs",
                "spaces_command_numbers",
                s.spaces_command_numbers,
                false
            ),
            pins_home = setting_bool_row(
                "Pins keep their site",
                "Putting a pin down returns it to its pinned home address",
                "pins_home",
                s.pins_home,
                false
            ),
            pins_peek = setting_bool_row(
                "Cross-site links inside pins open in Peek",
                "Keep pins anchored on their home site",
                "pins_peek",
                s.pins_peek,
                false
            ),
            pins_list = setting_bool_row(
                "Pinned rows",
                "Display pins as titled rows instead of a 4-column icon grid",
                "pins_list",
                s.pins_list,
                false
            ),
            pins_folders = setting_bool_row(
                "Folders in pinned rows",
                "Organize pinned rows into collapsible folders",
                "pins_folders",
                s.pins_folders,
                !s.pins_list
            ),
            newtab_floating = setting_bool_row(
                "New tab in a floating field",
                "Ctrl+T raises the floating field before allocating a tab",
                "newtab_floating",
                s.newtab_floating,
                false
            ),
            newtab_recent = setting_bool_row(
                "Recent places in the new tab",
                "Show recent non-private history links on search://newtab",
                "newtab_recent",
                s.newtab_recent,
                false
            ),
            commandbar = setting_bool_row(
                "Address bar commands",
                "Allow typing whole-word commands such as Settings or History",
                "commandbar",
                s.commandbar,
                false
            ),
            start_fresh = setting_bool_row(
                "Start with a fresh window",
                "Do not restore previous session tabs on launch",
                "start_fresh",
                s.start_fresh,
                false
            ),
        ),
        SettingsPage::Passwords => format!(
            r#"<div class="card">
  {save}
  {fill}
  <div class="row setting-row" data-search="passkeys webauthn linux unsupported">
    <div>
      <div class="row-title">Passkeys (WebAuthn)</div>
      <div class="row-desc">Platform WebAuthn ceremonies are unavailable in WebKitGTK on Linux (ADR-0005)</div>
    </div>
    <span class="badge">{passkey_badge}</span>
  </div>
  <div class="row setting-row" data-search="passwords view vault">
    <div><div class="row-title">Saved passwords</div><div class="row-desc">Inspect saved accounts (masked secrets)</div></div>
    <button class="btn" data-action="open_internal" data-page="passwords">Open Passwords…</button>
  </div>
</div>"#,
            save = setting_bool_row(
                "Offer to save passwords",
                "Prompt to store sign-ins in the local vault",
                "passwords_save",
                s.passwords_save,
                false
            ),
            fill = setting_bool_row(
                "Fill in sign-ins",
                "Enable origin-verified credential autofill",
                "passwords_fill",
                s.passwords_fill,
                false
            ),
            passkey_badge = if ctx.capabilities_passkeys {
                "Supported"
            } else {
                "Unavailable on Linux"
            },
        ),
        SettingsPage::Privacy => format!(
            r#"<div class="card">
  {shield}
  {tracking}
  {click_ids}
  {gpc}
  {notif}
</div>
<div class="card">
  <div class="row setting-row" data-search="permissions forget choices camera mic location notifications">
    <div>
      <div class="row-title">Site permissions</div>
      <div class="row-desc">{perm_count} origin decision(s) stored</div>
    </div>
    <button class="btn" data-action="forget_permissions">Forget choices</button>
  </div>
  <div class="row setting-row" data-search="clear history browsing data cookies">
    <div>
      <div class="row-title">Clear browsing history</div>
      <div class="row-desc">Remove history, archive, cached favicons, and download source links</div>
    </div>
    <button class="btn danger" data-action="clear_history">Clear All History</button>
  </div>
</div>"#,
            shield = setting_bool_row(
                "Block ads and trackers",
                "Compile WebKit content-blocker rules and YouTube ad filters",
                "shield",
                s.shield,
                false
            ),
            tracking = setting_bool_row(
                "Prevent cross-site tracking",
                "Isolate third-party tracking state across sites",
                "prevent_tracking",
                s.prevent_cross_site_tracking(),
                false
            ),
            click_ids = setting_bool_row(
                "Remove click trackers",
                "Strip gclid, fbclid, msclkid, and similar click IDs on copy",
                "privacy_click_ids",
                s.privacy_click_ids,
                false
            ),
            gpc = setting_bool_row(
                "Global Privacy Control",
                "Signal GPC opt-out preference to sites",
                "privacy_gpc",
                s.privacy_gpc,
                false
            ),
            notif = setting_bool_row(
                "Let sites ask to send notifications",
                "Allow websites to request desktop notification permission",
                "notifications_ask",
                s.notifications_ask,
                false
            ),
            perm_count = ctx.saved_permissions_count,
        ),
        SettingsPage::Extensions => format!(
            r#"<div class="card">
  <div class="row setting-row" data-search="extensions chrome webextension webkitgtk linux">
    <div>
      <div class="row-title">Web Extensions</div>
      <div class="row-desc">Extensions require macOS WKWebExtension and are not supported on Linux WebKitGTK (ADR-0003).</div>
    </div>
    <span class="badge">{ext_badge}</span>
  </div>
</div>"#,
            ext_badge = if ctx.capabilities_extensions {
                "Supported"
            } else {
                "macOS Only"
            },
        ),
        SettingsPage::KeyboardAndMouse => format!(
            r#"<div class="card">
  {peek}
  {peek_stays}
  {opt_split}
  {autoscroll}
  {swipe_hist}
</div>
<div class="card">
  <div class="row"><div class="row-title">Floating Address Bar</div><span class="mono">Ctrl+L</span></div>
  <div class="row"><div class="row-title">New Tab / Private Tab</div><span class="mono">Ctrl+T / Ctrl+Shift+N</span></div>
  <div class="row"><div class="row-title">Fold Sidebar</div><span class="mono">Ctrl+S</span></div>
  <div class="row"><div class="row-title">Find on Page</div><span class="mono">Ctrl+F (Ctrl+G / Shift+Ctrl+G)</span></div>
  <div class="row"><div class="row-title">Split View / Focus Panes</div><span class="mono">Ctrl+Alt+N / Ctrl+Alt+Left|Right</span></div>
  <div class="row"><div class="row-title">Hide Elements / Darken Site</div><span class="mono">Shift+Ctrl+H / Shift+Ctrl+D</span></div>
  <div class="row"><div class="row-title">Reading Mode / Zoom</div><span class="mono">Shift+Ctrl+R / Ctrl+=/-/0</span></div>
</div>"#,
            peek = setting_bool_row(
                "Peek at a link",
                "Shift-click links to open a floating Peek preview",
                "links_peek",
                s.links_peek,
                false
            ),
            peek_stays = setting_bool_row(
                "Peek stays with its tab",
                "Preserve Peek panel when switching away and returning to a tab",
                "links_peek_stays",
                s.links_peek_stays,
                false
            ),
            opt_split = setting_bool_row(
                "Alt-click a link opens Split View",
                "Open modified link clicks beside the current page",
                "links_option_split",
                s.links_option_split,
                false
            ),
            autoscroll = setting_bool_row(
                "Scroll with middle button",
                "Enable middle-click auto-scroll on pages",
                "autoscroll",
                s.autoscroll,
                false
            ),
            swipe_hist = setting_bool_row(
                "Hold a swipe to pick from history",
                "Horizontal trackpad swipe back/forward navigation",
                "swipe_history",
                s.swipe_history,
                false
            ),
        ),
        SettingsPage::About | SettingsPage::Ai => format!(
            r#"<div class="card">
  <div class="row setting-row" data-search="about searlx version webkitgtk linux">
    <div>
      <div class="row-title">searlx (Search for Linux)</div>
      <div class="row-desc">Engine: {engine} &middot; No telemetry, no account, system webview per OS</div>
    </div>
    <button class="btn" data-action="open_internal" data-page="release-notes">What's New</button>
  </div>
  {bench}
  {agents}
</div>"#,
            engine = html_escape(&ctx.engine_name),
            bench = setting_bool_row(
                "Let a script drive Search",
                "Enable local automation probe hooks",
                "bench",
                s.bench,
                false
            ),
            agents = setting_bool_row(
                "Let coding agents use Search",
                "Allow web-only restricted coding agent tabs",
                "agents",
                s.agents,
                false
            ),
        ),
    };

    // Include cross-section search matches from `search_settings` at the bottom when user searches.
    let mut index_rows = String::new();
    for hit in search_settings("", s).into_iter().take(60) {
        let dimmed = if hit.dimmed_by_parent { " dimmed" } else { "" };
        let kind_badge = if hit.always_on_note {
            "Always on"
        } else {
            hit.page.title()
        };
        index_rows.push_str(&format!(
            r#"<div class="row setting-row{dimmed}" data-search="{search}" style="display:none">
  <div>
    <div class="row-title">{label}</div>
    <div class="row-desc">{page_title} &middot; <code>{key}</code></div>
  </div>
  <button class="btn" data-action="open_internal" data-page="settings" data-detail="{slug}">Open {kind_badge}</button>
</div>"#,
            search = html_escape(&format!("{} {} {}", hit.label, hit.key, hit.page.title())),
            label = html_escape(hit.label),
            page_title = html_escape(hit.page.title()),
            key = html_escape(hit.key),
            slug = hit.page.slug(),
        ));
    }

    format!(
        r#"<div class="wrap">
  <h1>Settings</h1>
  <p class="sub">Search for Linux &middot; {page_title}</p>
  <div style="margin-bottom:20px;">
    <input id="settings-search" class="search-input" type="search" placeholder="Search settings (Ctrl+F)…">
  </div>
  <div class="settings-shell">
    <nav class="settings-nav">{nav}</nav>
    <div>
      {section_rows}
      <div class="card">{index_rows}</div>
    </div>
  </div>
</div>"#,
        page_title = html_escape(active_page.title()),
    )
}

fn render_history(ctx: &InternalPageContext) -> String {
    let mut body = String::from(
        r#"<div class="wrap">
  <div style="display:flex;align-items:center;justify-content:space-between;">
    <div>
      <h1>History</h1>
      <p class="sub">Visits recorded in this profile (or isolated space). Private tabs never appear here.</p>
    </div>
    <button class="btn danger" data-action="clear_history">Clear All History</button>
  </div>
  <div style="margin-bottom:18px;">
    <input id="history-search" class="search-input" type="search" placeholder="Filter history by title or URL…">
  </div>"#,
    );

    if !ctx.recently_closed.is_empty() {
        body.push_str(
            r#"<div style="display:flex;align-items:center;justify-content:space-between;">
  <h2>Recently Closed</h2>
  <button class="btn" data-action="reopen_closed">Reopen Last Closed</button>
</div><div class="card">"#,
        );
        for item in &ctx.recently_closed {
            body.push_str(&format!(
                r#"<div class="row history-row" data-search="{search}">
  <div>
    <div class="row-title"><a class="link" href="{url}">{title}</a></div>
    <div class="row-desc">{url}</div>
  </div>
</div>"#,
                search = html_escape(&format!("{} {}", item.title, item.url.display())),
                url = html_escape(item.url.as_str()),
                title = html_escape(&item.title),
            ));
        }
        body.push_str("</div>");
    }

    if ctx.history_days.is_empty() {
        body.push_str(r#"<div class="card"><div class="row"><div class="row-desc">No history visits recorded yet.</div></div></div>"#);
    } else {
        for group in &ctx.history_days {
            body.push_str(&format!(
                "<h2>{}</h2><div class=\"card\">",
                html_escape(&group.label)
            ));
            for v in &group.visits {
                let title = if v.title.trim().is_empty() {
                    v.url.display()
                } else {
                    v.title.clone()
                };
                body.push_str(&format!(
                    r#"<div class="row history-row" data-search="{search}">
  <div>
    <div class="row-title"><a class="link" href="{url}">{title}</a></div>
    <div class="row-desc">{display} &middot; {count} visit(s)</div>
  </div>
  <span class="badge">{stamp}</span>
</div>"#,
                    search = html_escape(&format!("{title} {}", v.url.display())),
                    url = html_escape(v.url.as_str()),
                    title = html_escape(&title),
                    display = html_escape(&v.url.display()),
                    count = v.visit_count,
                    stamp = html_escape(&v.last_visit),
                ));
            }
            body.push_str("</div>");
        }
    }

    body.push_str("</div>");
    body
}

fn render_downloads(ctx: &InternalPageContext) -> String {
    let mut rows = String::new();
    if ctx.downloads.is_empty() {
        rows.push_str(r#"<div class="row"><div class="row-desc">No downloads yet. Private downloads are unlisted.</div></div>"#);
    } else {
        for entry in &ctx.downloads {
            let state_label = match &entry.state {
                DownloadState::InProgress => "In progress".to_owned(),
                DownloadState::Paused => "Paused".to_owned(),
                DownloadState::Completed => "Completed".to_owned(),
                DownloadState::Failed { reason } => format!("Failed: {reason}"),
                DownloadState::Cancelled => "Cancelled".to_owned(),
            };
            let source = entry
                .source_url
                .as_ref()
                .map_or_else(|| "Source cleared".to_owned(), PageUrl::display);
            let mut actions = String::new();
            if entry.can_pause() {
                actions.push_str(&format!(
                    r#"<button class="btn" data-action="download_pause" data-id="{id}">Pause</button>"#,
                    id = html_escape(&entry.id)
                ));
            }
            if matches!(entry.state, DownloadState::Paused) {
                actions.push_str(&format!(
                    r#"<button class="btn" data-action="download_resume" data-id="{id}">Resume</button>"#,
                    id = html_escape(&entry.id)
                ));
            }
            if matches!(
                entry.state,
                DownloadState::InProgress | DownloadState::Paused
            ) {
                actions.push_str(&format!(
                    r#"<button class="btn" data-action="download_cancel" data-id="{id}">Cancel</button>"#,
                    id = html_escape(&entry.id)
                ));
            }
            if entry.can_retry() {
                actions.push_str(&format!(
                    r#"<button class="btn" data-action="download_retry" data-id="{id}">Retry</button>"#,
                    id = html_escape(&entry.id)
                ));
            }
            rows.push_str(&format!(
                r#"<div class="row">
  <div>
    <div class="row-title">{name}</div>
    <div class="row-desc">{source} &middot; {bytes} bytes &middot; {state}</div>
  </div>
  <div style="display:flex;gap:6px;">{actions}</div>
</div>"#,
                name = html_escape(&entry.file_name),
                source = html_escape(&source),
                bytes = entry.bytes_received,
                state = html_escape(&state_label),
            ));
        }
    }

    format!(
        r#"<div class="wrap">
  <h1>Downloads</h1>
  <p class="sub">Saving to <code>{dir}</code></p>
  <div class="card">{rows}</div>
</div>"#,
        dir = html_escape(&ctx.settings.downloads_dir),
    )
}

fn render_bookmarks(ctx: &InternalPageContext) -> String {
    let mut rows = String::new();
    if ctx.bookmarks.roots.is_empty() {
        rows.push_str(r#"<div class="row"><div class="row-desc">No bookmarks yet. Press Shift+Ctrl+B on any page or add one below.</div></div>"#);
    } else {
        render_bookmark_items(&ctx.bookmarks.roots, 0, &mut rows);
    }

    format!(
        r#"<div class="wrap">
  <h1>Bookmarks</h1>
  <p class="sub">Shared across all spaces in the active profile.</p>
  <div class="card" style="padding:14px 16px;">
    <form data-action-form="add_bookmark" style="display:flex;gap:8px;flex-wrap:wrap;">
      <input type="text" name="title" placeholder="Bookmark title" style="flex:1;min-width:160px;">
      <input type="text" name="url" placeholder="https://example.com" style="flex:2;min-width:220px;">
      <button class="btn primary" type="submit">Add Bookmark</button>
    </form>
  </div>
  <div class="card">{rows}</div>
</div>"#
    )
}

fn render_bookmark_items(items: &[BookmarkItem], depth: usize, out: &mut String) {
    let pad = 16 + depth * 20;
    for item in items {
        match item {
            BookmarkItem::Link { id, title, url } => {
                out.push_str(&format!(
                    r#"<div class="row" style="padding-left:{pad}px;">
  <div>
    <div class="row-title"><a class="link" href="{url}">{title}</a></div>
    <div class="row-desc">{display}</div>
  </div>
  <button class="btn" data-action="remove_bookmark" data-id="{id}">Remove</button>
</div>"#,
                    url = html_escape(url.as_str()),
                    title = html_escape(title),
                    display = html_escape(&url.display()),
                    id = html_escape(id),
                ));
            }
            BookmarkItem::Folder {
                id,
                title,
                children,
            } => {
                out.push_str(&format!(
                    r#"<div class="row" style="padding-left:{pad}px;">
  <div>
    <div class="row-title">Folder: {title}</div>
    <div class="row-desc">{count} item(s)</div>
  </div>
  <button class="btn" data-action="remove_bookmark" data-id="{id}">Remove</button>
</div>"#,
                    title = html_escape(title),
                    count = children.len(),
                    id = html_escape(id),
                ));
                render_bookmark_items(children, depth + 1, out);
            }
        }
    }
}

fn render_passwords(ctx: &InternalPageContext) -> String {
    let mut rows = String::new();
    if ctx.passwords.is_empty() {
        rows.push_str(r#"<div class="row"><div class="row-desc">No saved credentials in the vault.</div></div>"#);
    } else {
        for (host, user, last_used, clear_http) in &ctx.passwords {
            let warn = if *clear_http {
                r#"<span class="badge warn">Saved over HTTP</span>"#
            } else {
                r#"<span class="badge ok">HTTPS</span>"#
            };
            rows.push_str(&format!(
                r#"<div class="row">
  <div>
    <div class="row-title">{host} &middot; {user}</div>
    <div class="row-desc">Password: &bull;&bull;&bull;&bull;&bull;&bull;&bull;&bull; &middot; Last used: {last}</div>
  </div>
  {warn}
</div>"#,
                host = html_escape(host),
                user = html_escape(user),
                last = html_escape(last_used),
            ));
        }
    }

    format!(
        r#"<div class="wrap">
  <h1>Passwords</h1>
  <p class="sub">Masked vault view — secrets are never rendered into web content or logs.</p>
  <div class="card">{rows}</div>
</div>"#
    )
}

fn render_archive(ctx: &InternalPageContext) -> String {
    let mut rows = String::new();
    if ctx.archived_tabs.is_empty() {
        rows.push_str(r#"<div class="row"><div class="row-desc">No archived auto-closed tabs yet.</div></div>"#);
    } else {
        for tab in &ctx.archived_tabs {
            let title = if tab.title.trim().is_empty() {
                tab.url.clone()
            } else {
                tab.title.clone()
            };
            rows.push_str(&format!(
                r#"<div class="row">
  <div>
    <div class="row-title"><a class="link" href="{url}">{title}</a></div>
    <div class="row-desc">{url} &middot; Closed {closed}</div>
  </div>
</div>"#,
                url = html_escape(&tab.url),
                title = html_escape(&title),
                closed = html_escape(&tab.closed_at),
            ));
        }
    }

    format!(
        r#"<div class="wrap">
  <h1>Archived Tabs</h1>
  <p class="sub">Auto-closed tabs retained for 30 days / 500 entries.</p>
  <div class="card">{rows}</div>
</div>"#
    )
}

fn render_import(source: Option<&str>) -> String {
    let selected = source.unwrap_or("search");
    format!(
        r#"<div class="wrap">
  <h1>Bring Things Over</h1>
  <p class="sub">Active source: <code>{selected}</code> &middot; Existing Search JSON stores, Chrome, Firefox, and Safari profiles.</p>
  <div class="card">
    <div class="row">
      <div><div class="row-title">Search (macOS JSON store)</div><div class="row-desc">Imports bookmarks, history, spaces, pins, and rules safely with symlink confinement.</div></div>
      <span class="badge">Supported</span>
    </div>
  </div>
</div>"#,
        selected = html_escape(selected),
    )
}

fn render_release_notes() -> String {
    r#"<div class="wrap">
  <h1>What's New in searlx</h1>
  <p class="sub">GTK3 + WebKitGTK shell driven by BrowserController.</p>
  <div class="card">
    <div class="row"><div><div class="row-title">Quiet Search Chrome</div><div class="row-desc">No permanent toolbar, resizable sidebar, folded edge-peek, and rounded 10 pt page card.</div></div></div>
    <div class="row"><div><div class="row-title">Floating Address Field</div><div class="row-desc">Ctrl+L overlay with live frecency suggestions, Tab-to-search, and shake validation.</div></div></div>
    <div class="row"><div><div class="row-title">Lazy Sleep & Split View</div><div class="row-desc">Session tabs restore asleep; 2-4 pane Split View with draggable dividers and drag-to-split edges.</div></div></div>
  </div>
</div>"#
        .to_owned()
}

fn setting_bool_row(title: &str, desc: &str, key: &str, checked: bool, dimmed: bool) -> String {
    let checked_attr = if checked { " checked" } else { "" };
    let dimmed_cls = if dimmed { " dimmed" } else { "" };
    format!(
        r#"<div class="row setting-row{dimmed_cls}" data-search="{search}">
  <div><div class="row-title">{title}</div><div class="row-desc">{desc}</div></div>
  <input type="checkbox" data-action="set_setting" data-type="bool" data-key="{key}"{checked_attr}>
</div>"#,
        search = html_escape(&format!("{title} {desc} {key}")),
        title = html_escape(title),
        desc = html_escape(desc),
        key = html_escape(key),
    )
}

fn setting_select_row(
    title: &str,
    desc: &str,
    key: &str,
    current: &str,
    options: &[(&str, &str)],
) -> String {
    let mut opts = String::new();
    for &(val, label) in options {
        let sel = if val == current { " selected" } else { "" };
        opts.push_str(&format!(
            r#"<option value="{val}"{sel}>{label}</option>"#,
            val = html_escape(val),
            label = html_escape(label),
        ));
    }
    format!(
        r#"<div class="row setting-row" data-search="{search}">
  <div><div class="row-title">{title}</div><div class="row-desc">{desc}</div></div>
  <select data-action="set_setting" data-type="string" data-key="{key}">{opts}</select>
</div>"#,
        search = html_escape(&format!("{title} {desc} {key}")),
        title = html_escape(title),
        desc = html_escape(desc),
        key = html_escape(key),
    )
}

fn setting_number_row(
    title: &str,
    desc: &str,
    key: &str,
    value: u16,
    min: u16,
    max: u16,
    step: u16,
) -> String {
    format!(
        r#"<div class="row setting-row" data-search="{search}">
  <div><div class="row-title">{title}</div><div class="row-desc">{desc}</div></div>
  <input type="number" min="{min}" max="{max}" step="{step}" value="{value}" data-action="set_setting" data-type="number" data-key="{key}">
</div>"#,
        search = html_escape(&format!("{title} {desc} {key}")),
        title = html_escape(title),
        desc = html_escape(desc),
        key = html_escape(key),
    )
}

fn setting_text_row(title: &str, desc: &str, key: &str, value: &str) -> String {
    format!(
        r#"<div class="row setting-row" data-search="{search}">
  <div><div class="row-title">{title}</div><div class="row-desc">{desc}</div></div>
  <input type="text" value="{val}" data-action="set_setting" data-type="string" data-key="{key}">
</div>"#,
        search = html_escape(&format!("{title} {desc} {key}")),
        title = html_escape(title),
        desc = html_escape(desc),
        key = html_escape(key),
        val = html_escape(value),
    )
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use searlx_core::{BrowserController, EngineCapabilities, RestoreMode};

    #[test]
    fn renders_all_internal_routes_with_strict_csp_and_no_inline_scripts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (ctrl, _) = BrowserController::open(
            dir.path(),
            EngineCapabilities::baseline("webkitgtk"),
            RestoreMode::FreshStart,
        )
        .expect("open");
        let snap = ctrl.ui_snapshot();
        let palette = ShellPalette::resolve(&snap.settings, true);
        let ctx = InternalPageContext::from_snapshot(
            &snap,
            palette,
            Vec::new(),
            Vec::new(),
            BookmarkTree::new(),
            vec![(
                "example.com".into(),
                "alice".into(),
                "2026-11-24".into(),
                true,
            )],
            Vec::new(),
        );

        for uri in [
            "search://",
            "search://newtab",
            "search://settings",
            "search://settings/appearance",
            "search://settings/privacy",
            "search://history",
            "search://bookmarks",
            "search://downloads",
            "search://passwords",
            "search://archive",
            "search://import",
            "search://release-notes",
        ] {
            let resp = render_internal_uri(uri, &ctx);
            assert_eq!(resp.mime_type, "text/html; charset=utf-8");
            let html = String::from_utf8(resp.body).expect("utf8");
            assert!(html.contains(INTERNAL_PAGE_CSP), "missing CSP for {uri}");
            assert!(
                !html.contains("<script"),
                "inline <script> forbidden in {uri}"
            );
            assert!(
                !html.contains("onclick="),
                "inline onclick forbidden in {uri}"
            );
        }
    }

    #[test]
    fn ipc_parser_rejects_external_origins_and_applies_settings_mutations() {
        let ok = parse_internal_action(
            r#"{"kind":"internal_action","action":"set_setting","key":"look","value":"dark"}"#,
            "search://settings/appearance",
        )
        .expect("parse ok");
        assert_eq!(
            ok,
            Some(InternalAction::SetSetting {
                key: "look".into(),
                value: "dark".into(),
            })
        );

        let rejected = parse_internal_action(
            r#"{"kind":"internal_action","action":"clear_history"}"#,
            "https://evil.example.com/",
        );
        assert!(rejected.is_err());

        let mut settings = Settings::default();
        assert!(apply_setting_mutation(&mut settings, "look", "dark"));
        assert_eq!(settings.look, LookPreference::Dark);
        assert!(apply_setting_mutation(
            &mut settings,
            "prevent_tracking",
            "false"
        ));
        assert!(settings.sites_keep);
    }
}
