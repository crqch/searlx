//! Address field parsing, look-alike defence, search engines, site search,
//! completions, and suggestions (`FEATURES.md` §4, P1-07).
//!
//! Parsing rules (`FEATURES.md` §4.2):
//! - Whitespace → not an address → search (unless a keyword shortcut matches).
//! - `http://`, `https://`, `file://`, `about:`, `data:`, `search://` are
//!   accepted as written; other schemes are refused (`mailto:` is parsed as
//!   [`AddressResolution::Mailto`] or refused when only page URLs are allowed).
//! - `/path` and `~/path` open a local file if it exists; non-existent paths
//!   are rejected so private paths are never sent to a search engine.
//! - Anything with `@` is treated as an email address and refused.
//! - `[::1]:port` → `http://`; `localhost`, `*.localhost`, `*.local`,
//!   `127.0.0.1`, `0.0.0.0`, and private IPv4 ranges → `http://`; everything
//!   else gets `https://`.
//! - `0.0.0.0` is rewritten to `localhost` because WebKit won't load it.
//! - A host must have a letter TLD (≥2 letters); dotted version numbers are not
//!   hosts.
//! - Look-alike / confusable IDN hosts are kept in punycode (`xn--…`) and
//!   flagged by [`inspect_host_lookalike`].

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::domain::{BookmarkItem, BookmarkTree, HistoryVisit};
use crate::error::{CoreError, Result};
use crate::ids::TabId;
use crate::url::PageUrl;

/// Built-in search engines (`FEATURES.md` §4.4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    /// Google (`https://www.google.com/search?q=%s`).
    #[default]
    Google,
    /// DuckDuckGo (`https://duckduckgo.com/?q=%s`).
    DuckDuckGo,
    /// Bing (`https://www.bing.com/search?q=%s`).
    Bing,
    /// Ecosia (`https://www.ecosia.org/search?q=%s`).
    Ecosia,
    /// Startpage (`https://www.startpage.com/do/dsearch?query=%s`).
    Startpage,
    /// Kagi (`https://kagi.com/search?q=%s`).
    Kagi,
    /// Brave (`https://search.brave.com/search?q=%s`).
    Brave,
    /// Qwant (`https://www.qwant.com/?q=%s`).
    Qwant,
    /// User-supplied template containing `%s` or `{searchTerms}`.
    Custom(String),
}

impl SearchEngine {
    /// Display name of the search engine.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Google => "Google",
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Bing => "Bing",
            Self::Ecosia => "Ecosia",
            Self::Startpage => "Startpage",
            Self::Kagi => "Kagi",
            Self::Brave => "Brave",
            Self::Qwant => "Qwant",
            Self::Custom(_) => "Custom",
        }
    }

    /// URL template using `%s` for the query.
    #[must_use]
    pub fn template(&self) -> &str {
        match self {
            Self::Google => "https://www.google.com/search?q=%s",
            Self::DuckDuckGo => "https://duckduckgo.com/?q=%s",
            Self::Bing => "https://www.bing.com/search?q=%s",
            Self::Ecosia => "https://www.ecosia.org/search?q=%s",
            Self::Startpage => "https://www.startpage.com/do/dsearch?query=%s",
            Self::Kagi => "https://kagi.com/search?q=%s",
            Self::Brave => "https://search.brave.com/search?q=%s",
            Self::Qwant => "https://www.qwant.com/?q=%s",
            Self::Custom(template) => template.as_str(),
        }
    }

    /// Build a search [`PageUrl`] for `query`.
    pub fn search_url(&self, query: &str) -> Result<PageUrl> {
        expand_search_template(self.template(), query)
    }
}

/// Expand a `%s` or `{searchTerms}` URL template safely, validating that the
/// substituted query cannot alter the destination host (`FEATURES.md` §4.4).
pub fn expand_search_template(template: &str, query: &str) -> Result<PageUrl> {
    let placeholder_url = template
        .replace("{searchTerms}", "__SEARLX_QUERY__")
        .replace("%s", "__SEARLX_QUERY__");
    let base = PageUrl::parse(&placeholder_url)?;
    let expected_host = base.host().map(ToOwned::to_owned);

    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let expanded = template
        .replace("{searchTerms}", &encoded)
        .replace("%s", &encoded);
    let final_url = PageUrl::parse(&expanded)?;
    if final_url.host() != expected_host.as_deref() {
        return Err(CoreError::InvalidUrl(
            "search query altered the template destination host".into(),
        ));
    }
    Ok(final_url)
}

/// A tab-to-search site or keyword shortcut (`FEATURES.md` §4.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteSearch {
    /// Display name (e.g. `"YouTube"`).
    pub name: String,
    /// Canonical host (e.g. `"youtube.com"`).
    pub host: String,
    /// Trigger aliases / keywords (e.g. `["youtube", "yt"]`).
    pub aliases: Vec<String>,
    /// Search template with `%s`.
    pub template: String,
}

impl SiteSearch {
    /// Build a search [`PageUrl`] on this site.
    pub fn search_url(&self, query: &str) -> Result<PageUrl> {
        expand_search_template(&self.template, query)
    }
}

