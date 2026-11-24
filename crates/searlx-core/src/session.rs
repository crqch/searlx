//! Session persistence and restoration (`FEATURES.md` §2.1, §3.2, §3.5, §25, P1-08).
//!
//! Rules:
//! - Tabs restore **asleep** (`sleeping = true`, `should_load_now = false`)
//!   except for the active tab, which wakes immediately (`sleeping = false`,
//!   `should_load_now = true`). A 30-tab session therefore opens without
//!   spawning 30 web processes at launch.
//! - Private tabs (`private = true`) are **never** written to `session.json`
//!   or `windows.json` (`AGENTS.md` §9, `FEATURES.md` §3.2).
//! - Fresh-start mode (`start.fresh = true`) keeps pinned tabs (put down at
//!   their home address) and replaces loose tabs with a single blank tab.
//! - Corrupt `session.json` or `windows.json` files are quarantined intact as
//!   `*.unreadable-<UTC>.json` and the browser starts with a clean default
//!   window (`1180×780`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::data::{
    DataStore, Frame, ScrollPosition, SessionFile, SessionTab, Split, TabGroup, WindowRecord,
    WindowsFile,
};
use crate::domain::CURRENT_SCHEMA_VERSION;
use crate::error::Result;
use crate::ids::{ProfileId, SpaceId, TabId, TabIdAllocator, WindowId};
use crate::url::PageUrl;

/// Default window width in logical pixels (`FEATURES.md` §2.1).
pub const DEFAULT_WINDOW_WIDTH: f64 = 1180.0;
/// Default window height in logical pixels (`FEATURES.md` §2.1).
pub const DEFAULT_WINDOW_HEIGHT: f64 = 780.0;
/// Minimum window width in logical pixels (`FEATURES.md` §2.1).
pub const MIN_WINDOW_WIDTH: f64 = 640.0;
/// Minimum window height in logical pixels (`FEATURES.md` §2.1).
pub const MIN_WINDOW_HEIGHT: f64 = 420.0;

/// Window geometry clamped to Search's minimum size (`640×420`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowFrame {
    /// Left screen coordinate.
    pub x: f64,
    /// Top screen coordinate.
    pub y: f64,
    /// Width in logical pixels (`>= 640.0`).
    pub width: f64,
    /// Height in logical pixels (`>= 420.0`).
    pub height: f64,
}

impl Default for WindowFrame {
    fn default() -> Self {
        Self {
            x: 80.0,
            y: 64.0,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
        }
    }
}

impl WindowFrame {
    /// Clamp width and height to [`MIN_WINDOW_WIDTH`] × [`MIN_WINDOW_HEIGHT`].
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            x: if self.x.is_finite() { self.x } else { 80.0 },
            y: if self.y.is_finite() { self.y } else { 64.0 },
            width: if self.width.is_finite() {
                self.width.max(MIN_WINDOW_WIDTH)
            } else {
                DEFAULT_WINDOW_WIDTH
            },
            height: if self.height.is_finite() {
                self.height.max(MIN_WINDOW_HEIGHT)
            } else {
                DEFAULT_WINDOW_HEIGHT
            },
        }
    }

    /// Convert from a raw [`Frame`].
    #[must_use]
    pub fn from_data(frame: &Frame) -> Self {
        Self {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        }
        .clamped()
    }

    /// Convert to a raw [`Frame`].
    #[must_use]
    pub fn to_data(self) -> Frame {
        let clamped = self.clamped();
        Frame {
            x: clamped.x,
            y: clamped.y,
            width: clamped.width,
            height: clamped.height,
            extra: serde_json::Map::new(),
        }
    }
}

/// How the session should be restored at launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestoreMode {
    /// Restore all saved tabs asleep, waking only the active tab.
    #[default]
    Normal,
    /// `start.fresh`: keep pinned tabs asleep at home, drop loose tabs, and open a fresh blank tab.
    FreshStart,
}

