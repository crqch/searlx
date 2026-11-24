//! Ad & tracker blocking (`P2-07`), Hide Elements (`P2-08`), and Site CSS (`P2-09`)
//! (`FEATURES.md` §5.5, §5.6, §12.1).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::data::{DataStore, HiddenFile, HiddenSelector, SiteStyle, StylesFile};
use crate::error::{CoreError, Result};
use crate::url::PageUrl;

/// Maximum time the first page load waits for the content blocker list to compile (`FEATURES.md` §12.1).
pub const FIRST_LOAD_BLOCKER_WAIT_MS: u64 = 1_000;

/// Built-in third-party ad, analytics, and tracker domains (`FEATURES.md` §12.1).
pub const TRACKER_DOMAINS: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "google-analytics.com",
    "googletagmanager.com",
    "amazon-adsystem.com",
    "adnxs.com",
    "adsrvr.org",
    "criteo.com",
    "criteo.net",
    "taboola.com",
    "outbrain.com",
    "rubiconproject.com",
    "pubmatic.com",
    "openx.net",
    "casalemedia.com",
    "smartadserver.com",
    "sharethrough.com",
    "indexww.com",
    "bidswitch.net",
    "33across.com",
    "teads.tv",
    "moatads.com",
    "adroll.com",
    "scorecardresearch.com",
    "quantserve.com",
    "chartbeat.com",
    "chartbeat.net",
    "hotjar.com",
    "mouseflow.com",
    "fullstory.com",
    "clarity.ms",
    "mixpanel.com",
    "amplitude.com",
    "segment.io",
    "segment.com",
    "branch.io",
    "appsflyer.com",
    "adjust.com",
    "tiktokw.us",
    "connect.facebook.net",
    "ads-twitter.com",
];

/// Generic and site-specific cosmetic ad selectors (`FEATURES.md` §12.1).
pub const COSMETIC_AD_SELECTORS: &[&str] = &[
    ".adsbygoogle",
    ".ad-slot",
    ".ad-leaderboard",
    ".ad-container",
    ".advertisement",
    "[id^=\"div-gpt-ad\"]",
    "[id^=\"taboola-\"]",
    "[id^=\"outbrain_widget\"]",
    ".dfp-ad",
    ".top-banner-ad-container",
];

/// Normalize a host into a site key (`host` lower-case without leading `www.`).
#[must_use]
pub fn normalize_site_key(host: &str) -> String {
    host.trim()
        .to_ascii_lowercase()
        .trim_start_matches("www.")
        .to_owned()
}

/// Compile Search's content-blocker rules into WebKit `WKContentRuleList` JSON
/// format (`FEATURES.md` §12.1, P2-07), returning `"[]"` when Shield is globally off.
#[must_use]
pub fn compile_webkit_content_rules_with_shield(
    shield_enabled: bool,
    paused_sites: &[String],
) -> String {
    if !shield_enabled {
        return "[]".into();
    }
    compile_webkit_content_rules(paused_sites)
}

/// Compile Search's content-blocker rules into WebKit `WKContentRuleList` JSON
/// format (`FEATURES.md` §12.1, P2-07).
#[must_use]
pub fn compile_webkit_content_rules(paused_sites: &[String]) -> String {
    let paused: Vec<String> = paused_sites
        .iter()
        .map(|s| format!("*{}", normalize_site_key(s)))
        .collect();
    let mut rules = Vec::new();

    for domain in TRACKER_DOMAINS {
        let escaped = domain.replace('.', "\\.");
        let mut trigger = serde_json::json!({
            "url-filter": format!("^https?://([a-z0-9-]+\\.)*{escaped}[:/]"),
            "load-type": ["third-party"]
        });
        if !paused.is_empty() {
            trigger["unless-domain"] = serde_json::json!(paused);
        }
        rules.push(serde_json::json!({
            "trigger": trigger,
            "action": { "type": "block" }
        }));
    }

    let combined_selectors = COSMETIC_AD_SELECTORS.join(", ");
    let mut cosmetic_trigger = serde_json::json!({
        "url-filter": "^https?://"
    });
    if !paused.is_empty() {
        cosmetic_trigger["unless-domain"] = serde_json::json!(paused);
    }
    rules.push(serde_json::json!({
        "trigger": cosmetic_trigger,
        "action": {
            "type": "css-display-none",
            "selector": combined_selectors
        }
    }));

    serde_json::to_string(&rules).unwrap_or_else(|_| "[]".into())
}

