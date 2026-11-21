//! Pure domain types for searlx (P1-01).
//!
//! These types model the browser's runtime and durable concepts without I/O,
//! platform dependencies, or engine types:
//! - [`EngineId`] — identifies an engine backend
//! - [`PermissionKind`], [`PermissionDecision`], [`PermissionKey`] — per-origin
//!   permission keys (`capture.<origin>|<kind>`) and port-disagreement checks
//! - [`HistoryVisit`] — normalized visit record with URL identity merging
//! - [`BookmarkItem`], [`BookmarkTree`] — arbitrary-depth bookmark tree with
//!   search, count, and non-destructive import merge
//! - [`PinItem`] — pinned tab identity, home URL, letter glyph, wandered state
//! - [`TabGroupItem`], [`SpaceColor`] — named, colored, collapsible tab groups
//! - [`SplitAxis`], [`SplitItem`] — 2–4 pane split view with share clamping
//!   (20–80% for two panes, ≥10% for three or four)
//! - [`SessionShape`], [`TabSnapshot`] — validated session model with schema
//!   migration from legacy [`crate::data::SessionFile`]

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::data::{
    BookmarkNode, BookmarksFile, HistoryFile, Pin, SessionFile, SessionTab, Split, TabGroup, Visit,
};
use crate::error::{CoreError, Result};
use crate::ids::{GroupId, PinId, ProfileId, SpaceId, TabId};
use crate::url::PageUrl;

/// Current schema version for searlx domain files (`session.json` v3, `bookmarks.json` v2..=3).
pub const CURRENT_SCHEMA_VERSION: u32 = 3;

/// Identifies which web engine backend is active.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineId {
    /// Apple WebKit (`WKWebView`) on macOS.
    WkWebView,
    /// Microsoft Edge WebView2 on Windows.
    WebView2,
    /// WebKitGTK on Linux.
    WebKitGtk,
    /// Headless fake engine used in tests.
    Fake,
    /// Any custom or future backend identifier.
    Custom(String),
}

impl EngineId {
    /// Parse an engine identifier string.
    #[must_use]
    pub fn parse(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "wkwebview" | "webkit" => Self::WkWebView,
            "webview2" => Self::WebView2,
            "webkitgtk" => Self::WebKitGtk,
            "fake" => Self::Fake,
            other => Self::Custom(other.to_owned()),
        }
    }

    /// Canonical wire name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::WkWebView => "wkwebview",
            Self::WebView2 => "webview2",
            Self::WebKitGtk => "webkitgtk",
            Self::Fake => "fake",
            Self::Custom(name) => name.as_str(),
        }
    }
}

impl fmt::Display for EngineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Site permission categories (`FEATURES.md` §12.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    /// Camera access.
    Camera,
    /// Microphone access.
    Microphone,
    /// Geolocation access.
    Location,
    /// Web notifications.
    Notifications,
    /// Screen or window sharing.
    ScreenCapture,
    /// Programmatic clipboard read.
    ClipboardRead,
}

impl PermissionKind {
    /// Wire token used in `capture.<origin>|<kind>`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::Location => "location",
            Self::Notifications => "notifications",
            Self::ScreenCapture => "screen_capture",
            Self::ClipboardRead => "clipboard_read",
        }
    }

    /// Parse a permission kind token.
    pub fn parse(token: &str) -> Result<Self> {
        match token.trim() {
            "camera" => Ok(Self::Camera),
            "microphone" | "mic" => Ok(Self::Microphone),
            "location" | "geolocation" => Ok(Self::Location),
            "notifications" | "notification" => Ok(Self::Notifications),
            "screen_capture" | "screen" => Ok(Self::ScreenCapture),
            "clipboard_read" | "clipboard" => Ok(Self::ClipboardRead),
            other => Err(CoreError::Invalid(format!(
                "unknown permission kind: {other:?}"
            ))),
        }
    }
}

impl fmt::Display for PermissionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// User decision for a site permission (`FEATURES.md` §5.8, §12.3).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    /// Prompt when the site asks.
    #[default]
    Ask,
    /// Always allow for this origin.
    Allow,
    /// Allow for the current session only (not persisted).
    AllowOnce,
    /// Always refuse for this origin.
    Refuse,
}

/// A persisted permission key in `capture.<origin>|<kind>` format (`FEATURES.md` §12.3, §25).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PermissionKey {
    /// Origin string (`scheme://host[:port]`).
    pub origin: String,
    /// Permission category.
    pub kind: PermissionKind,
}

