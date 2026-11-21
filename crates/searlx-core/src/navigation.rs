//! Per-tab navigation history, redirects, long-press history menus, internal-page
//! trails, scroll restoration, and in-page error states (`FEATURES.md` §4.5, §26, P2-01).

use serde::{Deserialize, Serialize};

use crate::url::PageUrl;

/// Direction for history navigation and long-press/swipe-hold menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavDirection {
    /// Back in history.
    Back,
    /// Forward in history.
    Forward,
}

/// One entry in a tab's session history stack (`FEATURES.md` §4.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavEntry {
    /// Committed page URL.
    pub url: PageUrl,
    /// Page title at commit/finish.
    pub title: String,
    /// Saved scroll offset `(x, y)` for scroll restoration on back/forward.
    pub scroll: (f64, f64),
    /// If this entry was reached via an HTTP redirect, the original URL.
    pub redirected_from: Option<PageUrl>,
}

/// In-page navigation error model (`FEATURES.md` §3.5, §4.5, P2-01).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InPageError {
    /// Failed destination URL.
    pub url: PageUrl,
    /// Human-readable failure reason (redacted of any query/fragment secrets).
    pub reason: String,
    /// Number of consecutive failures for this load.
    pub failure_count: u32,
    /// Whether the page should offer a Retry button.
    pub can_retry: bool,
}

/// Per-tab navigation controller (`P2-01`).
#[derive(Clone, Debug, Default)]
pub struct TabNavigation {
    entries: Vec<NavEntry>,
    current: usize,
    internal_trail: Vec<PageUrl>,
    internal_index: usize,
    loading: bool,
    in_page_error: Option<InPageError>,
    is_small_window: bool,
}

impl TabNavigation {
    /// Maximum items shown in a swipe-hold history pick-list (`FEATURES.md` §4.5).
    pub const SWIPE_HOLD_MAX_ITEMS: usize = 8;

    /// Create an empty navigation stack.
    #[must_use]
    pub fn new(is_small_window: bool) -> Self {
        Self {
            entries: Vec::new(),
            current: 0,
            internal_trail: Vec::new(),
            internal_index: 0,
            loading: false,
            in_page_error: None,
            is_small_window,
        }
    }

    /// All committed web/page history entries in this tab.
    #[must_use]
    pub fn entries(&self) -> &[NavEntry] {
        &self.entries
    }

    /// Current entry index.
    #[must_use]
    pub const fn current_index(&self) -> usize {
        self.current
    }

    /// Current committed entry, if any.
    #[must_use]
    pub fn current_entry(&self) -> Option<&NavEntry> {
        self.entries.get(self.current)
    }

    /// Current in-page error state, if the last load failed.
    #[must_use]
    pub const fn in_page_error(&self) -> Option<&InPageError> {
        self.in_page_error.as_ref()
    }

    /// Whether a load is currently in flight.
    #[must_use]
    pub const fn is_loading(&self) -> bool {
        self.loading
    }

    /// Whether the tab can go back.
    #[must_use]
    pub fn can_go_back(&self) -> bool {
        self.current > 0
    }

    /// Whether the tab can go forward.
    #[must_use]
    pub fn can_go_forward(&self) -> bool {
        !self.entries.is_empty() && self.current + 1 < self.entries.len()
    }

    /// Record the current page's scroll offset before navigating away.
    pub fn save_current_scroll(&mut self, x: f64, y: f64) {
        if let Some(entry) = self.entries.get_mut(self.current) {
            entry.scroll = (x, y);
        }
    }

    /// Commit a new navigation (`url`, `title`).
    ///
    /// Internal `search://` pages are also tracked in the separate internal-page
    /// trail (`FEATURES.md` §4.5, §26).
    pub fn commit_navigation(&mut self, url: PageUrl, title: impl Into<String>) {
        self.loading = false;
        self.in_page_error = None;
        let title = title.into();

        if url.is_internal() {
            if !self.internal_trail.is_empty() {
                self.internal_trail.truncate(self.internal_index + 1);
            }
            self.internal_trail.push(url.clone());
            self.internal_index = self.internal_trail.len() - 1;
        }

        if !self.entries.is_empty() {
            self.entries.truncate(self.current + 1);
        }
        self.entries.push(NavEntry {
            url,
            title,
            scroll: (0.0, 0.0),
            redirected_from: None,
        });
        self.current = self.entries.len() - 1;
    }

