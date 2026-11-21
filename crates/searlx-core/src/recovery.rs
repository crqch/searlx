//! Signed update verification & staged rollout (`P2-18`) and process crash &
//! error recovery (`P2-19`) (`FEATURES.md` §2.1, §3.5, §20.1, `AGENTS.md` §9).

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::ids::TabId;
use crate::url::PageUrl;

// ============================================================================
// P2-18 — Update mechanism (`FEATURES.md` §20.1, `AGENTS.md` §9)
// ============================================================================

/// Minimum interval between automatic update checks (`50 minutes`, `FEATURES.md` §20.1).
pub const UPDATE_CHECK_GUARD_SECS: u64 = 50 * 60;

/// Signed appcast release descriptor (`FEATURES.md` §20.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppcastRelease {
    /// Feed identifier (must equal `"com.officecommun.search.appcast"`).
    pub feed_identifier: String,
    /// Signing team identifier (must equal `"7BYKA895MC"`).
    pub team_id: String,
    /// Semantic version string.
    pub version: String,
    /// Monotonically increasing build number timestamp.
    pub build_number: u64,
    /// Minimum OS version required by this release.
    pub minimum_system_version: String,
    /// HTTPS download URL for the bundle archive (must share host with the feed URL).
    pub archive_url: String,
    /// Lowercase hex SHA-256 digest of the bundle archive.
    pub archive_sha256: String,
    /// Detached signature token over `(feed_identifier, version, build_number, archive_sha256)`.
    pub signature: String,
    /// First paragraph of release notes.
    pub release_notes: String,
}

/// Expected appcast identifier (`FEATURES.md` §20.1).
pub const EXPECTED_APPCAST_ID: &str = "com.officecommun.search.appcast";
/// Expected Developer ID Team (`FEATURES.md` §20.1).
pub const EXPECTED_TEAM_ID: &str = "7BYKA895MC";

/// Outcome of staged bundle installation (`FEATURES.md` §20.1, P2-18).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedInstallOutcome {
    /// Active bundle path after atomic swap.
    pub active_bundle: PathBuf,
    /// Previous bundle path (`*.old`), kept for rollback and swept on next launch.
    pub old_bundle: PathBuf,
    /// Automatic relaunch is strictly forbidden (`false`, `FEATURES.md` §20.1).
    pub auto_relaunched: bool,
}

/// Verify an [`AppcastRelease`] against the feed origin, current build number,
/// signature validator, and archive SHA-256 (`FEATURES.md` §20.1, `AGENTS.md` §9).
pub fn verify_update_release<F>(
    feed_url: &PageUrl,
    release: &AppcastRelease,
    current_build_number: u64,
    downloaded_sha256_hex: &str,
    verify_signature: F,
) -> Result<bool>
where
    F: FnOnce(&AppcastRelease) -> bool,
{
    if feed_url.scheme() != "https" {
        return Err(CoreError::Invalid(
            "update feed must be served over HTTPS".into(),
        ));
    }
    if release.feed_identifier != EXPECTED_APPCAST_ID {
        return Err(CoreError::Invalid(format!(
            "unexpected appcast identifier: {:?}",
            release.feed_identifier
        )));
    }
    if release.team_id != EXPECTED_TEAM_ID {
        return Err(CoreError::Invalid(format!(
            "unexpected signing team: {:?}",
            release.team_id
        )));
    }
    let archive_url = PageUrl::parse(&release.archive_url)?;
    if archive_url.scheme() != "https" || archive_url.host() != feed_url.host() {
        return Err(CoreError::Invalid(
            "update archive must use HTTPS on the same host as the appcast feed".into(),
        ));
    }
    if !verify_signature(release) {
        return Err(CoreError::Invalid(
            "update signature verification failed".into(),
        ));
    }
    if !release
        .archive_sha256
        .eq_ignore_ascii_case(downloaded_sha256_hex.trim())
    {
        return Err(CoreError::Invalid(
            "downloaded update archive SHA-256 does not match signed appcast".into(),
        ));
    }
    Ok(release.build_number > current_build_number)
}

