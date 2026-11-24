//! Downloads manager, pause/resume/retry/cancel, POST non-pausable rules,
//! duplicate naming, path traversal sanitization, and private-download privacy
//! (`FEATURES.md` §11, P2-05).
//!
//! Rules (`FEATURES.md` §11):
//! - Pause/resume/retry only for replayable `GET`/`HEAD` requests with no
//!   request body. A `POST` or body-carrying download refuses to pause or retry
//!   with a clear message explaining why (`"POST downloads cannot be paused"`).
//! - Duplicate file names receive `" 2"`, `" 3"`, etc. before the extension;
//!   a retry renumbers afresh.
//! - Suggested file names keep only their last path component; extension
//!   subfolders are sanitized against `..` traversal.
//! - Private-tab downloads still write the file to disk and count toward the
//!   global progress ring, but are **never listed** in the Downloads UI or
//!   `downloads.json`, and never retain a source URL.
//! - **No Notification Center notifications** are ever sent for downloads.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::data::{DataStore, DownloadRecord, DownloadsFile};
use crate::error::{CoreError, Result};
use crate::url::PageUrl;

/// Maximum remembered downloads persisted in `downloads.json` (`FEATURES.md` §11).
pub const DOWNLOADS_HISTORY_CAP: usize = 50;

/// HTTP method used to initiate a download (`FEATURES.md` §11).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    /// `GET` (replayable when bodiless).
    #[default]
    Get,
    /// `HEAD` (replayable when bodiless).
    Head,
    /// `POST` (cannot pause or retry).
    Post,
    /// Any other method.
    Other(String),
}

impl HttpMethod {
    /// Whether a download with this method and `has_request_body` can be paused or retried (`FEATURES.md` §11).
    #[must_use]
    pub const fn is_replayable(&self, has_request_body: bool) -> bool {
        !has_request_body && matches!(self, Self::Get | Self::Head)
    }
}

/// Lifecycle state of a download row (`FEATURES.md` §11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    /// Actively transferring bytes.
    InProgress,
    /// Paused with in-memory resume data.
    Paused,
    /// Finished and written to `destination`.
    Completed,
    /// Failed with a user-visible reason.
    Failed {
        /// Failure explanation.
        reason: String,
    },
    /// Cancelled by the user.
    Cancelled,
}

/// One active or completed download tracked by [`DownloadsManager`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadEntry {
    /// Download identifier.
    pub id: String,
    /// Source URL (`None` for private downloads or after clearing History).
    pub source_url: Option<PageUrl>,
    /// HTTP method.
    pub method: HttpMethod,
    /// Whether the initiating request carried a body or body stream.
    pub has_request_body: bool,
    /// Resolved file name (after sanitization and duplicate numbering).
    pub file_name: String,
    /// Full destination path on disk.
    pub destination: PathBuf,
    /// Bytes received so far.
    pub bytes_received: u64,
    /// Total expected bytes (`None` when indeterminate).
    pub total_bytes: Option<u64>,
    /// Current state.
    pub state: DownloadState,
    /// Whether this download originated from a private tab (unlisted!).
    pub private: bool,
    /// In-memory resume token/offset (lost on quit, `FEATURES.md` §11).
    pub resume_offset: Option<u64>,
}

impl DownloadEntry {
    /// Whether this download can be paused right now (`FEATURES.md` §11).
    #[must_use]
    pub const fn can_pause(&self) -> bool {
        matches!(self.state, DownloadState::InProgress)
            && self.method.is_replayable(self.has_request_body)
    }

    /// Whether this download can be retried (`FEATURES.md` §11).
    #[must_use]
    pub const fn can_retry(&self) -> bool {
        matches!(
            self.state,
            DownloadState::Failed { .. } | DownloadState::Cancelled
        ) && self.method.is_replayable(self.has_request_body)
    }

    /// Completion fraction `0.0..=1.0`, or `None` when total size is unknown.
    #[must_use]
    pub fn progress_fraction(&self) -> Option<f64> {
        let total = self.total_bytes?;
        if total == 0 {
            return Some(1.0);
        }
        Some((self.bytes_received as f64 / total as f64).clamp(0.0, 1.0))
    }
}

