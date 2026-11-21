//! WebKitGTK backend (Linux).
//!
//! Phase 0 measures whether WebKitGTK can live inside a gpui window on X11 and
//! Wayland. Until that spike passes, this crate exposes only the capability
//! descriptor and a clear unsupported result; it never pretends to render.

use searlx_engine::Capabilities;

/// The backend's name.
pub const ENGINE_NAME: &str = "webkitgtk";

/// What this backend supports, based on the Phase 0 measurements and the
/// documented WebKitGTK API.
///
/// A `false` flag is a real "hide or disable the feature" signal, not a
/// to-do. `native_pip` is false because the P0-07 probe found no
/// `document.pictureInPictureEnabled` and no `requestPictureInPicture` on
/// WebKitGTK 2.54.2. `pdf`, `find_while_pdf`, and `downloads_pause` stay false
/// until a test proves them.
#[must_use]
pub fn target_capabilities() -> Capabilities {
    let mut capabilities = Capabilities::baseline(ENGINE_NAME);
    capabilities.content_blockers = true;
    capabilities.custom_user_agent = true;
    capabilities.print = true;
    capabilities
}

/// Whether the experimental backend is built in this binary.
#[must_use]
pub const fn enabled() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_does_not_claim_unsupported_features() {
        let capabilities = target_capabilities();
        assert!(!capabilities.extensions);
        assert!(!capabilities.passkeys);
        assert!(!capabilities.native_pip);
        assert!(!capabilities.site_isolation);
        assert!(!capabilities.floating_video);
        assert!(capabilities.content_blockers);
    }
}