impl PermissionKey {
    /// Build a permission key for a [`PageUrl`] and [`PermissionKind`].
    ///
    /// Only `http` and `https` origins may persist permissions (`FEATURES.md` §5.8).
    pub fn for_url(url: &PageUrl, kind: PermissionKind) -> Result<Self> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(CoreError::Invalid(format!(
                "permissions require http or https origin, got {}",
                url.scheme()
            )));
        }
        let host = url
            .host()
            .ok_or_else(|| CoreError::Invalid("permission URL has no host".into()))?;
        let origin = match url.as_url().port() {
            Some(port) => format!("{}://{host}:{port}", url.scheme()),
            None => format!("{}://{host}", url.scheme()),
        };
        Ok(Self { origin, kind })
    }

    /// Format as `capture.<origin>|<kind>` (`FEATURES.md` §25).
    #[must_use]
    pub fn to_storage_key(&self) -> String {
        format!("capture.{}|{}", self.origin, self.kind.as_str())
    }

    /// Parse a `capture.<origin>|<kind>` storage key.
    pub fn from_storage_key(raw: &str) -> Result<Self> {
        let rest = raw.strip_prefix("capture.").ok_or_else(|| {
            CoreError::Invalid(format!("permission key missing 'capture.' prefix: {raw:?}"))
        })?;
        let (origin, kind_str) = rest.rsplit_once('|').ok_or_else(|| {
            CoreError::Invalid(format!("permission key missing '|' separator: {raw:?}"))
        })?;
        if origin.is_empty() {
            return Err(CoreError::Invalid("permission key has empty origin".into()));
        }
        let kind = PermissionKind::parse(kind_str)?;
        Ok(Self {
            origin: origin.to_owned(),
            kind,
        })
    }

    /// Host without explicit default port (`https://example.com:443` → `https://example.com`),
    /// used to detect when two port spellings disagree for the same site (`FEATURES.md` §5.8).
    #[must_use]
    pub fn canonical_origin(&self) -> String {
        if let Some(stripped) = self.origin.strip_suffix(":443")
            && stripped.starts_with("https://")
        {
            return stripped.to_owned();
        }
        if let Some(stripped) = self.origin.strip_suffix(":80")
            && stripped.starts_with("http://")
        {
            return stripped.to_owned();
        }
        self.origin.clone()
    }

    /// Whether two keys refer to the same canonical origin and permission kind
    /// under different port spellings (e.g. `https://example.com` vs `https://example.com:443`).
    #[must_use]
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.origin != other.origin
            && self.canonical_origin() == other.canonical_origin()
    }
}

impl fmt::Display for PermissionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_storage_key())
    }
}

/// Named space and tab-group color (`FEATURES.md` §6.1, §8).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum SpaceColor {
    /// No tint.
    #[default]
    None,
    /// Blue tint.
    Blue,
    /// Green tint.
    Green,
    /// Orange tint.
    Orange,
    /// Red tint.
    Red,
    /// Violet tint.
    Violet,
}

impl SpaceColor {
    /// Parse a color name from persisted JSON.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "blue" => Self::Blue,
            "green" => Self::Green,
            "orange" => Self::Orange,
            "red" => Self::Red,
            "violet" | "purple" => Self::Violet,
            _ => Self::None,
        }
    }

    /// Canonical name for persistence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Orange => "orange",
            Self::Red => "red",
            Self::Violet => "violet",
        }
    }
}

/// A history visit record (`FEATURES.md` §10).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryVisit {
    /// Visited page URL.
    pub url: PageUrl,
    /// Total visit count.
    pub visit_count: u32,
    /// ISO-8601 timestamp of first visit, when known.
    pub first_visit: Option<String>,
    /// ISO-8601 timestamp of most recent visit.
    pub last_visit: String,
    /// Most recent page title.
    pub title: String,
    /// Owning space, if recorded.
    pub space: Option<String>,
    /// Owning profile, if recorded.
    pub profile: Option<String>,
}

impl HistoryVisit {
    /// Create a single visit at `timestamp`.
    #[must_use]
    pub fn new(url: PageUrl, title: impl Into<String>, timestamp: impl Into<String>) -> Self {
        let stamp = timestamp.into();
        Self {
            url,
            visit_count: 1,
            first_visit: Some(stamp.clone()),
            last_visit: stamp,
            title: title.into(),
            space: None,
            profile: None,
        }
    }

    /// Stable history identity key (`PageUrl::identity`, ignoring fragment).
    #[must_use]
    pub fn identity(&self) -> String {
        self.url.identity()
    }

