//! Pins (`P3-04`), 3-level Pin Folders, and Live RSS/Atom Folders (`P3-05`)
//! (`FEATURES.md` §3.6, §7.1–7.3).
//!
//! Rules (`FEATURES.md` §3.6, §7.1–7.3):
//! - **Pinned grid (`FEATURES.md` §3.6):** up to 4 squares per row, evenly
//!   divided (`5 = 3+2`, `7 = 4+3`, `9 = 3+3+3`, `10 = 4+3+3`).
//! - **Pin peek (`pinsPeek`):** a plain click on a link to another site inside
//!   a pin opens in a peek so the pin stays at its home site; links within the
//!   same site navigate inside the pin.
//! - **⌘-click on a pin:** if the pin has wandered from `home`, the wandered
//!   page becomes a loose tab and the pin resets to `home`; if already at
//!   `home`, opens a new tab at `home`.
//! - **Pin folders (`P3-05`):** nested up to 3 levels (`MAX_PIN_FOLDER_DEPTH = 3`),
//!   names capped at 200 characters. Squares, shared pins (`Keep in Every Space`),
//!   and private tabs cannot enter a folder. A folder cannot move into itself,
//!   a descendant, or a live folder. Deleting a folder lifts its child folders
//!   and pins to the parent without closing any pin.
//! - **Live folders (`P3-05`):** RSS/Atom feed folder holding up to 12 posts,
//!   30-minute refresh cadence, 2 MB payload cap, cookieless/cacheless fetch;
//!   fetch failure preserves existing posts and records a tooltip error.

use serde::{Deserialize, Serialize};

use crate::domain::PinItem;
use crate::error::{CoreError, Result};
use crate::ids::{PinId, SpaceId};
use crate::url::PageUrl;

/// Maximum nesting depth for pin folders (`3 levels`, `FEATURES.md` §7.2).
pub const MAX_PIN_FOLDER_DEPTH: usize = 3;
/// Maximum character length of a pin folder name (`200`, `FEATURES.md` §7.2).
pub const MAX_PIN_FOLDER_NAME_CHARS: usize = 200;
/// Maximum posts kept in a live RSS/Atom folder (`12`, `FEATURES.md` §7.3).
pub const MAX_LIVE_FEED_POSTS: usize = 12;
/// Maximum byte size of an RSS/Atom feed response (`2 MB`, `FEATURES.md` §7.3).
pub const MAX_LIVE_FEED_BYTES: usize = 2 * 1024 * 1024;
/// Refresh interval for live folders while running (`30 minutes`, `FEATURES.md` §7.3).
pub const LIVE_FEED_REFRESH_SECS: u64 = 30 * 60;
/// Initial delay after launch before refreshing live folders (`5 seconds`, `FEATURES.md` §7.3).
pub const LIVE_FEED_LAUNCH_DELAY_SECS: u64 = 5;

/// Compute the even row division for `count` pinned squares (`FEATURES.md` §3.6):
/// up to 4 squares per row, evenly divided (`5 = [3, 2]`, `7 = [4, 3]`,
/// `9 = [3, 3, 3]`, `10 = [4, 3, 3]`).
#[must_use]
pub fn compute_pin_grid_rows(count: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let row_count = count.div_ceil(4);
    let base = count / row_count;
    let remainder = count % row_count;
    (0..row_count)
        .map(|row_idx| base + usize::from(row_idx < remainder))
        .collect()
}

/// Outcome of clicking a link or pressing `⌘-click` on a pin (`FEATURES.md` §7.1, P3-04).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinNavigationAction {
    /// Navigate within the pin itself (same registrable site).
    NavigateInPin(PageUrl),
    /// Open the cross-site link in a Peek panel over the pin (`pinsPeek`).
    OpenInPeek(PageUrl),
    /// `⌘-click` on a wandered pin: spin off `wandered_url` as a loose tab and reset pin to `home_url`.
    SplitWanderedToLooseTab {
        wandered_url: PageUrl,
        home_url: PageUrl,
    },
    /// `⌘-click` on a pin at home: open a fresh loose tab at `home_url`.
    OpenHomeInNewTab(PageUrl),
}

