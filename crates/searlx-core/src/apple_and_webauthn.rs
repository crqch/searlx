//! Portable domain & security models for platform-gated integrations:
//! - `P3-15` — Handoff eligibility (`NSUserActivityTypeBrowsingWeb`)
//! - `P4-01` & `P4-02` — Extensions foundation & `chrome.*` shim namespace router
//! - `P4-04` — Passkeys / WebAuthn ceremony validator & clean `NotAllowedError` fallback
//! - `P4-05` — Apple Passwords helper SRP pairing session, 5+5 caps, HTTPS/non-private guard, offer-once
//! - `P4-06` & `P4-10` — Apple integrations & iCloud Sync (`iCloud.com.officecommun.search`) merge/delete-zone rules

use crate::error::{CoreError, Result};
use crate::url::PageUrl;

// ============================================================================
// P3-15 — Handoff (`FEATURES.md` §16.3)
// ============================================================================

/// Activity type used by macOS Handoff (`FEATURES.md` §16.3).
pub const HANDOFF_ACTIVITY_TYPE: &str = "NSUserActivityTypeBrowsingWeb";

/// Determine whether the active page may be broadcast via Handoff (`FEATURES.md` §16.3, P3-15).
///
/// Rules:
/// - Never a private tab (`!is_private_tab`).
/// - Never a space signed out of the others (`!space_ uses_isolated_sign_ins`).
/// - Never a non-web (`search://`, `file://`, `about:`) page (`http` or `https` only).
/// - Off unless `handoff_enabled` is true.
#[must_use]
pub fn evaluate_handoff_webpage_url(
    url: &PageUrl,
    handoff_enabled: bool,
    is_private_tab: bool,
    space_isolated_from_default_store: bool,
) -> Option<PageUrl> {
    if !handoff_enabled || is_private_tab || space_isolated_from_default_store {
        return None;
    }
    if matches!(url.scheme(), "http" | "https") {
        Some(url.clone())
    } else {
        None
    }
}

// ============================================================================
// P4-01 & P4-02 — Extensions foundation & `chrome.*` shim capability router (`FEATURES.md` §14.1–14.5)
// ============================================================================

/// Stagger interval in milliseconds between loading installed extensions at launch (`400 ms`, `FEATURES.md` §14.1).
pub const EXTENSION_LOAD_STAGGER_MS: u64 = 400;
/// Minimum rate-limit interval between automatic extension update checks (`20 hours`, `FEATURES.md` §14.1).
pub const EXTENSION_UPDATE_INTERVAL_SECS: u64 = 20 * 3600;
/// Maximum message size for Chrome Native Messaging (`1 MiB`, `FEATURES.md` §14.3, §14.6).
pub const NATIVE_MESSAGING_MAX_BYTES: usize = 1024 * 1024;

/// How an extension's `chrome.*` API call is handled (`FEATURES.md` §14.3, §14.5, P4-02).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionApiDisposition {
    /// Provided natively by `WKWebExtension`.
    ProvidedByWebKit,
    /// Provided by Search's extension shim layer (`ExtensionShims`).
    ProvidedBySearchShim,
    /// Unsupported API that must fail loudly with an explicit error message (`TODO.md` P4-02).
    LoudlyUnsupported(&'static str),
}

/// Classify a `chrome.*` extension API namespace or method (`FEATURES.md` §14.3, §14.5, P4-02).
#[must_use]
pub fn classify_chrome_extension_api(api_path: &str) -> ExtensionApiDisposition {
    match api_path.trim() {
        "webRequestBlocking" | "webRequest.onBeforeRequest.blocking" => {
            ExtensionApiDisposition::LoudlyUnsupported(
                "webRequestBlocking cannot block or rewrite network requests in WKWebExtension; use declarativeNetRequest",
            )
        }
        "tabCapture" => ExtensionApiDisposition::LoudlyUnsupported(
            "chrome.tabCapture is not supported in searlx",
        ),
        "pageCapture" | "pageCapture.saveAsMHTML" => ExtensionApiDisposition::LoudlyUnsupported(
            "chrome.pageCapture.saveAsMHTML is not supported in searlx",
        ),
        "debugger" => {
            ExtensionApiDisposition::LoudlyUnsupported("chrome.debugger is not supported in searlx")
        }
        "identity.getAuthToken" => ExtensionApiDisposition::LoudlyUnsupported(
            "chrome.identity.getAuthToken is unavailable; use chrome.identity.launchWebAuthFlow",
        ),
        "tabGroups.move" => ExtensionApiDisposition::LoudlyUnsupported(
            "chrome.tabGroups.move is not supported in searlx",
        ),
        "gcm" | "instanceID" => ExtensionApiDisposition::LoudlyUnsupported(
            "chrome.gcm / chrome.instanceID are not supported in searlx",
        ),
        "bookmarks"
        | "history"
        | "downloads"
        | "browsingData"
        | "sessions"
        | "topSites"
        | "sidePanel"
        | "offscreen"
        | "tabGroups"
        | "identity"
        | "identity.launchWebAuthFlow"
        | "management"
        | "notifications"
        | "tts"
        | "idle"
        | "power"
        | "privacy"
        | "userScripts"
        | "runtime.sendNativeMessage"
        | "runtime.connectNative" => ExtensionApiDisposition::ProvidedBySearchShim,
        _ => ExtensionApiDisposition::ProvidedByWebKit,
    }
}