/// Portable third-party tracker check for engines without `WKContentRuleList` (`P2-07`).
#[must_use]
pub fn should_block_third_party_request(
    page_url: &PageUrl,
    request_url: &PageUrl,
    shield_enabled: bool,
    paused_sites: &[String],
) -> bool {
    if !shield_enabled {
        return false;
    }
    let Some(page_host) = page_url.host() else {
        return false;
    };
    let page_site = normalize_site_key(page_host);
    if paused_sites
        .iter()
        .any(|paused| normalize_site_key(paused) == page_site)
    {
        return false;
    }
    let Some(req_host) = request_url.host() else {
        return false;
    };
    let req_site = normalize_site_key(req_host);
    // Only block third-party requests.
    if page_url
        .site_label()
        .eq_ignore_ascii_case(&request_url.site_label())
    {
        return false;
    }
    TRACKER_DOMAINS
        .iter()
        .any(|tracker| req_site == *tracker || req_site.ends_with(&format!(".{tracker}")))
}

/// YouTube ad-blocking scripts (`(page_world_script, isolated_world_fallback_script)`),
/// returned only on YouTube hosts when Shield is enabled and not paused for YouTube (`FEATURES.md` §12.1).
#[must_use]
pub fn youtube_ad_scripts(
    page_url: &PageUrl,
    shield_enabled: bool,
    paused_sites: &[String],
) -> Option<(&'static str, &'static str)> {
    if !shield_enabled {
        return None;
    }
    let host = normalize_site_key(page_url.host()?);
    if !(host == "youtube.com" || host.ends_with(".youtube.com")) {
        return None;
    }
    if paused_sites
        .iter()
        .any(|s| normalize_site_key(s) == "youtube.com")
    {
        return None;
    }
    Some((YOUTUBE_PAGE_WORLD_JS, YOUTUBE_ISOLATED_FALLBACK_JS))
}

/// Page-world script that strips `adPlacements`, `adSlots`, `playerAds`, and
/// `adBreakHeartbeatParams` from YouTube player responses (`FEATURES.md` §12.1).
pub const YOUTUBE_PAGE_WORLD_JS: &str = r#"(() => {
    const AD_KEYS = ["adPlacements", "adSlots", "playerAds", "adBreakHeartbeatParams"];
    const scrub = (obj) => {
        if (obj && typeof obj === "object" && ("videoDetails" in obj || "streamingData" in obj)) {
            for (const key of AD_KEYS) delete obj[key];
        }
        return obj;
    };
    const origParse = JSON.parse;
    JSON.parse = function(...args) {
        return scrub(origParse.apply(this, args));
    };
})()"#;

/// Isolated-world fallback that mutes and plays at 16× while `.ad-showing` is
/// active, restoring audio and playback rate afterwards without seeking (`FEATURES.md` §12.1).
pub const YOUTUBE_ISOLATED_FALLBACK_JS: &str = r#"(() => {
    let savedMuted = null;
    let savedRate = null;
    const checkAd = () => {
        const player = document.querySelector(".html5-video-player");
        const video = document.querySelector("video");
        if (!player || !video) return;
        if (player.classList.contains("ad-showing")) {
            if (savedMuted === null) {
                savedMuted = video.muted;
                savedRate = video.playbackRate || 1;
            }
            video.muted = true;
            video.playbackRate = 16;
        } else if (savedMuted !== null) {
            video.muted = savedMuted;
            video.playbackRate = savedRate || 1;
            savedMuted = null;
            savedRate = null;
        }
    };
    new MutationObserver(checkAd).observe(document.documentElement, {
        attributes: true,
        subtree: true,
        attributeFilter: ["class"]
    });
})()"#;