/// Decide how a link click inside `pin` should be handled (`FEATURES.md` §7.1).
#[must_use]
pub fn evaluate_pin_link_click(
    pin: &PinItem,
    target_url: PageUrl,
    pins_peek_enabled: bool,
) -> PinNavigationAction {
    if pins_peek_enabled
        && !pin
            .home
            .site_label()
            .eq_ignore_ascii_case(&target_url.site_label())
    {
        PinNavigationAction::OpenInPeek(target_url)
    } else {
        PinNavigationAction::NavigateInPin(target_url)
    }
}

/// Handle `⌘-click` on `pin`, mutating `pin` back to `home` if it had wandered (`FEATURES.md` §7.1).
pub fn handle_command_click_on_pin(pin: &mut PinItem) -> PinNavigationAction {
    if pin.has_wandered() {
        let wandered = pin.url.clone();
        let home = pin.home.clone();
        pin.go_home();
        PinNavigationAction::SplitWanderedToLooseTab {
            wandered_url: wandered,
            home_url: home,
        }
    } else {
        PinNavigationAction::OpenHomeInNewTab(pin.home.clone())
    }
}

/// One parsed post inside a live RSS/Atom folder (`FEATURES.md` §7.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveFeedPost {
    /// Post title.
    pub title: String,
    /// Post link URL.
    pub url: PageUrl,
    /// Optional publication timestamp string.
    pub published: Option<String>,
}

/// A pin folder or live RSS/Atom folder (`FEATURES.md` §7.2, §7.3, P3-05).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinFolderNode {
    /// Folder ID.
    pub id: String,
    /// Folder name (`<= 200` chars).
    pub name: String,
    /// Whether the folder is currently collapsed in the sidebar.
    pub collapsed: bool,
    /// Member pin IDs (empty for a live folder).
    pub pins: Vec<PinId>,
    /// Nested child folders (depth capped at [`MAX_PIN_FOLDER_DEPTH`] = 3; empty for a live folder).
    pub children: Vec<PinFolderNode>,
    /// When `Some`, this is a Live Folder backed by the given RSS/Atom feed URL.
    pub live_feed_url: Option<PageUrl>,
    /// Latest posts (up to [`MAX_LIVE_FEED_POSTS`] = 12) for a live folder.
    pub live_posts: Vec<LiveFeedPost>,
    /// Tooltip error message if the last feed refresh failed (old posts are kept!).
    pub last_refresh_error: Option<String>,
}

impl PinFolderNode {
    /// Whether this folder is a Live RSS/Atom folder (`FEATURES.md` §7.3).
    #[must_use]
    pub const fn is_live(&self) -> bool {
        self.live_feed_url.is_some()
    }

    /// Maximum folder nesting depth of this subtree (`1` for a leaf folder).
    #[must_use]
    pub fn max_subtree_depth(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(Self::max_subtree_depth)
            .max()
            .unwrap_or(0)
    }
}

