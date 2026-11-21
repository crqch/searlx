//! Operating-system services behind traits.
//!
//! Every service has a fake for tests. A platform that cannot provide a
//! service returns [`Unsupported`] rather than panicking, and the UI hides the
//! feature.

use searlx_core::PageUrl;

pub mod credentials;
pub mod fake;
pub mod files;
pub mod notifications;
pub mod opener;
pub mod packaging;
pub mod power;

pub use credentials::{Credential, CredentialStore};
pub use files::{FileDialog, FileFilter};
pub use notifications::Notifier;
pub use opener::Opener;
pub use packaging::{
    ALLOWED_DOCUMENT_EXTENSIONS, WEBVIEW2_CLIENT_GUID, WEBVIEW2_MISSING_MESSAGE, WebView2Status,
    check_webview2_runtime, evaluate_webview2_version, macos_info_plist, stage_macos_app_bundle,
};
pub use power::{PowerAssertion, PowerManager};

/// A service this platform cannot provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unsupported on this platform")]
pub struct Unsupported;

/// A boxed platform error.
pub type PlatformError = Box<dyn std::error::Error + Send + Sync>;

/// A platform result.
pub type PlatformResult<T> = Result<T, PlatformError>;

/// The platform's user-visible name.
#[must_use]
pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "unknown"
    }
}

/// A platform opener that refuses everything, used in tests and headless runs.
#[derive(Debug, Default)]
pub struct NoopOpener;

impl Opener for NoopOpener {
    fn open_url(&self, _url: &PageUrl) -> PlatformResult<()> {
        Ok(())
    }

    fn open_path(&self, _path: &std::path::Path) -> PlatformResult<()> {
        Ok(())
    }
}