/// Stage and swap an verified update bundle into `current_bundle` (`FEATURES.md` §20.1, P2-18):
/// - Renames `current_bundle` to `<current_bundle>.old`.
/// - Renames `verified_new_bundle` into `current_bundle`.
/// - If the second rename fails, restores `<current_bundle>.old` back to `current_bundle`.
/// - **Never auto-relaunches** (`auto_relaunched = false`).
pub fn install_verified_bundle_swap(
    current_bundle: &Path,
    verified_new_bundle: &Path,
) -> Result<StagedInstallOutcome> {
    if !current_bundle.exists() {
        return Err(CoreError::Invalid(format!(
            "current bundle does not exist: {}",
            current_bundle.display()
        )));
    }
    if !verified_new_bundle.exists() {
        return Err(CoreError::Invalid(format!(
            "new bundle does not exist: {}",
            verified_new_bundle.display()
        )));
    }
    let old_bundle = current_bundle.with_extension("old");
    if old_bundle.exists() {
        remove_path_all(&old_bundle)?;
    }

    fs::rename(current_bundle, &old_bundle)
        .map_err(|source| CoreError::io(current_bundle, source))?;

    if let Err(swap_err) = fs::rename(verified_new_bundle, current_bundle) {
        // Roll back `.old` into place (`FEATURES.md` §20.1).
        let _ = fs::rename(&old_bundle, current_bundle);
        return Err(CoreError::io(current_bundle, swap_err));
    }

    Ok(StagedInstallOutcome {
        active_bundle: current_bundle.to_path_buf(),
        old_bundle,
        auto_relaunched: false,
    })
}

/// Sweep a left-over `<current_bundle>.old` bundle on launch or quit (`FEATURES.md` §20.1).
pub fn sweep_old_bundle(current_bundle: &Path) -> Result<bool> {
    let old_bundle = current_bundle.with_extension("old");
    if !old_bundle.exists() {
        return Ok(false);
    }
    remove_path_all(&old_bundle)?;
    Ok(true)
}

fn remove_path_all(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|source| CoreError::io(path, source))
    } else {
        fs::remove_file(path).map_err(|source| CoreError::io(path, source))
    }
}

// ============================================================================
// P2-19 — Crash and error recovery (`FEATURES.md` §2.1, §3.5, §3.8)
// ============================================================================

/// Maximum automatic crash reloads within a 60-second window before suppressing
/// reload loops and showing a manual Retry affordance (`2`, `FEATURES.md` §3.5, §3.8, P2-19).
pub const MAX_AUTO_RELOADS_PER_MINUTE: u32 = 2;

/// Action taken when a tab's web process terminates unexpectedly (`P2-19`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrashRecoveryAction {
    /// Automatically reload the page (`attempt` 1 or 2 within 60 seconds).
    AutoReload {
        /// Reload attempt number (`1..=2`).
        attempt: u32,
    },
    /// Suppress reload loop after 2 crashes and display the in-page Retry affordance.
    ShowManualRetry {
        /// Total crashes in the current window.
        crash_count: u32,
        /// Redacted message shown to the user.
        message: String,
    },
}

/// Tracks web-process crashes per tab and writes sanitized local `crash.log` lines (`P2-19`).
#[derive(Clone, Debug, Default)]
pub struct CrashRecoveryManager {
    /// Per-tab crash timestamps (in monotonic/epoch seconds).
    crashes_by_tab: BTreeMap<TabId, Vec<u64>>,
}

impl CrashRecoveryManager {
    /// Create an empty crash recovery tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a web process crash for `tab` at `now_secs` (`FEATURES.md` §3.5, §3.8, P2-19).
    ///
    /// First two crashes within 60 seconds can auto-reload or immediately show
    /// a retry after two failures (`Done when: a crashing page shows a retry after two failures`).
    pub fn on_process_crashed(
        &mut self,
        tab: TabId,
        now_secs: u64,
        reason: &str,
    ) -> CrashRecoveryAction {
        let stamps = self.crashes_by_tab.entry(tab).or_default();
        stamps.retain(|&t| now_secs.saturating_sub(t) <= 60);
        stamps.push(now_secs);
        let count = stamps.len() as u32;

        if count >= MAX_AUTO_RELOADS_PER_MINUTE {
            CrashRecoveryAction::ShowManualRetry {
                crash_count: count,
                message: format!("This page stopped working ({reason}). Try reloading."),
            }
        } else {
            CrashRecoveryAction::AutoReload { attempt: count }
        }
    }