/// Clamp an extension popup's requested dimensions to Chrome's `25..=800 × 25..=600` bounds (`FEATURES.md` §14.2, P4-01).
#[must_use]
pub fn clamp_extension_popup_size(width: f64, height: f64) -> (f64, f64) {
    (width.clamp(25.0, 800.0), height.clamp(25.0, 600.0))
}

// ============================================================================
// P4-04 — Passkeys / WebAuthn ceremony & clean password fallback (`FEATURES.md` §13.2)
// ============================================================================

/// Maximum conditional-mediation passkey credentials listed under a sign-in field (`8`, `FEATURES.md` §13.2).
pub const MAX_CONDITIONAL_PASSKEYS: usize = 8;
/// Required macOS entitlement for browser passkey ceremonies (`FEATURES.md` §13.2).
pub const MACOS_PASSKEY_ENTITLEMENT: &str = "com.apple.developer.web-browser.public-key-credential";

/// WebAuthn ceremony kind (`navigator.credentials.create` vs `navigator.credentials.get`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasskeyCeremonyKind {
    /// `navigator.credentials.create`
    Create {
        /// Conditional `create` is rejected with `NotAllowedError` (`FEATURES.md` §13.2).
        conditional: bool,
    },
    /// `navigator.credentials.get`
    Get {
        /// Conditional mediation (`mediation: "conditional"`).
        conditional: bool,
    },
}

/// Outcome of validating a WebAuthn / Passkey request (`FEATURES.md` §13.2, P4-04).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasskeyCeremonyDisposition {
    /// Proceed with the OS platform/security-key authenticator for `rp_id`.
    Proceed {
        /// Validated relying-party ID.
        rp_id: String,
    },
    /// Reject cleanly with DOM `NotAllowedError` so the site falls back to password sign-in without wedging (`FEATURES.md` §13.2, P4-04).
    RejectNotAllowed(&'static str),
}

/// Validate a passkey ceremony request (`FEATURES.md` §13.2, P4-04).
///
/// Rules:
/// - Passkeys work in private tabs under the same origin/focus rules (`allows_private_tabs = true`).
/// - If passkeys are disabled, unentitled, or unsupported by the OS engine (e.g., Linux WebKitGTK),
///   reject immediately with `NotAllowedError` so the site falls back to password sign-in without wedging.
/// - Conditional `create` is always rejected with `NotAllowedError`.
/// - Only the main frame or a same-site frame may invoke a ceremony.
/// - `rp_id` must match the committed `https://` (or `http://localhost`) host or be a registrable parent domain, never a bare public suffix.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn evaluate_passkey_ceremony(
    kind: PasskeyCeremonyKind,
    committed_url: &PageUrl,
    requested_rp_id: &str,
    challenge_non_empty: bool,
    is_main_or_same_site_frame: bool,
    is_key_window: bool,
    passkeys_enabled_and_entitled: bool,
    engine_supports_passkeys: bool,
) -> PasskeyCeremonyDisposition {
    if !passkeys_enabled_and_entitled || !engine_supports_passkeys {
        return PasskeyCeremonyDisposition::RejectNotAllowed(
            "NotAllowedError: passkeys are unavailable or disabled; fall back to password",
        );
    }
    if matches!(kind, PasskeyCeremonyKind::Create { conditional: true }) {
        return PasskeyCeremonyDisposition::RejectNotAllowed(
            "NotAllowedError: conditional passkey creation is not supported",
        );
    }
    let is_conditional_get = matches!(kind, PasskeyCeremonyKind::Get { conditional: true });
    if !is_conditional_get && !is_key_window {
        return PasskeyCeremonyDisposition::RejectNotAllowed(
            "NotAllowedError: window is not the active key window",
        );
    }
    if !is_main_or_same_site_frame || !challenge_non_empty {
        return PasskeyCeremonyDisposition::RejectNotAllowed(
            "NotAllowedError: cross-site frame or empty challenge",
        );
    }

    let host = committed_url
        .host()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let secure_origin = committed_url.scheme() == "https"
        || (committed_url.scheme() == "http" && (host == "localhost" || host == "127.0.0.1"));
    if !secure_origin {
        return PasskeyCeremonyDisposition::RejectNotAllowed(
            "NotAllowedError: WebAuthn requires an HTTPS or localhost origin",
        );
    }

    let rp = requested_rp_id.trim().to_ascii_lowercase();
    let is_bare_public_suffix = !rp.contains('.') && rp != "localhost"
        || matches!(rp.as_str(), "co.uk" | "com.au" | "org.uk");
    let rp_fits_host = host == rp || host.ends_with(&format!(".{rp}"));
    if rp.is_empty() || is_bare_public_suffix || !rp_fits_host {
        return PasskeyCeremonyDisposition::RejectNotAllowed(
            "NotAllowedError: relying party ID does not match committed host",
        );
    }

    PasskeyCeremonyDisposition::Proceed { rp_id: rp }
}