/// The 16 built-in tab-to-search sites (`FEATURES.md` §4.4).
#[must_use]
pub fn builtin_site_searches() -> Vec<SiteSearch> {
    vec![
        SiteSearch {
            name: "Google".into(),
            host: "google.com".into(),
            aliases: vec!["google".into(), "g".into()],
            template: "https://www.google.com/search?q=%s".into(),
        },
        SiteSearch {
            name: "Reddit".into(),
            host: "reddit.com".into(),
            aliases: vec!["reddit".into(), "r".into()],
            template: "https://www.reddit.com/search/?q=%s".into(),
        },
        SiteSearch {
            name: "YouTube".into(),
            host: "youtube.com".into(),
            aliases: vec!["youtube".into(), "yt".into()],
            template: "https://www.youtube.com/results?search_query=%s".into(),
        },
        SiteSearch {
            name: "X".into(),
            host: "x.com".into(),
            aliases: vec!["x".into(), "twitter".into()],
            template: "https://x.com/search?q=%s".into(),
        },
        SiteSearch {
            name: "ChatGPT".into(),
            host: "chatgpt.com".into(),
            aliases: vec!["chatgpt".into(), "gpt".into(), "openai".into()],
            template: "https://chatgpt.com/?q=%s".into(),
        },
        SiteSearch {
            name: "Claude".into(),
            host: "claude.ai".into(),
            aliases: vec!["claude".into()],
            template: "https://claude.ai/new?q=%s".into(),
        },
        SiteSearch {
            name: "Perplexity".into(),
            host: "perplexity.ai".into(),
            aliases: vec!["perplexity".into(), "pplx".into()],
            template: "https://www.perplexity.ai/search?q=%s".into(),
        },
        SiteSearch {
            name: "Wikipedia".into(),
            host: "en.wikipedia.org".into(),
            aliases: vec!["wikipedia".into(), "wiki".into()],
            template: "https://en.wikipedia.org/w/index.php?search=%s".into(),
        },
        SiteSearch {
            name: "GitHub".into(),
            host: "github.com".into(),
            aliases: vec!["github".into(), "gh".into()],
            template: "https://github.com/search?q=%s".into(),
        },
        SiteSearch {
            name: "Stack Overflow".into(),
            host: "stackoverflow.com".into(),
            aliases: vec!["stackoverflow".into(), "so".into()],
            template: "https://stackoverflow.com/search?q=%s".into(),
        },
        SiteSearch {
            name: "MDN".into(),
            host: "developer.mozilla.org".into(),
            aliases: vec!["mdn".into()],
            template: "https://developer.mozilla.org/en-US/search?q=%s".into(),
        },
        SiteSearch {
            name: "Amazon".into(),
            host: "amazon.com".into(),
            aliases: vec!["amazon".into(), "amz".into()],
            template: "https://www.amazon.com/s?k=%s".into(),
        },
        SiteSearch {
            name: "Google Maps".into(),
            host: "maps.google.com".into(),
            aliases: vec!["maps".into(), "gmaps".into()],
            template: "https://www.google.com/maps/search/%s".into(),
        },
        SiteSearch {
            name: "IMDb".into(),
            host: "imdb.com".into(),
            aliases: vec!["imdb".into()],
            template: "https://www.imdb.com/find/?q=%s".into(),
        },
        SiteSearch {
            name: "Spotify".into(),
            host: "open.spotify.com".into(),
            aliases: vec!["spotify".into()],
            template: "https://open.spotify.com/search/%s".into(),
        },
        SiteSearch {
            name: "Figma Community".into(),
            host: "figma.com".into(),
            aliases: vec!["figma".into()],
            template: "https://www.figma.com/community/search?query=%s".into(),
        },
    ]
}

/// Find a tab-to-search site matching a typed prefix or alias (`FEATURES.md` §4.4).
#[must_use]
pub fn match_site_search_offer<'a>(typed: &str, sites: &'a [SiteSearch]) -> Option<&'a SiteSearch> {
    let needle = typed.trim().to_ascii_lowercase();
    if needle.is_empty() || needle.contains(char::is_whitespace) {
        return None;
    }
    sites.iter().find(|site| {
        site.aliases
            .iter()
            .any(|alias| alias == &needle || (needle.len() >= 2 && alias.starts_with(&needle)))
            || (needle.len() >= 2 && site.name.to_ascii_lowercase().starts_with(&needle))
            || (needle.len() >= 2 && site.host.to_ascii_lowercase().starts_with(&needle))
    })
}

/// Built-in known sites for field suggestions and completion (`FEATURES.md` §4.3).
pub const BUILTIN_KNOWN_SITES: &[(&str, &str)] = &[
    ("Google", "https://www.google.com/"),
    ("Gmail", "https://mail.google.com/"),
    ("Google Drive", "https://drive.google.com/"),
    ("Google Calendar", "https://calendar.google.com/"),
    ("Google Maps", "https://maps.google.com/"),
    ("YouTube", "https://www.youtube.com/"),
    ("GitHub", "https://github.com/"),
    ("Figma", "https://www.figma.com/"),
    ("Notion", "https://www.notion.so/"),
    ("Linear", "https://linear.app/"),
    ("Slack", "https://app.slack.com/"),
    ("Discord", "https://discord.com/"),
    ("X", "https://x.com/"),
    ("LinkedIn", "https://www.linkedin.com/"),
    ("Instagram", "https://www.instagram.com/"),
    ("Reddit", "https://www.reddit.com/"),
    ("Hacker News", "https://news.ycombinator.com/"),
    ("Stack Overflow", "https://stackoverflow.com/"),
    ("Claude", "https://claude.ai/"),
    ("ChatGPT", "https://chatgpt.com/"),
    ("Dribbble", "https://dribbble.com/"),
    ("Behance", "https://www.behance.net/"),
    ("Awwwards", "https://www.awwwards.com/"),
    ("Mobbin", "https://mobbin.com/"),
    ("SiteInspire", "https://www.siteinspire.com/"),
    ("Are.na", "https://www.are.na/"),
    ("Pinterest", "https://www.pinterest.com/"),
    ("Framer", "https://www.framer.com/"),
    ("Webflow", "https://webflow.com/"),
    ("Apple Developer", "https://developer.apple.com/"),
    ("Swift", "https://www.swift.org/"),
    ("npm", "https://www.npmjs.com/"),
    ("Supabase", "https://supabase.com/"),
    ("Stripe", "https://stripe.com/"),
    ("Shopify", "https://www.shopify.com/"),
    ("Cloudflare", "https://www.cloudflare.com/"),
    ("Netlify", "https://www.netlify.com/"),
];

