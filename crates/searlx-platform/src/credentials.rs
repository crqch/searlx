//! Credential storage.

use crate::{PlatformResult, Unsupported};
use searlx_core::PageUrl;

/// One saved sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credential {
    /// The origin the credential belongs to.
    pub origin: String,
    /// The user name.
    pub username: String,
    /// The secret. Never log or format this value.
    pub password: SecretString,
}

/// A secret that redacts itself in `Debug` and `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap a secret.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the secret for a credential prompt.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretString(…)")
    }
}

/// The platform password store.
pub trait CredentialStore: Send + Sync {
    /// Save or replace a credential for an origin.
    fn save(&self, credential: &Credential) -> PlatformResult<()>;

    /// Find credentials for an origin, newest first.
    fn find(&self, origin: &str) -> PlatformResult<Vec<Credential>>;

    /// Delete one credential.
    fn delete(&self, origin: &str, username: &str) -> PlatformResult<()>;

    /// Delete every credential for an origin.
    fn delete_origin(&self, origin: &str) -> PlatformResult<()>;

    /// The store's stable backend name, for Settings.
    fn backend_name(&self) -> &'static str;
}

/// A store for platforms without one. Every method returns [`Unsupported`].
#[derive(Debug, Default)]
pub struct UnsupportedCredentialStore;

impl CredentialStore for UnsupportedCredentialStore {
    fn save(&self, _credential: &Credential) -> PlatformResult<()> {
        Err(Box::new(Unsupported))
    }

    fn find(&self, _origin: &str) -> PlatformResult<Vec<Credential>> {
        Err(Box::new(Unsupported))
    }

    fn delete(&self, _origin: &str, _username: &str) -> PlatformResult<()> {
        Err(Box::new(Unsupported))
    }

    fn delete_origin(&self, _origin: &str) -> PlatformResult<()> {
        Err(Box::new(Unsupported))
    }

    fn backend_name(&self) -> &'static str {
        "unsupported"
    }
}

/// Refuse credentials for insecure origins where the spec requires it.
#[must_use]
pub fn may_fill(credential: &Credential, page: &PageUrl) -> bool {
    let Some(origin) = page.origin() else {
        return false;
    };
    origin == credential.origin && !page.is_insecure()
}