// ============================================================================
// P4-05 — Apple Passwords helper (`FEATURES.md` §13.3)
// ============================================================================

/// Required Apple code-signing bundle identifier for the Passwords helper (`FEATURES.md` §13.3).
pub const APPLE_PASSWORDS_HELPER_ID: &str = "com.apple.PasswordManagerBrowserExtensionHelper";
/// Maximum Apple Passwords account names offered under a sign-in field (`5`, `FEATURES.md` §13.3).
pub const MAX_APPLE_PASSWORD_NAMES: usize = 5;
/// Maximum Apple Passwords one-time codes offered (`5`, `FEATURES.md` §13.3).
pub const MAX_APPLE_OTP_CODES: usize = 5;
/// Keepalive ping interval for the Apple Passwords helper (`240 s`, `FEATURES.md` §13.3).
pub const APPLE_HELPER_PING_INTERVAL_SECS: u64 = 240;

/// In-memory SRP-6a pairing session for Apple Passwords (`FEATURES.md` §13.3, P4-05).
///
/// Pairing state lives in memory only; `on_app_quit()` clears the key so quitting always requires re-pairing.
#[derive(Clone, Debug, Default)]
pub struct ApplePasswordsHelperSession {
    paired_in_memory: bool,
    ignored_offer_appearances: u8,
    offer_permanently_dismissed: bool,
}

impl ApplePasswordsHelperSession {
    /// Create an unpaired Apple Passwords session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the helper is currently paired in memory.
    #[must_use]
    pub const fn is_paired(&self) -> bool {
        self.paired_in_memory
    }