/// Address-bar commands (`FEATURES.md` §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddressCommand {
    /// Open `search://settings`.
    Settings,
    /// Open a new tab.
    NewTab,
    /// Open a new private tab.
    NewPrivateTab,
    /// Create a new space.
    NewSpace,
    /// Open `search://bookmarks`.
    Bookmarks,
    /// Open `search://history`.
    History,
    /// Open `search://downloads`.
    Downloads,
    /// Open `search://passwords`.
    Passwords,
    /// Fold or show the sidebar (`⌘S`).
    ToggleSidebar,
}

impl AddressCommand {
    /// Human-readable command label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::NewTab => "New Tab",
            Self::NewPrivateTab => "New Private Tab",
            Self::NewSpace => "New Space",
            Self::Bookmarks => "Bookmarks",
            Self::History => "History",
            Self::Downloads => "Downloads",
            Self::Passwords => "Passwords",
            Self::ToggleSidebar => "Toggle Sidebar",
        }
    }

    /// Match an exact whole-word command (`FEATURES.md` §4.3).
    #[must_use]
    pub fn match_exact(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "settings" | "preferences" => Some(Self::Settings),
            "new tab" => Some(Self::NewTab),
            "new private tab" | "private tab" => Some(Self::NewPrivateTab),
            "new space" => Some(Self::NewSpace),
            "bookmarks" => Some(Self::Bookmarks),
            "history" => Some(Self::History),
            "downloads" => Some(Self::Downloads),
            "passwords" => Some(Self::Passwords),
            "toggle sidebar" | "fold sidebar" | "hide sidebar" => Some(Self::ToggleSidebar),
            _ => None,
        }
    }
}

/// Why an address-field submission was rejected and must shake (`FEATURES.md` §3.6, §4.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddressInvalidReason {
    /// Empty or whitespace-only input.
    Empty,
    /// Contains `@` (email address or userinfo credential).
    EmailOrUserinfo,
    /// Unsupported or dangerous scheme (e.g. `javascript:`, `ftp:`, `mailto:` when refused).
    UnsupportedScheme(String),
    /// A local file path (`/path` or `~/path`) that does not exist on disk.
    MissingLocalFile(PathBuf),
    /// A malformed host, port, or dotted version number that cannot be loaded as a URL.
    MalformedAddress(String),
}

/// Where a submitted address or search should open (`FEATURES.md` §4.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubmitDisposition {
    /// Plain `Return`: navigate the current tab.
    #[default]
    CurrentTab,
    /// `⌘Return`: open in a new tab behind.
    NewTabBehind,
    /// `⇧⌘Return`: open in a new tab in front.
    NewTabInFront,
}

impl SubmitDisposition {
    /// Compute disposition from keyboard modifiers on `Return`.
    #[must_use]
    pub const fn from_modifiers(command: bool, shift: bool) -> Self {
        match (command, shift) {
            (true, true) => Self::NewTabInFront,
            (true, false) => Self::NewTabBehind,
            (false, _) => Self::CurrentTab,
        }
    }
}

/// Configuration for [`resolve_address`].
#[derive(Clone, Debug)]
pub struct ParseOptions {
    /// Primary search engine.
    pub engine: SearchEngine,
    /// Optional secondary engine for `⇧Return` (`FEATURES.md` §4.4).
    pub shift_engine: Option<SearchEngine>,
    /// Whether `⇧Return` was pressed.
    pub shift_pressed: bool,
    /// Active locked site-search chip (`Tab`-to-search).
    pub site_chip: Option<SiteSearch>,
    /// Whether keyword shortcuts (`"yt cats"`) are enabled (`search.keywords`).
    pub keywords_enabled: bool,
    /// Available site searches / keywords.
    pub site_searches: Vec<SiteSearch>,
    /// Whether address-bar commands (`commandbar`) are enabled.
    pub commands_enabled: bool,
    /// Home directory used to expand `~/path`.
    pub home_dir: Option<PathBuf>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            engine: SearchEngine::Google,
            shift_engine: None,
            shift_pressed: false,
            site_chip: None,
            keywords_enabled: false,
            site_searches: builtin_site_searches(),
            commands_enabled: false,
            home_dir: std::env::var_os("HOME").map(PathBuf::from),
        }
    }
}

/// Result of resolving user input in the address field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddressResolution {
    /// Navigate directly to a validated [`PageUrl`].
    Navigate(PageUrl),
    /// Perform a search query (via primary/shift engine, site chip, or keyword).
    Search {
        /// Human-readable source name (e.g. `"Google"` or `"YouTube"`).
        source: String,
        /// Search terms.
        query: String,
        /// Expanded search URL.
        url: PageUrl,
    },
    /// Execute a browser command.
    Command(AddressCommand),
    /// Input is not a valid destination; the field shakes and does not navigate.
    Invalid(AddressInvalidReason),
}

/// Resolve raw address-field input using `std::path::Path::exists` for local file checks.
#[must_use]
pub fn resolve_address(input: &str, options: &ParseOptions) -> AddressResolution {
    resolve_address_with_fs(input, options, |path| path.exists())
}

