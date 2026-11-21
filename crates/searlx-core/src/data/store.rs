//! Thread-safe data store and schema migrations (P1-02).
//!
//! [`DataStore`] wraps a profile/app data directory (`FEATURES.md` §25).
//! - All writes acquire an internal lock so concurrent saves are serialized.
//! - Every save writes a temporary sibling and atomically renames it into place.
//! - A corrupt file is quarantined as `*.unreadable-<UTC>.json` without
//!   overwriting any previous quarantine, and callers can fall back to defaults.
//! - Versioned files (`session.json`, `bookmarks.json`) upgrade version `0` to
//!   their current schema version and refuse future unsupported versions.
//! - [`migrate_legacy_office_browser_dir`] performs the one-time
//!   `"Office Browser"` → `"Search"` directory migration described in
//!   `FEATURES.md` §25.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::data::{
    ArchiveFile, BookmarksFile, DataError, DownloadsFile, HiddenFile, HistoryFile, PinFoldersFile,
    PinsFile, ProfilesFile, SearchSitesFile, SessionFile, SpacesFile, StylesFile, WindowsFile,
    read_json, write_json,
};
use crate::domain::CURRENT_SCHEMA_VERSION;
use crate::error::{CoreError, Result};

/// Current schema version for `bookmarks.json`.
pub const CURRENT_BOOKMARKS_VERSION: u32 = 2;

/// Outcome of loading a data file with automatic fallback on missing or corrupt files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadOutcome<T> {
    /// The decoded (or default) value.
    pub value: T,
    /// If a corrupt file was encountered and quarantined, the path it was moved to.
    pub quarantined_to: Option<PathBuf>,
}

/// Thread-safe data store for a `Search` Application Support directory.
#[derive(Debug)]
pub struct DataStore {
    root: PathBuf,
    write_lock: Mutex<()>,
}