    /// Merge an incoming visit with the same identity (`FEATURES.md` §15.1):
    /// count uses `max` (or increments on live visit), last-visit uses the
    /// later timestamp, and a missing title is filled.
    pub fn merge_imported(&mut self, incoming: &Self) {
        self.visit_count = self.visit_count.max(incoming.visit_count);
        if incoming.last_visit > self.last_visit {
            self.last_visit.clone_from(&incoming.last_visit);
            if !incoming.title.is_empty() {
                self.title.clone_from(&incoming.title);
            }
        } else if self.title.is_empty() && !incoming.title.is_empty() {
            self.title.clone_from(&incoming.title);
        }
        if let (Some(existing), Some(inc)) = (&self.first_visit, &incoming.first_visit) {
            if inc < existing {
                self.first_visit = Some(inc.clone());
            }
        } else if self.first_visit.is_none() {
            self.first_visit.clone_from(&incoming.first_visit);
        }
    }

    /// Convert from a raw data [`Visit`].
    pub fn try_from_data(visit: &Visit) -> Result<Self> {
        let url = PageUrl::parse(&visit.url)?;
        Ok(Self {
            url,
            visit_count: visit.visit_count.max(1),
            first_visit: visit.first_visit.clone(),
            last_visit: visit.last_visit.clone().unwrap_or_default(),
            title: visit.title.clone(),
            space: visit.space.clone(),
            profile: visit.profile.clone(),
        })
    }

    /// Convert to a raw data [`Visit`].
    #[must_use]
    pub fn to_data(&self) -> Visit {
        Visit {
            url: self.url.as_str().to_owned(),
            title: self.title.clone(),
            visit_count: self.visit_count,
            first_visit: self.first_visit.clone(),
            last_visit: (!self.last_visit.is_empty()).then(|| self.last_visit.clone()),
            space: self.space.clone(),
            profile: self.profile.clone(),
            extra: BTreeMap::new().into_iter().collect(),
        }
    }
}

/// Convert a [`HistoryFile`] into deduplicated [`HistoryVisit`] records keyed by URL identity.
pub fn visits_from_file(file: &HistoryFile) -> Vec<HistoryVisit> {
    let mut by_identity: Vec<HistoryVisit> = Vec::new();
    for raw in &file.visits {
        let Ok(visit) = HistoryVisit::try_from_data(raw) else {
            continue;
        };
        let key = visit.identity();
        if let Some(existing) = by_identity.iter_mut().find(|item| item.identity() == key) {
            existing.merge_imported(&visit);
        } else {
            by_identity.push(visit);
        }
    }
    by_identity
}

/// A node in the bookmark tree (`FEATURES.md` §9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookmarkItem {
    /// A saved link.
    Link {
        /// Stable node identifier.
        id: String,
        /// Display title.
        title: String,
        /// Target URL.
        url: PageUrl,
    },
    /// A folder containing child bookmarks or nested folders.
    Folder {
        /// Stable node identifier.
        id: String,
        /// Folder title.
        title: String,
        /// Ordered children.
        children: Vec<BookmarkItem>,
    },
}

impl BookmarkItem {
    /// Node identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Link { id, .. } | Self::Folder { id, .. } => id.as_str(),
        }
    }

    /// Node title.
    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            Self::Link { title, .. } | Self::Folder { title, .. } => title.as_str(),
        }
    }

    /// Count of link items inside this subtree.
    #[must_use]
    pub fn link_count(&self) -> usize {
        match self {
            Self::Link { .. } => 1,
            Self::Folder { children, .. } => children.iter().map(Self::link_count).sum(),
        }
    }

    /// Convert from a raw [`BookmarkNode`].
    pub fn try_from_node(node: &BookmarkNode) -> Result<Self> {
        let is_folder = node.kind.as_deref() == Some("folder")
            || (node.url.is_none() && !node.children.is_empty())
            || node.url.is_none();
        if is_folder {
            let mut children = Vec::with_capacity(node.children.len());
            for child in &node.children {
                children.push(Self::try_from_node(child)?);
            }
            Ok(Self::Folder {
                id: node.id.clone(),
                title: node.title.clone(),
                children,
            })
        } else {
            let raw_url = node.url.as_deref().unwrap_or_default();
            let url = PageUrl::parse(raw_url)?;
            Ok(Self::Link {
                id: node.id.clone(),
                title: node.title.clone(),
                url,
            })
        }
    }

    /// Convert back to a raw [`BookmarkNode`].
    #[must_use]
    pub fn to_node(&self) -> BookmarkNode {
        match self {
            Self::Link { id, title, url } => BookmarkNode {
                id: id.clone(),
                title: title.clone(),
                url: Some(url.as_str().to_owned()),
                kind: Some("bookmark".into()),
                children: Vec::new(),
                extra: serde_json::Map::new(),
            },
            Self::Folder {
                id,
                title,
                children,
            } => BookmarkNode {
                id: id.clone(),
                title: title.clone(),
                url: None,
                kind: Some("folder".into()),
                children: children.iter().map(Self::to_node).collect(),
                extra: serde_json::Map::new(),
            },
        }
    }
}