    /// Replace the current entry on an automatic HTTP redirect so that `Back`
    /// returns to the page before the redirect rather than getting trapped (`P2-01`).
    pub fn commit_redirect(&mut self, from: PageUrl, to: PageUrl, title: impl Into<String>) {
        self.loading = false;
        self.in_page_error = None;
        let title = title.into();
        if let Some(current) = self.entries.get_mut(self.current)
            && current.url.identity() == from.identity()
        {
            current.redirected_from = Some(from);
            current.url = to;
            current.title = title;
            return;
        }
        self.entries.push(NavEntry {
            url: to,
            title,
            scroll: (0.0, 0.0),
            redirected_from: Some(from),
        });
        self.current = self.entries.len() - 1;
    }

    /// Step back one entry.
    ///
    /// In a normal tab, Back from the first page returns `None` and does nothing.
    /// Callers can check [`TabNavigation::should_close_small_window_on_back`] for
    /// small windows (`FEATURES.md` §4.5).
    pub fn go_back(&mut self) -> Option<&NavEntry> {
        if !self.can_go_back() {
            return None;
        }
        self.in_page_error = None;
        self.current -= 1;
        self.entries.get(self.current)
    }

    /// Whether pressing Back at the first page should close this window
    /// (true only for a small window at index 0; false for tabs, `FEATURES.md` §4.5).
    #[must_use]
    pub fn should_close_small_window_on_back(&self) -> bool {
        self.is_small_window && !self.can_go_back()
    }

    /// Step forward one entry.
    pub fn go_forward(&mut self) -> Option<&NavEntry> {
        if !self.can_go_forward() {
            return None;
        }
        self.in_page_error = None;
        self.current += 1;
        self.entries.get(self.current)
    }

    /// Jump directly to `target_index` from a long-press or swipe-hold history menu.
    pub fn jump_to(&mut self, target_index: usize) -> Option<&NavEntry> {
        if target_index >= self.entries.len() {
            return None;
        }
        self.in_page_error = None;
        self.current = target_index;
        self.entries.get(self.current)
    }

