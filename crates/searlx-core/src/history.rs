//! History recording, day grouping, search, 2,000/2,250 caps, recently closed,
//! archive, and full clear semantics (`FEATURES.md` §10, P2-03).
//!
//! Rules (`FEATURES.md` §10):
//! - Every committed web page visit (never internal `search://` / `about:` pages,
//!   never private tabs) is recorded by [`PageUrl::identity`] (fragments ignored).
//! - Caps: 2,000 places on disk, 2,000 in memory with 250 slack (2,250 max before
//!   pruning low-frecency old entries).
//! - Grouped by day: `"Today"`, `"Yesterday"`, then ISO date strings (`YYYY-MM-DD`).
//! - Search: single contiguous case-insensitive substring over address/title.
//! - **No date-range deletion.**
//! - Clearing history clears all (or one signed-out space's own), plus the
//!   archive, link-chooser choices, favicons, learned site searches, and
//!   download source addresses.

use serde::{Deserialize, Serialize};

use crate::data::{ArchiveFile, ArchivedTab, DataStore, HistoryFile};
use crate::domain::{HistoryVisit, visits_from_file};
use crate::error::Result;
use crate::url::PageUrl;

/// Maximum places stored on disk (`FEATURES.md` §10).
pub const HISTORY_DISK_CAP: usize = 2_000;
/// Maximum places held in memory before pruning (`2,000 + 250 slack`, `FEATURES.md` §10).
pub const HISTORY_MEMORY_CAP: usize = 2_250;
/// Maximum archived auto-closed tabs (`FEATURES.md` §10).
pub const ARCHIVE_CAP: usize = 500;
/// Maximum recently closed tabs/windows in the History menu (`FEATURES.md` §10).
pub const RECENTLY_CLOSED_CAP: usize = 10;

/// A day bucket on `search://history` (`FEATURES.md` §10).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryDayGroup {
    /// `"Today"`, `"Yesterday"`, or `"YYYY-MM-DD"`.
    pub label: String,
    /// Visits on that day, newest first.
    pub visits: Vec<HistoryVisit>,
}

/// Which history scope to clear (`FEATURES.md` §10).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClearHistoryScope {
    /// Clear all history for the profile.
    All,
    /// Clear only a specific signed-out space's history.
    SignedOutSpace(String),
}

/// Summary of everything wiped by [`HistoryManager::clear_history`] (`FEATURES.md` §10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClearHistoryOutcome {
    /// Number of history entries removed.
    pub visits_removed: usize,
    /// Whether the auto-closed tab archive was cleared.
    pub archive_cleared: bool,
    /// Whether link-chooser space memories were cleared.
    pub link_chooser_cleared: bool,
    /// Whether the favicon cache was cleared.
    pub favicons_cleared: bool,
    /// Whether learned OpenSearch site searches were cleared.
    pub learned_site_searches_cleared: bool,
    /// Whether source addresses were stripped from download history rows.
    pub download_sources_stripped: bool,
}

/// A recently closed tab or group kept in memory for `⇧⌘T` (`FEATURES.md` §10).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentlyClosedItem {
    /// URL of the closed tab.
    pub url: PageUrl,
    /// Title of the closed tab.
    pub title: String,
    /// Scroll offset `(x, y)` preserved for `⇧⌘T`.
    pub scroll: (f64, f64),
    /// Optional group ID if closed as part of a group.
    pub group: Option<String>,
}

/// Profile history, archive, and recently-closed controller (`P2-03`).
#[derive(Clone, Debug, Default)]
pub struct HistoryManager {
    visits: Vec<HistoryVisit>,
    archive: Vec<ArchivedTab>,
    recently_closed: Vec<RecentlyClosedItem>,
    link_chooser_sites: Vec<String>,
    cached_favicons: Vec<String>,
    learned_site_searches: Vec<String>,
}

impl HistoryManager {
    /// Create an empty history manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load history and archive from `store`.
    pub fn load_from(store: &DataStore, profile_suffix: Option<&str>) -> Result<Self> {
        let history_name = match profile_suffix {
            Some(s) if !s.is_empty() => format!("history-{s}.json"),
            _ => "history.json".to_owned(),
        };
        let archive_name = match profile_suffix {
            Some(s) if !s.is_empty() => format!("archive-{s}.json"),
            _ => "archive.json".to_owned(),
        };
        let history_file = store.read_or_default::<HistoryFile>(&history_name)?.value;
        let archive_file = store.read_or_default::<ArchiveFile>(&archive_name)?.value;
        let mut manager = Self {
            visits: visits_from_file(&history_file),
            archive: archive_file.tabs,
            ..Self::default()
        };
        manager.prune_to_disk_cap();
        Ok(manager)
    }