/// Parse an RSS 2.0 (`<item>`) or Atom 1.0 (`<entry>`) XML payload into up to
/// 12 [`LiveFeedPost`]s, enforcing the 2 MB payload cap (`FEATURES.md` §7.3, P3-05).
pub fn parse_rss_or_atom_feed(xml_bytes: &[u8]) -> Result<Vec<LiveFeedPost>> {
    if xml_bytes.len() > MAX_LIVE_FEED_BYTES {
        return Err(CoreError::Invalid(format!(
            "live feed response is {} bytes, above the 2 MB cap",
            xml_bytes.len()
        )));
    }
    let text = std::str::from_utf8(xml_bytes)
        .map_err(|e| CoreError::Invalid(format!("feed is not UTF-8: {e}")))?;

    let mut posts = Vec::new();
    let lower = text.to_ascii_lowercase();
    let tag = if lower.contains("<item") {
        "item"
    } else if lower.contains("<entry") {
        "entry"
    } else {
        return Err(CoreError::Invalid(
            "feed contains no RSS <item> or Atom <entry> elements".into(),
        ));
    };

    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let mut cursor = 0_usize;
    while let Some(pos) = lower[cursor..].find(&open_pat) {
        if posts.len() >= MAX_LIVE_FEED_POSTS {
            break;
        }
        let start = cursor + pos;
        let Some(end_offset) = lower[start..].find(&close_pat) else {
            break;
        };
        let block = &text[start..start + end_offset];
        cursor = start + end_offset + close_pat.len();

        let title = extract_xml_tag(block, "title").unwrap_or_else(|| "Untitled".into());
        let raw_url = extract_xml_tag(block, "link")
            .or_else(|| extract_atom_link_href(block))
            .unwrap_or_default();
        if let Ok(url) = PageUrl::parse(&raw_url)
            && matches!(url.scheme(), "http" | "https")
        {
            let published = extract_xml_tag(block, "pubDate")
                .or_else(|| extract_xml_tag(block, "published"))
                .or_else(|| extract_xml_tag(block, "updated"));
            posts.push(LiveFeedPost {
                title,
                url,
                published,
            });
        }
    }

    Ok(posts)
}

fn extract_xml_tag(block: &str, tag: &str) -> Option<String> {
    let lower = block.to_ascii_lowercase();
    let open = format!("<{}", tag.to_ascii_lowercase());
    let close = format!("</{}>", tag.to_ascii_lowercase());
    let start = lower.find(&open)?;
    let gt = lower[start..].find('>')?;
    let inner_start = start + gt + 1;
    let end = lower[inner_start..].find(&close)?;
    let raw = block[inner_start..inner_start + end]
        .trim()
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>")
        .trim();
    (!raw.is_empty()).then(|| raw.to_owned())
}

fn extract_atom_link_href(block: &str) -> Option<String> {
    let lower = block.to_ascii_lowercase();
    let link_pos = lower.find("<link")?;
    let end = lower[link_pos..].find('>')?;
    let tag = &block[link_pos..link_pos + end];
    let href_pos = tag.to_ascii_lowercase().find("href=\"")? + 6;
    let quote_end = tag[href_pos..].find('"')?;
    Some(tag[href_pos..href_pos + quote_end].to_owned())
}

/// Manager for pins, shared pins, 3-level pin folders, and live folders (`P3-04`, `P3-05`).
#[derive(Clone, Debug, Default)]
pub struct PinsAndFoldersManager {
    pins: Vec<PinItem>,
    folders: Vec<PinFolderNode>,
    /// Whether pinned rows (`pins.list`) are enabled (required for pin folders, `FEATURES.md` §7.2).
    pub pinned_rows_enabled: bool,
}

impl PinsAndFoldersManager {
    /// Create a manager.
    #[must_use]
    pub fn new(pinned_rows_enabled: bool) -> Self {
        Self {
            pins: Vec::new(),
            folders: Vec::new(),
            pinned_rows_enabled,
        }
    }

    /// Add a pin (`shared = true` makes it visible in every space after the space's own pins).
    pub fn add_pin(&mut self, mut pin: PinItem) -> PinId {
        if pin.shared {
            pin.space = None;
        }
        let id = pin.id;
        self.pins.push(pin);
        id
    }

    /// Visible pins for `space`, with the space's own pins first followed by
    /// shared (`Keep in Every Space`) pins (`FEATURES.md` §6.1, §7.1).
    #[must_use]
    pub fn pins_for_space(&self, space: SpaceId) -> Vec<&PinItem> {
        let mut out: Vec<&PinItem> = self
            .pins
            .iter()
            .filter(|p| !p.shared && p.space == Some(space))
            .collect();
        out.extend(self.pins.iter().filter(|p| p.shared));
        out
    }