/// The profile-wide bookmark tree (`FEATURES.md` §9).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkTree {
    /// Schema version.
    pub version: u32,
    /// Top-level items.
    pub roots: Vec<BookmarkItem>,
}

impl BookmarkTree {
    /// Create an empty bookmark tree at [`CURRENT_SCHEMA_VERSION`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            version: CURRENT_SCHEMA_VERSION,
            roots: Vec::new(),
        }
    }

    /// Total number of saved links across all folders.
    #[must_use]
    pub fn total_links(&self) -> usize {
        self.roots.iter().map(BookmarkItem::link_count).sum()
    }

    /// Whether any bookmark matches `url` by [`PageUrl::identity`].
    #[must_use]
    pub fn contains_url(&self, url: &PageUrl) -> bool {
        let target = url.identity();
        Self::slice_contains_identity(&self.roots, &target)
    }

    fn slice_contains_identity(items: &[BookmarkItem], target_identity: &str) -> bool {
        items.iter().any(|item| match item {
            BookmarkItem::Link { url, .. } => url.identity() == target_identity,
            BookmarkItem::Folder { children, .. } => {
                Self::slice_contains_identity(children, target_identity)
            }
        })
    }

    /// Merge imported items at the top level, skipping links whose URL identity
    /// already exists at the same level (`FEATURES.md` §9, §15.1).
    pub fn merge_items(&mut self, incoming: Vec<BookmarkItem>) {
        Self::merge_into_list(&mut self.roots, incoming);
    }

    fn merge_into_list(target: &mut Vec<BookmarkItem>, incoming: Vec<BookmarkItem>) {
        for item in incoming {
            match item {
                BookmarkItem::Link { id, title, url } => {
                    let identity = url.identity();
                    let exists = target.iter().any(|existing| {
                        matches!(existing, BookmarkItem::Link { url: existing_url, .. } if existing_url.identity() == identity)
                    });
                    if !exists {
                        target.push(BookmarkItem::Link { id, title, url });
                    }
                }
                BookmarkItem::Folder {
                    id,
                    title,
                    children,
                } => {
                    if let Some(BookmarkItem::Folder {
                        children: existing_children,
                        ..
                    }) = target.iter_mut().find(|existing| {
                        matches!(existing, BookmarkItem::Folder { title: existing_title, .. } if existing_title == &title)
                    }) {
                        Self::merge_into_list(existing_children, children);
                    } else {
                        target.push(BookmarkItem::Folder {
                            id,
                            title,
                            children,
                        });
                    }
                }
            }
        }
    }

    /// Convert from a [`BookmarksFile`].
    pub fn try_from_file(file: &BookmarksFile) -> Result<Self> {
        let mut roots = Vec::with_capacity(file.roots.len());
        for node in &file.roots {
            roots.push(BookmarkItem::try_from_node(node)?);
        }
        Ok(Self {
            version: file.version.max(CURRENT_SCHEMA_VERSION),
            roots,
        })
    }

    /// Convert to a [`BookmarksFile`].
    #[must_use]
    pub fn to_file(&self) -> BookmarksFile {
        BookmarksFile {
            version: self.version.max(CURRENT_SCHEMA_VERSION),
            roots: self.roots.iter().map(BookmarkItem::to_node).collect(),
            imported_from: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }
}

/// A pinned page (`FEATURES.md` §7.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinItem {
    /// Stable pin UUID across windows.
    pub id: PinId,
    /// Current URL in this pin.
    pub url: PageUrl,
    /// The home URL the pin wakes at.
    pub home: PageUrl,
    /// Title of the pin.
    pub title: String,
    /// Single-character square mark.
    pub letter: char,
    /// Optional row index when pinned rows are enabled.
    pub row: Option<u32>,
    /// Owning space (`None` when shared across all spaces).
    pub space: Option<SpaceId>,
    /// Visible in every space of the profile (`Keep in Every Space`).
    pub shared: bool,
}

impl PinItem {
    /// Create a new pin at `home`.
    #[must_use]
    pub fn new(id: PinId, home: PageUrl, title: impl Into<String>) -> Self {
        let title = title.into();
        let letter = default_pin_letter(&home, &title);
        Self {
            id,
            url: home.clone(),
            home,
            title,
            letter,
            row: None,
            space: None,
            shared: false,
        }
    }

    /// Whether the pin has navigated away from its home URL (`FEATURES.md` §7.1).
    #[must_use]
    pub fn has_wandered(&self) -> bool {
        self.url.identity() != self.home.identity()
    }