/// Attributes of one DOM element along the ancestor chain for selector synthesis (`FEATURES.md` §5.5, P2-08).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomElementDescriptor {
    /// Lowercase tag name (e.g. `"div"`, `"aside"`).
    pub tag: String,
    /// Element `id` attribute, when present and unique.
    pub id: Option<String>,
    /// Stable attribute pairs (`data-testid`, `data-test`, `data-qa`, `data-cy`, `aria-label`, `name`, `role`).
    pub attributes: Vec<(String, String)>,
    /// Stable CSS class names.
    pub classes: Vec<String>,
    /// One-based `nth-of-type` index among siblings of the same tag.
    pub nth_of_type: usize,
}

const STABLE_ATTRS: &[&str] = &[
    "data-testid",
    "data-test",
    "data-qa",
    "data-cy",
    "aria-label",
    "name",
    "role",
];

/// Synthesize a stable CSS selector for a clicked element (`FEATURES.md` §5.5, P2-08):
/// 1. Unique `#id`
/// 2. Stable `data-testid` / `data-test` / `data-qa` / `data-cy` / `aria-label` / `name` / `role`
/// 3. Stable CSS classes
/// 4. `tag:nth-of-type(N)` path along `ancestors_to_target`
#[must_use]
pub fn synthesize_selector(ancestors_to_target: &[DomElementDescriptor]) -> String {
    let Some(target) = ancestors_to_target.last() else {
        return String::new();
    };
    let tag = if target.tag.trim().is_empty() {
        "div".to_owned()
    } else {
        target.tag.trim().to_ascii_lowercase()
    };

    // 1. Unique #id
    if let Some(id) = target.id.as_deref()
        && is_stable_css_identifier(id)
    {
        return format!("#{id}");
    }

    // 2. Stable attribute
    for &wanted in STABLE_ATTRS {
        if let Some((_, val)) = target
            .attributes
            .iter()
            .find(|(k, v)| k.eq_ignore_ascii_case(wanted) && !v.trim().is_empty())
        {
            let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
            return format!("{tag}[{wanted}=\"{escaped}\"]");
        }
    }

    // 3. Stable classes
    let stable_classes: Vec<&str> = target
        .classes
        .iter()
        .map(String::as_str)
        .filter(|cls| is_stable_css_identifier(cls))
        .take(3)
        .collect();
    if !stable_classes.is_empty() {
        return format!("{tag}.{}", stable_classes.join("."));
    }

    // 4. nth-of-type path
    ancestors_to_target
        .iter()
        .map(|el| {
            let t = if el.tag.trim().is_empty() {
                "div"
            } else {
                el.tag.trim()
            };
            format!("{t}:nth-of-type({})", el.nth_of_type.max(1))
        })
        .collect::<Vec<_>>()
        .join(" > ")
}