/// A live or restored tab entry managed by the session layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RestoredTab {
    /// Fresh runtime handle assigned for this process run.
    pub id: TabId,
    /// Persisted tab key from the session file.
    pub key: String,
    /// Validated page URL.
    pub url: PageUrl,
    /// Tab title.
    pub title: String,
    /// Whether this tab is pinned.
    pub pinned: bool,
    /// Persisted mute state.
    pub muted: bool,
    /// Whether the tab is asleep (holds no web view yet).
    pub sleeping: bool,
    /// Whether the engine should create and navigate a web view immediately at launch.
    pub should_load_now: bool,
    /// Whether this is a private tab (never persisted to disk).
    pub private: bool,
    /// Optional tab group identifier.
    pub group: Option<String>,
    /// Optional space UUID string.
    pub space: Option<String>,
    /// Optional profile UUID string.
    pub profile: Option<String>,
    /// Saved scroll offset `(x, y)`.
    pub scroll: Option<(f64, f64)>,
}

impl RestoredTab {
    /// Create a fresh blank tab (`search://`) that is awake.
    pub fn blank(allocator: &mut TabIdAllocator) -> Result<Self> {
        let id = allocator.allocate();
        Ok(Self {
            id,
            key: format!("tab-{}", id.get()),
            url: PageUrl::parse("search://")?,
            title: "New Tab".into(),
            pinned: false,
            muted: false,
            sleeping: false,
            should_load_now: true,
            private: false,
            group: None,
            space: None,
            profile: None,
            scroll: None,
        })
    }
}

/// A restored browser window and its tabs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RestoredWindow {
    /// Window identifier.
    pub id: WindowId,
    /// Clamped window frame.
    pub frame: WindowFrame,
    /// Current space key, if any.
    pub space: Option<String>,
    /// Active tab index in `tabs`.
    pub active: usize,
    /// Restored tabs in row order.
    pub tabs: Vec<RestoredTab>,
    /// Restored tab groups.
    pub groups: Vec<TabGroup>,
    /// Restored split views.
    pub splits: Vec<Split>,
    /// Whether the sidebar is folded (`⌘S`).
    pub folded: bool,
    /// Whether this window is part of a merged profile-in-one-window set.
    pub merged: bool,
    /// Whether this window is currently tucked behind a sibling profile window.
    pub tucked: bool,
}

/// Complete launch restoration result across the primary session and any additional windows.
#[derive(Clone, Debug, PartialEq)]
pub struct RestoredSession {
    /// Primary window plus any additional windows from `windows.json`.
    pub windows: Vec<RestoredWindow>,
    /// Any corrupt files that were quarantined during restore.
    pub quarantined_files: Vec<PathBuf>,
}

/// Restore the primary session and additional windows from `store`.
pub fn restore_session(
    store: &DataStore,
    mode: RestoreMode,
    allocator: &mut TabIdAllocator,
) -> Result<RestoredSession> {
    let mut quarantined_files = Vec::new();

    let session_outcome = store.read_or_default::<SessionFile>("session.json")?;
    if let Some(q) = session_outcome.quarantined_to {
        quarantined_files.push(q);
    }

    let windows_outcome = store.read_or_default::<WindowsFile>("windows.json")?;
    if let Some(q) = windows_outcome.quarantined_to {
        quarantined_files.push(q);
    }

    let primary = build_restored_window_from_session(
        WindowId::new(),
        WindowFrame::default(),
        &session_outcome.value,
        mode,
        allocator,
    )?;

    let mut windows = vec![primary];
    if mode == RestoreMode::Normal {
        for record in &windows_outcome.value.windows {
            windows.push(build_restored_window_from_record(record, allocator)?);
        }
    }

    Ok(RestoredSession {
        windows,
        quarantined_files,
    })
}

/// Restore a single named space's `session-<space_id>.json` from `store` (`FEATURES.md` §1, §25).
pub fn restore_space_session(
    store: &DataStore,
    space_id: &str,
    mode: RestoreMode,
    allocator: &mut TabIdAllocator,
) -> Result<(RestoredWindow, Option<PathBuf>)> {
    let filename = format!("session-{space_id}.json");
    let outcome = store.read_or_default::<SessionFile>(&filename)?;
    let mut win = build_restored_window_from_session(
        WindowId::new(),
        WindowFrame::default(),
        &outcome.value,
        mode,
        allocator,
    )?;
    win.space = Some(space_id.to_owned());
    Ok((win, outcome.quarantined_to))
}