    /// Top-level pin folders.
    #[must_use]
    pub fn folders(&self) -> &[PinFolderNode] {
        &self.folders
    }

    /// Create a normal pin folder inside `parent_folder_id` (`None` = top level),
    /// enforcing `pinned_rows_enabled`, the 200-char name cap, and the 3-level depth cap (`FEATURES.md` §7.2).
    pub fn create_folder(
        &mut self,
        parent_folder_id: Option<&str>,
        id: impl Into<String>,
        name: &str,
    ) -> Result<String> {
        self.create_folder_internal(parent_folder_id, id.into(), name, None)
    }

    /// Create a Live RSS/Atom folder (`FEATURES.md` §7.3).
    pub fn create_live_folder(
        &mut self,
        parent_folder_id: Option<&str>,
        id: impl Into<String>,
        name: &str,
        feed_url: PageUrl,
    ) -> Result<String> {
        self.create_folder_internal(parent_folder_id, id.into(), name, Some(feed_url))
    }

    fn create_folder_internal(
        &mut self,
        parent_folder_id: Option<&str>,
        id: String,
        name: &str,
        live_feed_url: Option<PageUrl>,
    ) -> Result<String> {
        if !self.pinned_rows_enabled {
            return Err(CoreError::Invalid(
                "pin folders require pinned rows (pins.list) to be enabled".into(),
            ));
        }
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.chars().count() > MAX_PIN_FOLDER_NAME_CHARS {
            return Err(CoreError::Invalid(
                "pin folder name must be 1..=200 characters".into(),
            ));
        }

        let node = PinFolderNode {
            id: id.clone(),
            name: trimmed.to_owned(),
            collapsed: false,
            pins: Vec::new(),
            children: Vec::new(),
            live_feed_url,
            live_posts: Vec::new(),
            last_refresh_error: None,
        };

        let Some(parent_id) = parent_folder_id else {
            self.folders.push(node);
            return Ok(id);
        };

        let parent_depth = Self::depth_of(&self.folders, parent_id, 1)
            .ok_or_else(|| CoreError::Invalid(format!("unknown parent folder {parent_id:?}")))?;
        if parent_depth >= MAX_PIN_FOLDER_DEPTH {
            return Err(CoreError::Invalid(
                "pin folders cannot be nested deeper than 3 levels".into(),
            ));
        }
        let parent = Self::find_folder_mut(&mut self.folders, parent_id)
            .ok_or_else(|| CoreError::Invalid("unknown parent folder".into()))?;
        if parent.is_live() {
            return Err(CoreError::Invalid(
                "cannot nest a folder inside a live RSS/Atom folder".into(),
            ));
        }
        parent.children.push(node);
        Ok(id)
    }

    /// Put a pin into `folder_id` (`FEATURES.md` §7.2):
    /// - Squares (`!pinned_rows_enabled`), shared pins (`pin.shared`), and live folders are refused.
    pub fn add_pin_to_folder(&mut self, pin_id: PinId, folder_id: &str) -> Result<()> {
        if !self.pinned_rows_enabled {
            return Err(CoreError::Invalid(
                "pinned squares cannot enter a pin folder".into(),
            ));
        }
        let pin = self
            .pins
            .iter()
            .find(|p| p.id == pin_id)
            .ok_or_else(|| CoreError::Invalid("unknown pin".into()))?;
        if pin.shared {
            return Err(CoreError::Invalid(
                "shared pins (Keep in Every Space) cannot enter a pin folder".into(),
            ));
        }
        let folder = Self::find_folder_mut(&mut self.folders, folder_id)
            .ok_or_else(|| CoreError::Invalid("unknown folder".into()))?;
        if folder.is_live() {
            return Err(CoreError::Invalid(
                "pins cannot be dropped into a live RSS/Atom folder".into(),
            ));
        }
        if !folder.pins.contains(&pin_id) {
            folder.pins.push(pin_id);
        }
        Ok(())
    }