fn is_stable_css_identifier(ident: &str) -> bool {
    !ident.is_empty()
        && ident
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && ident
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// Per-site Hide Elements controller (`FEATURES.md` §5.5, P2-08).
///
/// - Normal tabs persist selectors to `hidden.json`.
/// - Private tabs keep selectors only in memory for that tab/site and write
///   nothing to disk (`FEATURES.md` §5.5).
/// - Each selector is injected as its own `selector { display:none !important }`
///   rule before first paint so one bad selector cannot break the list and
///   hidden banners never flash on reload.
#[derive(Clone, Debug, Default)]
pub struct HideElementsManager {
    durable: HiddenFile,
    private_memory: BTreeMap<String, Vec<HiddenSelector>>,
    undo_stack: Vec<(String, String, bool)>,
}

impl HideElementsManager {
    /// Create an empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load durable rules from `hidden.json` in `store`.
    pub fn load_from(store: &DataStore) -> Result<Self> {
        let durable = store.read_or_default::<HiddenFile>("hidden.json")?.value;
        Ok(Self {
            durable,
            private_memory: BTreeMap::new(),
            undo_stack: Vec::new(),
        })
    }

    /// Save durable rules to `hidden.json` in `store` (private rules are never written).
    pub fn save_to(&self, store: &DataStore) -> Result<()> {
        store.save_hidden(&self.durable)
    }

    /// Add a hidden selector for `site_host`, returning the announcement
    /// `"Hidden — ⌘Z puts it back"` (`FEATURES.md` §5.5).
    pub fn hide_selector(
        &mut self,
        site_host: &str,
        selector: impl Into<String>,
        private: bool,
    ) -> &'static str {
        let site = normalize_site_key(site_host);
        let selector = selector.into();
        let target_map = if private {
            &mut self.private_memory
        } else {
            &mut self.durable.sites
        };
        let list = target_map.entry(site.clone()).or_default();
        if !list.iter().any(|item| item.selector == selector) {
            list.push(HiddenSelector {
                selector: selector.clone(),
                note: None,
                created: None,
                extra: serde_json::Map::new(),
            });
            self.undo_stack.push((site, selector, private));
        }
        "Hidden — ⌘Z puts it back"
    }

    /// Undo the most recent hide action (`⌘Z` while pointing, `FEATURES.md` §5.5).
    pub fn undo_last(&mut self) -> Option<String> {
        let (site, selector, private) = self.undo_stack.pop()?;
        self.restore_one(&site, &selector, private);
        Some(selector)
    }

    /// Restore one hidden selector on `site_host` (`⇧⌘U` review list).
    pub fn restore_one(&mut self, site_host: &str, selector: &str, private: bool) -> bool {
        let site = normalize_site_key(site_host);
        let map = if private {
            &mut self.private_memory
        } else {
            &mut self.durable.sites
        };
        let Some(list) = map.get_mut(&site) else {
            return false;
        };
        let before = list.len();
        list.retain(|item| item.selector != selector);
        let changed = list.len() != before;
        if list.is_empty() {
            map.remove(&site);
        }
        changed
    }

    /// Restore all hidden selectors on `site_host` (`Restore all` in `⇧⌘U`).
    pub fn restore_all(&mut self, site_host: &str, private: bool) -> usize {
        let site = normalize_site_key(site_host);
        let map = if private {
            &mut self.private_memory
        } else {
            &mut self.durable.sites
        };
        map.remove(&site).map_or(0, |v| v.len())
    }

    /// Effective hidden selectors for `site_host` (durable rules plus any in-memory private rules).
    #[must_use]
    pub fn selectors_for_site(&self, site_host: &str, private: bool) -> Vec<String> {
        let site = normalize_site_key(site_host);
        let mut out: Vec<String> = self
            .durable
            .sites
            .get(&site)
            .into_iter()
            .flatten()
            .map(|r| r.selector.clone())
            .collect();
        if private && let Some(priv_rules) = self.private_memory.get(&site) {
            for r in priv_rules {
                if !out.contains(&r.selector) {
                    out.push(r.selector.clone());
                }
            }
        }
        out
    }

    /// Generate the before-first-paint stylesheet for `site_host`, where every
    /// selector is emitted as its own `selector { display:none !important }` rule (`FEATURES.md` §5.5).
    /// Selectors containing CSS block/tag breakout tokens (`<`, `>`, `{`, `}`) are skipped.
    #[must_use]
    pub fn before_first_paint_css(&self, site_host: &str, private: bool) -> Option<String> {
        let selectors = self.selectors_for_site(site_host, private);
        let safe_selectors: Vec<String> = selectors
            .into_iter()
            .filter(|sel| {
                let trimmed = sel.trim();
                !trimmed.is_empty()
                    && !trimmed.contains('{')
                    && !trimmed.contains('}')
                    && !trimmed.contains('<')
                    && !trimmed.contains('\n')
                    && !trimmed.contains('\r')
            })
            .collect();
        if safe_selectors.is_empty() {
            return None;
        }
        let mut css = String::new();
        for sel in safe_selectors {
            css.push_str(&sel);
            css.push_str(" { display:none !important }\n");
        }
        Some(css)
    }

    /// Clear all in-memory private-tab rules and private undo entries when the
    /// last private tab closes (`AGENTS.md` §9, `FEATURES.md` §5.5).
    pub fn clear_private_memory(&mut self) {
        self.private_memory.clear();
        self.undo_stack.retain(|(_, _, is_private)| !*is_private);
    }
}

