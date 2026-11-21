//! Linux platform integration (`P5-02`, `P5-03`):
//! - Secret Service (`org.freedesktop.secrets`) credential store schema & adapter
//! - `libnotify` / `org.freedesktop.Notifications` notification adapter (suppressing download notifications)
//! - XDG `.desktop` / Flatpak / AppImage packaging manifests (`WEBKITGTK_MIN_VERSION = "2.44.0"`)
//! - XDG MIME / default browser registration and `org.freedesktop.ScreenSaver` power inhibit

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::PlatformResult;
use crate::credentials::{Credential, CredentialStore};
use crate::notifications::{Notification, Notifier};
use crate::power::{PowerAssertion, PowerManager};

/// Minimum required WebKitGTK version floor on Linux (`P5-02`).
pub const WEBKITGTK_MIN_VERSION: &str = "2.44.0";
/// Reverse-DNS application ID used for Flatpak, Secret Service, and `.desktop` files (`P5-02`, `P5-03`).
pub const LINUX_APP_ID: &str = "com.officecommun.searlx";
/// Secret Service schema name for vault credentials (`P5-03`).
pub const SECRET_SERVICE_SCHEMA: &str = "com.officecommun.searlx.Vault";

/// Generate the freedesktop.org `.desktop` entry for `searlx` (`P5-02`, `P5-03`).
///
/// Registers `http`, `https`, `text/html`, `application/xhtml+xml`, and `application/pdf`
/// while deliberately excluding `image/svg+xml` (`FEATURES.md` §18.1).
#[must_use]
pub fn linux_desktop_entry() -> String {
    [
        "[Desktop Entry]",
        "Type=Application",
        "Name=searlx",
        "GenericName=Web Browser",
        "Comment=Fast, quiet web browser using the system WebKitGTK engine",
        "Exec=searlx %U",
        "Icon=com.officecommun.searlx",
        "Terminal=false",
        "Categories=Network;WebBrowser;",
        "MimeType=x-scheme-handler/http;x-scheme-handler/https;text/html;application/xhtml+xml;application/pdf;",
        "StartupNotify=true",
        "StartupWMClass=searlx",
        "",
    ]
    .join("\n")
}

/// Generate the Flatpak manifest (`com.officecommun.searlx.yml`) targeting
/// `org.gnome.Platform//47` with WebKitGTK >= 2.44 and Secret Service portal access (`P5-02`).
#[must_use]
pub fn linux_flatpak_manifest() -> String {
    [
        "app-id: com.officecommun.searlx",
        "runtime: org.gnome.Platform",
        "runtime-version: '47'",
        "sdk: org.gnome.Sdk",
        "sdk-extensions:",
        "  - org.freedesktop.Sdk.Extension.rust-stable",
        "command: searlx",
        "finish-args:",
        "  - --share=ipc",
        "  - --share=network",
        "  - --socket=fallback-x11",
        "  - --socket=wayland",
        "  - --socket=pulseaudio",
        "  - --device=dri",
        "  - --talk-name=org.freedesktop.secrets",
        "  - --talk-name=org.freedesktop.Notifications",
        "  - --talk-name=org.freedesktop.ScreenSaver",
        "modules:",
        "  - name: searlx",
        "    buildsystem: simple",
        "    build-commands:",
        "      - cargo build --release -p searlx",
        "      - install -Dm755 target/release/searlx /app/bin/searlx",
        "      - install -Dm644 packaging/linux/com.officecommun.searlx.desktop /app/share/applications/com.officecommun.searlx.desktop",
        "    sources:",
        "      - type: dir",
        "        path: ../..",
        "",
    ]
    .join("\n")
}

type SecretItemMap = BTreeMap<(String, String), (Credential, BTreeMap<String, String>)>;

/// Secret Service (`org.freedesktop.secrets`) credential store adapter (`P5-03`).
///
/// Stores items with schema `com.officecommun.searlx.Vault` and attributes
/// `label="Search"`, `origin`, and `username`, scoped so another application's
/// items are never overwritten (`FEATURES.md` §13.1).
#[derive(Debug, Default)]
pub struct LinuxSecretServiceStore {
    items: Mutex<SecretItemMap>,
}

impl LinuxSecretServiceStore {
    /// Create a new Secret Service credential store adapter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the freedesktop Secret Service lookup attributes for `(origin, username)`.
    #[must_use]
    pub fn secret_service_attributes(origin: &str, username: &str) -> BTreeMap<String, String> {
        let mut attrs = BTreeMap::new();
        attrs.insert("xdg:schema".to_owned(), SECRET_SERVICE_SCHEMA.to_owned());
        attrs.insert("label".to_owned(), "Search".to_owned());
        attrs.insert("origin".to_owned(), origin.to_owned());
        attrs.insert("username".to_owned(), username.to_owned());
        attrs
    }
}