/// Resolve raw address-field input with an injectable file-existence predicate for tests.
#[allow(clippy::too_many_lines)]
pub fn resolve_address_with_fs<F>(
    input: &str,
    options: &ParseOptions,
    file_exists: F,
) -> AddressResolution
where
    F: Fn(&Path) -> bool,
{
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return AddressResolution::Invalid(AddressInvalidReason::Empty);
    }

    // 1. If a site chip is locked in the field, the entire text searches that site.
    if let Some(site) = &options.site_chip {
        return match site.search_url(trimmed) {
            Ok(url) => AddressResolution::Search {
                source: site.name.clone(),
                query: trimmed.to_owned(),
                url,
            },
            Err(err) => {
                AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(err.to_string()))
            }
        };
    }

    // 2. Shift-Return with a secondary engine searches the exact typed text (`FEATURES.md` §4.4).
    if options.shift_pressed
        && let Some(shift_engine) = &options.shift_engine
    {
        return match shift_engine.search_url(trimmed) {
            Ok(url) => AddressResolution::Search {
                source: shift_engine.name().to_owned(),
                query: trimmed.to_owned(),
                url,
            },
            Err(err) => {
                AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(err.to_string()))
            }
        };
    }

    // 3. Optional whole-word command matching (`commandbar`).
    if options.commands_enabled
        && let Some(cmd) = AddressCommand::match_exact(trimmed)
    {
        return AddressResolution::Command(cmd);
    }

    // 4. Local file paths (`/path` and `~/path`): open if it exists, shake if not (`FEATURES.md` §4.2).
    if trimmed.starts_with('/') || trimmed.starts_with("~/") || trimmed == "~" {
        let candidate = if trimmed == "~" {
            options
                .home_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from("/"))
        } else if let Some(rel) = trimmed.strip_prefix("~/") {
            options
                .home_dir
                .as_ref()
                .map_or_else(|| PathBuf::from(trimmed), |home| home.join(rel))
        } else {
            PathBuf::from(trimmed)
        };
        if file_exists(&candidate)
            && let Ok(url) = Url::from_file_path(&candidate)
            && let Ok(page_url) = PageUrl::from_url(&url)
        {
            return AddressResolution::Navigate(page_url);
        }
        return AddressResolution::Invalid(AddressInvalidReason::MissingLocalFile(candidate));
    }

    // 5. Whitespace inside the input -> keyword search or general search (`FEATURES.md` §4.2).
    if trimmed.chars().any(char::is_whitespace) {
        // However, if the user explicitly typed a scheme like `http://foo bar`, refuse it.
        if has_explicit_scheme(trimmed).is_some() {
            return AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(
                trimmed.to_owned(),
            ));
        }
        if options.keywords_enabled
            && let Some((keyword, rest)) = trimmed.split_once(char::is_whitespace)
        {
            let rest = rest.trim();
            let kw = keyword.to_ascii_lowercase();
            if !rest.is_empty()
                && let Some(site) = options
                    .site_searches
                    .iter()
                    .find(|site| site.aliases.iter().any(|alias| alias == &kw))
                && let Ok(url) = site.search_url(rest)
            {
                return AddressResolution::Search {
                    source: site.name.clone(),
                    query: rest.to_owned(),
                    url,
                };
            }
        }
        return search_with_engine(&options.engine, trimmed);
    }

    // 6. Explicit scheme (`scheme:...`).
    if let Some(scheme) = has_explicit_scheme(trimmed) {
        let lower = scheme.to_ascii_lowercase();
        if !matches!(
            lower.as_str(),
            "http" | "https" | "file" | "about" | "data" | "search"
        ) {
            return AddressResolution::Invalid(AddressInvalidReason::UnsupportedScheme(lower));
        }
        // Refuse `@` userinfo even on explicit http/https URLs in the field (`FEATURES.md` §4.2).
        if matches!(lower.as_str(), "http" | "https") && authority_has_at_sign(trimmed) {
            return AddressResolution::Invalid(AddressInvalidReason::EmailOrUserinfo);
        }
        return match PageUrl::parse(&rewrite_zero_host_with_scheme(trimmed)) {
            Ok(page_url) => {
                if matches!(page_url.scheme(), "http" | "https")
                    && !is_valid_web_url_host(&page_url)
                {
                    AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(
                        trimmed.to_owned(),
                    ))
                } else {
                    AddressResolution::Navigate(page_url)
                }
            }
            Err(err) => {
                AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(err.to_string()))
            }
        };
    }

    // 7. Anything with `@` (without whitespace) is treated as an email address and refused (`FEATURES.md` §4.2).
    if trimmed.contains('@') {
        return AddressResolution::Invalid(AddressInvalidReason::EmailOrUserinfo);
    }

    // 8. Check if it is a bare host/address (has a dot, colon/port, slash, IPv6 brackets, or is `localhost`).
    let looks_like_address = trimmed.contains('.')
        || trimmed.contains(':')
        || trimmed.contains('/')
        || trimmed.starts_with('[')
        || trimmed.eq_ignore_ascii_case("localhost");

    if !looks_like_address {
        // Single word without punctuation -> search query.
        return search_with_engine(&options.engine, trimmed);
    }

    // Split host[:port] from path/query/fragment.
    let (authority, suffix) = split_authority_and_rest(trimmed);
    if authority.is_empty() {
        return AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(
            trimmed.to_owned(),
        ));
    }

    let Some((scheme, normalized_host, port)) = classify_bare_authority(authority) else {
        return AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(
            trimmed.to_owned(),
        ));
    };

    let candidate = match port {
        Some(p) => format!("{scheme}://{normalized_host}:{p}{suffix}"),
        None => format!("{scheme}://{normalized_host}{suffix}"),
    };

    match PageUrl::parse(&candidate) {
        Ok(url) => AddressResolution::Navigate(url),
        Err(err) => {
            AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(err.to_string()))
        }
    }
}

fn search_with_engine(engine: &SearchEngine, query: &str) -> AddressResolution {
    match engine.search_url(query) {
        Ok(url) => AddressResolution::Search {
            source: engine.name().to_owned(),
            query: query.to_owned(),
            url,
        },
        Err(err) => {
            AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(err.to_string()))
        }
    }
}

/// Detect an explicit `scheme:` prefix when the prefix is not a `host:port`
/// pair (e.g. `localhost:3000` or `example.com:8080` is a host + numeric port,
/// whereas `mailto:a@b.com`, `http://...`, `about:blank`, `javascript:alert(1)`
/// have a scheme).
fn has_explicit_scheme(input: &str) -> Option<&str> {
    if input.starts_with('[') {
        return None;
    }
    let (prefix, rest) = input.split_once(':')?;
    if prefix.is_empty()
        || !prefix.chars().next()?.is_ascii_alphabetic()
        || !prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    if rest.starts_with("//") {
        return Some(prefix);
    }
    // Without `//`, if `rest` starts with digits (and stops at `/`, `?`, `#`, or end),
    // `prefix:rest` is `host:port` (like `localhost:3000`).
    let port_part = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if !port_part.is_empty() && port_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(prefix)
}

