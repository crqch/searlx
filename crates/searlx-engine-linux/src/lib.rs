//! WebKitGTK backend (Linux).
//!
//! Phase 0 measures whether WebKitGTK can live inside a gpui window on X11 and
//! Wayland. Until that spike passes, this crate exposes only the capability
//! descriptor and a clear unsupported result; it never pretends to render.

use searlx_engine::Capabilities;

/// The backend's name.
pub const ENGINE_NAME: &str = "webkitgtk";

/// What this backend is expected to support if the Linux spike passes.
#[must_use]
pub fn target_capabilities() -> Capabilities {
    let mut capabilities = Capabilities::baseline(ENGINE_NAME);
    capabilities.content_blockers = false;
    capabilities.native_pip = true;
    capabilities.custom_user_agent = true;
    capabilities.print = true;
    capabilities.pdf = true;
    capabilities.downloads_pause = true;
    capabilities.cookie_import = true;
    capabilities
}

/// Whether the experimental backend is built in this binary.
#[must_use]
pub const fn enabled() -> bool {
    false
}