    /// Return the pin to its home URL (`Back to Pinned Page` or `⌘W` put-down).
    pub fn go_home(&mut self) {
        self.url = self.home.clone();
    }

    /// Replace the pinned home page with the current URL (`FEATURES.md` §7.1).
    pub fn rehome_to_current(&mut self) {
        self.home = self.url.clone();
    }

    /// Convert from a raw [`Pin`] record.
    pub fn try_from_data(pin: &Pin) -> Result<Self> {
        let id = PinId::parse(&pin.id).unwrap_or_default();
        let url = PageUrl::parse(&pin.url)?;
        let home = match &pin.home {
            Some(home_str) if !home_str.is_empty() => PageUrl::parse(home_str)?,
            _ => url.clone(),
        };
        let letter = pin
            .letter
            .chars()
            .next()
            .unwrap_or_else(|| default_pin_letter(&home, &pin.title));
        let space = pin
            .space
            .as_deref()
            .and_then(|raw| SpaceId::parse(raw).ok());
        Ok(Self {
            id,
            url,
            home,
            title: pin.title.clone(),
            letter,
            row: pin.row,
            space,
            shared: pin.shared,
        })
    }

    /// Convert to a raw [`Pin`] record.
    #[must_use]
    pub fn to_data(&self) -> Pin {
        Pin {
            id: self.id.to_string(),
            url: self.url.as_str().to_owned(),
            title: self.title.clone(),
            letter: self.letter.to_string(),
            home: Some(self.home.as_str().to_owned()),
            row: self.row,
            space: self.space.map(|space| space.to_string()),
            shared: self.shared,
            extra: serde_json::Map::new(),
        }
    }
}

fn default_pin_letter(url: &PageUrl, title: &str) -> char {
    url.host()
        .and_then(|host| host.trim_start_matches("www.").chars().next())
        .or_else(|| title.chars().next())
        .map_or('·', |ch| ch.to_ascii_uppercase())
}

/// A named, colored tab group (`FEATURES.md` §8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabGroupItem {
    /// Group identifier.
    pub id: GroupId,
    /// Display name.
    pub name: String,
    /// Optional tint color.
    pub color: SpaceColor,
    /// Whether member tabs are folded under the heading.
    pub collapsed: bool,
    /// Member tab IDs in order.
    pub tabs: Vec<TabId>,
}

impl TabGroupItem {
    /// Create a new group with the given name and member tabs.
    #[must_use]
    pub fn new(id: GroupId, name: impl Into<String>, tabs: Vec<TabId>) -> Self {
        Self {
            id,
            name: name.into(),
            color: SpaceColor::None,
            collapsed: false,
            tabs,
        }
    }

    /// Whether the group has no remaining tabs (empty groups disappear: `FEATURES.md` §8).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

/// Split-view stacking axis (`FEATURES.md` §3.7).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum SplitAxis {
    /// Side-by-side panes (the UI default).
    #[default]
    Horizontal,
    /// Vertically stacked panes (supported by the persisted format).
    Vertical,
}

impl SplitAxis {
    /// Parse an axis string from JSON.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "vertical" | "v" => Self::Vertical,
            _ => Self::Horizontal,
        }
    }

    /// Canonical wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

/// A split view holding 2 to 4 pages (`FEATURES.md` §3.7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplitItem {
    /// Stable split identifier.
    pub id: String,
    /// Layout axis.
    pub axis: SplitAxis,
    /// Ordered member tabs (2..=4).
    pub panes: Vec<TabId>,
    /// Normalized width/height shares summing to `1.0`.
    pub shares: Vec<f64>,
    /// Index of the focused pane inside `panes`.
    pub focused: usize,
}

impl SplitItem {
    /// Minimum panes in a split.
    pub const MIN_PANES: usize = 2;
    /// Maximum panes in a split (`FEATURES.md` §3.7).
    pub const MAX_PANES: usize = 4;

    /// Build a split view from 2–4 distinct tabs with equal shares.
    pub fn new(id: impl Into<String>, panes: Vec<TabId>) -> Result<Self> {
        Self::validate_panes(&panes)?;
        let count = panes.len();
        let share = 1.0 / (count as f64);
        Ok(Self {
            id: id.into(),
            axis: SplitAxis::Horizontal,
            panes,
            shares: vec![share; count],
            focused: 0,
        })
    }

    fn validate_panes(panes: &[TabId]) -> Result<()> {
        if !(Self::MIN_PANES..=Self::MAX_PANES).contains(&panes.len()) {
            return Err(CoreError::Invalid(format!(
                "split view requires 2..=4 panes, got {}",
                panes.len()
            )));
        }
        for (i, a) in panes.iter().enumerate() {
            if panes[i + 1..].contains(a) {
                return Err(CoreError::Invalid(format!(
                    "duplicate tab {a} in split view"
                )));
            }
        }
        Ok(())
    }