    /// Reset the crash counter for `tab` after a successful manual reload or navigation.
    pub fn clear_tab(&mut self, tab: TabId) {
        self.crashes_by_tab.remove(&tab);
    }
}

/// Append one sanitized line to `crash.log` inside `data_dir` (`FEATURES.md` §2.1, §25, `AGENTS.md` §2, P2-19).
///
/// Never uploads anything over the network and strips newlines/URL query values.
pub fn append_crash_log(
    data_dir: &Path,
    timestamp_iso: &str,
    component: &str,
    url: Option<&PageUrl>,
    summary: &str,
) -> Result<PathBuf> {
    fs::create_dir_all(data_dir).map_err(|source| CoreError::io(data_dir, source))?;
    let path = data_dir.join("crash.log");
    let single_line_summary = summary.replace(['\r', '\n'], " ");
    let redacted_url = url.map_or_else(|| "-".to_owned(), PageUrl::redacted);
    let line = format!("{timestamp_iso} [{component}] url={redacted_url} {single_line_summary}\n");

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| CoreError::io(&path, source))?;
    file.write_all(line.as_bytes())
        .map_err(|source| CoreError::io(&path, source))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_bad_update_signature_and_swaps_verified_bundle_then_sweeps_old() {
        let feed_url = PageUrl::parse("https://updates.example.com/appcast.json").expect("feed");
        let release = AppcastRelease {
            feed_identifier: EXPECTED_APPCAST_ID.into(),
            team_id: EXPECTED_TEAM_ID.into(),
            version: "1.1.0".into(),
            build_number: 20261121,
            minimum_system_version: "14.0".into(),
            archive_url: "https://updates.example.com/Search-1.1.0.zip".into(),
            archive_sha256: "abcdef0123456789".into(),
            signature: "valid-sig".into(),
            release_notes: "Faster tab switching.".into(),
        };

        // Bad signature is refused (`P2-18`).
        assert!(
            verify_update_release(&feed_url, &release, 20261120, "abcdef0123456789", |_| false)
                .is_err()
        );
        // Bad SHA-256 is refused.
        assert!(
            verify_update_release(&feed_url, &release, 20261120, "wrong-sha", |_| true).is_err()
        );
        // Valid signature and hash succeeds.
        assert!(
            verify_update_release(&feed_url, &release, 20261120, "abcdef0123456789", |_| true)
                .expect("valid update")
        );

        // Staged bundle swap moves current to .old, puts new in place, does not auto-relaunch, and sweeps .old.
        let temp = tempfile::tempdir().expect("tempdir");
        let active = temp.path().join("searlx.bin");
        let staged = temp.path().join("searlx.new");
        fs::write(&active, b"v1.0").expect("write v1.0");
        fs::write(&staged, b"v1.1").expect("write v1.1");

        let outcome = install_verified_bundle_swap(&active, &staged).expect("swap bundle");
        assert!(!outcome.auto_relaunched);
        assert_eq!(fs::read(&active).expect("read active"), b"v1.1");
        assert_eq!(fs::read(&outcome.old_bundle).expect("read old"), b"v1.0");

        assert!(sweep_old_bundle(&active).expect("sweep old"));
        assert!(!outcome.old_bundle.exists());
    }

    #[test]
    fn crashing_page_shows_manual_retry_after_two_failures_and_logs_redacted_line() {
        let mut mgr = CrashRecoveryManager::new();
        let tab = TabId::new(1);

        let first = mgr.on_process_crashed(tab, 100, "SIGSEGV");
        assert_eq!(first, CrashRecoveryAction::AutoReload { attempt: 1 });

        let second = mgr.on_process_crashed(tab, 110, "SIGSEGV");
        assert!(matches!(
            second,
            CrashRecoveryAction::ShowManualRetry { crash_count: 2, .. }
        ));

        let temp = tempfile::tempdir().expect("tempdir");
        let url = PageUrl::parse("https://example.com/app?token=supersecret#frag").expect("url");
        let log_path = append_crash_log(
            temp.path(),
            "2026-11-21T10:00:00Z",
            "WebProcess",
            Some(&url),
            "process exited cleanly=false",
        )
        .expect("write crash.log");

        let contents = fs::read_to_string(log_path).expect("read crash.log");
        assert!(contents.contains("https://example.com/app?token=…#…"));
        assert!(!contents.contains("supersecret"));
    }
}
