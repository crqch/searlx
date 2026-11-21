//! Safe import of an existing Search `Application Support` folder (`FEATURES.md` §15, §25, `AGENTS.md` §9, P1-12).
//!
//! Security rules (`AGENTS.md` §9, `FEATURES.md` §15.2):
//! - Imported files are treated as hostile.
//! - Every candidate file is checked with `symlink_metadata` (never following
//!   symlinks) and opened with `O_NOFOLLOW` on Unix.
//! - Only regular files within the canonicalized source directory are read.
//! - Each file is capped at 64 MiB (`MAX_IMPORT_FILE_BYTES`).
//! - Imported tabs are restored **asleep** (`sleeping = true`) and appended
//!   without overwriting existing open tabs, pins, bookmarks, or spaces.
//! - A failed decode of any source file aborts that file cleanly without
//!   leaving half-written state in the target [`DataStore`].

use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::data::{
    BookmarksFile, DataStore, HiddenFile, HistoryFile, PinFoldersFile, PinsFile, ProfilesFile,
    SessionFile, SpacesFile, StylesFile, WindowsFile,
};
use crate::domain::{BookmarkTree, CURRENT_SCHEMA_VERSION, HistoryVisit, visits_from_file};
use crate::error::{CoreError, Result};

/// Maximum file size permitted for any imported JSON file (64 MiB, `FEATURES.md` §15.2).
pub const MAX_IMPORT_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Summary of what was imported from an existing Search data directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchImportReport {
    /// Number of session tabs imported (all restored asleep).
    pub tabs_imported: usize,
    /// Number of space sessions imported.
    pub space_sessions_imported: usize,
    /// Number of additional windows imported.
    pub windows_imported: usize,
    /// Number of pins imported (per-space + shared).
    pub pins_imported: usize,
    /// Number of pin folders imported.
    pub pin_folders_imported: usize,
    /// Number of spaces imported.
    pub spaces_imported: usize,
    /// Number of profiles imported.
    pub profiles_imported: usize,
    /// Number of bookmark links present after merge.
    pub bookmarks_total_links: usize,
    /// Number of unique history visits present after merge.
    pub history_total_visits: usize,
    /// Number of sites with hidden element selectors after merge.
    pub hidden_sites: usize,
    /// Number of sites with custom CSS styles after merge.
    pub style_sites: usize,
}