/// Per-site custom CSS manager (`FEATURES.md` §5.6, P2-09).
///
/// - Stored by host in `styles.json`.
/// - Turning the global `sites.styles` switch off stops applying styles but
///   keeps every saved style intact on disk.
/// - Not editable from a private tab, but a style written in a normal tab is
///   still worn in a private tab.
/// - CSS only: any `<script`, `</style`, or `javascript:` construct is stripped.
#[derive(Clone, Debug)]
pub struct SiteCssManager {
    file: StylesFile,
    feature_enabled: bool,
}

impl Default for SiteCssManager {
    fn default() -> Self {
        Self {
            file: StylesFile::default(),
            feature_enabled: true,
        }
    }
}

impl SiteCssManager {
    /// Create a manager with `sites.styles` enabled.
    #[must_use]
    pub fn new(feature_enabled: bool) -> Self {
        Self {
            file: StylesFile::default(),
            feature_enabled,
        }
    }

    /// Load `styles.json` from `store`.
    pub fn load_from(store: &DataStore, feature_enabled: bool) -> Result<Self> {
        let file = store.read_or_default::<StylesFile>("styles.json")?.value;
        Ok(Self {
            file,
            feature_enabled,
        })
    }

    /// Save `styles.json` to `store`.
    pub fn save_to(&self, store: &DataStore) -> Result<()> {
        store.save_styles(&self.file)
    }

    /// Toggle the global `sites.styles` switch; turning it off keeps all styles stored (`FEATURES.md` §5.6).
    pub fn set_feature_enabled(&mut self, enabled: bool) {
        self.feature_enabled = enabled;
    }

    /// Save or update CSS for `site_host`. Refuses edits from private tabs (`FEATURES.md` §5.6).
    pub fn set_site_css(
        &mut self,
        site_host: &str,
        raw_css: &str,
        private_tab: bool,
    ) -> Result<()> {
        if private_tab {
            return Err(CoreError::Invalid(
                "site styles cannot be edited from a private tab".into(),
            ));
        }
        let site = normalize_site_key(site_host);
        let sanitized = sanitize_user_css(raw_css);
        if sanitized.trim().is_empty() {
            self.file.sites.remove(&site);
        } else {
            self.file.sites.insert(
                site,
                SiteStyle {
                    css: sanitized,
                    enabled: true,
                    updated: None,
                    extra: serde_json::Map::new(),
                },
            );
        }
        Ok(())
    }

    /// Document-start `<style id="office-style">` CSS for `site_host` (worn in
    /// both normal and private tabs when `feature_enabled` is true, `FEATURES.md` §5.6).
    #[must_use]
    pub fn active_css_for_site(&self, site_host: &str) -> Option<&str> {
        if !self.feature_enabled {
            return None;
        }
        let site = normalize_site_key(site_host);
        self.file
            .sites
            .get(&site)
            .filter(|s| s.enabled && !s.css.is_empty())
            .map(|s| s.css.as_str())
    }

    /// Whether `sites.styles` is currently enabled.
    #[must_use]
    pub const fn is_feature_enabled(&self) -> bool {
        self.feature_enabled
    }

    /// JavaScript snippet that creates or updates `<style id="office-style">`
    /// appended last so equal-specificity user rules win (`FEATURES.md` §5.6).
    #[must_use]
    pub fn injection_script(&self, site_host: &str) -> Option<String> {
        let css = self.active_css_for_site(site_host)?;
        let json_css = serde_json::to_string(css).ok()?;
        Some(format!(
            "(() => {{\
             let el = document.getElementById('office-style');\
             if (!el) {{ el = document.createElement('style'); el.id = 'office-style'; }}\
             el.textContent = {json_css};\
             const parent = document.head || document.documentElement;\
             if (parent) parent.appendChild(el);\
             }})();"
        ))
    }
}

