//! Test doubles for every platform service.

use crate::credentials::{Credential, CredentialStore};
use crate::files::{FileDialog, FileFilter};
use crate::notifications::{Notification, Notifier};
use crate::opener::Opener;
use crate::{NoopOpener, PlatformResult};
use searlx_core::PageUrl;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// An in-memory credential store.
#[derive(Debug, Default)]
pub struct MemoryCredentialStore {
    entries: Mutex<BTreeMap<String, Vec<Credential>>>,
}

impl CredentialStore for MemoryCredentialStore {
    fn save(&self, credential: &Credential) -> PlatformResult<()> {
        let mut entries = self.entries.lock().expect("test store poisoned");
        let list = entries.entry(credential.origin.clone()).or_default();
        list.retain(|existing| existing.username != credential.username);
        list.insert(0, credential.clone());
        Ok(())
    }

    fn find(&self, origin: &str) -> PlatformResult<Vec<Credential>> {
        let entries = self.entries.lock().expect("test store poisoned");
        Ok(entries.get(origin).cloned().unwrap_or_default())
    }

    fn delete(&self, origin: &str, username: &str) -> PlatformResult<()> {
        let mut entries = self.entries.lock().expect("test store poisoned");
        if let Some(list) = entries.get_mut(origin) {
            list.retain(|credential| credential.username != username);
        }
        Ok(())
    }

    fn delete_origin(&self, origin: &str) -> PlatformResult<()> {
        self.entries
            .lock()
            .expect("test store poisoned")
            .remove(origin);
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "memory"
    }
}

/// A scripted file dialog.
#[derive(Debug, Default)]
pub struct ScriptedFileDialog {
    /// What `pick_file` returns.
    pub next_file: Mutex<Option<PathBuf>>,
    /// What `pick_directory` returns.
    pub next_directory: Mutex<Option<PathBuf>>,
    /// What `save_file` returns.
    pub next_save: Mutex<Option<PathBuf>>,
}

impl FileDialog for ScriptedFileDialog {
    fn pick_file(&self, _title: &str, _filters: &[FileFilter]) -> PlatformResult<Option<PathBuf>> {
        Ok(self.next_file.lock().expect("dialog poisoned").clone())
    }

    fn pick_files(&self, _title: &str, _filters: &[FileFilter]) -> PlatformResult<Vec<PathBuf>> {
        Ok(self
            .next_file
            .lock()
            .expect("dialog poisoned")
            .clone()
            .into_iter()
            .collect())
    }

    fn pick_directory(&self, _title: &str) -> PlatformResult<Option<PathBuf>> {
        Ok(self.next_directory.lock().expect("dialog poisoned").clone())
    }

    fn save_file(
        &self,
        _title: &str,
        _suggested_name: &str,
        _filters: &[FileFilter],
    ) -> PlatformResult<Option<PathBuf>> {
        Ok(self.next_save.lock().expect("dialog poisoned").clone())
    }

    fn reveal(&self, _path: &Path) -> PlatformResult<()> {
        Ok(())
    }
}

/// A notifier that records what it was asked to show.
#[derive(Debug, Default)]
pub struct RecordingNotifier {
    /// Everything shown.
    pub notifications: Mutex<Vec<Notification>>,
}

impl Notifier for RecordingNotifier {
    fn notify(&self, notification: &Notification) -> PlatformResult<()> {
        self.notifications
            .lock()
            .expect("notifier poisoned")
            .push(notification.clone());
        Ok(())
    }
}

/// An opener that records URLs.
#[derive(Debug, Default)]
pub struct RecordingOpener {
    /// Opened URLs.
    pub urls: Mutex<Vec<String>>,
}

impl Opener for RecordingOpener {
    fn open_url(&self, url: &PageUrl) -> PlatformResult<()> {
        self.urls
            .lock()
            .expect("opener poisoned")
            .push(url.as_str().to_owned());
        Ok(())
    }

    fn open_path(&self, path: &Path) -> PlatformResult<()> {
        self.urls
            .lock()
            .expect("opener poisoned")
            .push(path.display().to_string());
        Ok(())
    }
}

/// A bundle of fakes wired into one struct.
#[derive(Debug, Default)]
pub struct FakePlatform {
    /// Credential store.
    pub credentials: MemoryCredentialStore,
    /// File dialog.
    pub files: ScriptedFileDialog,
    /// Notifier.
    pub notifications: RecordingNotifier,
    /// Opener.
    pub opener: RecordingOpener,
}

impl FakePlatform {
    /// A platform whose opener is a no-op.
    #[must_use]
    pub fn noop_opener() -> (Self, NoopOpener) {
        (Self::default(), NoopOpener)
    }
}