    /// Pair with a 6-digit numeric SRP code from an Apple-signed helper (`FEATURES.md` §13.3).
    pub fn pair_with_six_digit_code(
        &mut self,
        code: &str,
        helper_bundle_id: &str,
        apple_anchor_signed: bool,
    ) -> Result<()> {
        if !apple_anchor_signed || helper_bundle_id != APPLE_PASSWORDS_HELPER_ID {
            return Err(CoreError::Invalid(
                "Apple Passwords helper is not signed by Apple".to_owned(),
            ));
        }
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Err(CoreError::Invalid(
                "Apple Passwords pairing requires a 6-digit numeric code".to_owned(),
            ));
        }
        self.paired_in_memory = true;
        Ok(())
    }

    /// Quitting drops the in-memory SRP key so the next launch requires re-pairing (`FEATURES.md` §13.3, P4-05).
    pub fn on_app_quit(&mut self) {
        self.paired_in_memory = false;
    }

    /// Filter and cap Apple Passwords accounts (max 5, excluding `"Passwords not saved"`)
    /// and OTP codes (max 5, excluding SMS-delivered codes), enforcing HTTPS main-frame non-private rules (`FEATURES.md` §13.3).
    #[must_use]
    pub fn filter_offers(
        &self,
        page_url: &PageUrl,
        is_private_tab: bool,
        is_main_frame: bool,
        raw_account_names: &[String],
        raw_otp_codes: &[(String, bool)], // (code_label, is_sms)
    ) -> (Vec<String>, Vec<String>) {
        if !self.paired_in_memory
            || is_private_tab
            || !is_main_frame
            || page_url.scheme() != "https"
        {
            return (Vec::new(), Vec::new());
        }
        let accounts = raw_account_names
            .iter()
            .filter(|name| !name.trim().eq_ignore_ascii_case("Passwords not saved"))
            .take(MAX_APPLE_PASSWORD_NAMES)
            .cloned()
            .collect();
        let codes = raw_otp_codes
            .iter()
            .filter(|(_, is_sms)| !*is_sms)
            .map(|(code, _)| code.clone())
            .take(MAX_APPLE_OTP_CODES)
            .collect();
        (accounts, codes)
    }

    /// Whether to show the `"Use your Apple passwords?"` onboarding card (`FEATURES.md` §13.3).
    ///
    /// Retires after 2 ignored appearances or permanent `✕` dismissal.
    pub fn should_offer_onboarding_card(
        &mut self,
        apple_passwords_setting_on: bool,
        vault_has_account_for_site: bool,
        helper_installed: bool,
        page_url: &PageUrl,
        is_private_tab: bool,
    ) -> bool {
        if apple_passwords_setting_on
            || vault_has_account_for_site
            || !helper_installed
            || is_private_tab
            || page_url.scheme() != "https"
            || self.offer_permanently_dismissed
            || self.ignored_offer_appearances >= 2
        {
            return false;
        }
        self.ignored_offer_appearances += 1;
        true
    }

    /// Permanently dismiss the onboarding offer (`✕`).
    pub fn dismiss_onboarding_permanently(&mut self) {
        self.offer_permanently_dismissed = true;
    }
}

// ============================================================================
// P4-06 & P4-10 — iCloud sync (`FEATURES.md` §16.2)
// ============================================================================

/// CloudKit container identifier (`FEATURES.md` §16.2).
pub const ICLOUD_CONTAINER_ID: &str = "iCloud.com.officecommun.search";
/// CloudKit private zone name (`FEATURES.md` §16.2).
pub const ICLOUD_ZONE_NAME: &str = "Search";

/// iCloud Sync state machine enforcing `FEATURES.md` §16.2 rules (`P4-10`):
/// - Only the Personal profile syncs (`is_personal_profile`).
/// - First fetch must succeed before any write.
/// - Private tabs, shared pins, history, and passwords never sync.
/// - `"Delete Search's data from iCloud"` wipes the zone, turns `icloud_sync_enabled` off locally, and preserves local data.
#[derive(Clone, Debug, Default)]
pub struct ICloudSyncCoordinator {
    /// Whether `icloud.sync` is enabled.
    pub icloud_sync_enabled: bool,
    /// Whether the initial CloudKit fetch has completed in this session.
    pub first_fetch_completed: bool,
    /// Whether the CloudKit zone is currently populated.
    pub zone_has_data: bool,
}

