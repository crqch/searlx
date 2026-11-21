//! Bookmarks tree manager, add/edit/delete/move, bar & lazy menu queries,
//! search-all-words with folder paths, and 1,500-bookmark performance (`FEATURES.md` §9, P2-04).
//!
//! Notice (`FEATURES.md` §9):
//! - If the current page is already bookmarked, `Add This Page` (`⇧⌘B`) becomes
//!   `Edit Bookmark…`.
//! - Deleting a node reports the count of what goes.
//! - Folder drag/drop reorder and nesting prevents moving a folder into itself
//!   or its descendants.
//! - There is **no bookmark export writer** — only readers and importers.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::data::DataStore;
use crate::domain::{BookmarkItem, BookmarkTree};
use crate::error::{CoreError, Result};
use crate::url::PageUrl;

/// State of the `⇧⌘B` bookmark action for the active page (`FEATURES.md` §9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookmarkMenuAction {
    /// Page is not yet bookmarked: show `"Add This Page…"`.
    AddThisPage,
    /// Page is already bookmarked: show `"Edit Bookmark…"` for `id`.
    EditBookmark {
        /// Existing bookmark node ID.
        id: String,
        /// Existing title.
        title: String,
    },
}

/// One search hit on `search://bookmarks` (`FEATURES.md` §9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkSearchHit {
    /// Node ID.
    pub id: String,
    /// Bookmark or folder title.
    pub title: String,
    /// Link URL (`None` for a folder).
    pub url: Option<PageUrl>,
    /// Folder path from `"Top Level"` down to the item's parent folder.
    pub folder_path: Vec<String>,
}

/// Profile bookmark manager (`P2-04`).
#[derive(Clone, Debug, Default)]
pub struct BookmarksManager {
    tree: BookmarkTree,
}