    /// Delete `folder_id`, lifting its child folders and pins to the parent
    /// level without closing or deleting any pins (`FEATURES.md` §7.2).
    pub fn delete_folder_lifting_children(&mut self, folder_id: &str) -> Result<()> {
        if Self::lift_from_list(&mut self.folders, folder_id) {
            Ok(())
        } else {
            Err(CoreError::Invalid(format!("no pin folder {folder_id:?}")))
        }
    }

    fn lift_from_list(list: &mut Vec<PinFolderNode>, target_id: &str) -> bool {
        if let Some(idx) = list.iter().position(|f| f.id == target_id) {
            let mut removed = list.remove(idx);
            for (offset, child) in removed.children.drain(..).enumerate() {
                list.insert(idx + offset, child);
            }
            return true;
        }
        for folder in list {
            if let Some(child_idx) = folder.children.iter().position(|c| c.id == target_id) {
                let mut removed = folder.children.remove(child_idx);
                for pin in removed.pins.drain(..) {
                    if !folder.pins.contains(&pin) {
                        folder.pins.push(pin);
                    }
                }
                for (offset, grandchild) in removed.children.drain(..).enumerate() {
                    folder.children.insert(child_idx + offset, grandchild);
                }
                return true;
            }
            if Self::lift_from_list(&mut folder.children, target_id) {
                return true;
            }
        }
        false
    }

    /// Apply a live-feed refresh result to `folder_id`:
    /// - On `Ok(xml_bytes)`, parses up to 12 posts and clears `last_refresh_error`.
    /// - On `Err(reason)` or parse error, keeps the existing `live_posts` intact
    ///   and records the error in `last_refresh_error` for the heading tooltip (`FEATURES.md` §7.3).
    pub fn apply_live_feed_refresh(
        &mut self,
        folder_id: &str,
        fetch_result: std::result::Result<&[u8], &str>,
    ) -> Result<usize> {
        let folder = Self::find_folder_mut(&mut self.folders, folder_id)
            .ok_or_else(|| CoreError::Invalid("unknown live folder".into()))?;
        if !folder.is_live() {
            return Err(CoreError::Invalid(
                "folder is not a live feed folder".into(),
            ));
        }
        match fetch_result {
            Ok(bytes) => match parse_rss_or_atom_feed(bytes) {
                Ok(posts) => {
                    folder.live_posts = posts;
                    folder.last_refresh_error = None;
                }
                Err(err) => {
                    folder.last_refresh_error = Some(err.to_string());
                }
            },
            Err(network_err) => {
                folder.last_refresh_error = Some(network_err.to_owned());
            }
        }
        Ok(folder.live_posts.len())
    }

    fn depth_of(list: &[PinFolderNode], target_id: &str, current_depth: usize) -> Option<usize> {
        for f in list {
            if f.id == target_id {
                return Some(current_depth);
            }
            if let Some(d) = Self::depth_of(&f.children, target_id, current_depth + 1) {
                return Some(d);
            }
        }
        None
    }

