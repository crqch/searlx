//! WKWebView backend (macOS).
//!
//! The concrete backend is gated to macOS. On other hosts this crate exposes
//! only the capability descriptor so the workspace still checks everywhere.

use searlx_engine::Capabilities;

/// The backend's name.
pub const ENGINE_NAME: &str = "wkwebview";

/// What this backend is expected to support once P1-04 lands.
///
/// These flags are the Phase 0 target, not a claim: the backend must pass the
/// contract suite before the UI enables a feature.
#[must_use]
pub fn target_capabilities() -> Capabilities {
    let mut capabilities = Capabilities::baseline(ENGINE_NAME);
    capabilities.extensions = true;
    capabilities.passkeys = true;
    capabilities.content_blockers = true;
    capabilities.native_pip = true;
    capabilities.site_isolation = true;
    capabilities.custom_user_agent = true;
    capabilities.print = true;
    capabilities.pdf = true;
    capabilities.find_while_pdf = true;
    capabilities.downloads_pause = true;
    capabilities.cookie_import = true;
    capabilities.floating_video = true;
    capabilities
}