    /// Save history and archive to `store`, enforcing the 2,000 and 500 disk caps.
    pub fn save_to(&mut self, store: &DataStore, profile_suffix: Option<&str>) -> Result<()> {
        self.prune_to_disk_cap();
        let history_file = HistoryFile {
            visits: self.visits.iter().map(HistoryVisit::to_data).collect(),
            extra: serde_json::Map::new(),
        };
        store.save_history(profile_suffix, &history_file)?;
        let archive_file = ArchiveFile {
            tabs: self.archive.clone(),
            extra: serde_json::Map::new(),
        };
        store.save_archive(profile_suffix, &archive_file)
    }

    /// All recorded visits, newest `last_visit` first.
    #[must_use]
    pub fn visits(&self) -> &[HistoryVisit] {
        &self.visits
    }

    /// All archived auto-closed tabs, newest first.
    #[must_use]
    pub fn archive(&self) -> &[ArchivedTab] {
        &self.archive
    }

    /// Recently closed tabs/windows (up to 10, newest first).
    #[must_use]
    pub fn recently_closed(&self) -> &[RecentlyClosedItem] {
        &self.recently_closed
    }

    /// Record a committed page visit (`FEATURES.md` §10).
    ///
    /// Ignores private tabs, internal `search://` pages, and `about:` pages.
    pub fn record_visit(
        &mut self,
        url: PageUrl,
        title: impl Into<String>,
        timestamp_iso: impl Into<String>,
        private: bool,
        space: Option<String>,
    ) -> bool {
        if private || url.is_internal() || url.scheme() == "about" {
            return false;
        }
        let title = title.into();
        let stamp = timestamp_iso.into();
        let identity = url.identity();

        if let Some(existing) = self.visits.iter_mut().find(|v| v.identity() == identity) {
            existing.visit_count = existing.visit_count.saturating_add(1);
            existing.last_visit = stamp;
            if !title.is_empty() {
                existing.title = title;
            }
            if space.is_some() {
                existing.space = space;
            }
        } else {
            let mut visit = HistoryVisit::new(url, title, stamp);
            visit.space = space;
            self.visits.push(visit);
        }

        // Keep newest first.
        self.visits.sort_by(|a, b| b.last_visit.cmp(&a.last_visit));

        if self.visits.len() > HISTORY_MEMORY_CAP {
            self.prune_to_disk_cap();
        }
        true
    }

    /// Prune low-frecency old entries down to [`HISTORY_DISK_CAP`] (`2,000`, `FEATURES.md` §10).
    pub fn prune_to_disk_cap(&mut self) {
        if self.visits.len() <= HISTORY_DISK_CAP {
            return;
        }
        // Sort by visit_count descending, then last_visit descending, keep top 2,000,
        // then restore chronological newest-first order.
        self.visits.sort_by(|a, b| {
            b.visit_count
                .cmp(&a.visit_count)
                .then_with(|| b.last_visit.cmp(&a.last_visit))
        });
        self.visits.truncate(HISTORY_DISK_CAP);
        self.visits.sort_by(|a, b| b.last_visit.cmp(&a.last_visit));
    }