    fn find_folder_mut<'a>(
        list: &'a mut [PinFolderNode],
        target_id: &str,
    ) -> Option<&'a mut PinFolderNode> {
        for f in list {
            if f.id == target_id {
                return Some(f);
            }
            if let Some(found) = Self::find_folder_mut(&mut f.children, target_id) {
                return Some(found);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_even_pin_grid_and_handles_peek_and_command_click() {
        assert_eq!(compute_pin_grid_rows(5), vec![3, 2]);
        assert_eq!(compute_pin_grid_rows(7), vec![4, 3]);
        assert_eq!(compute_pin_grid_rows(9), vec![3, 3, 3]);
        assert_eq!(compute_pin_grid_rows(10), vec![4, 3, 3]);

        let home = PageUrl::parse("https://news.ycombinator.com/").expect("home");
        let same_site = PageUrl::parse("https://news.ycombinator.com/item?id=1").expect("same");
        let external = PageUrl::parse("https://rust-lang.org/").expect("ext");
        let mut pin = PinItem::new(PinId::new(), home.clone(), "HN");

        assert_eq!(
            evaluate_pin_link_click(&pin, same_site.clone(), true),
            PinNavigationAction::NavigateInPin(same_site.clone())
        );
        assert_eq!(
            evaluate_pin_link_click(&pin, external.clone(), true),
            PinNavigationAction::OpenInPeek(external.clone())
        );

        // Wander to external page, then ⌘-click splits wandered page to loose tab and resets pin home.
        pin.url = external.clone();
        assert_eq!(
            handle_command_click_on_pin(&mut pin),
            PinNavigationAction::SplitWanderedToLooseTab {
                wandered_url: external,
                home_url: home.clone(),
            }
        );
        assert!(!pin.has_wandered());
    }

    #[test]
    fn enforces_three_level_folder_depth_refuses_shared_pins_and_parses_live_feeds() {
        let mut mgr = PinsAndFoldersManager::new(true);
        let space = SpaceId::new();

        let mut local_pin = PinItem::new(
            PinId::new(),
            PageUrl::parse("https://example.com/local").expect("url"),
            "Local",
        );
        local_pin.space = Some(space);
        let local_id = mgr.add_pin(local_pin);

        let mut shared_pin = PinItem::new(
            PinId::new(),
            PageUrl::parse("https://example.com/shared").expect("url"),
            "Shared",
        );
        shared_pin.shared = true;
        let shared_id = mgr.add_pin(shared_pin);

        // Shared pins appear after space's own pins.
        let space_pins = mgr.pins_for_space(space);
        assert_eq!(space_pins.len(), 2);
        assert_eq!(space_pins[0].id, local_id);
        assert_eq!(space_pins[1].id, shared_id);

        // 3 levels of folders succeed; 4th level is rejected.
        mgr.create_folder(None, "f1", "Level 1").expect("l1");
        mgr.create_folder(Some("f1"), "f2", "Level 2").expect("l2");
        mgr.create_folder(Some("f2"), "f3", "Level 3").expect("l3");
        assert!(
            mgr.create_folder(Some("f3"), "f4", "Level 4").is_err(),
            "depth > 3 must be rejected"
        );

        // Local pin enters folder; shared pin is refused.
        mgr.add_pin_to_folder(local_id, "f2")
            .expect("add local pin");
        assert!(mgr.add_pin_to_folder(shared_id, "f2").is_err());

        // Deleting f2 lifts f3 and local_id into f1 without closing local_id.
        mgr.delete_folder_lifting_children("f2")
            .expect("delete f2 lifting children");
        assert_eq!(mgr.folders()[0].children[0].id, "f3");
        assert!(mgr.folders()[0].pins.contains(&local_id));

        // Live RSS folder parses up to 12 posts and preserves them on refresh failure.
        let feed_url = PageUrl::parse("https://blog.rust-lang.org/feed.xml").expect("feed");
        mgr.create_live_folder(None, "live-1", "Rust Blog", feed_url)
            .expect("create live folder");

        let mut rss = String::from("<rss><channel>");
        for i in 1..=15 {
            rss.push_str(&format!(
                "<item><title>Post {i}</title><link>https://blog.rust-lang.org/post-{i}</link></item>"
            ));
        }
        rss.push_str("</channel></rss>");

        let count = mgr
            .apply_live_feed_refresh("live-1", Ok(rss.as_bytes()))
            .expect("refresh ok");
        assert_eq!(count, 12, "live feed must cap at 12 posts");

        // Subsequent network error keeps the 12 cached posts and sets tooltip error.
        let count_after_err = mgr
            .apply_live_feed_refresh("live-1", Err("timeout after 10s"))
            .expect("refresh err");
        assert_eq!(count_after_err, 12);
        assert!(mgr.folders()[1].last_refresh_error.is_some());
    }
}