fn authority_has_at_sign(url_str: &str) -> bool {
    let Some((_, after_scheme)) = url_str.split_once("://") else {
        return false;
    };
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    authority.contains('@')
}

fn rewrite_zero_host_with_scheme(url_str: &str) -> String {
    let Some((scheme, after_scheme)) = url_str.split_once("://") else {
        return url_str.to_owned();
    };
    let (authority, suffix) = split_authority_and_rest(after_scheme);
    if authority == "0.0.0.0" {
        format!("{scheme}://localhost{suffix}")
    } else if let Some(port) = authority.strip_prefix("0.0.0.0:") {
        format!("{scheme}://localhost:{port}{suffix}")
    } else {
        url_str.to_owned()
    }
}

fn split_authority_and_rest(input: &str) -> (&str, &str) {
    match input.find(['/', '?', '#']) {
        Some(idx) => (&input[..idx], &input[idx..]),
        None => (input, ""),
    }
}

fn classify_bare_authority(authority: &str) -> Option<(&'static str, String, Option<u16>)> {
    // IPv6 literal: `[::1]` or `[::1]:3000`
    if let Some(after_bracket) = authority.strip_prefix('[') {
        let (ipv6_str, rest) = after_bracket.split_once(']')?;
        let ipv6: Ipv6Addr = ipv6_str.parse().ok()?;
        let port = if rest.is_empty() {
            None
        } else {
            let port_str = rest.strip_prefix(':')?;
            Some(port_str.parse::<u16>().ok()?)
        };
        let scheme = if ipv6.is_loopback() || ipv6.is_unique_local() || ipv6.is_unspecified() {
            "http"
        } else {
            "https"
        };
        return Some((scheme, format!("[{ipv6}]"), port));
    }

    let (host_raw, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            if h.is_empty() || p.is_empty() {
                return None;
            }
            let parsed_port = p.parse::<u16>().ok()?;
            (h, Some(parsed_port))
        }
        None => (authority, None),
    };

    if host_raw == "0.0.0.0" {
        return Some(("http", "localhost".to_owned(), port));
    }

    if let Ok(ipv4) = host_raw.parse::<Ipv4Addr>() {
        let scheme = if is_local_or_private_ipv4(ipv4) {
            "http"
        } else {
            "https"
        };
        return Some((scheme, ipv4.to_string(), port));
    }

    let lower = host_raw.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
        if !is_syntactically_valid_hostname(&lower) {
            return None;
        }
        return Some(("http", lower, port));
    }

    // Must be a valid hostname with a letter TLD of at least 2 letters (`FEATURES.md` §4.2).
    let ascii_host = idna_to_ascii(&lower)?;
    if !has_valid_letter_tld(&ascii_host) {
        return None;
    }
    Some(("https", ascii_host, port))
}

fn is_local_or_private_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
}

fn idna_to_ascii(host: &str) -> Option<String> {
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return None;
    }
    let parsed = Url::parse(&format!("https://{host}")).ok()?;
    let host_str = parsed.host_str()?.to_owned();
    Some(host_str)
}

fn is_syntactically_valid_hostname(host: &str) -> bool {
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

fn has_valid_letter_tld(ascii_host: &str) -> bool {
    if !is_syntactically_valid_hostname(ascii_host) {
        return false;
    }
    let mut labels = ascii_host.split('.');
    let Some(first) = labels.next() else {
        return false;
    };
    let Some(tld) = labels.next_back() else {
        // Single-label host without a TLD
        return false;
    };
    let _ = first;
    if let Some(puny) = tld.strip_prefix("xn--") {
        return puny.len() >= 2 && puny.chars().all(|c| c.is_ascii_alphanumeric());
    }
    tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

fn is_valid_web_url_host(url: &PageUrl) -> bool {
    match url.as_url().host() {
        Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)) => true,
        Some(url::Host::Domain(domain)) => {
            let lower = domain.to_ascii_lowercase();
            if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
                is_syntactically_valid_hostname(&lower)
            } else {
                has_valid_letter_tld(&lower)
            }
        }
        None => false,
    }
}

/// Information about a host's IDN / look-alike status (`FEATURES.md` §4.2, §29).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostLookalikeInfo {
    /// ASCII / punycode host (`xn--…`).
    pub ascii_host: String,
    /// Whether the host uses IDN punycode or contains characters that can pass
    /// for Latin (homoglyphs / mixed scripts) and must remain displayed as
    /// `xn--…` punycode on the tab and site card.
    pub is_lookalike: bool,
    /// Safe display host (always preserves punycode when `is_lookalike` is true).
    pub safe_display_host: String,
}

/// Inspect a raw or parsed host for IDN look-alike confusables (`FEATURES.md` §4.2, §29).
#[must_use]
pub fn inspect_host_lookalike(raw_host: &str) -> HostLookalikeInfo {
    let trimmed = raw_host.trim().trim_start_matches("www.");
    let ascii_host = idna_to_ascii(trimmed).unwrap_or_else(|| trimmed.to_ascii_lowercase());
    let has_punycode = ascii_host.split('.').any(|label| label.starts_with("xn--"));
    let has_confusable_unicode = trimmed.chars().any(is_latin_confusable_char)
        || has_mixed_ascii_and_non_ascii_label(trimmed);
    let is_lookalike = has_punycode || has_confusable_unicode;
    HostLookalikeInfo {
        safe_display_host: ascii_host.clone(),
        ascii_host,
        is_lookalike,
    }
}

fn has_mixed_ascii_and_non_ascii_label(host: &str) -> bool {
    host.split('.').any(|label| {
        let has_ascii_alpha = label.chars().any(|c| c.is_ascii_alphabetic());
        let has_non_ascii = !label.is_ascii();
        has_ascii_alpha && has_non_ascii
    })
}