    /// Clamp and normalize pane shares (`FEATURES.md` §3.7):
    /// - 2 panes clamp to `0.20..=0.80`
    /// - 3 or 4 panes enforce a `0.10` minimum per pane and normalize to `1.0`.
    pub fn set_shares(&mut self, requested: &[f64]) -> Result<()> {
        if requested.len() != self.panes.len() {
            return Err(CoreError::Invalid(
                "share count must match pane count".into(),
            ));
        }
        if self.panes.len() == 2 {
            let first = requested[0].clamp(0.20, 0.80);
            self.shares = vec![first, 1.0 - first];
            return Ok(());
        }
        let min_share = 0.10;
        let clamped: Vec<f64> = requested
            .iter()
            .map(|value| {
                if value.is_finite() {
                    value.max(min_share)
                } else {
                    min_share
                }
            })
            .collect();
        let sum: f64 = clamped.iter().sum();
        self.shares = clamped.into_iter().map(|v| v / sum).collect();
        Ok(())
    }

    /// Reset all panes to equal shares (`Even Out` / double-click divider).
    pub fn even_out(&mut self) {
        let share = 1.0 / (self.panes.len() as f64);
        self.shares.fill(share);
    }

    /// Reverse pane order and shares (`Swap Pages` in `FEATURES.md` §3.7).
    pub fn swap(&mut self) {
        self.panes.reverse();
        self.shares.reverse();
        self.focused = self.panes.len().saturating_sub(1 + self.focused);
    }

    /// Add a pane and make all shares even (`FEATURES.md` §3.7).
    pub fn add_pane(&mut self, tab: TabId) -> Result<()> {
        if self.panes.len() >= Self::MAX_PANES {
            return Err(CoreError::Invalid("split view already has 4 panes".into()));
        }
        if self.panes.contains(&tab) {
            return Err(CoreError::Invalid(
                "tab is already in this split view".into(),
            ));
        }
        self.panes.push(tab);
        self.shares.resize(self.panes.len(), 0.0);
        self.even_out();
        self.focused = self.panes.len() - 1;
        Ok(())
    }

    /// Remove a pane and redistribute its share (`FEATURES.md` §3.7).
    /// Returns `Ok(false)` when fewer than 2 panes remain and the split should dissolve.
    pub fn remove_pane(&mut self, tab: TabId) -> Result<bool> {
        let Some(index) = self.panes.iter().position(|&item| item == tab) else {
            return Err(CoreError::Invalid("tab is not in this split view".into()));
        };
        self.panes.remove(index);
        self.shares.remove(index);
        if self.panes.len() < Self::MIN_PANES {
            return Ok(false);
        }
        let sum: f64 = self.shares.iter().sum();
        if sum > 0.0 {
            for share in &mut self.shares {
                *share /= sum;
            }
        } else {
            self.even_out();
        }
        self.focused = self.focused.min(self.panes.len() - 1);
        Ok(true)
    }
}

/// A durable tab snapshot inside a [`SessionShape`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabSnapshot {
    /// Persisted tab key inside the session file.
    pub key: String,
    /// Validated page URL.
    pub url: PageUrl,
    /// Last known title.
    pub title: String,
    /// Whether the tab is pinned.
    pub pinned: bool,
    /// Persisted audio mute state.
    pub muted: bool,
    /// Whether the tab is asleep.
    pub sleeping: bool,
    /// Optional group key.
    pub group: Option<String>,
    /// Optional space ID.
    pub space: Option<SpaceId>,
    /// Optional profile ID.
    pub profile: Option<ProfileId>,
    /// Optional scroll offset (`(x, y)`).
    pub scroll: Option<(f64, f64)>,
}

/// Validated session shape migrated to [`CURRENT_SCHEMA_VERSION`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionShape {
    /// Schema version (always [`CURRENT_SCHEMA_VERSION`] after migration).
    pub version: u32,
    /// Active tab index (clamped to `tabs.len().saturating_sub(1)`).
    pub active: usize,
    /// Open tabs in row order.
    pub tabs: Vec<TabSnapshot>,
    /// Tab groups.
    pub groups: Vec<TabGroup>,
    /// Split pairs.
    pub splits: Vec<Split>,
}