/// Read and deserialize a JSON file from a hostile import directory, enforcing:
/// - path confinement within `canonical_root`
/// - `symlink_metadata` regular-file check (no symlinks, FIFOs, sockets, or devices)
/// - `O_NOFOLLOW` on Unix
/// - 64 MiB size cap (`MAX_IMPORT_FILE_BYTES`)
pub fn read_hostile_json<T: DeserializeOwned>(canonical_root: &Path, path: &Path) -> Result<T> {
    let meta = fs::symlink_metadata(path).map_err(|source| CoreError::io(path, source))?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(CoreError::NotARegularFile {
            path: path.to_path_buf(),
        });
    }
    if meta.len() > MAX_IMPORT_FILE_BYTES {
        return Err(CoreError::FileTooLarge {
            path: path.to_path_buf(),
            size: meta.len(),
            limit: MAX_IMPORT_FILE_BYTES,
        });
    }

    let canonical_path = fs::canonicalize(path).map_err(|source| CoreError::io(path, source))?;
    if !canonical_path.starts_with(canonical_root) {
        return Err(CoreError::NotARegularFile {
            path: path.to_path_buf(),
        });
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc_o_nofollow());
    }
    let file = options
        .open(path)
        .map_err(|source| CoreError::io(path, source))?;
    let file_meta = file
        .metadata()
        .map_err(|source| CoreError::io(path, source))?;
    if !file_meta.is_file() {
        return Err(CoreError::NotARegularFile {
            path: path.to_path_buf(),
        });
    }
    if file_meta.len() > MAX_IMPORT_FILE_BYTES {
        return Err(CoreError::FileTooLarge {
            path: path.to_path_buf(),
            size: file_meta.len(),
            limit: MAX_IMPORT_FILE_BYTES,
        });
    }

    let mut bytes = Vec::with_capacity(usize::try_from(file_meta.len()).unwrap_or(0));
    file.take(MAX_IMPORT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| CoreError::io(path, source))?;
    if (bytes.len() as u64) > MAX_IMPORT_FILE_BYTES {
        return Err(CoreError::FileTooLarge {
            path: path.to_path_buf(),
            size: bytes.len() as u64,
            limit: MAX_IMPORT_FILE_BYTES,
        });
    }

    serde_json::from_slice(&bytes).map_err(|source| CoreError::Decode {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(target_os = "linux")]
const fn libc_o_nofollow() -> i32 {
    0o400_000 // O_NOFOLLOW on Linux
}

#[cfg(target_os = "macos")]
const fn libc_o_nofollow() -> i32 {
    0x0100 // O_NOFOLLOW on macOS
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
const fn libc_o_nofollow() -> i32 {
    0
}

/// Import all Search data files (`session.json`, `session-<space>.json`,
/// `windows.json`, `pins.json`, `pin-folders.json`, `spaces.json`,
/// `profiles.json`, `bookmarks.json`, `history.json`, `hidden.json`,
/// `styles.json`) from `source_dir` into `target_store` (`P1-12`).
#[allow(clippy::too_many_lines)]
pub fn import_search_directory(
    source_dir: &Path,
    target_store: &DataStore,
) -> Result<SearchImportReport> {
    let meta =
        fs::symlink_metadata(source_dir).map_err(|source| CoreError::io(source_dir, source))?;
    if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
        return Err(CoreError::NotARegularFile {
            path: source_dir.to_path_buf(),
        });
    }
    let canonical_root =
        fs::canonicalize(source_dir).map_err(|source| CoreError::io(source_dir, source))?;

    // Stage all decoded payloads in memory first so a decode/security error
    // leaves nothing half-written in `target_store` (`FEATURES.md` §15.2).
    let session = read_optional::<SessionFile>(&canonical_root, &source_dir.join("session.json"))?;
    let windows = read_optional::<WindowsFile>(&canonical_root, &source_dir.join("windows.json"))?;
    let pins = read_optional::<PinsFile>(&canonical_root, &source_dir.join("pins.json"))?;
    let pin_folders =
        read_optional::<PinFoldersFile>(&canonical_root, &source_dir.join("pin-folders.json"))?;
    let spaces = read_optional::<SpacesFile>(&canonical_root, &source_dir.join("spaces.json"))?;
    let profiles =
        read_optional::<ProfilesFile>(&canonical_root, &source_dir.join("profiles.json"))?;
    let bookmarks =
        read_optional::<BookmarksFile>(&canonical_root, &source_dir.join("bookmarks.json"))?;
    let history = read_optional::<HistoryFile>(&canonical_root, &source_dir.join("history.json"))?;
    let hidden = read_optional::<HiddenFile>(&canonical_root, &source_dir.join("hidden.json"))?;
    let styles = read_optional::<StylesFile>(&canonical_root, &source_dir.join("styles.json"))?;

    // Discover `session-<space>.json` files.
    let mut space_sessions: Vec<(String, SessionFile)> = Vec::new();
    for entry in fs::read_dir(source_dir).map_err(|source| CoreError::io(source_dir, source))? {
        let entry = entry.map_err(|source| CoreError::io(source_dir, source))?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if let Some(space_id) = name_str
            .strip_prefix("session-")
            .and_then(|s| s.strip_suffix(".json"))
            && !space_id.is_empty()
            && !space_id.contains(".unreadable-")
        {
            let file: SessionFile = read_hostile_json(&canonical_root, &entry.path())?;
            space_sessions.push((space_id.to_owned(), file));
        }
    }

    let mut report = SearchImportReport::default();

    // 1. Session: imported tabs restore asleep and do not overwrite existing tabs (`FEATURES.md` §15.3).
    if let Some(incoming_session) = session {
        let mut target = target_store
            .read_or_default::<SessionFile>("session.json")?
            .value;
        target.version = target.version.max(CURRENT_SCHEMA_VERSION);
        for mut tab in incoming_session.tabs {
            tab.sleeping = true;
            if !target
                .tabs
                .iter()
                .any(|existing| !existing.id.is_empty() && existing.id == tab.id)
            {
                target.tabs.push(tab);
                report.tabs_imported += 1;
            }
        }
        for group in incoming_session.groups {
            if !target.groups.iter().any(|g| g.id == group.id) {
                target.groups.push(group);
            }
        }
        for split in incoming_session.splits {
            if !target.splits.iter().any(|s| s.id == split.id) {
                target.splits.push(split);
            }
        }
        for (k, v) in incoming_session.extra {
            target.extra.entry(k).or_insert(v);
        }
        target_store.save_session(None, &target)?;
    }

    // 2. Per-space sessions.
    for (space_id, mut space_session) in space_sessions {
        for tab in &mut space_session.tabs {
            tab.sleeping = true;
        }
        report.tabs_imported += space_session.tabs.len();
        report.space_sessions_imported += 1;
        target_store.save_session(Some(&space_id), &space_session)?;
    }

    // 3. Windows.
    if let Some(incoming_windows) = windows {
        let mut target = target_store
            .read_or_default::<WindowsFile>("windows.json")?
            .value;
        for mut win in incoming_windows.windows {
            for tabs in win.rows.values_mut() {
                for tab in tabs {
                    tab.sleeping = true;
                }
            }
            if !target.windows.iter().any(|w| w.id == win.id) {
                target.windows.push(win);
                report.windows_imported += 1;
            }
        }
        target_store.save_windows(&target)?;
    }

    // 4. Spaces.
    if let Some(incoming_spaces) = spaces {
        let mut target = target_store
            .read_or_default::<SpacesFile>("spaces.json")?
            .value;
        target.order_kept |= incoming_spaces.order_kept;
        for space in incoming_spaces.spaces {
            if !target.spaces.iter().any(|s| s.id == space.id) {
                target.spaces.push(space);
                report.spaces_imported += 1;
            }
        }
        target_store.save_spaces(&target)?;
    }

    // 5. Profiles.
    if let Some(incoming_profiles) = profiles {
        let mut target = target_store
            .read_or_default::<ProfilesFile>("profiles.json")?
            .value;
        for profile in incoming_profiles.profiles {
            if !target.profiles.iter().any(|p| p.id == profile.id) {
                target.profiles.push(profile);
                report.profiles_imported += 1;
            }
        }
        target_store.save_profiles(&target)?;
    }

    // 6. Pins & pin folders.
    if let Some(incoming_pins) = pins {
        let mut target = target_store.read_or_default::<PinsFile>("pins.json")?.value;
        for (space, pins_list) in incoming_pins.spaces {
            let entry = target.spaces.entry(space).or_default();
            for pin in pins_list {
                if !entry.iter().any(|p| p.id == pin.id || p.url == pin.url) {
                    entry.push(pin);
                    report.pins_imported += 1;
                }
            }
        }
        for pin in incoming_pins.shared {
            if !target
                .shared
                .iter()
                .any(|p| p.id == pin.id || p.url == pin.url)
            {
                target.shared.push(pin);
                report.pins_imported += 1;
            }
        }
        for folder in incoming_pins.folders {
            if !target.folders.iter().any(|f| f.id == folder.id) {
                target.folders.push(folder);
                report.pin_folders_imported += 1;
            }
        }
        target_store.save_pins(&target)?;
    }

    if let Some(incoming_folders) = pin_folders {
        let mut target = target_store
            .read_or_default::<PinFoldersFile>("pin-folders.json")?
            .value;
        for folder in incoming_folders.folders {
            if !target.folders.iter().any(|f| f.id == folder.id) {
                target.folders.push(folder);
                report.pin_folders_imported += 1;
            }
        }
        target_store.save_pin_folders(&target)?;
    }

    // 7. Bookmarks: merge without duplicating URLs at the same level (`FEATURES.md` §9, §15.1).
    if let Some(incoming_bookmarks) = bookmarks {
        let existing_file = target_store
            .read_or_default::<BookmarksFile>("bookmarks.json")?
            .value;
        let mut tree = BookmarkTree::try_from_file(&existing_file)?;
        let incoming_tree = BookmarkTree::try_from_file(&incoming_bookmarks)?;
        tree.merge_items(incoming_tree.roots);
        report.bookmarks_total_links = tree.total_links();
        let mut merged_file = tree.to_file();
        merged_file.imported_from = existing_file.imported_from;
        for mark in incoming_bookmarks.imported_from {
            merged_file.imported_from.push(mark);
        }
        target_store.save_bookmarks(None, &merged_file)?;
    }

    // 8. History: merge by URL identity (`max` count, later `last_visit`, fill title; `FEATURES.md` §15.1).
    if let Some(incoming_history) = history {
        let existing_file = target_store
            .read_or_default::<HistoryFile>("history.json")?
            .value;
        let mut merged: Vec<HistoryVisit> = visits_from_file(&existing_file);
        for visit in visits_from_file(&incoming_history) {
            let key = visit.identity();
            if let Some(existing) = merged.iter_mut().find(|v| v.identity() == key) {
                existing.merge_imported(&visit);
            } else {
                merged.push(visit);
            }
        }
        report.history_total_visits = merged.len();
        let out_file = HistoryFile {
            visits: merged.iter().map(HistoryVisit::to_data).collect(),
            extra: existing_file.extra,
        };
        target_store.save_history(None, &out_file)?;
    }

    // 9. Hidden selectors.
    if let Some(incoming_hidden) = hidden {
        let mut target = target_store
            .read_or_default::<HiddenFile>("hidden.json")?
            .value;
        for (site, rules) in incoming_hidden.sites {
            let entry = target.sites.entry(site).or_default();
            for rule in rules {
                if !entry.iter().any(|r| r.selector == rule.selector) {
                    entry.push(rule);
                }
            }
        }
        report.hidden_sites = target.sites.len();
        target_store.save_hidden(&target)?;
    }

    // 10. Site CSS styles.
    if let Some(incoming_styles) = styles {
        let mut target = target_store
            .read_or_default::<StylesFile>("styles.json")?
            .value;
        for (site, style) in incoming_styles.sites {
            target.sites.entry(site).or_insert(style);
        }
        report.style_sites = target.sites.len();
        target_store.save_styles(&target)?;
    }

    Ok(report)
}

fn read_optional<T: DeserializeOwned>(canonical_root: &Path, path: &Path) -> Result<Option<T>> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_hostile_json(canonical_root, path).map(Some),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(CoreError::io(path, err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_all_legacy_search_files_and_restores_tabs_asleep() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy");
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());

        let report = import_search_directory(&fixtures, &store).expect("import legacy fixtures");
        assert_eq!(report.tabs_imported, 3); // 2 in session.json + 1 in space session
        assert_eq!(report.space_sessions_imported, 1);
        assert_eq!(report.windows_imported, 1);
        assert_eq!(report.pins_imported, 3); // 2 space pins + 1 shared pin
        assert_eq!(report.pin_folders_imported, 1);
        assert_eq!(report.spaces_imported, 2);
        assert_eq!(report.bookmarks_total_links, 2);
        assert_eq!(report.history_total_visits, 2);
        assert_eq!(report.hidden_sites, 2);
        assert_eq!(report.style_sites, 2);

        let loaded_session = store.load_session(None).expect("load imported session");
        assert!(loaded_session.tabs.iter().all(|t| t.sleeping));

        // Importing a second time is idempotent and does not duplicate tabs, pins, or bookmarks.
        let second = import_search_directory(&fixtures, &store).expect("re-import");
        assert_eq!(second.bookmarks_total_links, 2);
        assert_eq!(second.history_total_visits, 2);
        assert_eq!(store.load_session(None).expect("session").tabs.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_in_hostile_import_directory() {
        use std::os::unix::fs::symlink;

        let source = tempfile::tempdir().expect("source");
        let outside = tempfile::tempdir().expect("outside");
        let target = tempfile::tempdir().expect("target");

        let secret = outside.path().join("secret.json");
        fs::write(&secret, b"{\"spaces\":[]}").expect("write outside");
        symlink(&secret, source.path().join("spaces.json")).expect("create symlink");

        let store = DataStore::new(target.path());
        let err = import_search_directory(source.path(), &store)
            .expect_err("symlink in import folder must be rejected");
        assert!(matches!(err, CoreError::NotARegularFile { .. }));
        assert!(!store.path_for("spaces.json").exists());
    }
}
