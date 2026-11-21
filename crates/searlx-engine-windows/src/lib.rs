//! WebView2 backend (Windows).
//!
//! The concrete backend is gated to Windows. On other hosts this crate exposes
//! only the capability descriptor.

use searlx_engine::Capabilities;

/// The backend's name.
pub const ENGINE_NAME: &str = "webview2";

/// What this backend is expected to support once P1-05 lands.
#[must_use]
pub fn target_capabilities() -> Capabilities {
    let mut capabilities = Capabilities::baseline(ENGINE_NAME);
    capabilities.passkeys = true;
    capabilities.content_blockers = false;
    capabilities.native_pip = true;
    capabilities.custom_user_agent = true;
    capabilities.print = true;
    capabilities.pdf = true;
    capabilities.downloads_pause = true;
    capabilities.cookie_import = true;
    capabilities
}