/// Sanitize a page-suggested filename, keeping only its final path component
/// and replacing control/path characters (`FEATURES.md` §11).
#[must_use]
pub fn sanitize_suggested_filename(suggested: &str) -> String {
    let trimmed = suggested.trim();
    let last_component = trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty() && *part != "." && *part != "..")
        .unwrap_or("download");
    let cleaned: String = last_component
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | '\0'))
        .collect();
    let cleaned = cleaned.trim_matches('.');
    if cleaned.is_empty() {
        "download".to_owned()
    } else {
        cleaned.to_owned()
    }
}

/// Sanitize an extension-requested relative subfolder, rejecting any `..` or root traversal (`FEATURES.md` §11).
pub fn sanitize_extension_subfolder(subfolder: &str) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for part in subfolder.split(['/', '\\']) {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return Err(CoreError::Invalid(format!(
                "unsafe download subfolder traversal: {subfolder:?}"
            )));
        }
        out.push(part);
    }
    Ok(out)
}

/// Number duplicate file names as `"name 2.ext"`, `"name 3.ext"`, etc. (`FEATURES.md` §11).
#[must_use]
pub fn number_duplicate_filename(desired: &str, existing_names: &[&str]) -> String {
    let clean = sanitize_suggested_filename(desired);
    if !existing_names.contains(&clean.as_str()) {
        return clean;
    }
    let (stem, ext) = match clean.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s, Some(e)),
        _ => (clean.as_str(), None),
    };
    let mut counter = 2_u32;
    loop {
        let candidate = match ext {
            Some(e) => format!("{stem} {counter}.{e}"),
            None => format!("{stem} {counter}"),
        };
        if !existing_names.contains(&candidate.as_str()) {
            return candidate;
        }
        counter = counter.saturating_add(1);
    }
}

/// Downloads controller (`P2-05`).
#[derive(Clone, Debug, Default)]
pub struct DownloadsManager {
    entries: Vec<DownloadEntry>,
    next_id: u64,
}

impl DownloadsManager {
    /// Create an empty downloads manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Downloads never post to the OS Notification Center (`FEATURES.md` §11).
    #[must_use]
    pub const fn uses_notification_center(&self) -> bool {
        false
    }

    /// Start a new download (`FEATURES.md` §11).
    #[allow(clippy::too_many_arguments)]
    pub fn start_download(
        &mut self,
        url: PageUrl,
        method: HttpMethod,
        has_request_body: bool,
        suggested_name: &str,
        destination_folder: &Path,
        total_bytes: Option<u64>,
        private: bool,
    ) -> String {
        self.next_id = self.next_id.saturating_add(1);
        let id = format!("dl-{}", self.next_id);
        let existing: Vec<&str> = self
            .entries
            .iter()
            .filter(|e| !matches!(e.state, DownloadState::Cancelled))
            .map(|e| e.file_name.as_str())
            .collect();
        let file_name = number_duplicate_filename(suggested_name, &existing);
        let destination = destination_folder.join(&file_name);

        self.entries.insert(
            0,
            DownloadEntry {
                id: id.clone(),
                source_url: (!private).then_some(url),
                method,
                has_request_body,
                file_name,
                destination,
                bytes_received: 0,
                total_bytes,
                state: DownloadState::InProgress,
                private,
                resume_offset: None,
            },
        );
        id
    }

    /// Advance received byte count on an in-progress download.
    pub fn record_progress(&mut self, id: &str, bytes_received: u64) -> Result<()> {
        let entry = self.entry_mut(id)?;
        if !matches!(entry.state, DownloadState::InProgress) {
            return Err(CoreError::Invalid(format!(
                "download {id} is not in progress"
            )));
        }
        entry.bytes_received = bytes_received;
        if let Some(total) = entry.total_bytes
            && bytes_received >= total
        {
            entry.state = DownloadState::Completed;
            entry.resume_offset = None;
        }
        Ok(())
    }

    /// Pause an in-progress download (`FEATURES.md` §11).
    ///
    /// Refuses `POST` or body-carrying requests with a clear explanation.
    pub fn pause(&mut self, id: &str) -> Result<()> {
        let entry = self.entry_mut(id)?;
        if !entry.method.is_replayable(entry.has_request_body) {
            return Err(CoreError::Invalid(
                "POST or form downloads cannot be paused".into(),
            ));
        }
        if !matches!(entry.state, DownloadState::InProgress) {
            return Err(CoreError::Invalid(format!(
                "download {id} is not in progress"
            )));
        }
        entry.resume_offset = Some(entry.bytes_received);
        entry.state = DownloadState::Paused;
        Ok(())
    }