    /// Search visits by a single contiguous case-insensitive substring over address or title (`FEATURES.md` §10).
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&HistoryVisit> {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return self.visits.iter().collect();
        }
        self.visits
            .iter()
            .filter(|v| {
                v.url.as_str().to_ascii_lowercase().contains(&needle)
                    || v.url.display().to_ascii_lowercase().contains(&needle)
                    || v.title.to_ascii_lowercase().contains(&needle)
            })
            .collect()
    }

    /// Group visits by day (`"Today"`, `"Yesterday"`, then `"YYYY-MM-DD"`, `FEATURES.md` §10).
    #[must_use]
    pub fn grouped_by_day(&self, today_ymd: &str, yesterday_ymd: &str) -> Vec<HistoryDayGroup> {
        let mut groups: Vec<HistoryDayGroup> = Vec::new();
        for visit in &self.visits {
            let ymd = visit.last_visit.get(..10).unwrap_or("Unknown");
            let label = if ymd == today_ymd {
                "Today".to_owned()
            } else if ymd == yesterday_ymd {
                "Yesterday".to_owned()
            } else {
                ymd.to_owned()
            };
            if let Some(last) = groups.last_mut().filter(|g| g.label == label) {
                last.visits.push(visit.clone());
            } else {
                groups.push(HistoryDayGroup {
                    label,
                    visits: vec![visit.clone()],
                });
            }
        }
        groups
    }

    /// Remove a single history row by URL identity (`FEATURES.md` §10).
    pub fn remove_one(&mut self, url_or_identity: &str) -> bool {
        let target = PageUrl::parse(url_or_identity)
            .map_or_else(|_| url_or_identity.to_owned(), |u| u.identity());
        let before = self.visits.len();
        self.visits.retain(|v| v.identity() != target);
        self.visits.len() != before
    }

    /// Record an automatically closed tab in the archive (capped at 500, never private, `FEATURES.md` §10).
    pub fn archive_auto_closed_tab(
        &mut self,
        id: impl Into<String>,
        url: &PageUrl,
        title: impl Into<String>,
        closed_at_iso: impl Into<String>,
        private: bool,
        space: Option<String>,
    ) {
        if private || url.is_internal() {
            return;
        }
        self.archive.insert(
            0,
            ArchivedTab {
                id: id.into(),
                url: url.as_str().to_owned(),
                title: title.into(),
                closed_at: closed_at_iso.into(),
                space,
                extra: serde_json::Map::new(),
            },
        );
        if self.archive.len() > ARCHIVE_CAP {
            self.archive.truncate(ARCHIVE_CAP);
        }
    }

    /// Push a user-closed tab to the Recently Closed stack (capped at 10, never private).
    pub fn record_recently_closed(&mut self, item: RecentlyClosedItem, private: bool) {
        if private {
            return;
        }
        self.recently_closed.insert(0, item);
        if self.recently_closed.len() > RECENTLY_CLOSED_CAP {
            self.recently_closed.truncate(RECENTLY_CLOSED_CAP);
        }
    }

    /// Pop the most recently closed tab for `⇧⌘T`.
    pub fn pop_recently_closed(&mut self) -> Option<RecentlyClosedItem> {
        if self.recently_closed.is_empty() {
            None
        } else {
            Some(self.recently_closed.remove(0))
        }
    }

    /// Register auxiliary state (link-chooser site memories, favicons, learned site searches)
    /// that must be wiped when history is cleared (`FEATURES.md` §10).
    pub fn track_auxiliary_history_data(
        &mut self,
        link_chooser_site: Option<String>,
        favicon_key: Option<String>,
        learned_site_search: Option<String>,
    ) {
        if let Some(s) = link_chooser_site {
            self.link_chooser_sites.push(s);
        }
        if let Some(f) = favicon_key {
            self.cached_favicons.push(f);
        }
        if let Some(ls) = learned_site_search {
            self.learned_site_searches.push(ls);
        }
    }

    /// Clear history and all dependent history artifacts (`FEATURES.md` §10):
    /// - All visits (or only `SignedOutSpace(space)`)
    /// - Archived tabs
    /// - Recently closed tabs
    /// - Link-chooser site memories
    /// - Favicons
    /// - Learned site searches
    /// - Strips source addresses from `store`'s `downloads.json` and `search-sites.json`
    pub fn clear_history(
        &mut self,
        scope: &ClearHistoryScope,
        store: Option<&DataStore>,
    ) -> Result<ClearHistoryOutcome> {
        let before = self.visits.len();
        let clear_all = match scope {
            ClearHistoryScope::All => {
                self.visits.clear();
                self.archive.clear();
                self.recently_closed.clear();
                self.link_chooser_sites.clear();
                self.cached_favicons.clear();
                self.learned_site_searches.clear();
                true
            }
            ClearHistoryScope::SignedOutSpace(space_id) => {
                self.visits
                    .retain(|v| v.space.as_deref() != Some(space_id.as_str()));
                false
            }
        };
        let visits_removed = before - self.visits.len();

        if clear_all && let Some(store) = store {
            // Strip download source addresses but leave rows (`FEATURES.md` §10, §11).
            let mut downloads = store
                .read_or_default::<crate::data::DownloadsFile>("downloads.json")?
                .value;
            for row in &mut downloads.downloads {
                row.source_url = None;
            }
            store.save_downloads(&downloads)?;
            store.save_search_sites(&crate::data::SearchSitesFile::default())?;
            self.save_to(store, None)?;
        }

        Ok(ClearHistoryOutcome {
            visits_removed,
            archive_cleared: clear_all,
            link_chooser_cleared: clear_all,
            favicons_cleared: clear_all,
            learned_site_searches_cleared: clear_all,
            download_sources_stripped: clear_all,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{DownloadRecord, DownloadsFile, SearchSitesFile, SiteSearchRecord};

    #[test]
    fn groups_by_day_searches_caps_at_2000_and_clears_all_artifacts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());
        let mut mgr = HistoryManager::new();

        // Private and internal visits are ignored.
        assert!(!mgr.record_visit(
            PageUrl::parse("https://example.com/secret").expect("url"),
            "Secret",
            "2026-11-21T09:00:00Z",
            true,
            None,
        ));
        assert!(!mgr.record_visit(
            PageUrl::parse("search://settings").expect("url"),
            "Settings",
            "2026-11-21T09:00:00Z",
            false,
            None,
        ));

        // Record visits across Today, Yesterday, and an older date.
        mgr.record_visit(
            PageUrl::parse("https://example.com/today#frag").expect("url"),
            "Today Article",
            "2026-11-21T09:00:00Z",
            false,
            Some("space-1".into()),
        );
        mgr.record_visit(
            PageUrl::parse("https://example.com/today#other-frag").expect("url"),
            "Today Article Updated",
            "2026-11-21T09:05:00Z",
            false,
            Some("space-1".into()),
        );
        mgr.record_visit(
            PageUrl::parse("https://docs.example.org/yesterday").expect("url"),
            "Yesterday Guide",
            "2026-11-20T14:00:00Z",
            false,
            Some("space-2".into()),
        );
        mgr.record_visit(
            PageUrl::parse("https://rust-lang.org/older").expect("url"),
            "Rust Older",
            "2026-11-15T10:00:00Z",
            false,
            Some("space-1".into()),
        );

        assert_eq!(mgr.visits().len(), 3);
        assert_eq!(mgr.visits()[0].visit_count, 2);

        let groups = mgr.grouped_by_day("2026-11-21", "2026-11-20");
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].label, "Today");
        assert_eq!(groups[1].label, "Yesterday");
        assert_eq!(groups[2].label, "2026-11-15");

        assert_eq!(mgr.search("yesterday").len(), 1);
        assert!(mgr.remove_one("https://rust-lang.org/older"));
        assert_eq!(mgr.visits().len(), 2);

        // Seed downloads with source_url and learned search sites in `store`.
        store
            .save_downloads(&DownloadsFile {
                downloads: vec![DownloadRecord {
                    id: "d1".into(),
                    file_name: "archive.tar.gz".into(),
                    destination: "/tmp/archive.tar.gz".into(),
                    source_url: Some("https://example.com/archive.tar.gz".into()),
                    bytes_received: 1024,
                    total_bytes: Some(1024),
                    state: "completed".into(),
                    finished_at: None,
                    extra: serde_json::Map::new(),
                }],
                extra: serde_json::Map::new(),
            })
            .expect("save downloads");
        store
            .save_search_sites(&SearchSitesFile {
                sites: vec![SiteSearchRecord {
                    host: "crates.io".into(),
                    name: "Crates".into(),
                    keyword: Some("cr".into()),
                    template: "https://crates.io/search?q=%s".into(),
                    extra: serde_json::Map::new(),
                }],
                extra: serde_json::Map::new(),
            })
            .expect("save search sites");

        mgr.archive_auto_closed_tab(
            "a1",
            &PageUrl::parse("https://example.com/old").expect("url"),
            "Old",
            "2026-11-01T00:00:00Z",
            false,
            None,
        );
        mgr.track_auxiliary_history_data(
            Some("example.com".into()),
            Some("icon-1".into()),
            Some("crates.io".into()),
        );

        let outcome = mgr
            .clear_history(&ClearHistoryScope::All, Some(&store))
            .expect("clear history");
        assert_eq!(outcome.visits_removed, 2);
        assert!(outcome.archive_cleared);
        assert!(outcome.favicons_cleared);
        assert!(outcome.learned_site_searches_cleared);
        assert!(outcome.download_sources_stripped);
        assert!(mgr.visits().is_empty());
        assert!(mgr.archive().is_empty());

        // Download row remains, but its source_url was stripped (`FEATURES.md` §10, §11).
        let post_downloads = store.load_downloads().expect("load downloads");
        assert_eq!(post_downloads.downloads.len(), 1);
        assert!(post_downloads.downloads[0].source_url.is_none());
        assert!(
            store
                .load_search_sites()
                .expect("load search sites")
                .sites
                .is_empty()
        );
    }
}