/// Characters from Cyrillic, Greek, and Latin-lookalike ranges commonly used in
/// IDN homograph attacks against Latin domains.
fn is_latin_confusable_char(ch: char) -> bool {
    matches!(
        ch,
        // Cyrillic homoglyphs for a, c, e, i, j, k, m, o, p, s, t, u, x, y
        '\u{0430}'
            | '\u{0441}'
            | '\u{0435}'
            | '\u{0456}'
            | '\u{0458}'
            | '\u{043A}'
            | '\u{043C}'
            | '\u{043E}'
            | '\u{0440}'
            | '\u{0455}'
            | '\u{0442}'
            | '\u{0443}'
            | '\u{0445}'
            | '\u{044B}'
            | '\u{0410}'..='\u{044F}'
            // Greek homoglyphs
            | '\u{0391}'..='\u{03C9}'
    )
}

/// An open tab candidate for address-field suggestions (`FEATURES.md` §4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenTabCandidate {
    /// Tab handle.
    pub id: TabId,
    /// Tab title.
    pub title: String,
    /// Tab URL.
    pub url: PageUrl,
}

/// A row in the suggestion list under the address field (`FEATURES.md` §4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuggestionRow {
    /// Switch to an already-open tab (up to 6, excluding the current tab).
    OpenTab {
        /// Matching tab handle.
        tab: TabId,
        /// Tab title.
        title: String,
        /// Tab URL.
        url: PageUrl,
    },
    /// Lock a site-search chip with `Tab` (`Search YouTube…`).
    SiteSearchOffer {
        /// Matching site search definition.
        site: SiteSearch,
    },
    /// A visited history entry scored by frecency and prefix/substring fit.
    History {
        /// Page title.
        title: String,
        /// Page URL.
        url: PageUrl,
        /// Computed frecency score.
        score: u32,
    },
    /// A saved bookmark or bookmark folder slip.
    Bookmark {
        /// Bookmark title.
        title: String,
        /// Bookmark URL (`None` for a folder slip).
        url: Option<PageUrl>,
    },
    /// A built-in known site (`FEATURES.md` §4.3).
    KnownSite {
        /// Site name.
        title: String,
        /// Site URL.
        url: PageUrl,
    },
    /// An internal page or command (`FEATURES.md` §4.3).
    Command(AddressCommand),
    /// The final "search these words" fallback row.
    SearchWords {
        /// Search engine name.
        engine: String,
        /// Search query.
        query: String,
        /// Target search URL.
        url: PageUrl,
    },
}

/// Input corpus for building address-field suggestions and inline grey completion.
#[derive(Debug)]
pub struct SuggestionContext<'a> {
    /// Active tab ID (excluded from open-tab suggestions).
    pub current_tab: Option<TabId>,
    /// Open tabs in the current space/profile.
    pub open_tabs: &'a [OpenTabCandidate],
    /// History visits for the current profile/space.
    pub history: &'a [HistoryVisit],
    /// Bookmark tree for the current profile.
    pub bookmarks: &'a BookmarkTree,
    /// Available site searches.
    pub site_searches: &'a [SiteSearch],
    /// Primary search engine.
    pub engine: &'a SearchEngine,
    /// Whether address commands (`commandbar`) are enabled.
    pub commands_enabled: bool,
}

/// Compute the grey inline completion suffix for `typed` (`FEATURES.md` §4.3).
///
/// Remember: `Return` accepts the grey completion; `Tab` walks suggestions or
/// locks a site-search chip (`FEATURES.md` §4.3, §29).
#[must_use]
pub fn inline_completion(typed: &str, ctx: &SuggestionContext<'_>) -> Option<String> {
    let needle = typed.trim();
    if needle.is_empty() || needle.contains(char::is_whitespace) {
        return None;
    }
    let lower = needle.to_ascii_lowercase();

    // 1. Check history hosts by visit count (highest visit count first).
    let mut best_history: Option<(u32, String)> = None;
    for visit in ctx.history {
        if let Some(host) = visit.url.host() {
            let clean = host.trim_start_matches("www.");
            if clean.starts_with(&lower) && clean.len() > lower.len() {
                let candidate_suffix = clean[lower.len()..].to_owned();
                if best_history
                    .as_ref()
                    .is_none_or(|(score, _)| visit.visit_count > *score)
                {
                    best_history = Some((visit.visit_count, candidate_suffix));
                }
            }
        }
    }
    if let Some((_, suffix)) = best_history {
        return Some(suffix);
    }

    // 2. Check built-in known sites.
    for (_, raw_url) in BUILTIN_KNOWN_SITES {
        if let Ok(url) = PageUrl::parse(raw_url)
            && let Some(host) = url.host()
        {
            let clean = host.trim_start_matches("www.");
            if clean.starts_with(&lower) && clean.len() > lower.len() {
                return Some(clean[lower.len()..].to_owned());
            }
        }
    }
    None
}