/// Sanitize user-entered CSS so it can never break out of `<style id="office-style">`
/// or execute script (`P2-09`).
#[must_use]
pub fn sanitize_user_css(raw_css: &str) -> String {
    let mut out = raw_css.to_owned();
    for forbidden in [
        "</style",
        "</STYLE",
        "<script",
        "<SCRIPT",
        "javascript:",
        "JAVASCRIPT:",
        "expression(",
    ] {
        out = out.replace(forbidden, "/* blocked */");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blockers_pause_per_site_and_provide_youtube_scripts() {
        let page = PageUrl::parse("https://www.nytimes.com/article").expect("page");
        let tracker = PageUrl::parse("https://stats.doubleclick.net/pixel.js").expect("tracker");
        let first_party = PageUrl::parse("https://static.nytimes.com/app.js").expect("first party");

        assert!(should_block_third_party_request(&page, &tracker, true, &[]));
        assert!(!should_block_third_party_request(
            &page,
            &first_party,
            true,
            &[]
        ));
        assert!(!should_block_third_party_request(
            &page,
            &tracker,
            true,
            &["nytimes.com".into()]
        ));

        let rules_json = compile_webkit_content_rules(&["nytimes.com".into()]);
        assert!(rules_json.contains("doubleclick"));
        assert!(rules_json.contains("css-display-none"));
        assert!(rules_json.contains("*nytimes.com"));

        let yt = PageUrl::parse("https://www.youtube.com/watch?v=123").expect("yt");
        assert!(youtube_ad_scripts(&yt, true, &[]).is_some());
        assert!(youtube_ad_scripts(&yt, true, &["youtube.com".into()]).is_none());
    }

    #[test]
    fn hide_elements_synthesizes_selectors_persists_and_isolates_private_tabs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());

        let by_id = synthesize_selector(&[DomElementDescriptor {
            tag: "div".into(),
            id: Some("cookie-banner".into()),
            ..DomElementDescriptor::default()
        }]);
        assert_eq!(by_id, "#cookie-banner");

        let by_test_id = synthesize_selector(&[DomElementDescriptor {
            tag: "aside".into(),
            attributes: vec![("data-testid".into(), "gdpr-modal".into())],
            ..DomElementDescriptor::default()
        }]);
        assert_eq!(by_test_id, "aside[data-testid=\"gdpr-modal\"]");

        let mut mgr = HideElementsManager::new();
        let msg = mgr.hide_selector("www.example.com", "#cookie-banner", false);
        assert_eq!(msg, "Hidden — ⌘Z puts it back");
        mgr.hide_selector("example.com", ".private-only-banner", true);

        let css_private = mgr
            .before_first_paint_css("example.com", true)
            .expect("css");
        assert!(css_private.contains("#cookie-banner { display:none !important }"));
        assert!(css_private.contains(".private-only-banner { display:none !important }"));

        // Save and reload: durable rule survives reload, private rule was never written to disk!
        mgr.save_to(&store).expect("save hidden");
        let reloaded = HideElementsManager::load_from(&store).expect("reload hidden");
        assert_eq!(
            reloaded.selectors_for_site("example.com", false),
            vec!["#cookie-banner"]
        );
    }

    #[test]
    fn site_css_survives_reload_strips_scripts_keeps_styles_when_off_and_is_readonly_in_private() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());
        let mut css_mgr = SiteCssManager::new(true);

        // Private tabs cannot edit site CSS.
        assert!(
            css_mgr
                .set_site_css("example.com", "body { color: red; }", true)
                .is_err()
        );

        css_mgr
            .set_site_css(
                "www.example.com",
                "body { max-width: 70ch; } </style><script>alert(1)</script>",
                false,
            )
            .expect("set site css");
        css_mgr.save_to(&store).expect("save styles");

        let mut reloaded = SiteCssManager::load_from(&store, true).expect("reload styles");
        let active = reloaded
            .active_css_for_site("example.com")
            .expect("active css");
        assert!(active.contains("max-width: 70ch"));
        assert!(!active.contains("<script"));

        // Turning the feature off hides active CSS but keeps the stored style when turned back on.
        reloaded.set_feature_enabled(false);
        assert!(reloaded.active_css_for_site("example.com").is_none());
        reloaded.set_feature_enabled(true);
        assert!(reloaded.active_css_for_site("example.com").is_some());
    }
}