impl DataStore {
    /// Open or target a data store rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            write_lock: Mutex::new(()),
        }
    }

    /// Root directory of this store.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a relative file name inside the store root.
    #[must_use]
    pub fn path_for(&self, file_name: &str) -> PathBuf {
        self.root.join(file_name)
    }

    /// Read and decode a file inside the store, quarantining on bad JSON.
    pub fn read_file<T: DeserializeOwned>(
        &self,
        file_name: &str,
    ) -> std::result::Result<T, DataError> {
        read_json(&self.path_for(file_name))
    }

    /// Read a file inside the store, returning `T::default()` if the file does
    /// not exist or if it was corrupt and quarantined.
    pub fn read_or_default<T: DeserializeOwned + Default>(
        &self,
        file_name: &str,
    ) -> Result<LoadOutcome<T>> {
        let path = self.path_for(file_name);
        match read_json::<T>(&path) {
            Ok(value) => Ok(LoadOutcome {
                value,
                quarantined_to: None,
            }),
            Err(DataError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Ok(LoadOutcome {
                    value: T::default(),
                    quarantined_to: None,
                })
            }
            Err(DataError::Decode { quarantined_to, .. }) => Ok(LoadOutcome {
                value: T::default(),
                quarantined_to,
            }),
            Err(DataError::Read { path, source }) => Err(CoreError::Io { path, source }),
            Err(DataError::Encode { path, source }) => Err(CoreError::Decode { path, source }),
            Err(DataError::Write { path, source }) => Err(CoreError::Io { path, source }),
            Err(DataError::Quarantine { path, source }) => {
                Err(CoreError::Quarantine { path, source })
            }
        }
    }

    /// Write a JSON file atomically with serialized write access.
    pub fn write_file<T: Serialize>(&self, file_name: &str, value: &T) -> Result<()> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = self.path_for(file_name);
        write_json(&path, value).map_err(map_data_error)
    }

    /// Load and migrate `session.json` (or `session-<space>.json`).
    pub fn load_session(&self, space_id: Option<&str>) -> Result<SessionFile> {
        let name = match space_id {
            Some(id) if !id.is_empty() => format!("session-{id}.json"),
            _ => "session.json".to_owned(),
        };
        let path = self.path_for(&name);
        let mut session: SessionFile = read_json(&path).map_err(map_data_error)?;
        migrate_session_file(&path, &mut session)?;
        Ok(session)
    }

    /// Save `session.json` (or `session-<space>.json`) at the current schema version.
    pub fn save_session(&self, space_id: Option<&str>, session: &SessionFile) -> Result<()> {
        let name = match space_id {
            Some(id) if !id.is_empty() => format!("session-{id}.json"),
            _ => "session.json".to_owned(),
        };
        let mut copy = session.clone();
        if copy.version == 0 {
            copy.version = CURRENT_SCHEMA_VERSION;
        }
        self.write_file(&name, &copy)
    }

    /// Load and migrate `bookmarks.json` (or `bookmarks-<profile>.json`).
    pub fn load_bookmarks(&self, profile_id: Option<&str>) -> Result<BookmarksFile> {
        let name = match profile_id {
            Some(id) if !id.is_empty() => format!("bookmarks-{id}.json"),
            _ => "bookmarks.json".to_owned(),
        };
        let path = self.path_for(&name);
        let mut bookmarks: BookmarksFile = read_json(&path).map_err(map_data_error)?;
        migrate_bookmarks_file(&path, &mut bookmarks)?;
        Ok(bookmarks)
    }

    /// Save `bookmarks.json` (or `bookmarks-<profile>.json`).
    pub fn save_bookmarks(
        &self,
        profile_id: Option<&str>,
        bookmarks: &BookmarksFile,
    ) -> Result<()> {
        let name = match profile_id {
            Some(id) if !id.is_empty() => format!("bookmarks-{id}.json"),
            _ => "bookmarks.json".to_owned(),
        };
        let mut copy = bookmarks.clone();
        if copy.version == 0 {
            copy.version = CURRENT_BOOKMARKS_VERSION;
        }
        self.write_file(&name, &copy)
    }

    /// Load `windows.json`.
    pub fn load_windows(&self) -> Result<WindowsFile> {
        self.read_file("windows.json").map_err(map_data_error)
    }

    /// Save `windows.json`.
    pub fn save_windows(&self, windows: &WindowsFile) -> Result<()> {
        self.write_file("windows.json", windows)
    }

    /// Load `pins.json`.
    pub fn load_pins(&self) -> Result<PinsFile> {
        self.read_file("pins.json").map_err(map_data_error)
    }

    /// Save `pins.json`.
    pub fn save_pins(&self, pins: &PinsFile) -> Result<()> {
        self.write_file("pins.json", pins)
    }

    /// Load `pin-folders.json`.
    pub fn load_pin_folders(&self) -> Result<PinFoldersFile> {
        self.read_file("pin-folders.json").map_err(map_data_error)
    }

    /// Save `pin-folders.json`.
    pub fn save_pin_folders(&self, folders: &PinFoldersFile) -> Result<()> {
        self.write_file("pin-folders.json", folders)
    }

    /// Load `spaces.json`.
    pub fn load_spaces(&self) -> Result<SpacesFile> {
        self.read_file("spaces.json").map_err(map_data_error)
    }

    /// Save `spaces.json`.
    pub fn save_spaces(&self, spaces: &SpacesFile) -> Result<()> {
        self.write_file("spaces.json", spaces)
    }

    /// Load `profiles.json`.
    pub fn load_profiles(&self) -> Result<ProfilesFile> {
        self.read_file("profiles.json").map_err(map_data_error)
    }

    /// Save `profiles.json`.
    pub fn save_profiles(&self, profiles: &ProfilesFile) -> Result<()> {
        self.write_file("profiles.json", profiles)
    }

    /// Load `history.json` (or `history-<profile>.json` / `history-space-<space>.json`).
    pub fn load_history(&self, suffix: Option<&str>) -> Result<HistoryFile> {
        let name = match suffix {
            Some(s) if !s.is_empty() => format!("history-{s}.json"),
            _ => "history.json".to_owned(),
        };
        self.read_file(&name).map_err(map_data_error)
    }

    /// Save `history.json` (or scoped variant), enforcing the 2,000-entry disk cap (`FEATURES.md` §10).
    pub fn save_history(&self, suffix: Option<&str>, history: &HistoryFile) -> Result<()> {
        let name = match suffix {
            Some(s) if !s.is_empty() => format!("history-{s}.json"),
            _ => "history.json".to_owned(),
        };
        let mut capped = history.clone();
        if capped.visits.len() > 2_000 {
            capped.visits.truncate(2_000);
        }
        self.write_file(&name, &capped)
    }

    /// Load `archive.json` (or `archive-<profile>.json`).
    pub fn load_archive(&self, profile_id: Option<&str>) -> Result<ArchiveFile> {
        let name = match profile_id {
            Some(id) if !id.is_empty() => format!("archive-{id}.json"),
            _ => "archive.json".to_owned(),
        };
        self.read_file(&name).map_err(map_data_error)
    }

    /// Save `archive.json` (or `archive-<profile>.json`), enforcing the 500-entry cap (`FEATURES.md` §10).
    pub fn save_archive(&self, profile_id: Option<&str>, archive: &ArchiveFile) -> Result<()> {
        let name = match profile_id {
            Some(id) if !id.is_empty() => format!("archive-{id}.json"),
            _ => "archive.json".to_owned(),
        };
        let mut capped = archive.clone();
        if capped.tabs.len() > 500 {
            capped.tabs.truncate(500);
        }
        self.write_file(&name, &capped)
    }

    /// Load `downloads.json`.
    pub fn load_downloads(&self) -> Result<DownloadsFile> {
        self.read_file("downloads.json").map_err(map_data_error)
    }

    /// Save `downloads.json`, enforcing the 50-row cap (`FEATURES.md` §11).
    pub fn save_downloads(&self, downloads: &DownloadsFile) -> Result<()> {
        let mut capped = downloads.clone();
        if capped.downloads.len() > 50 {
            capped.downloads.truncate(50);
        }
        self.write_file("downloads.json", &capped)
    }

    /// Load `hidden.json`.
    pub fn load_hidden(&self) -> Result<HiddenFile> {
        self.read_file("hidden.json").map_err(map_data_error)
    }

    /// Save `hidden.json`.
    pub fn save_hidden(&self, hidden: &HiddenFile) -> Result<()> {
        self.write_file("hidden.json", hidden)
    }

    /// Load `styles.json`.
    pub fn load_styles(&self) -> Result<StylesFile> {
        self.read_file("styles.json").map_err(map_data_error)
    }

    /// Save `styles.json`.
    pub fn save_styles(&self, styles: &StylesFile) -> Result<()> {
        self.write_file("styles.json", styles)
    }

    /// Load `search-sites.json`.
    pub fn load_search_sites(&self) -> Result<SearchSitesFile> {
        self.read_file("search-sites.json").map_err(map_data_error)
    }

    /// Save `search-sites.json`.
    pub fn save_search_sites(&self, sites: &SearchSitesFile) -> Result<()> {
        self.write_file("search-sites.json", sites)
    }
}