/// Build the ordered suggestion list under the address field (`FEATURES.md` §4.3).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn build_suggestions(typed: &str, ctx: &SuggestionContext<'_>) -> Vec<SuggestionRow> {
    let needle = typed.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let lower = needle.to_ascii_lowercase();
    let mut rows = Vec::new();

    // 1. Tab-to-search site offer when typing the start of a known site's name.
    if let Some(site) = match_site_search_offer(needle, ctx.site_searches) {
        rows.push(SuggestionRow::SiteSearchOffer { site: site.clone() });
    }

    // 2. Open tabs matching the text (up to 6; no duplicate of current tab).
    let mut open_count = 0_usize;
    for tab in ctx.open_tabs {
        if ctx.current_tab == Some(tab.id) {
            continue;
        }
        if tab.title.to_ascii_lowercase().contains(&lower)
            || tab.url.display().to_ascii_lowercase().contains(&lower)
        {
            rows.push(SuggestionRow::OpenTab {
                tab: tab.id,
                title: tab.title.clone(),
                url: tab.url.clone(),
            });
            open_count += 1;
            if open_count >= 6 {
                break;
            }
        }
    }

    // 3. History, scored by frecency and prefix/substring fit.
    let mut scored_history: Vec<(u32, &HistoryVisit)> = ctx
        .history
        .iter()
        .filter_map(|visit| {
            let display = visit.url.display().to_ascii_lowercase();
            let title = visit.title.to_ascii_lowercase();
            let fit = if display.starts_with(&lower) {
                100_u32
            } else if title.starts_with(&lower) {
                75_u32
            } else if display.contains(&lower) || title.contains(&lower) {
                40_u32
            } else {
                return None;
            };
            let frecency = visit.visit_count.saturating_mul(10).saturating_add(fit);
            Some((frecency, visit))
        })
        .collect();
    scored_history.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (score, visit) in scored_history.into_iter().take(6) {
        rows.push(SuggestionRow::History {
            title: visit.title.clone(),
            url: visit.url.clone(),
            score,
        });
    }

    // 4. Bookmarks, including folders ("slips").
    collect_bookmark_suggestions(&ctx.bookmarks.roots, &lower, &mut rows, 6);

    // 5. Built-in known sites (up to 4 not already covered by history/bookmarks).
    let mut known_added = 0_usize;
    for (name, raw_url) in BUILTIN_KNOWN_SITES {
        if known_added >= 4 {
            break;
        }
        if (name.to_ascii_lowercase().starts_with(&lower)
            || raw_url.to_ascii_lowercase().contains(&lower))
            && let Ok(url) = PageUrl::parse(raw_url)
        {
            let already_present = rows.iter().any(|row| match row {
                SuggestionRow::History { url: u, .. }
                | SuggestionRow::Bookmark { url: Some(u), .. } => u.identity() == url.identity(),
                _ => false,
            });
            if !already_present {
                rows.push(SuggestionRow::KnownSite {
                    title: (*name).to_owned(),
                    url,
                });
                known_added += 1;
            }
        }
    }

    // 6. Internal pages / commands.
    if ctx.commands_enabled {
        for cmd in [
            AddressCommand::Settings,
            AddressCommand::NewTab,
            AddressCommand::NewPrivateTab,
            AddressCommand::NewSpace,
            AddressCommand::Bookmarks,
            AddressCommand::History,
            AddressCommand::Downloads,
            AddressCommand::Passwords,
            AddressCommand::ToggleSidebar,
        ] {
            if cmd.label().to_ascii_lowercase().contains(&lower) {
                rows.push(SuggestionRow::Command(cmd));
            }
        }
    }

    // 7. Final "search these words" row.
    if let Ok(url) = ctx.engine.search_url(needle) {
        rows.push(SuggestionRow::SearchWords {
            engine: ctx.engine.name().to_owned(),
            query: needle.to_owned(),
            url,
        });
    }

    rows
}