    /// Build the history menu items in `direction` (`FEATURES.md` §4.5):
    /// - `limit = None`: full long-press button menu (nearest first).
    /// - `limit = Some(8)`: swipe-hold menu capped at 8 items.
    #[must_use]
    pub fn history_menu(
        &self,
        direction: NavDirection,
        limit: Option<usize>,
    ) -> Vec<(usize, &NavEntry)> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        let max_items = limit.unwrap_or(usize::MAX);
        match direction {
            NavDirection::Back => (0..self.current)
                .rev()
                .take(max_items)
                .filter_map(|idx| self.entries.get(idx).map(|e| (idx, e)))
                .collect(),
            NavDirection::Forward => ((self.current + 1)..self.entries.len())
                .take(max_items)
                .filter_map(|idx| self.entries.get(idx).map(|e| (idx, e)))
                .collect(),
        }
    }

    /// Navigate back within the internal `search://` trail (`FEATURES.md` §4.5, §26).
    pub fn internal_back(&mut self) -> Option<&PageUrl> {
        if self.internal_index == 0 || self.internal_trail.is_empty() {
            return None;
        }
        self.internal_index -= 1;
        self.internal_trail.get(self.internal_index)
    }

    /// Navigate forward within the internal `search://` trail.
    pub fn internal_forward(&mut self) -> Option<&PageUrl> {
        if self.internal_trail.is_empty() || self.internal_index + 1 >= self.internal_trail.len() {
            return None;
        }
        self.internal_index += 1;
        self.internal_trail.get(self.internal_index)
    }

    /// Start or reload a load (`from_origin` tracks `⌥⌘R`).
    pub fn begin_load(&mut self) {
        self.loading = true;
        self.in_page_error = None;
    }

    /// Stop an in-flight load (`⌘.`).
    pub fn stop_load(&mut self) {
        self.loading = false;
    }

    /// Record a navigation failure as an in-page error (`FEATURES.md` §3.5, P2-01).
    pub fn fail_navigation(&mut self, url: PageUrl, reason: impl Into<String>) -> &InPageError {
        self.loading = false;
        let count = self
            .in_page_error
            .as_ref()
            .filter(|prev| prev.url.identity() == url.identity())
            .map_or(1, |prev| prev.failure_count.saturating_add(1));
        self.in_page_error = Some(InPageError {
            url,
            reason: reason.into(),
            failure_count: count,
            can_retry: true,
        });
        self.in_page_error.as_ref().expect("just set")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifty_link_session_restores_scroll_and_back_from_first_does_nothing() {
        let mut nav = TabNavigation::new(false);
        assert!(nav.go_back().is_none());
        assert!(!nav.should_close_small_window_on_back());

        for i in 0..50_usize {
            if i > 0 {
                nav.save_current_scroll(0.0, (i as f64) * 50.0);
            }
            let url = PageUrl::parse(&format!("https://example.com/page-{i}")).expect("url");
            nav.commit_navigation(url, format!("Page {i}"));
        }
        assert_eq!(nav.entries().len(), 50);
        assert_eq!(nav.current_index(), 49);

        // Swipe-hold back menu returns the 8 nearest back pages.
        let back_menu = nav.history_menu(
            NavDirection::Back,
            Some(TabNavigation::SWIPE_HOLD_MAX_ITEMS),
        );
        assert_eq!(back_menu.len(), 8);
        assert_eq!(back_menu[0].0, 48);
        assert_eq!(back_menu[7].0, 41);

        // Walk all the way back to index 0, verifying scroll restoration.
        for expected_idx in (0..49_usize).rev() {
            let entry = nav.go_back().expect("step back");
            assert_eq!(
                entry.url.as_str(),
                format!("https://example.com/page-{expected_idx}")
            );
            assert_eq!(entry.scroll, (0.0, ((expected_idx + 1) as f64) * 50.0));
        }

        // At index 0, Back does nothing in a tab.
        assert_eq!(nav.current_index(), 0);
        assert!(nav.go_back().is_none());
        assert_eq!(nav.current_index(), 0);

        // Jump forward via long-press menu to page 25.
        let jumped = nav.jump_to(25).expect("jump to 25");
        assert_eq!(jumped.url.as_str(), "https://example.com/page-25");
    }

    #[test]
    fn redirects_collapse_and_errors_are_in_page() {
        let mut nav = TabNavigation::new(true);
        assert!(nav.should_close_small_window_on_back());

        let first = PageUrl::parse("https://example.com/start").expect("start");
        let bounce = PageUrl::parse("https://example.com/redirect").expect("bounce");
        let final_url = PageUrl::parse("https://example.com/final").expect("final");

        nav.commit_navigation(first.clone(), "Start");
        nav.commit_navigation(bounce.clone(), "Redirecting");
        nav.commit_redirect(bounce.clone(), final_url.clone(), "Final");

        assert_eq!(nav.entries().len(), 2);
        assert_eq!(nav.current_entry().expect("cur").url, final_url);
        assert_eq!(
            nav.current_entry().expect("cur").redirected_from,
            Some(bounce)
        );
        // Back goes straight to `start`, not the redirect bounce.
        assert_eq!(nav.go_back().expect("back").url, first);

        // In-page error state records failure count and offers retry.
        let broken = PageUrl::parse("https://unresolvable.example/").expect("broken");
        let err = nav.fail_navigation(broken.clone(), "DNS lookup failed");
        assert_eq!(err.failure_count, 1);
        assert!(err.can_retry);
        let err2 = nav.fail_navigation(broken, "DNS lookup failed");
        assert_eq!(err2.failure_count, 2);
    }
}