impl BookmarksManager {
    /// Create a manager around an empty [`BookmarkTree`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            tree: BookmarkTree::new(),
        }
    }

    /// Wrap an existing [`BookmarkTree`].
    #[must_use]
    pub const fn from_tree(tree: BookmarkTree) -> Self {
        Self { tree }
    }

    /// Borrow the underlying [`BookmarkTree`].
    #[must_use]
    pub const fn tree(&self) -> &BookmarkTree {
        &self.tree
    }

    /// Load bookmarks from `store`.
    pub fn load_from(store: &DataStore, profile_id: Option<&str>) -> Result<Self> {
        let name = match profile_id {
            Some(id) if !id.is_empty() => format!("bookmarks-{id}.json"),
            _ => "bookmarks.json".to_owned(),
        };
        let file = store
            .read_or_default::<crate::data::BookmarksFile>(&name)?
            .value;
        Ok(Self {
            tree: BookmarkTree::try_from_file(&file)?,
        })
    }

    /// Save bookmarks to `store`.
    pub fn save_to(&self, store: &DataStore, profile_id: Option<&str>) -> Result<()> {
        store.save_bookmarks(profile_id, &self.tree.to_file())
    }

    /// Total count of bookmark links across all folders.
    #[must_use]
    pub fn total_links(&self) -> usize {
        self.tree.total_links()
    }

    /// Determine whether `⇧⌘B` should show `"Add This Page…"` or `"Edit Bookmark…"` (`FEATURES.md` §9).
    #[must_use]
    pub fn menu_action_for(&self, page_url: &PageUrl) -> BookmarkMenuAction {
        let target = page_url.identity();
        if let Some((id, title)) = Self::find_by_identity(&self.tree.roots, &target) {
            BookmarkMenuAction::EditBookmark { id, title }
        } else {
            BookmarkMenuAction::AddThisPage
        }
    }

    fn find_by_identity(items: &[BookmarkItem], target_identity: &str) -> Option<(String, String)> {
        for item in items {
            match item {
                BookmarkItem::Link { id, title, url } => {
                    if url.identity() == target_identity {
                        return Some((id.clone(), title.clone()));
                    }
                }
                BookmarkItem::Folder { children, .. } => {
                    if let Some(found) = Self::find_by_identity(children, target_identity) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// Add a bookmark link inside `parent_folder_id` (`None` adds at `"Top Level"`),
    /// deduplicating if the same URL identity already exists in that folder (`FEATURES.md` §9).
    pub fn add_bookmark(
        &mut self,
        parent_folder_id: Option<&str>,
        title: impl Into<String>,
        url: PageUrl,
    ) -> Result<String> {
        let list = self.folder_children_mut(parent_folder_id)?;
        let identity = url.identity();
        if let Some(BookmarkItem::Link { id, .. }) = list.iter().find(|item| {
            matches!(item, BookmarkItem::Link { url: existing, .. } if existing.identity() == identity)
        }) {
            return Ok(id.clone());
        }
        let id = format!("bm-{}", Uuid::new_v4());
        list.push(BookmarkItem::Link {
            id: id.clone(),
            title: title.into(),
            url,
        });
        Ok(id)
    }

    /// Create a new folder inside `parent_folder_id` (`None` adds at `"Top Level"`).
    pub fn add_folder(
        &mut self,
        parent_folder_id: Option<&str>,
        title: impl Into<String>,
    ) -> Result<String> {
        let list = self.folder_children_mut(parent_folder_id)?;
        let id = format!("fld-{}", Uuid::new_v4());
        list.push(BookmarkItem::Folder {
            id: id.clone(),
            title: title.into(),
            children: Vec::new(),
        });
        Ok(id)
    }

    /// Rename any bookmark or folder by ID.
    pub fn rename(&mut self, id: &str, new_title: impl Into<String>) -> Result<()> {
        let node = Self::find_node_mut(&mut self.tree.roots, id)
            .ok_or_else(|| CoreError::Invalid(format!("no bookmark node {id:?}")))?;
        match node {
            BookmarkItem::Link { title, .. } | BookmarkItem::Folder { title, .. } => {
                *title = new_title.into();
            }
        }
        Ok(())
    }

    /// Edit the address of a bookmark link by ID.
    pub fn edit_address(&mut self, id: &str, new_url: PageUrl) -> Result<()> {
        let node = Self::find_node_mut(&mut self.tree.roots, id)
            .ok_or_else(|| CoreError::Invalid(format!("no bookmark node {id:?}")))?;
        match node {
            BookmarkItem::Link { url, .. } => {
                *url = new_url;
                Ok(())
            }
            BookmarkItem::Folder { .. } => Err(CoreError::Invalid(
                "cannot set a URL on a bookmark folder".into(),
            )),
        }
    }

    /// Remove a bookmark or folder by ID, returning the number of link items removed (`FEATURES.md` §9).
    pub fn remove(&mut self, id: &str) -> Result<usize> {
        let removed = Self::take_node(&mut self.tree.roots, id)
            .ok_or_else(|| CoreError::Invalid(format!("no bookmark node {id:?}")))?;
        Ok(removed.link_count())
    }

    /// Move/reorder a bookmark or folder to `target_folder_id` (`None` = `"Top Level"`)
    /// at `target_index`, rejecting attempts to move a folder into itself or a descendant.
    pub fn move_node(
        &mut self,
        id: &str,
        target_folder_id: Option<&str>,
        target_index: usize,
    ) -> Result<()> {
        if let Some(target_id) = target_folder_id {
            if target_id == id {
                return Err(CoreError::Invalid(
                    "cannot move a folder into itself".into(),
                ));
            }
            if let Some(BookmarkItem::Folder { children, .. }) =
                Self::find_node(&self.tree.roots, id)
                && Self::find_node(children, target_id).is_some()
            {
                return Err(CoreError::Invalid(
                    "cannot move a folder into its own descendant".into(),
                ));
            }
        }

        let node = Self::take_node(&mut self.tree.roots, id)
            .ok_or_else(|| CoreError::Invalid(format!("no bookmark node {id:?}")))?;
        let dest_list = self.folder_children_mut(target_folder_id)?;
        let clamped = target_index.min(dest_list.len());
        dest_list.insert(clamped, node);
        Ok(())
    }

    /// Search bookmarks matching all whitespace-separated words in `query`,
    /// returning each hit with its folder path (`FEATURES.md` §9).
    #[must_use]
    pub fn search_all_words(&self, query: &str) -> Vec<BookmarkSearchHit> {
        let words: Vec<String> = query
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        let mut results = Vec::new();
        let mut path = vec!["Top Level".to_owned()];
        Self::search_recursive(&self.tree.roots, &words, &mut path, &mut results);
        results
    }

    fn search_recursive(
        items: &[BookmarkItem],
        words: &[String],
        path: &mut Vec<String>,
        out: &mut Vec<BookmarkSearchHit>,
    ) {
        for item in items {
            match item {
                BookmarkItem::Link { id, title, url } => {
                    let hay_title = title.to_ascii_lowercase();
                    let hay_url = url.as_str().to_ascii_lowercase();
                    if words
                        .iter()
                        .all(|w| hay_title.contains(w) || hay_url.contains(w))
                    {
                        out.push(BookmarkSearchHit {
                            id: id.clone(),
                            title: title.clone(),
                            url: Some(url.clone()),
                            folder_path: path.clone(),
                        });
                    }
                }
                BookmarkItem::Folder {
                    id,
                    title,
                    children,
                } => {
                    let hay_title = title.to_ascii_lowercase();
                    if !words.is_empty() && words.iter().all(|w| hay_title.contains(w)) {
                        out.push(BookmarkSearchHit {
                            id: id.clone(),
                            title: title.clone(),
                            url: None,
                            folder_path: path.clone(),
                        });
                    }
                    path.push(title.clone());
                    Self::search_recursive(children, words, path, out);
                    path.pop();
                }
            }
        }
    }

    /// Items shown on the Bookmarks bar: if the top level is a single folder
    /// (such as `"Bookmarks Bar"`), returns its children; otherwise the top-level roots (`FEATURES.md` §9).
    #[must_use]
    pub fn bar_items(&self) -> &[BookmarkItem] {
        if self.tree.roots.len() == 1
            && let BookmarkItem::Folder { children, .. } = &self.tree.roots[0]
        {
            return children;
        }
        &self.tree.roots
    }

    fn folder_children_mut(
        &mut self,
        parent_folder_id: Option<&str>,
    ) -> Result<&mut Vec<BookmarkItem>> {
        let Some(folder_id) = parent_folder_id else {
            return Ok(&mut self.tree.roots);
        };
        let node = Self::find_node_mut(&mut self.tree.roots, folder_id)
            .ok_or_else(|| CoreError::Invalid(format!("no bookmark folder {folder_id:?}")))?;
        match node {
            BookmarkItem::Folder { children, .. } => Ok(children),
            BookmarkItem::Link { .. } => Err(CoreError::Invalid(format!(
                "{folder_id:?} is a bookmark link, not a folder"
            ))),
        }
    }

    fn find_node<'a>(items: &'a [BookmarkItem], id: &str) -> Option<&'a BookmarkItem> {
        for item in items {
            if item.id() == id {
                return Some(item);
            }
            if let BookmarkItem::Folder { children, .. } = item
                && let Some(found) = Self::find_node(children, id)
            {
                return Some(found);
            }
        }
        None
    }

    fn find_node_mut<'a>(items: &'a mut [BookmarkItem], id: &str) -> Option<&'a mut BookmarkItem> {
        for item in items {
            if item.id() == id {
                return Some(item);
            }
            if let BookmarkItem::Folder { children, .. } = item
                && let Some(found) = Self::find_node_mut(children, id)
            {
                return Some(found);
            }
        }
        None
    }

    fn take_node(items: &mut Vec<BookmarkItem>, id: &str) -> Option<BookmarkItem> {
        if let Some(pos) = items.iter().position(|item| item.id() == id) {
            return Some(items.remove(pos));
        }
        for item in items {
            if let BookmarkItem::Folder { children, .. } = item
                && let Some(removed) = Self::take_node(children, id)
            {
                return Some(removed);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn fifteen_hundred_imported_bookmarks_open_search_and_reorder_fast() {
        let started = Instant::now();
        let mut mgr = BookmarksManager::new();
        let root_folder = mgr.add_folder(None, "Imported").expect("root folder");
        let mut sub_folders = Vec::new();
        for f in 0..15_usize {
            let sub = mgr
                .add_folder(Some(&root_folder), format!("Section {f}"))
                .expect("subfolder");
            for i in 0..100_usize {
                let idx = f * 100 + i;
                let url = PageUrl::parse(&format!("https://example.com/section-{f}/doc-{idx}"))
                    .expect("url");
                mgr.add_bookmark(Some(&sub), format!("Document {idx} Rust Guide"), url)
                    .expect("add bookmark");
            }
            sub_folders.push(sub);
        }

        assert_eq!(mgr.total_links(), 1_500);

        let hits = mgr.search_all_words("document 1499 rust");
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].folder_path,
            vec!["Top Level", "Imported", "Section 14"]
        );

        // Moving a folder into its own descendant is rejected.
        assert!(
            mgr.move_node(&root_folder, Some(&sub_folders[0]), 0)
                .is_err()
        );

        // Reordering Section 14 to index 0 under Imported succeeds.
        mgr.move_node(&sub_folders[14], Some(&root_folder), 0)
            .expect("reorder folder");
        assert_eq!(mgr.bar_items()[0].title(), "Section 14");

        // Removing Section 14 reports 100 removed links.
        let removed_count = mgr.remove(&sub_folders[14]).expect("remove folder");
        assert_eq!(removed_count, 100);
        assert_eq!(mgr.total_links(), 1_400);

        let elapsed = started.elapsed();
        assert!(
            elapsed.as_millis() < 200,
            "1,500 bookmarks took {elapsed:?}"
        );
    }

    #[test]
    fn menu_action_switches_to_edit_when_page_is_already_bookmarked() {
        let mut mgr = BookmarksManager::new();
        let url = PageUrl::parse("https://example.com/article#top").expect("url");
        assert_eq!(mgr.menu_action_for(&url), BookmarkMenuAction::AddThisPage);

        let id = mgr.add_bookmark(None, "Article", url.clone()).expect("add");
        let same_identity = PageUrl::parse("https://example.com/article#bottom").expect("url 2");
        assert_eq!(
            mgr.menu_action_for(&same_identity),
            BookmarkMenuAction::EditBookmark {
                id: id.clone(),
                title: "Article".into()
            }
        );

        mgr.rename(&id, "Updated Article").expect("rename");
        let new_url = PageUrl::parse("https://example.com/updated").expect("new url");
        mgr.edit_address(&id, new_url.clone()).expect("edit url");
        assert!(mgr.tree().contains_url(&new_url));
    }
}