fn collect_bookmark_suggestions(
    items: &[BookmarkItem],
    lower_needle: &str,
    out: &mut Vec<SuggestionRow>,
    limit: usize,
) {
    for item in items {
        let count = out
            .iter()
            .filter(|r| matches!(r, SuggestionRow::Bookmark { .. }))
            .count();
        if count >= limit {
            return;
        }
        match item {
            BookmarkItem::Link { title, url, .. } => {
                if title.to_ascii_lowercase().contains(lower_needle)
                    || url.display().to_ascii_lowercase().contains(lower_needle)
                {
                    out.push(SuggestionRow::Bookmark {
                        title: title.clone(),
                        url: Some(url.clone()),
                    });
                }
            }
            BookmarkItem::Folder {
                title, children, ..
            } => {
                if title.to_ascii_lowercase().contains(lower_needle) {
                    out.push(SuggestionRow::Bookmark {
                        title: title.clone(),
                        url: None,
                    });
                }
                collect_bookmark_suggestions(children, lower_needle, out, limit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_localhost_loopback_and_zero_host_to_http() {
        let opts = ParseOptions::default();

        assert_eq!(
            resolve_address("localhost:3000", &opts),
            AddressResolution::Navigate(PageUrl::parse("http://localhost:3000/").expect("url"))
        );
        assert_eq!(
            resolve_address("app.localhost:8080/api", &opts),
            AddressResolution::Navigate(
                PageUrl::parse("http://app.localhost:8080/api").expect("url")
            )
        );
        assert_eq!(
            resolve_address("printer.local", &opts),
            AddressResolution::Navigate(PageUrl::parse("http://printer.local/").expect("url"))
        );
        assert_eq!(
            resolve_address("[::1]:3000", &opts),
            AddressResolution::Navigate(PageUrl::parse("http://[::1]:3000/").expect("url"))
        );
        assert_eq!(
            resolve_address("127.0.0.1:9000", &opts),
            AddressResolution::Navigate(PageUrl::parse("http://127.0.0.1:9000/").expect("url"))
        );
        assert_eq!(
            resolve_address("192.168.1.42/admin", &opts),
            AddressResolution::Navigate(PageUrl::parse("http://192.168.1.42/admin").expect("url"))
        );
        // 0.0.0.0 is rewritten to localhost because WebKit won't load it.
        assert_eq!(
            resolve_address("0.0.0.0:5173", &opts),
            AddressResolution::Navigate(PageUrl::parse("http://localhost:5173/").expect("url"))
        );
        assert_eq!(
            resolve_address("http://0.0.0.0:5173/index.html", &opts),
            AddressResolution::Navigate(
                PageUrl::parse("http://localhost:5173/index.html").expect("url")
            )
        );
    }

    #[test]
    fn resolves_domains_to_https_and_rejects_version_numbers() {
        let opts = ParseOptions::default();

        assert_eq!(
            resolve_address("example.com", &opts),
            AddressResolution::Navigate(PageUrl::parse("https://example.com/").expect("url"))
        );
        assert_eq!(
            resolve_address("docs.example.org/guide?x=1", &opts),
            AddressResolution::Navigate(
                PageUrl::parse("https://docs.example.org/guide?x=1").expect("url")
            )
        );

        // Dotted version numbers and single-letter TLDs are not hosts -> Invalid (shakes!)
        assert!(matches!(
            resolve_address("1.2.3", &opts),
            AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(_))
        ));
        assert!(matches!(
            resolve_address("v1.2.3", &opts),
            AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(_))
        ));
        assert!(matches!(
            resolve_address("example.c", &opts),
            AddressResolution::Invalid(AddressInvalidReason::MalformedAddress(_))
        ));
    }

    #[test]
    fn resolves_local_files_only_when_present() {
        let opts = ParseOptions {
            home_dir: Some(PathBuf::from("/home/alice")),
            ..ParseOptions::default()
        };

        let existing = resolve_address_with_fs("~/notes.txt", &opts, |path| {
            path == Path::new("/home/alice/notes.txt")
        });
        assert_eq!(
            existing,
            AddressResolution::Navigate(
                PageUrl::parse("file:///home/alice/notes.txt").expect("file url")
            )
        );

        let missing = resolve_address_with_fs("~/secret.txt", &opts, |_| false);
        assert_eq!(
            missing,
            AddressResolution::Invalid(AddressInvalidReason::MissingLocalFile(PathBuf::from(
                "/home/alice/secret.txt"
            )))
        );
    }

    #[test]
    fn refuses_emails_mailto_and_unknown_schemes() {
        let opts = ParseOptions::default();

        assert_eq!(
            resolve_address("alice@example.com", &opts),
            AddressResolution::Invalid(AddressInvalidReason::EmailOrUserinfo)
        );
        assert_eq!(
            resolve_address("https://user:pass@example.com/", &opts),
            AddressResolution::Invalid(AddressInvalidReason::EmailOrUserinfo)
        );
        assert_eq!(
            resolve_address("mailto:alice@example.com", &opts),
            AddressResolution::Invalid(AddressInvalidReason::UnsupportedScheme("mailto".into()))
        );
        assert_eq!(
            resolve_address("javascript:alert(1)", &opts),
            AddressResolution::Invalid(AddressInvalidReason::UnsupportedScheme(
                "javascript".into()
            ))
        );
    }

    #[test]
    fn searches_multi_word_and_single_word_queries_and_keywords() {
        let mut opts = ParseOptions::default();
        assert_eq!(
            resolve_address("hello world", &opts),
            AddressResolution::Search {
                source: "Google".into(),
                query: "hello world".into(),
                url: PageUrl::parse("https://www.google.com/search?q=hello+world").expect("url"),
            }
        );
        assert_eq!(
            resolve_address("rust", &opts),
            AddressResolution::Search {
                source: "Google".into(),
                query: "rust".into(),
                url: PageUrl::parse("https://www.google.com/search?q=rust").expect("url"),
            }
        );

        opts.keywords_enabled = true;
        assert_eq!(
            resolve_address("yt cats", &opts),
            AddressResolution::Search {
                source: "YouTube".into(),
                query: "cats".into(),
                url: PageUrl::parse("https://www.youtube.com/results?search_query=cats")
                    .expect("url"),
            }
        );
    }

    #[test]
    fn detects_lookalike_idn_hosts_and_preserves_punycode() {
        // Cyrillic 'а' (\u{0430}) in "аpple.com" -> punycode xn--pple-43d.com
        let info = inspect_host_lookalike("\u{0430}pple.com");
        assert!(info.is_lookalike);
        assert_eq!(info.safe_display_host, "xn--pple-43d.com");

        let info_puny = inspect_host_lookalike("xn--pple-43d.com");
        assert!(info_puny.is_lookalike);
        assert_eq!(info_puny.safe_display_host, "xn--pple-43d.com");

        let normal = inspect_host_lookalike("apple.com");
        assert!(!normal.is_lookalike);
        assert_eq!(normal.safe_display_host, "apple.com");
    }

    #[test]
    fn builds_suggestions_and_completions_in_spec_order() {
        let current = TabId::new(1);
        let other = TabId::new(2);
        let open_tabs = vec![
            OpenTabCandidate {
                id: current,
                title: "GitHub Current".into(),
                url: PageUrl::parse("https://github.com/current").expect("url"),
            },
            OpenTabCandidate {
                id: other,
                title: "GitHub Issues".into(),
                url: PageUrl::parse("https://github.com/issues").expect("url"),
            },
        ];
        let mut visit = HistoryVisit::new(
            PageUrl::parse("https://github.com/rust-lang/rust").expect("url"),
            "Rust Repo",
            "2026-11-21T08:00:00Z",
        );
        visit.visit_count = 15;
        let history = vec![visit];
        let bookmarks = BookmarkTree::new();
        let sites = builtin_site_searches();
        let engine = SearchEngine::Google;

        let ctx = SuggestionContext {
            current_tab: Some(current),
            open_tabs: &open_tabs,
            history: &history,
            bookmarks: &bookmarks,
            site_searches: &sites,
            engine: &engine,
            commands_enabled: true,
        };

        assert_eq!(inline_completion("git", &ctx).as_deref(), Some("hub.com"));

        let rows = build_suggestions("git", &ctx);
        assert!(
            matches!(rows.first(), Some(SuggestionRow::SiteSearchOffer { site }) if site.name == "GitHub")
        );
        assert!(
            rows.iter()
                .any(|row| matches!(row, SuggestionRow::OpenTab { tab, .. } if *tab == other))
        );
        assert!(
            rows.iter()
                .any(|row| matches!(row, SuggestionRow::History { .. }))
        );
        assert!(
            matches!(rows.last(), Some(SuggestionRow::SearchWords { query, .. }) if query == "git")
        );
    }
}