fn build_restored_window_from_session(
    window_id: WindowId,
    frame: WindowFrame,
    file: &SessionFile,
    mode: RestoreMode,
    allocator: &mut TabIdAllocator,
) -> Result<RestoredWindow> {
    let mut tabs = Vec::new();
    for raw in &file.tabs {
        if mode == RestoreMode::FreshStart && !raw.pinned {
            continue;
        }
        if let Some(tab) = restore_one_tab(raw, allocator) {
            tabs.push(tab);
        }
    }

    if tabs.is_empty() || (mode == RestoreMode::FreshStart && tabs.iter().all(|t| t.pinned)) {
        tabs.push(RestoredTab::blank(allocator)?);
    }

    let active = if mode == RestoreMode::FreshStart {
        tabs.len() - 1
    } else {
        file.active.min(tabs.len() - 1)
    };

    let (groups, splits) = if mode == RestoreMode::FreshStart {
        (Vec::new(), Vec::new())
    } else {
        (
            file.groups
                .iter()
                .filter(|g| !g.tab_ids.is_empty())
                .cloned()
                .collect(),
            validate_splits(&file.splits, &tabs),
        )
    };

    apply_sleep_policy_with_splits(&mut tabs, active, &splits);

    Ok(RestoredWindow {
        id: window_id,
        frame: frame.clamped(),
        space: None,
        active,
        tabs,
        groups,
        splits,
        folded: false,
        merged: false,
        tucked: false,
    })
}

fn build_restored_window_from_record(
    record: &WindowRecord,
    allocator: &mut TabIdAllocator,
) -> Result<RestoredWindow> {
    let id = WindowId::parse(&record.id).unwrap_or_default();
    let frame = record
        .frame
        .as_ref()
        .map_or_else(WindowFrame::default, WindowFrame::from_data);
    let space_key = record
        .space
        .clone()
        .or_else(|| record.rows.keys().next().cloned());
    let raw_tabs = space_key
        .as_ref()
        .and_then(|key| record.rows.get(key))
        .cloned()
        .unwrap_or_default();

    let mut tabs = Vec::new();
    for raw in &raw_tabs {
        if let Some(tab) = restore_one_tab(raw, allocator) {
            tabs.push(tab);
        }
    }
    if tabs.is_empty() {
        tabs.push(RestoredTab::blank(allocator)?);
    }
    let active = record.active.min(tabs.len() - 1);
    apply_sleep_policy(&mut tabs, active);

    Ok(RestoredWindow {
        id,
        frame,
        space: space_key,
        active,
        tabs,
        groups: Vec::new(),
        splits: Vec::new(),
        folded: record.folded,
        merged: record.merged,
        tucked: record.tucked,
    })
}

fn restore_one_tab(raw: &SessionTab, allocator: &mut TabIdAllocator) -> Option<RestoredTab> {
    let url = if raw.url.trim().is_empty() {
        PageUrl::parse("search://").ok()?
    } else {
        PageUrl::parse(&raw.url).ok()?
    };
    let id = allocator.allocate();
    let key = if raw.id.is_empty() {
        format!("tab-{}", id.get())
    } else {
        raw.id.clone()
    };
    let title = if raw.title.is_empty() {
        url.display()
    } else {
        raw.title.clone()
    };
    Some(RestoredTab {
        id,
        key,
        url,
        title,
        pinned: raw.pinned,
        muted: raw.muted,
        sleeping: true,
        should_load_now: false,
        private: false,
        group: raw.group.clone(),
        space: raw.space.clone(),
        profile: raw.profile.clone(),
        scroll: raw.scroll.as_ref().map(|s| (s.x, s.y)),
    })
}

/// Mark every tab asleep except the active tab and any panes splitting the screen with it (`FEATURES.md` §1, §3.5, §3.7).
fn apply_sleep_policy(tabs: &mut [RestoredTab], active: usize) {
    apply_sleep_policy_with_splits(tabs, active, &[]);
}

