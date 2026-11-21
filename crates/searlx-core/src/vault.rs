//! Password vault, strict origin & scheme checks before autofill, Touch ID /
//! device-owner authentication gates, and no-export enforcement (`FEATURES.md` §13.1, `AGENTS.md` §9, P2-15).
//!
//! Security rules (`AGENTS.md` §2, §9, `FEATURES.md` §13.1):
//! - **Origin check before filling credentials:** the page must still be on the
//!   exact committed origin (`scheme://host[:port]`) the account list was
//!   offered on, and must be the main frame (`!is_subframe`).
//! - **Never fill a password saved from `https` on `http`:** an account saved
//!   from an HTTPS page (`saved_from_clear_http == false`) is never offered or
//!   filled on an insecure HTTP page. Only accounts explicitly saved on plain
//!   HTTP (`saved_from_clear_http == true`) may fill on HTTP.
//! - **Never save** from a private tab, an agent tab, an extension page, a site
//!   in `never_save_hosts`, or a password just filled from Apple Passwords.
//! - **Show / Copy gates:** showing a password requires device-owner
//!   authentication (`Touch ID` / `Windows Hello` / `Secret Service`), masks
//!   unrevealed passwords as 10 dots (`"••••••••••"`) regardless of length, and
//!   auto-hides after 15 s. Copying requires authentication and clears the
//!   concealed clipboard after 90 s.
//! - **No password export exists.**
//! - Passwords are never formatted in `Debug`, `Display`, or error messages.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::blocking::normalize_site_key;
use crate::error::{CoreError, Result};
use crate::url::PageUrl;

/// Mask string shown for every hidden password regardless of length (`10 dots`, `FEATURES.md` §13.1).
pub const MASKED_PASSWORD_DOTS: &str = "••••••••••";
/// Seconds before a revealed password auto-hides (`15 s`, `FEATURES.md` §13.1).
pub const SHOW_AUTO_HIDE_SECS: u64 = 15;
/// Seconds before a copied password is cleared from the clipboard (`90 s`, `FEATURES.md` §13.1).
pub const CLIPBOARD_CLEAR_SECS: u64 = 90;
/// Seconds before an unanswered password-save offer expires (`45 s`, `FEATURES.md` §13.1).
pub const SAVE_OFFER_EXPIRY_SECS: u64 = 45;
/// Milliseconds after opening the autofill dropdown during which clicks are ignored (`500 ms`, `FEATURES.md` §13.1).
pub const AUTOFILL_CLICK_GUARD_MS: u64 = 500;
/// Maximum keychain accounts offered in the autofill dropdown (`5`, `FEATURES.md` §13.1).
pub const MAX_VAULT_OFFERS: usize = 5;

/// A redacted password value that never prints its contents in `Debug`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultSecret(String);

impl VaultSecret {
    /// Wrap a plaintext password.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Expose the password only after an origin or authentication check has passed.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for VaultSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VaultSecret([REDACTED])")
    }
}

/// One saved account in the password vault (`FEATURES.md` §13.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultAccount {
    /// Bare lowercase host (`kSecAttrServer`).
    pub host: String,
    /// Full origin (`scheme://host[:port]`) where the password was saved.
    pub origin: String,
    /// Account username (`kSecAttrAccount`).
    pub username: String,
    /// Redacted password secret (`kSecValueData`).
    pub password: VaultSecret,
    /// True ONLY when saved from a plain `http://` page (`clear` flag in `FEATURES.md` §13.1).
    pub saved_from_clear_http: bool,
    /// ISO-8601 timestamp of last use.
    pub last_used: String,
}

/// What the vault should do after a sign-in form submission succeeds (`FEATURES.md` §13.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostSignInAction {
    /// Do not offer to save (private tab, agent tab, extension page, `never` site, or Apple Passwords fill).
    Suppressed,
    /// Existing account had the exact same password; `last_used` was updated silently without prompting.
    UpdatedLastUsedOnly,
    /// Offer to save a new account (`Save`).
    OfferSave,
    /// Offer to update an existing account's changed password (`Update`).
    OfferUpdate,
}

/// Payload delivered to the clipboard when copying a password (`FEATURES.md` §13.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConcealedClipboardCopy {
    /// Redacted secret for the clipboard.
    pub secret: VaultSecret,
    /// Must be marked local-only (`currentHostOnly`, never Universal Clipboard).
    pub local_host_only: bool,
    /// Must be marked concealed and transient.
    pub concealed_and_transient: bool,
    /// Auto-clear timeout (`90 s`).
    pub clear_after_secs: u64,
}