    /// Resume a paused download from its in-memory offset (`FEATURES.md` §11).
    pub fn resume(&mut self, id: &str) -> Result<u64> {
        let entry = self.entry_mut(id)?;
        if !matches!(entry.state, DownloadState::Paused) {
            return Err(CoreError::Invalid(format!("download {id} is not paused")));
        }
        let offset = entry
            .resume_offset
            .ok_or_else(|| CoreError::Invalid("paused download lost its resume data".into()))?;
        entry.state = DownloadState::InProgress;
        Ok(offset)
    }

    /// Fail an active download with a reason.
    pub fn fail(&mut self, id: &str, reason: impl Into<String>) -> Result<()> {
        let entry = self.entry_mut(id)?;
        entry.state = DownloadState::Failed {
            reason: reason.into(),
        };
        entry.resume_offset = None;
        Ok(())
    }

    /// Cancel an active or paused download.
    pub fn cancel(&mut self, id: &str) -> Result<()> {
        let entry = self.entry_mut(id)?;
        entry.state = DownloadState::Cancelled;
        entry.resume_offset = None;
        Ok(())
    }

    /// Retry a failed or cancelled replayable download from the start,
    /// renumbering its file name afresh (`FEATURES.md` §11).
    pub fn retry(&mut self, id: &str) -> Result<()> {
        let (desired, folder) = {
            let entry = self.entry_mut(id)?;
            if !entry.method.is_replayable(entry.has_request_body) {
                return Err(CoreError::Invalid(
                    "POST or form downloads cannot be retried".into(),
                ));
            }
            if !matches!(
                entry.state,
                DownloadState::Failed { .. } | DownloadState::Cancelled
            ) {
                return Err(CoreError::Invalid(
                    "only failed or cancelled downloads can be retried".into(),
                ));
            }
            (
                entry.file_name.clone(),
                entry
                    .destination
                    .parent()
                    .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
            )
        };

        let existing: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.id != id && !matches!(e.state, DownloadState::Cancelled))
            .map(|e| e.file_name.clone())
            .collect();
        let existing_refs: Vec<&str> = existing.iter().map(String::as_str).collect();
        let new_name = number_duplicate_filename(&desired, &existing_refs);