impl SessionShape {
    /// Migrate and validate a raw [`SessionFile`] into a [`SessionShape`].
    ///
    /// - Rejects schema versions newer than [`CURRENT_SCHEMA_VERSION`].
    /// - Upgrades version `0` to [`CURRENT_SCHEMA_VERSION`].
    /// - Restores non-active tabs asleep (`FEATURES.md` §1, §3.5).
    pub fn migrate_from(file: &SessionFile) -> Result<Self> {
        if file.version > CURRENT_SCHEMA_VERSION {
            return Err(CoreError::UnsupportedVersion {
                path: "session.json".into(),
                found: file.version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        let mut tabs = Vec::with_capacity(file.tabs.len());
        for (index, raw) in file.tabs.iter().enumerate() {
            let url = if raw.url.trim().is_empty() {
                PageUrl::parse("search://")?
            } else {
                PageUrl::parse(&raw.url)?
            };
            let key = if raw.id.is_empty() {
                format!("tab-{}", index + 1)
            } else {
                raw.id.clone()
            };
            let space = raw.space.as_deref().and_then(|s| SpaceId::parse(s).ok());
            let profile = raw
                .profile
                .as_deref()
                .and_then(|p| ProfileId::parse(p).ok());
            let scroll = raw.scroll.as_ref().map(|s| (s.x, s.y));
            tabs.push(TabSnapshot {
                key,
                url,
                title: raw.title.clone(),
                pinned: raw.pinned,
                muted: raw.muted,
                sleeping: raw.sleeping || index != file.active,
                group: raw.group.clone(),
                space,
                profile,
                scroll,
            });
        }
        let active = if tabs.is_empty() {
            0
        } else {
            file.active.min(tabs.len() - 1)
        };
        if let Some(active_tab) = tabs.get_mut(active) {
            active_tab.sleeping = false;
        }
        Ok(Self {
            version: CURRENT_SCHEMA_VERSION,
            active,
            tabs,
            groups: file.groups.clone(),
            splits: file.splits.clone(),
        })
    }

    /// Convert back to a [`SessionFile`] at [`CURRENT_SCHEMA_VERSION`].
    #[must_use]
    pub fn to_file(&self) -> SessionFile {
        SessionFile {
            version: CURRENT_SCHEMA_VERSION,
            active: self.active,
            tabs: self
                .tabs
                .iter()
                .map(|tab| SessionTab {
                    id: tab.key.clone(),
                    url: tab.url.as_str().to_owned(),
                    title: tab.title.clone(),
                    pinned: tab.pinned,
                    muted: tab.muted,
                    sleeping: tab.sleeping,
                    group: tab.group.clone(),
                    space: tab.space.map(|s| s.to_string()),
                    profile: tab.profile.map(|p| p.to_string()),
                    scroll: tab.scroll.map(|(x, y)| crate::data::ScrollPosition {
                        x,
                        y,
                        extra: serde_json::Map::new(),
                    }),
                    extra: serde_json::Map::new(),
                })
                .collect(),
            groups: self.groups.clone(),
            splits: self.splits.clone(),
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_key_round_trips_and_detects_port_conflicts() {
        let a = PageUrl::parse("https://example.com/login").expect("url a");
        let key_a = PermissionKey::for_url(&a, PermissionKind::Camera).expect("key a");
        assert_eq!(key_a.to_storage_key(), "capture.https://example.com|camera");

        let parsed = PermissionKey::from_storage_key(&key_a.to_storage_key()).expect("parse key");
        assert_eq!(parsed, key_a);

        let key_default_port =
            PermissionKey::from_storage_key("capture.https://example.com:443|camera")
                .expect("port key");
        assert!(key_a.conflicts_with(&key_default_port));

        let internal = PageUrl::parse("search://settings").expect("internal");
        assert!(PermissionKey::for_url(&internal, PermissionKind::Camera).is_err());
    }

    #[test]
    fn history_visits_merge_by_url_identity() {
        let url_a = PageUrl::parse("https://Example.com/docs#intro").expect("url a");
        let url_b = PageUrl::parse("https://example.com:443/docs#part-2").expect("url b");
        let mut first = HistoryVisit::new(url_a, "", "2026-11-20T10:00:00Z");
        first.visit_count = 3;
        let mut second = HistoryVisit::new(url_b, "Documentation", "2026-11-21T08:00:00Z");
        second.visit_count = 5;

        assert_eq!(first.identity(), second.identity());
        first.merge_imported(&second);
        assert_eq!(first.visit_count, 5);
        assert_eq!(first.title, "Documentation");
        assert_eq!(first.last_visit, "2026-11-21T08:00:00Z");
        assert_eq!(first.first_visit.as_deref(), Some("2026-11-20T10:00:00Z"));
    }

    #[test]
    fn bookmark_tree_counts_and_merges_without_duplicating_urls() {
        let mut tree = BookmarkTree::new();
        let first_url = PageUrl::parse("https://example.com/a#one").expect("url 1");
        let duplicate_url = PageUrl::parse("https://example.com/a#two").expect("url 1 dup");
        let second_url = PageUrl::parse("https://example.com/b").expect("url 2");

        tree.merge_items(vec![
            BookmarkItem::Link {
                id: "bm-1".into(),
                title: "First".into(),
                url: first_url.clone(),
            },
            BookmarkItem::Folder {
                id: "fld-1".into(),
                title: "Work".into(),
                children: vec![BookmarkItem::Link {
                    id: "bm-2".into(),
                    title: "Second".into(),
                    url: second_url.clone(),
                }],
            },
        ]);
        assert_eq!(tree.total_links(), 2);
        assert!(tree.contains_url(&duplicate_url));

        // Re-merging a duplicate URL at the same level and inside the same folder
        // keeps total_links at 2.
        tree.merge_items(vec![
            BookmarkItem::Link {
                id: "bm-dup".into(),
                title: "First Again".into(),
                url: duplicate_url,
            },
            BookmarkItem::Folder {
                id: "fld-dup".into(),
                title: "Work".into(),
                children: vec![BookmarkItem::Link {
                    id: "bm-2-dup".into(),
                    title: "Second Again".into(),
                    url: second_url,
                }],
            },
        ]);
        assert_eq!(tree.total_links(), 2);
    }

    #[test]
    fn pin_tracks_wandered_state_and_rehomes() {
        let home = PageUrl::parse("https://news.ycombinator.com/").expect("home");
        let away = PageUrl::parse("https://example.com/article").expect("away");
        let mut pin = PinItem::new(PinId::new(), home.clone(), "Hacker News");
        assert_eq!(pin.letter, 'N');
        assert!(!pin.has_wandered());

        pin.url = away.clone();
        assert!(pin.has_wandered());

        pin.go_home();
        assert!(!pin.has_wandered());
        assert_eq!(pin.url, home);

        pin.url = away.clone();
        pin.rehome_to_current();
        assert!(!pin.has_wandered());
        assert_eq!(pin.home, away);
    }

    #[test]
    fn split_item_enforces_pane_bounds_and_share_clamping() {
        let t1 = TabId::new(1);
        let t2 = TabId::new(2);
        let t3 = TabId::new(3);
        let t4 = TabId::new(4);
        let t5 = TabId::new(5);

        assert!(SplitItem::new("s", vec![t1]).is_err());
        assert!(SplitItem::new("s", vec![t1, t1]).is_err());
        assert!(SplitItem::new("s", vec![t1, t2, t3, t4, t5]).is_err());

        let mut split = SplitItem::new("s", vec![t1, t2]).expect("2-pane split");
        split.set_shares(&[0.05, 0.95]).expect("clamp 2-pane");
        assert!((split.shares[0] - 0.20).abs() < 1e-9);
        assert!((split.shares[1] - 0.80).abs() < 1e-9);

        split.swap();
        assert_eq!(split.panes, vec![t2, t1]);
        assert!((split.shares[0] - 0.80).abs() < 1e-9);

        split.add_pane(t3).expect("add 3rd pane");
        assert_eq!(split.panes.len(), 3);
        for share in &split.shares {
            assert!((share - 1.0 / 3.0).abs() < 1e-9);
        }

        let still_split = split.remove_pane(t3).expect("remove 3rd pane");
        assert!(still_split);
        assert_eq!(split.panes.len(), 2);

        let still_split = split.remove_pane(t1).expect("remove 2nd pane");
        assert!(!still_split);
    }

    #[test]
    fn session_migration_upgrades_v0_and_rejects_future_versions() {
        let legacy = SessionFile {
            version: 0,
            active: 0,
            tabs: vec![
                SessionTab {
                    id: "t1".into(),
                    url: "https://example.com/one".into(),
                    title: "One".into(),
                    sleeping: true,
                    ..SessionTab::default()
                },
                SessionTab {
                    id: "t2".into(),
                    url: "https://example.com/two".into(),
                    title: "Two".into(),
                    sleeping: false,
                    ..SessionTab::default()
                },
            ],
            ..SessionFile::default()
        };
        let migrated = SessionShape::migrate_from(&legacy).expect("migrate v0");
        assert_eq!(migrated.version, CURRENT_SCHEMA_VERSION);
        assert!(!migrated.tabs[0].sleeping, "active tab wakes");
        assert!(migrated.tabs[1].sleeping, "inactive tab restores asleep");

        let future = SessionFile {
            version: CURRENT_SCHEMA_VERSION + 1,
            ..SessionFile::default()
        };
        assert!(matches!(
            SessionShape::migrate_from(&future),
            Err(CoreError::UnsupportedVersion { .. })
        ));
    }
}
