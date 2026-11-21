//! Serde models for the original app's JSON data files.
//!
//! The Swift source and real data files are not in this checkout. These
//! schemas are inferred from `FEATURES.md` §25 and the sections it points at,
//! then exercised by the fixtures in `tests/fixtures/legacy`. Treat them as a
//! compatibility contract to refine when real fixtures are available, not as
//! proof that the old app wrote exactly these shapes.
//!
//! Every object carries a flattened `extra` map. Unknown fields therefore
//! survive a read/write round trip, which is what protects data written by a
//! newer build from a lossy older build.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Unknown fields preserved from a decoded object.
pub type Extra = Map<String, Value>;

/// One `session.json` or `session-<spaceUUID>.json` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionFile {
    /// Format version, if the file carries one.
    #[serde(default)]
    pub version: u32,
    /// Index into `tabs` that is selected.
    #[serde(default)]
    pub active: usize,
    /// Open tabs in row order.
    #[serde(default)]
    pub tabs: Vec<SessionTab>,
    /// Tab groups in this space.
    #[serde(default)]
    pub groups: Vec<TabGroup>,
    /// Split-view pairs.
    #[serde(default)]
    pub splits: Vec<Split>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A serialised tab. The live page state lives in the engine, not here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionTab {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub sleeping: bool,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub space: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub scroll: Option<ScrollPosition>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Remembered scroll offset for a sleeping tab.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScrollPosition {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A named, coloured tab group.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TabGroup {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub tab_ids: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Two tabs shown side by side.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Split {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub axis: String,
    #[serde(default)]
    pub ratio: f64,
    #[serde(default)]
    pub first: String,
    #[serde(default)]
    pub second: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One `windows.json` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowsFile {
    #[serde(default)]
    pub windows: Vec<WindowRecord>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// An additional window and its per-space rows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub frame: Option<Frame>,
    #[serde(default)]
    pub space: Option<String>,
    #[serde(default)]
    pub active: usize,
    /// Space UUID to that space's tab row.
    #[serde(default)]
    pub rows: BTreeMap<String, Vec<SessionTab>>,
    #[serde(default)]
    pub merged: bool,
    #[serde(default)]
    pub tucked: bool,
    #[serde(default)]
    pub folded: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A window frame in screen coordinates.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One `pins.json` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PinsFile {
    /// Space UUID to that space's pins.
    #[serde(default)]
    pub spaces: BTreeMap<String, Vec<Pin>>,
    /// Pins visible in every space.
    #[serde(default)]
    pub shared: Vec<Pin>,
    /// Pin folders, including live-feed folders.
    #[serde(default)]
    pub folders: Vec<PinFolder>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A pinned square or row entry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub letter: String,
    #[serde(default)]
    pub home: Option<String>,
    #[serde(default)]
    pub row: Option<u32>,
    #[serde(default)]
    pub space: Option<String>,
    #[serde(default)]
    pub shared: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A pin folder, optionally backed by a live feed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PinFolder {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pins: Vec<Pin>,
    #[serde(default)]
    pub feeds: Vec<LiveFeed>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A live RSS/Atom folder.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LiveFeed {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub refreshed_at: Option<String>,
    #[serde(default)]
    pub posts: Vec<FeedPost>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One cached live-feed post.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FeedPost {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub published: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One `spaces.json` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpacesFile {
    #[serde(default)]
    pub spaces: Vec<Space>,
    #[serde(default)]
    pub order_kept: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A space, its appearance, and its store policy.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Space {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub gradient: Option<String>,
    #[serde(default)]
    pub strength: Option<f64>,
    #[serde(default)]
    pub profile: Option<String>,
    /// `own`, `shared`, or a profile UUID.
    #[serde(default)]
    pub store_sharing: Option<String>,
    #[serde(default)]
    pub downloads_folder: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One `bookmarks.json` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BookmarksFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub roots: Vec<BookmarkNode>,
    #[serde(default)]
    pub imported_from: Vec<ImportMark>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A bookmark or bookmark folder.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BookmarkNode {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// Present on links, absent on folders.
    #[serde(default)]
    pub url: Option<String>,
    /// `folder` or `bookmark`, when the old file stored a discriminator.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub children: Vec<BookmarkNode>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Provenance for imported bookmarks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportMark {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub imported_at: Option<String>,
    #[serde(default)]
    pub count: usize,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One `history.json`, `history-<profile>.json`, or `history-space-<space>.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryFile {
    #[serde(default)]
    pub visits: Vec<Visit>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A history entry. Dates stay strings so formatting is not rewritten.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub visit_count: u32,
    #[serde(default)]
    pub first_visit: Option<String>,
    #[serde(default)]
    pub last_visit: Option<String>,
    #[serde(default)]
    pub space: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One `hidden.json` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HiddenFile {
    /// Site key to hidden selectors.
    #[serde(default)]
    pub sites: BTreeMap<String, Vec<HiddenSelector>>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One hidden element rule.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HiddenSelector {
    #[serde(default)]
    pub selector: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One `styles.json` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StylesFile {
    /// Site key to its style.
    #[serde(default)]
    pub sites: BTreeMap<String, SiteStyle>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Per-site CSS.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SiteStyle {
    #[serde(default)]
    pub css: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub updated: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}