/// Migrate a [`SessionFile`] in-place to [`CURRENT_SCHEMA_VERSION`].
pub fn migrate_session_file(path: &Path, session: &mut SessionFile) -> Result<()> {
    if session.version > CURRENT_SCHEMA_VERSION {
        return Err(CoreError::UnsupportedVersion {
            path: path.to_path_buf(),
            found: session.version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if session.version < CURRENT_SCHEMA_VERSION {
        session.version = CURRENT_SCHEMA_VERSION;
    }
    if !session.tabs.is_empty() && session.active >= session.tabs.len() {
        session.active = session.tabs.len() - 1;
    }
    Ok(())
}

/// Migrate a [`BookmarksFile`] in-place to [`CURRENT_BOOKMARKS_VERSION`].
pub fn migrate_bookmarks_file(path: &Path, bookmarks: &mut BookmarksFile) -> Result<()> {
    if bookmarks.version > CURRENT_SCHEMA_VERSION {
        return Err(CoreError::UnsupportedVersion {
            path: path.to_path_buf(),
            found: bookmarks.version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if bookmarks.version == 0 {
        bookmarks.version = CURRENT_BOOKMARKS_VERSION;
    }
    Ok(())
}

/// One-time migration from `"Office Browser"` to `"Search"` (`FEATURES.md` §25):
/// if `search_dir` does not exist and `office_browser_dir` exists, renames it into place.
pub fn migrate_legacy_office_browser_dir(
    office_browser_dir: &Path,
    search_dir: &Path,
) -> Result<bool> {
    if search_dir.exists() || !office_browser_dir.exists() {
        return Ok(false);
    }
    if let Some(parent) = search_dir.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| CoreError::io(parent, source))?;
    }
    fs::rename(office_browser_dir, search_dir)
        .map_err(|source| CoreError::io(search_dir, source))?;
    Ok(true)
}

fn map_data_error(error: DataError) -> CoreError {
    match error {
        DataError::Read { path, source } | DataError::Write { path, source } => {
            CoreError::Io { path, source }
        }
        DataError::Decode { path, source, .. } | DataError::Encode { path, source } => {
            CoreError::Decode { path, source }
        }
        DataError::Quarantine { path, source } => CoreError::Quarantine { path, source },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn read_or_default_quarantines_corrupt_and_handles_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());

        let missing: LoadOutcome<SpacesFile> =
            store.read_or_default("spaces.json").expect("missing ok");
        assert!(missing.value.spaces.is_empty());
        assert!(missing.quarantined_to.is_none());

        fs::write(store.path_for("spaces.json"), b"{ broken").expect("write broken");
        let corrupt: LoadOutcome<SpacesFile> = store
            .read_or_default("spaces.json")
            .expect("corrupt falls back");
        assert!(corrupt.value.spaces.is_empty());
        let quarantined = corrupt.quarantined_to.expect("quarantined path");
        assert!(quarantined.exists());
        assert!(!store.path_for("spaces.json").exists());
    }

    #[test]
    fn concurrent_writes_are_serialized_and_atomic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(DataStore::new(temp.path()));
        let mut handles = Vec::new();
        for index in 0..8_u32 {
            let store = Arc::clone(&store);
            handles.push(thread::spawn(move || {
                let mut history = HistoryFile::default();
                history.visits.push(crate::data::Visit {
                    url: format!("https://example.com/{index}"),
                    title: format!("Page {index}"),
                    visit_count: index + 1,
                    ..crate::data::Visit::default()
                });
                store.save_history(None, &history).expect("save history");
            }));
        }
        for handle in handles {
            handle.join().expect("thread join");
        }
        let final_history = store.load_history(None).expect("load final history");
        assert_eq!(final_history.visits.len(), 1);
    }

    #[test]
    fn migrates_office_browser_directory_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let old_dir = temp.path().join("Office Browser");
        let new_dir = temp.path().join("Search");
        fs::create_dir_all(&old_dir).expect("create old");
        fs::write(old_dir.join("spaces.json"), b"{\"spaces\":[]}\n").expect("write spaces");

        let moved = migrate_legacy_office_browser_dir(&old_dir, &new_dir).expect("migrate");
        assert!(moved);
        assert!(!old_dir.exists());
        assert!(new_dir.join("spaces.json").exists());

        let moved_again =
            migrate_legacy_office_browser_dir(&old_dir, &new_dir).expect("migrate second time");
        assert!(!moved_again);
    }
}