        let entry = self.entry_mut(id)?;
        entry.file_name = new_name.clone();
        entry.destination = folder.join(new_name);
        entry.bytes_received = 0;
        entry.resume_offset = None;
        entry.state = DownloadState::InProgress;
        Ok(())
    }

    /// Rows visible in the Downloads tab/popover/panel (`FEATURES.md` §11):
    /// private-tab downloads are **never listed**.
    #[must_use]
    pub fn listed_entries(&self) -> Vec<&DownloadEntry> {
        self.entries.iter().filter(|e| !e.private).collect()
    }

    /// Strip `source_url` from all in-memory download rows while leaving rows intact (`FEATURES.md` §10, §11).
    pub fn strip_source_urls(&mut self) {
        for entry in &mut self.entries {
            entry.source_url = None;
        }
    }

    /// Count of currently active (in-progress or paused) downloads, including private tabs.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.state, DownloadState::InProgress | DownloadState::Paused))
            .count()
    }

    /// Aggregate progress `(received_bytes, total_bytes)` across all active
    /// downloads, **including** private downloads (`FEATURES.md` §11: "it still
    /// counts for the global progress ring").
    #[must_use]
    pub fn global_active_progress(&self) -> (u64, u64) {
        self.entries
            .iter()
            .filter(|e| matches!(e.state, DownloadState::InProgress))
            .fold((0_u64, 0_u64), |(rec, tot), e| {
                (
                    rec.saturating_add(e.bytes_received),
                    tot.saturating_add(e.total_bytes.unwrap_or(0)),
                )
            })
    }

    /// Persist non-private completed/failed rows to `downloads.json` (capped at 50;
    /// paused downloads are in-memory only and not persisted as resumable across quit, `FEATURES.md` §11).
    pub fn save_to(&self, store: &DataStore) -> Result<()> {
        let records: Vec<DownloadRecord> = self
            .entries
            .iter()
            .filter(|e| !e.private && !matches!(e.state, DownloadState::Paused))
            .take(DOWNLOADS_HISTORY_CAP)
            .map(|e| DownloadRecord {
                id: e.id.clone(),
                file_name: e.file_name.clone(),
                destination: e.destination.display().to_string(),
                source_url: e.source_url.as_ref().map(|u| u.as_str().to_owned()),
                bytes_received: e.bytes_received,
                total_bytes: e.total_bytes,
                state: match &e.state {
                    DownloadState::InProgress => "in_progress".into(),
                    DownloadState::Paused => "paused".into(),
                    DownloadState::Completed => "completed".into(),
                    DownloadState::Failed { reason } => format!("failed:{reason}"),
                    DownloadState::Cancelled => "cancelled".into(),
                },
                finished_at: None,
                extra: serde_json::Map::new(),
            })
            .collect();
        store.save_downloads(&DownloadsFile {
            downloads: records,
            extra: serde_json::Map::new(),
        })
    }

    fn entry_mut(&mut self, id: &str) -> Result<&mut DownloadEntry> {
        self.entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| CoreError::Invalid(format!("no such download: {id}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_gb_get_download_pauses_and_resumes_while_post_refuses() {
        let mut mgr = DownloadsManager::new();
        assert!(!mgr.uses_notification_center());
        let dir = Path::new("/tmp/downloads");
        let one_gb: u64 = 1_073_741_824;

        let get_id = mgr.start_download(
            PageUrl::parse("https://example.com/linux.iso").expect("url"),
            HttpMethod::Get,
            false,
            "linux.iso",
            dir,
            Some(one_gb),
            false,
        );
        mgr.record_progress(&get_id, 512 * 1024 * 1024)
            .expect("512 MiB progress");
        mgr.pause(&get_id).expect("pause 1 GB GET");
        let offset = mgr.resume(&get_id).expect("resume 1 GB GET");
        assert_eq!(offset, 512 * 1024 * 1024);
        mgr.record_progress(&get_id, one_gb).expect("finish 1 GB");
        assert_eq!(mgr.listed_entries()[0].state, DownloadState::Completed);

        // POST download refuses to pause with a clear message (`P2-05`).
        let post_id = mgr.start_download(
            PageUrl::parse("https://example.com/export").expect("url"),
            HttpMethod::Post,
            true,
            "report.csv",
            dir,
            Some(4096),
            false,
        );
        let err = mgr
            .pause(&post_id)
            .expect_err("POST download must refuse to pause");
        assert!(
            err.to_string().contains("POST"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn private_download_is_unlisted_but_counts_in_progress_and_duplicates_number() {
        let mut mgr = DownloadsManager::new();
        let dir = Path::new("/tmp/downloads");

        let d1 = mgr.start_download(
            PageUrl::parse("https://example.com/file.pdf").expect("url"),
            HttpMethod::Get,
            false,
            "../../etc/file.pdf",
            dir,
            Some(1000),
            false,
        );
        assert_eq!(mgr.listed_entries()[0].file_name, "file.pdf");

        let _d2 = mgr.start_download(
            PageUrl::parse("https://example.com/file.pdf").expect("url"),
            HttpMethod::Get,
            false,
            "file.pdf",
            dir,
            Some(1000),
            false,
        );
        assert_eq!(mgr.listed_entries()[0].file_name, "file 2.pdf");

        // Private download is never listed, has no source_url, but counts in global_active_progress.
        let priv_id = mgr.start_download(
            PageUrl::parse("https://example.com/secret.zip").expect("url"),
            HttpMethod::Get,
            false,
            "secret.zip",
            dir,
            Some(2000),
            true,
        );
        mgr.record_progress(&d1, 400).expect("d1 progress");
        mgr.record_progress(&priv_id, 600).expect("priv progress");

        assert_eq!(mgr.listed_entries().len(), 2);
        assert!(
            mgr.listed_entries()
                .iter()
                .all(|e| e.file_name != "secret.zip")
        );
        assert_eq!(mgr.global_active_progress(), (1000, 4000));

        assert!(sanitize_extension_subfolder("../escape").is_err());
        assert_eq!(
            sanitize_extension_subfolder("sub/folder").expect("subfolder"),
            PathBuf::from("sub/folder")
        );
    }
}