fn apply_sleep_policy_with_splits(tabs: &mut [RestoredTab], active: usize, splits: &[Split]) {
    let active_key = tabs.get(active).map(|t| t.key.as_str());
    let mut wake_keys: Vec<String> = Vec::new();
    if let Some(akey) = active_key {
        wake_keys.push(akey.to_owned());
        for split in splits {
            let keys = split_tab_keys(split);
            if keys.iter().any(|k| k == akey) {
                for key in keys {
                    if !wake_keys.iter().any(|existing| existing == &key) {
                        wake_keys.push(key);
                    }
                }
            }
        }
    }

    for (index, tab) in tabs.iter_mut().enumerate() {
        if index == active || wake_keys.iter().any(|k| k == &tab.key) {
            tab.sleeping = false;
            tab.should_load_now = true;
        } else {
            tab.sleeping = true;
            tab.should_load_now = false;
        }
    }
}

fn split_tab_keys(split: &Split) -> Vec<String> {
    if let Some(serde_json::Value::Array(arr)) = split.extra.get("panes") {
        let keys: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
            .filter(|k| !k.is_empty())
            .collect();
        if keys.len() >= 2 {
            return keys;
        }
    }
    let mut out = Vec::new();
    if !split.first.is_empty() {
        out.push(split.first.clone());
    }
    if !split.second.is_empty() && !out.contains(&split.second) {
        out.push(split.second.clone());
    }
    out
}

/// Filter persisted splits so only splits with `2..=4` existing, distinct tabs survive (`FEATURES.md` §3.7).
fn validate_splits(splits: &[Split], tabs: &[RestoredTab]) -> Vec<Split> {
    splits
        .iter()
        .filter(|split| {
            let keys = split_tab_keys(split);
            if !(2..=4).contains(&keys.len()) {
                return false;
            }
            let mut seen = std::collections::BTreeSet::new();
            keys.iter()
                .all(|key| seen.insert(key.as_str()) && tabs.iter().any(|t| t.key == *key))
        })
        .cloned()
        .collect()
}

/// Save a window's tabs to `session.json` (or `session-<space>.json`), excluding
/// private tabs (`AGENTS.md` §9).
pub fn save_window_session(
    store: &DataStore,
    space_id: Option<&str>,
    window: &RestoredWindow,
) -> Result<()> {
    let mut tabs = Vec::new();
    let mut active_in_persisted = 0_usize;
    for (index, tab) in window.tabs.iter().enumerate() {
        if tab.private {
            continue;
        }
        if index == window.active {
            active_in_persisted = tabs.len();
        }
        tabs.push(tab_to_session_tab(tab));
    }

    let file = SessionFile {
        version: CURRENT_SCHEMA_VERSION,
        active: active_in_persisted,
        tabs,
        groups: window.groups.clone(),
        splits: window.splits.clone(),
        extra: serde_json::Map::new(),
    };
    store.save_session(space_id, &file)
}

/// Save additional windows (beyond the primary window) to `windows.json`,
/// excluding private tabs.
pub fn save_additional_windows(store: &DataStore, windows: &[RestoredWindow]) -> Result<()> {
    let mut records = Vec::new();
    for window in windows {
        let space_key = window
            .space
            .clone()
            .unwrap_or_else(|| "space-personal".to_owned());
        let persisted_tabs: Vec<SessionTab> = window
            .tabs
            .iter()
            .filter(|tab| !tab.private)
            .map(tab_to_session_tab)
            .collect();
        if persisted_tabs.is_empty() {
            continue;
        }
        let mut rows = BTreeMap::new();
        rows.insert(space_key.clone(), persisted_tabs);
        records.push(WindowRecord {
            id: window.id.to_string(),
            frame: Some(window.frame.to_data()),
            space: Some(space_key),
            active: window.active,
            rows,
            merged: window.merged,
            tucked: window.tucked,
            folded: window.folded,
            extra: serde_json::Map::new(),
        });
    }
    store.save_windows(&WindowsFile {
        windows: records,
        extra: serde_json::Map::new(),
    })
}