/// Password vault controller (`P2-15`).
#[derive(Clone, Debug, Default)]
pub struct PasswordVault {
    accounts: Vec<VaultAccount>,
    never_save_hosts: BTreeSet<String>,
    /// Temporary 2-step sign-in username memory (`host -> username`).
    two_step_usernames: BTreeMap<String, String>,
}

impl PasswordVault {
    /// Create an empty password vault.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a site to `"Never here"` (`passwords.never`, `FEATURES.md` §13.1).
    /// Private-tab refusals should not call this with `persist = true`.
    pub fn add_never_save_host(&mut self, host: &str) {
        self.never_save_hosts.insert(normalize_site_key(host));
    }

    /// Whether a host is in the `"Never here"` list.
    #[must_use]
    pub fn is_never_save_host(&self, host: &str) -> bool {
        self.never_save_hosts.contains(&normalize_site_key(host))
    }

    /// Remember a username from step 1 of a two-step sign-in flow (`FEATURES.md` §13.1).
    pub fn remember_two_step_username(&mut self, host: &str, username: impl Into<String>) {
        self.two_step_usernames
            .insert(normalize_site_key(host), username.into());
    }

    /// Evaluate a completed sign-in submission after evidence it succeeded (`FEATURES.md` §13.1).
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_sign_in_success(
        &mut self,
        page_url: &PageUrl,
        username: &str,
        password: &str,
        timestamp_iso: &str,
        private_tab: bool,
        agent_tab: bool,
        from_apple_passwords: bool,
    ) -> PostSignInAction {
        if private_tab
            || agent_tab
            || from_apple_passwords
            || password.is_empty()
            || !matches!(page_url.scheme(), "http" | "https")
        {
            return PostSignInAction::Suppressed;
        }
        let Some(raw_host) = page_url.host() else {
            return PostSignInAction::Suppressed;
        };
        let host = normalize_site_key(raw_host);
        if self.is_never_save_host(&host) {
            return PostSignInAction::Suppressed;
        }

        let effective_user = if username.trim().is_empty() {
            self.two_step_usernames
                .get(&host)
                .cloned()
                .unwrap_or_default()
        } else {
            username.trim().to_owned()
        };

        if let Some(existing) = self
            .accounts
            .iter_mut()
            .find(|a| a.host == host && a.username == effective_user)
        {
            if existing.password.expose() == password {
                existing.last_used = timestamp_iso.to_owned();
                return PostSignInAction::UpdatedLastUsedOnly;
            }
            return PostSignInAction::OfferUpdate;
        }

        PostSignInAction::OfferSave
    }

    /// Commit a confirmed password save or update (`FEATURES.md` §13.1).
    ///
    /// Refuses private tabs, agent tabs, and non-http(s) URLs.
    pub fn commit_save(
        &mut self,
        page_url: &PageUrl,
        username: &str,
        password: &str,
        timestamp_iso: &str,
        private_tab: bool,
        agent_tab: bool,
    ) -> Result<()> {
        if private_tab || agent_tab {
            return Err(CoreError::Invalid(
                "passwords are never saved from a private or agent tab".into(),
            ));
        }
        if !matches!(page_url.scheme(), "http" | "https") {
            return Err(CoreError::Invalid(
                "passwords can only be saved for http or https pages".into(),
            ));
        }
        let host = normalize_site_key(
            page_url
                .host()
                .ok_or_else(|| CoreError::Invalid("missing host".into()))?,
        );
        let origin = page_url
            .origin()
            .ok_or_else(|| CoreError::Invalid("missing origin".into()))?;
        let is_clear_http = page_url.is_insecure();

        if let Some(existing) = self
            .accounts
            .iter_mut()
            .find(|a| a.host == host && a.username == username)
        {
            existing.password = VaultSecret::new(password);
            existing.origin = origin;
            existing.saved_from_clear_http = is_clear_http;
            existing.last_used = timestamp_iso.to_owned();
            return Ok(());
        }

        self.accounts.insert(
            0,
            VaultAccount {
                host,
                origin,
                username: username.to_owned(),
                password: VaultSecret::new(password),
                saved_from_clear_http: is_clear_http,
                last_used: timestamp_iso.to_owned(),
            },
        );
        Ok(())
    }