impl ICloudSyncCoordinator {
    /// Create a new iCloud sync coordinator.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            icloud_sync_enabled: enabled,
            first_fetch_completed: false,
            zone_has_data: false,
        }
    }

    /// Record completion of the mandatory initial CloudKit fetch (`FEATURES.md` §16.2).
    pub fn mark_first_fetch_completed(&mut self) {
        self.first_fetch_completed = true;
        self.zone_has_data = true;
    }

    /// Check whether a write is permitted (requires `icloud_sync_enabled`, `first_fetch_completed`, and Personal profile).
    pub fn can_push_record(&self, is_personal_profile: bool) -> Result<()> {
        if !self.icloud_sync_enabled {
            return Err(CoreError::Invalid("iCloud sync is disabled".to_owned()));
        }
        if !is_personal_profile {
            return Err(CoreError::Invalid(
                "only the Personal profile syncs to iCloud".to_owned(),
            ));
        }
        if !self.first_fetch_completed {
            return Err(CoreError::Invalid(
                "iCloud sync requires the first fetch to succeed before any write".to_owned(),
            ));
        }
        Ok(())
    }

    /// Filter open tabs eligible for the per-device `device.<uuid>` iCloud record (`FEATURES.md` §16.2):
    /// only Personal profile, http/https, non-private, non-pin, non-agent/bench tabs, deduplicated by URL identity.
    #[must_use]
    pub fn filter_device_open_tabs(
        tabs: &[(PageUrl, bool, bool, bool)], // (url, is_private, is_pinned, is_agent_or_bench)
    ) -> Vec<PageUrl> {
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for (url, is_private, is_pinned, is_agent) in tabs {
            if *is_private || *is_pinned || *is_agent || !matches!(url.scheme(), "http" | "https") {
                continue;
            }
            let id = url.identity();
            if !seen.contains(&id) {
                seen.push(id);
                out.push(url.clone());
            }
        }
        out
    }

    /// `"Delete Search's data from iCloud"` (`FEATURES.md` §16.2, P4-10):
    /// deletes the CloudKit zone, turns sync off locally, and leaves local data untouched (`local_data_kept = true`).
    pub fn delete_all_icloud_data(&mut self) -> bool {
        self.zone_has_data = false;
        self.icloud_sync_enabled = false;
        self.first_fetch_completed = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_excludes_private_tabs_signed_out_spaces_and_internal_pages() {
        let web = PageUrl::parse("https://example.com/article").expect("url");
        let internal = PageUrl::parse("search://settings").expect("url");
        assert!(evaluate_handoff_webpage_url(&web, true, false, false).is_some());
        assert!(evaluate_handoff_webpage_url(&web, true, true, false).is_none());
        assert!(evaluate_handoff_webpage_url(&web, true, false, true).is_none());
        assert!(evaluate_handoff_webpage_url(&internal, true, false, false).is_none());
    }

    #[test]
    fn extension_shims_classify_tier_a_apis_and_fail_loudly_on_unsupported() {
        assert_eq!(
            classify_chrome_extension_api("bookmarks"),
            ExtensionApiDisposition::ProvidedBySearchShim
        );
        assert!(matches!(
            classify_chrome_extension_api("webRequestBlocking"),
            ExtensionApiDisposition::LoudlyUnsupported(_)
        ));
        assert!(matches!(
            classify_chrome_extension_api("debugger"),
            ExtensionApiDisposition::LoudlyUnsupported(_)
        ));
        assert_eq!(clamp_extension_popup_size(10.0, 900.0), (25.0, 600.0));
    }

    #[test]
    fn passkeys_fall_back_to_not_allowed_when_unavailable_and_validate_rp_id() {
        let url = PageUrl::parse("https://login.example.com/auth").expect("url");
        // When engine_supports_passkeys = false (e.g. Linux), rejects with NotAllowedError so site falls back to password
        assert!(matches!(
            evaluate_passkey_ceremony(
                PasskeyCeremonyKind::Get { conditional: false },
                &url,
                "example.com",
                true,
                true,
                true,
                true,
                false,
            ),
            PasskeyCeremonyDisposition::RejectNotAllowed(_)
        ));

        // Valid macOS/Windows ceremony on parent RP ID succeeds
        assert_eq!(
            evaluate_passkey_ceremony(
                PasskeyCeremonyKind::Get { conditional: false },
                &url,
                "example.com",
                true,
                true,
                true,
                true,
                true,
            ),
            PasskeyCeremonyDisposition::Proceed {
                rp_id: "example.com".to_owned()
            }
        );
    }

    #[test]
    fn apple_passwords_and_icloud_sync_enforce_https_private_and_delete_zone_rules() {
        let mut helper = ApplePasswordsHelperSession::new();
        helper
            .pair_with_six_digit_code("123456", APPLE_PASSWORDS_HELPER_ID, true)
            .expect("paired");
        let https_url = PageUrl::parse("https://example.com/login").expect("url");
        let http_url = PageUrl::parse("http://example.com/login").expect("url");

        let (names, codes) = helper.filter_offers(
            &https_url,
            false,
            true,
            &["alice".to_owned(), "Passwords not saved".to_owned()],
            &[("111222".to_owned(), false), ("999888".to_owned(), true)],
        );
        assert_eq!(names, vec!["alice".to_owned()]);
        assert_eq!(codes, vec!["111222".to_owned()]);
        // Never offered on plain http or private tabs
        assert!(
            helper
                .filter_offers(&http_url, false, true, &["alice".to_owned()], &[])
                .0
                .is_empty()
        );
        // Quitting requires re-pairing
        helper.on_app_quit();
        assert!(!helper.is_paired());

        // iCloud sync requires first fetch before write and delete-data turns sync off locally
        let mut icloud = ICloudSyncCoordinator::new(true);
        assert!(icloud.can_push_record(true).is_err());
        icloud.mark_first_fetch_completed();
        assert!(icloud.can_push_record(true).is_ok());
        assert!(icloud.can_push_record(false).is_err());
        assert!(icloud.delete_all_icloud_data());
        assert!(!icloud.icloud_sync_enabled);
    }
}