fn tab_to_session_tab(tab: &RestoredTab) -> SessionTab {
    SessionTab {
        id: tab.key.clone(),
        url: tab.url.as_str().to_owned(),
        title: tab.title.clone(),
        pinned: tab.pinned,
        muted: tab.muted,
        sleeping: tab.sleeping,
        group: tab.group.clone(),
        space: tab.space.clone(),
        profile: tab.profile.clone(),
        scroll: tab.scroll.map(|(x, y)| ScrollPosition {
            x,
            y,
            extra: serde_json::Map::new(),
        }),
        extra: serde_json::Map::new(),
    }
}

/// Build a default single-window session when no session file exists yet.
pub fn default_window_session(
    space: SpaceId,
    profile: ProfileId,
    allocator: &mut TabIdAllocator,
) -> Result<RestoredWindow> {
    let mut blank = RestoredTab::blank(allocator)?;
    blank.space = Some(space.to_string());
    blank.profile = Some(profile.to_string());
    Ok(RestoredWindow {
        id: WindowId::new(),
        frame: WindowFrame::default(),
        space: Some(space.to_string()),
        active: 0,
        tabs: vec![blank],
        groups: Vec::new(),
        splits: Vec::new(),
        folded: false,
        merged: false,
        tucked: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;

    #[test]
    fn thirty_tab_session_restores_fast_with_only_active_tab_awake() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());

        let mut tabs = Vec::with_capacity(30);
        for index in 0..30_usize {
            tabs.push(SessionTab {
                id: format!("tab-{index}"),
                url: format!("https://example.com/page-{index}"),
                title: format!("Page {index}"),
                pinned: index < 2,
                muted: index == 4,
                sleeping: false,
                scroll: Some(ScrollPosition {
                    x: 0.0,
                    y: (index as f64) * 100.0,
                    extra: serde_json::Map::new(),
                }),
                ..SessionTab::default()
            });
        }
        let session_file = SessionFile {
            version: CURRENT_SCHEMA_VERSION,
            active: 17,
            tabs,
            ..SessionFile::default()
        };
        store
            .save_session(None, &session_file)
            .expect("save 30-tab session");

        let started = Instant::now();
        let mut allocator = TabIdAllocator::new();
        let restored =
            restore_session(&store, RestoreMode::Normal, &mut allocator).expect("restore 30 tabs");
        let elapsed = started.elapsed();

        assert!(
            elapsed.as_millis() < 50,
            "30-tab restore took {} ms",
            elapsed.as_millis()
        );
        assert!(restored.quarantined_files.is_empty());
        let window = &restored.windows[0];
        assert_eq!(window.tabs.len(), 30);
        assert_eq!(window.active, 17);

        let awake_count = window
            .tabs
            .iter()
            .filter(|t| !t.sleeping && t.should_load_now)
            .count();
        assert_eq!(awake_count, 1, "only the active tab may wake at restore");
        assert!(!window.tabs[17].sleeping);
        assert!(window.tabs[17].should_load_now);
        assert!(window.tabs[0].sleeping);
        assert_eq!(window.tabs[17].scroll, Some((0.0, 1700.0)));
    }

    #[test]
    fn private_tabs_are_never_persisted_and_fresh_start_keeps_only_pins() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());
        let mut allocator = TabIdAllocator::new();

        let pin = RestoredTab {
            id: allocator.allocate(),
            key: "pin-1".into(),
            url: PageUrl::parse("https://news.ycombinator.com/").expect("url"),
            title: "HN".into(),
            pinned: true,
            muted: false,
            sleeping: true,
            should_load_now: false,
            private: false,
            group: None,
            space: None,
            profile: None,
            scroll: None,
        };
        let normal = RestoredTab {
            id: allocator.allocate(),
            key: "tab-2".into(),
            url: PageUrl::parse("https://example.com/normal").expect("url"),
            title: "Normal".into(),
            pinned: false,
            muted: false,
            sleeping: false,
            should_load_now: true,
            private: false,
            group: None,
            space: None,
            profile: None,
            scroll: None,
        };
        let private = RestoredTab {
            id: allocator.allocate(),
            key: "tab-private".into(),
            url: PageUrl::parse("https://example.com/secret").expect("url"),
            title: "Secret".into(),
            pinned: false,
            muted: false,
            sleeping: false,
            should_load_now: true,
            private: true,
            group: None,
            space: None,
            profile: None,
            scroll: None,
        };

        let window = RestoredWindow {
            id: WindowId::new(),
            frame: WindowFrame::default(),
            space: None,
            active: 2,
            tabs: vec![pin, normal, private],
            groups: Vec::new(),
            splits: Vec::new(),
            folded: false,
            merged: false,
            tucked: false,
        };
        save_window_session(&store, None, &window).expect("save session");

        // Normal restore has 2 tabs (private tab excluded).
        let restored_normal =
            restore_session(&store, RestoreMode::Normal, &mut allocator).expect("normal restore");
        assert_eq!(restored_normal.windows[0].tabs.len(), 2);
        assert!(
            restored_normal.windows[0]
                .tabs
                .iter()
                .all(|t| t.url.as_str() != "https://example.com/secret")
        );

        // Fresh-start restore keeps the pin (asleep) plus 1 new blank tab (awake).
        let restored_fresh = restore_session(&store, RestoreMode::FreshStart, &mut allocator)
            .expect("fresh restore");
        let fresh_tabs = &restored_fresh.windows[0].tabs;
        assert_eq!(fresh_tabs.len(), 2);
        assert!(fresh_tabs[0].pinned);
        assert!(fresh_tabs[0].sleeping);
        assert!(!fresh_tabs[1].pinned);
        assert!(!fresh_tabs[1].sleeping);
        assert_eq!(fresh_tabs[1].url.as_str(), "search://");
    }

    #[test]
    fn corrupt_session_and_windows_files_are_quarantined_on_restore() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());
        fs::write(store.path_for("session.json"), b"{bad json").expect("write corrupt session");
        fs::write(store.path_for("windows.json"), b"[not an object").expect("write corrupt win");

        let mut allocator = TabIdAllocator::new();
        let restored = restore_session(&store, RestoreMode::Normal, &mut allocator)
            .expect("restore recovers from corrupt files");

        assert_eq!(restored.quarantined_files.len(), 2);
        for q in &restored.quarantined_files {
            assert!(q.exists());
        }
        assert_eq!(restored.windows.len(), 1);
        assert_eq!(restored.windows[0].tabs.len(), 1);
        assert_eq!(restored.windows[0].frame, WindowFrame::default());
    }

    #[test]
    fn restore_space_session_wakes_active_tab_and_split_panes_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = DataStore::new(temp.path());
        let space_id = "space-work";
        let tabs: Vec<SessionTab> = (0..5)
            .map(|i| SessionTab {
                id: format!("t-{i}"),
                url: format!("https://example.com/{i}"),
                title: format!("Tab {i}"),
                ..SessionTab::default()
            })
            .collect();
        let session_file = SessionFile {
            version: CURRENT_SCHEMA_VERSION,
            active: 1,
            tabs,
            splits: vec![Split {
                id: "split-1".into(),
                axis: "vertical".into(),
                ratio: 0.5,
                first: "t-1".into(),
                second: "t-3".into(),
                extra: serde_json::Map::new(),
            }],
            ..SessionFile::default()
        };
        store
            .save_session(Some(space_id), &session_file)
            .expect("save space session");

        let mut alloc = TabIdAllocator::new();
        let (win, quarantined) =
            restore_space_session(&store, space_id, RestoreMode::Normal, &mut alloc)
                .expect("restore space");
        assert!(quarantined.is_none());
        assert_eq!(win.space.as_deref(), Some(space_id));
        assert!(!win.tabs[1].sleeping);
        assert!(win.tabs[1].should_load_now);
        assert!(!win.tabs[3].sleeping, "split partner must wake too");
        assert!(win.tabs[3].should_load_now);
        assert!(win.tabs[0].sleeping);
        assert!(win.tabs[2].sleeping);
        assert!(win.tabs[4].sleeping);
    }
}