    /// List accounts eligible to be offered under a sign-in box on `page_url`
    /// (`FEATURES.md` §13.1, capped at [`MAX_VAULT_OFFERS`] = 5):
    /// - Never offered in subframes (`!is_main_frame`) or agent tabs.
    /// - On `http://`, **only** accounts with `saved_from_clear_http == true` are offered;
    ///   passwords saved from `https://` are never offered on `http://`.
    #[must_use]
    pub fn offers_for_page(
        &self,
        page_url: &PageUrl,
        is_main_frame: bool,
        agent_tab: bool,
    ) -> Vec<&VaultAccount> {
        if !is_main_frame || agent_tab || !matches!(page_url.scheme(), "http" | "https") {
            return Vec::new();
        }
        let Some(raw_host) = page_url.host() else {
            return Vec::new();
        };
        let host = normalize_site_key(raw_host);
        let page_is_http = page_url.is_insecure();

        self.accounts
            .iter()
            .filter(|acc| acc.host == host && (!page_is_http || acc.saved_from_clear_http))
            .take(MAX_VAULT_OFFERS)
            .collect()
    }

    /// Fill a chosen credential into the page after re-verifying the committed
    /// origin, scheme, and main-frame requirement (`AGENTS.md` §9, `FEATURES.md` §13.1, P2-15).
    ///
    /// - Fails if the page navigated away from `offered_origin` before the user chose the row.
    /// - Fails if `is_main_frame` is false.
    /// - Fails if the account was saved on `https://` and `current_committed_url` is `http://`.
    pub fn fill_credential(
        &self,
        offered_origin: &str,
        current_committed_url: &PageUrl,
        username: &str,
        is_main_frame: bool,
    ) -> Result<(&str, &str)> {
        if !is_main_frame {
            return Err(CoreError::Invalid(
                "refusing to fill credentials inside a subframe".into(),
            ));
        }
        let Some(current_origin) = current_committed_url.origin() else {
            return Err(CoreError::Invalid(
                "current page has no valid origin".into(),
            ));
        };
        if current_origin != offered_origin {
            return Err(CoreError::Invalid(format!(
                "page origin changed from {offered_origin} to {current_origin} before fill"
            )));
        }
        let host = normalize_site_key(
            current_committed_url
                .host()
                .ok_or_else(|| CoreError::Invalid("missing host".into()))?,
        );
        let account = self
            .accounts
            .iter()
            .find(|a| a.host == host && a.username == username)
            .ok_or_else(|| CoreError::Invalid("no matching account for host".into()))?;

        if current_committed_url.is_insecure() && !account.saved_from_clear_http {
            return Err(CoreError::Invalid(
                "refusing to fill an HTTPS-saved password on an insecure HTTP page".into(),
            ));
        }
        if account.origin != current_origin {
            return Err(CoreError::Invalid(
                "account origin does not match committed page origin".into(),
            ));
        }

        Ok((account.username.as_str(), account.password.expose()))
    }

    /// Display representation of a password row in `search://passwords`:
    /// returns `"••••••••••"` (10 dots) unless `owner_authenticated` is true (`FEATURES.md` §13.1).
    pub fn reveal_password(
        &self,
        host: &str,
        username: &str,
        owner_authenticated: bool,
    ) -> Result<&str> {
        let clean_host = normalize_site_key(host);
        let account = self
            .accounts
            .iter()
            .find(|a| a.host == clean_host && a.username == username)
            .ok_or_else(|| CoreError::Invalid("account not found".into()))?;
        if owner_authenticated {
            Ok(account.password.expose())
        } else {
            Ok(MASKED_PASSWORD_DOTS)
        }
    }

    /// Copy a password to the clipboard on `search://passwords`, requiring
    /// device-owner authentication (`Touch ID` / `Hello` / `Secret Service`, `FEATURES.md` §13.1).
    pub fn copy_password(
        &self,
        host: &str,
        username: &str,
        owner_authenticated: bool,
    ) -> Result<ConcealedClipboardCopy> {
        if !owner_authenticated {
            return Err(CoreError::Invalid(
                "device-owner authentication is required to copy a password".into(),
            ));
        }
        let clean_host = normalize_site_key(host);
        let account = self
            .accounts
            .iter()
            .find(|a| a.host == clean_host && a.username == username)
            .ok_or_else(|| CoreError::Invalid("account not found".into()))?;
        Ok(ConcealedClipboardCopy {
            secret: account.password.clone(),
            local_host_only: true,
            concealed_and_transient: true,
            clear_after_secs: CLIPBOARD_CLEAR_SECS,
        })
    }