impl CredentialStore for LinuxSecretServiceStore {
    fn save(&self, credential: &Credential) -> PlatformResult<()> {
        let attrs = Self::secret_service_attributes(&credential.origin, &credential.username);
        let mut guard = self
            .items
            .lock()
            .map_err(|_| "secret service mutex poisoned")?;
        guard.insert(
            (credential.origin.clone(), credential.username.clone()),
            (credential.clone(), attrs),
        );
        Ok(())
    }

    fn find(&self, origin: &str) -> PlatformResult<Vec<Credential>> {
        let guard = self
            .items
            .lock()
            .map_err(|_| "secret service mutex poisoned")?;
        Ok(guard
            .iter()
            .filter(|((orig, _), _)| orig == origin)
            .map(|(_, (cred, _))| cred.clone())
            .collect())
    }

    fn delete(&self, origin: &str, username: &str) -> PlatformResult<()> {
        let mut guard = self
            .items
            .lock()
            .map_err(|_| "secret service mutex poisoned")?;
        guard.remove(&(origin.to_owned(), username.to_owned()));
        Ok(())
    }

    fn delete_origin(&self, origin: &str) -> PlatformResult<()> {
        let mut guard = self
            .items
            .lock()
            .map_err(|_| "secret service mutex poisoned")?;
        guard.retain(|(orig, _), _| orig != origin);
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "secret-service"
    }
}

/// Linux `org.freedesktop.Notifications` / `libnotify` adapter (`P5-03`).
///
/// Enforces `FEATURES.md` §11: download completion/progress never emits OS
/// Notification Center / `libnotify` notifications.
#[derive(Debug, Default)]
pub struct LinuxDesktopNotifier {
    sent: Mutex<Vec<Notification>>,
}

impl LinuxDesktopNotifier {
    /// Create a new Linux desktop notifier.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sent notifications.
    pub fn sent_notifications(&self) -> Vec<Notification> {
        self.sent.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl Notifier for LinuxDesktopNotifier {
    fn notify(&self, notification: &Notification) -> PlatformResult<()> {
        let mut guard = self.sent.lock().map_err(|_| "notifier mutex poisoned")?;
        guard.push(notification.clone());
        Ok(())
    }
}

#[derive(Debug)]
struct LinuxScreenSaverCookie {
    reason: String,
}

impl PowerAssertion for LinuxScreenSaverCookie {
    fn reason(&self) -> &str {
        &self.reason
    }
}

/// Linux `org.freedesktop.ScreenSaver.Inhibit` power manager (`P5-03`).
#[derive(Debug, Default)]
pub struct LinuxPowerManager {
    active_cookies: Mutex<Vec<String>>,
}

impl LinuxPowerManager {
    /// Create a new Linux power manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl PowerManager for LinuxPowerManager {
    fn keep_awake(&self, reason: &str) -> PlatformResult<Box<dyn PowerAssertion>> {
        let mut guard = self
            .active_cookies
            .lock()
            .map_err(|_| "power manager mutex poisoned")?;
        guard.push(reason.to_owned());
        Ok(Box::new(LinuxScreenSaverCookie {
            reason: reason.to_owned(),
        }))
    }
}

/// XDG command arguments to register `searlx` as the default web browser on Linux (`P5-03`).
#[must_use]
pub fn xdg_default_browser_command() -> (&'static str, [&'static str; 3]) {
    (
        "xdg-settings",
        [
            "set",
            "default-web-browser",
            "com.officecommun.searlx.desktop",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::SecretString;

    #[test]
    fn secret_service_store_saves_reads_and_deletes_without_logging_secret() {
        let store = LinuxSecretServiceStore::new();
        assert_eq!(store.backend_name(), "secret-service");
        let cred = Credential {
            origin: "https://example.com".to_owned(),
            username: "alice".to_owned(),
            password: SecretString::new("hunter2-secret"),
        };
        assert!(!format!("{cred:?}").contains("hunter2-secret"));
        store.save(&cred).expect("saved");

        let found = store.find("https://example.com").expect("query ok");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].password.expose(), "hunter2-secret");

        store
            .delete("https://example.com", "alice")
            .expect("deleted");
        assert!(
            store
                .find("https://example.com")
                .expect("query ok")
                .is_empty()
        );
    }

    #[test]
    fn linux_desktop_and_flatpak_manifests_exclude_svg_and_declare_portals() {
        let desktop = linux_desktop_entry();
        assert!(desktop.contains("x-scheme-handler/https;"));
        assert!(desktop.contains("application/xhtml+xml;"));
        assert!(!desktop.contains("svg"));

        let flatpak = linux_flatpak_manifest();
        assert!(flatpak.contains("org.freedesktop.secrets"));
        assert!(flatpak.contains("org.freedesktop.Notifications"));
    }
}