    /// Single-row removal is immediate and not gated on device-owner authentication (`FEATURES.md` §13.1).
    pub fn remove_one(&mut self, host: &str, username: &str) -> bool {
        let clean_host = normalize_site_key(host);
        let before = self.accounts.len();
        self.accounts
            .retain(|a| !(a.host == clean_host && a.username == username));
        self.accounts.len() != before
    }

    /// `"Remove All"` requires device-owner authentication (`FEATURES.md` §13.1).
    pub fn remove_all(&mut self, owner_authenticated: bool) -> Result<usize> {
        if !owner_authenticated {
            return Err(CoreError::Invalid(
                "device-owner authentication is required to remove all passwords".into(),
            ));
        }
        let count = self.accounts.len();
        self.accounts.clear();
        Ok(count)
    }

    /// Number of stored accounts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    /// Whether the vault is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_after_success_verifies_origin_and_never_fills_https_password_on_http() {
        let mut vault = PasswordVault::new();
        let https_url = PageUrl::parse("https://example.com/login").expect("https url");
        let http_url = PageUrl::parse("http://example.com/login").expect("http url");
        let phishing_url = PageUrl::parse("https://evil.example.org/login").expect("evil url");

        // Private and agent tabs never offer to save.
        assert_eq!(
            vault.evaluate_sign_in_success(
                &https_url,
                "alice",
                "s3cr3t-pass",
                "2026-11-21T09:55:00Z",
                true,
                false,
                false
            ),
            PostSignInAction::Suppressed
        );

        // Normal HTTPS sign-in offers Save, then commits.
        assert_eq!(
            vault.evaluate_sign_in_success(
                &https_url,
                "alice",
                "s3cr3t-pass",
                "2026-11-21T09:55:00Z",
                false,
                false,
                false
            ),
            PostSignInAction::OfferSave
        );
        vault
            .commit_save(
                &https_url,
                "alice",
                "s3cr3t-pass",
                "2026-11-21T09:55:00Z",
                false,
                false,
            )
            .expect("commit save");

        // Same password on next sign-in only updates last_used.
        assert_eq!(
            vault.evaluate_sign_in_success(
                &https_url,
                "alice",
                "s3cr3t-pass",
                "2026-11-21T10:00:00Z",
                false,
                false,
                false
            ),
            PostSignInAction::UpdatedLastUsedOnly
        );

        // Offered on HTTPS main frame, NOT offered in subframe, NOT offered on plain HTTP!
        assert_eq!(vault.offers_for_page(&https_url, true, false).len(), 1);
        assert!(vault.offers_for_page(&https_url, false, false).is_empty());
        assert!(
            vault.offers_for_page(&http_url, true, false).is_empty(),
            "HTTPS-saved password must never be offered on plain HTTP"
        );

        // Filling succeeds on the exact committed HTTPS origin in the main frame.
        let (user, pass) = vault
            .fill_credential("https://example.com", &https_url, "alice", true)
            .expect("fill on https");
        assert_eq!(user, "alice");
        assert_eq!(pass, "s3cr3t-pass");

        // Filling is refused if the page navigated to another origin or downgraded to HTTP!
        assert!(
            vault
                .fill_credential("https://example.com", &phishing_url, "alice", true)
                .is_err()
        );
        assert!(
            vault
                .fill_credential("https://example.com", &http_url, "alice", true)
                .is_err()
        );
        assert!(
            vault
                .fill_credential("https://example.com", &https_url, "alice", false)
                .is_err()
        );
    }

    #[test]
    fn enforces_show_mask_and_copy_auth_gates_and_never_logs_secrets() {
        let mut vault = PasswordVault::new();
        let url = PageUrl::parse("https://example.com/").expect("url");
        vault
            .commit_save(
                &url,
                "bob",
                "my-very-secret-key",
                "2026-11-21T10:00:00Z",
                false,
                false,
            )
            .expect("save");

        let debug_text = format!("{vault:?}");
        assert!(
            !debug_text.contains("my-very-secret-key"),
            "Debug output must never contain a plaintext password"
        );

        assert_eq!(
            vault
                .reveal_password("example.com", "bob", false)
                .expect("masked"),
            MASKED_PASSWORD_DOTS
        );
        assert_eq!(
            vault
                .reveal_password("example.com", "bob", true)
                .expect("unmasked"),
            "my-very-secret-key"
        );

        assert!(vault.copy_password("example.com", "bob", false).is_err());
        let copied = vault
            .copy_password("example.com", "bob", true)
            .expect("copy with auth");
        assert!(copied.local_host_only);
        assert!(copied.concealed_and_transient);
        assert_eq!(copied.clear_after_secs, 90);
    }
}
