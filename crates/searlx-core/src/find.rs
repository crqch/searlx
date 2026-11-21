//! Find on page controller, status strings, 1,000-match cap, PDF fallback, and
//! isolated-world script (`FEATURES.md` §5.1, P2-02).
//!
//! Rules (`FEATURES.md` §5.1):
//! - Floating pill shows `"3 of 17"` count; when matches exceed 1,000, counts
//!   at most 1,000 and indicates `"more"` (`"1 of 1,000+"`).
//! - Match case and whole words options; whole words is unavailable on PDFs
//!   (falls back to native find without whole-words).
//! - `⌘F` on the Settings tab (`search://settings*`) routes to Settings search
//!   instead of page find.
//! - Results are computed in Search's isolated script world so a page cannot
//!   tamper with `Range`, `RegExp`, or `getSelection`.

use serde::{Deserialize, Serialize};

use crate::url::PageUrl;

/// Hard cap on matches counted per find pass (`FEATURES.md` §5.1).
pub const MAX_FIND_MATCHES: u32 = 1_000;

/// What `⌘F` targets for the current tab (`FEATURES.md` §5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindTarget {
    /// Normal web page or PDF document.
    Page {
        /// Whether the document is a PDF (disables `whole_words`).
        is_pdf: bool,
    },
    /// The `search://settings` tab, where `⌘F` focuses the Settings search field.
    SettingsSearch,
}

impl FindTarget {
    /// Classify a [`PageUrl`] into its find target.
    #[must_use]
    pub fn for_url(url: &PageUrl) -> Self {
        if url.is_internal() && url.display().starts_with("search://settings") {
            return Self::SettingsSearch;
        }
        let is_pdf = url.as_url().path().to_ascii_lowercase().ends_with(".pdf");
        Self::Page { is_pdf }
    }
}

/// Find-on-page session state (`P2-02`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindSession {
    /// Whether the find pill is visible.
    pub open: bool,
    /// Current search query.
    pub query: String,
    /// Match case switch.
    pub case_sensitive: bool,
    /// Whole words switch (ignored when `is_pdf` is true).
    pub whole_words: bool,
    /// Whether the current page is a PDF document.
    pub is_pdf: bool,
    /// Number of matches counted (capped at [`MAX_FIND_MATCHES`]).
    pub matches: u32,
    /// Whether more than [`MAX_FIND_MATCHES`] matches exist on the page.
    pub more_than_limit: bool,
    /// One-based active match index (`0` when `matches == 0`).
    pub active: u32,
}

impl FindSession {
    /// Create a find session for a page (`is_pdf` disables `whole_words`).
    #[must_use]
    pub fn new(is_pdf: bool) -> Self {
        Self {
            is_pdf,
            ..Self::default()
        }
    }

    /// Whether the whole-words toggle is available (`!self.is_pdf`, `FEATURES.md` §5.1).
    #[must_use]
    pub const fn whole_words_available(&self) -> bool {
        !self.is_pdf
    }

    /// Effective whole-words flag passed to the engine (`false` on PDFs).
    #[must_use]
    pub const fn effective_whole_words(&self) -> bool {
        self.whole_words && !self.is_pdf
    }

    /// Update query and run the pure matcher against `page_text`.
    pub fn search_in_text(&mut self, query: impl Into<String>, page_text: &str) {
        self.open = true;
        self.query = query.into();
        let (count, capped) = count_matches_in_text(
            page_text,
            &self.query,
            self.case_sensitive,
            self.effective_whole_words(),
        );
        self.matches = count;
        self.more_than_limit = capped;
        self.active = u32::from(count > 0);
    }

    /// Apply match counts reported by an engine backend.
    pub fn apply_engine_count(&mut self, total_found: u32, active_index: u32) {
        if self.query.is_empty() {
            self.matches = 0;
            self.more_than_limit = false;
            self.active = 0;
            return;
        }
        if total_found > MAX_FIND_MATCHES {
            self.matches = MAX_FIND_MATCHES;
            self.more_than_limit = true;
        } else {
            self.matches = total_found;
            self.more_than_limit = false;
        }
        self.active = if self.matches == 0 {
            0
        } else {
            active_index.clamp(1, self.matches)
        };
    }

    /// Advance to the next match (`Return` or `⌘G`), wrapping at the end (`FEATURES.md` §5.1).
    pub fn find_next(&mut self) -> u32 {
        if self.matches == 0 {
            self.active = 0;
        } else if self.active >= self.matches {
            self.active = 1;
        } else {
            self.active += 1;
        }
        self.active
    }

    /// Move to the previous match (`⇧Return` or `⇧⌘G`), wrapping at the start (`FEATURES.md` §5.1).
    pub fn find_previous(&mut self) -> u32 {
        if self.matches == 0 {
            self.active = 0;
        } else if self.active <= 1 {
            self.active = self.matches;
        } else {
            self.active -= 1;
        }
        self.active
    }

    /// Format the find pill status string (`FEATURES.md` §5.1, P2-02):
    /// - Empty query: `""`
    /// - 0 matches: `"0 of 0"`
    /// - Within cap: `"3 of 17"`
    /// - Above 1,000 cap: `"3 of 1,000+"`
    #[must_use]
    pub fn status_string(&self) -> String {
        if self.query.is_empty() {
            return String::new();
        }
        if self.matches == 0 {
            return "0 of 0".to_owned();
        }
        if self.more_than_limit {
            format!("{} of 1,000+", self.active)
        } else {
            format!("{} of {}", self.active, self.matches)
        }
    }
}

/// Count matches in `text` up to [`MAX_FIND_MATCHES`], returning `(capped_count, more_than_limit)`.
#[must_use]
pub fn count_matches_in_text(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_words: bool,
) -> (u32, bool) {
    if query.is_empty() {
        return (0, false);
    }
    let haystack_storage;
    let needle_storage;
    let (haystack, needle) = if case_sensitive {
        (text, query)
    } else {
        haystack_storage = text.to_lowercase();
        needle_storage = query.to_lowercase();
        (haystack_storage.as_str(), needle_storage.as_str())
    };
    if needle.is_empty() {
        return (0, false);
    }

    let mut count = 0_u32;
    let mut start = 0_usize;
    while let Some(pos) = haystack[start..].find(needle) {
        let match_start = start + pos;
        let match_end = match_start + needle.len();
        let word_ok = !whole_words || is_whole_word_boundary(haystack, match_start, match_end);
        if word_ok {
            if count == MAX_FIND_MATCHES {
                return (MAX_FIND_MATCHES, true);
            }
            count += 1;
        }
        start = match_end;
    }
    (count, false)
}

fn is_whole_word_boundary(text: &str, start: usize, end: usize) -> bool {
    let before_is_word = text[..start]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    let after_is_word = text[end..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    !before_is_word && !after_is_word
}

/// Isolated-world JavaScript source for computing page matches without relying
/// on page-mutable globals (`FEATURES.md` §5.1).
pub const ISOLATED_WORLD_FIND_JS: &str = r#"(() => {
    const MAX_MATCHES = 1000;
    window.__searlxFind = function(query, caseSensitive, wholeWords) {
        if (!query) return JSON.stringify({ matches: 0, more: false });
        const text = document.body ? (document.body.innerText || "") : "";
        const hay = caseSensitive ? text : text.toLowerCase();
        const needle = caseSensitive ? query : query.toLowerCase();
        let count = 0;
        let idx = 0;
        while ((idx = hay.indexOf(needle, idx)) !== -1) {
            count += 1;
            if (count > MAX_MATCHES) {
                return JSON.stringify({ matches: MAX_MATCHES, more: true });
            }
            idx += needle.length;
        }
        return JSON.stringify({ matches: count, more: false });
    };
})()"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn formats_status_strings_and_wraps_next_prev() {
        let mut find = FindSession::new(false);
        assert_eq!(find.status_string(), "");

        let text = (0..17)
            .map(|i| format!("token-{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        find.search_in_text("token", &text);
        assert_eq!(find.matches, 17);
        assert_eq!(find.active, 1);
        assert_eq!(find.status_string(), "1 of 17");

        find.find_next();
        find.find_next();
        assert_eq!(find.status_string(), "3 of 17");

        find.active = 17;
        assert_eq!(find.find_next(), 1);
        assert_eq!(find.find_previous(), 17);

        find.search_in_text("missing", &text);
        assert_eq!(find.status_string(), "0 of 0");
    }

    #[test]
    fn caps_at_one_thousand_matches_and_stays_fast() {
        let big_page = "searlx ".repeat(2_500);
        let started = Instant::now();
        let mut find = FindSession::new(false);
        find.search_in_text("searlx", &big_page);
        let elapsed = started.elapsed();

        assert!(
            elapsed.as_millis() < 25,
            "1,000-match find took {elapsed:?}"
        );
        assert_eq!(find.matches, 1_000);
        assert!(find.more_than_limit);
        assert_eq!(find.status_string(), "1 of 1,000+");
    }

    #[test]
    fn enforces_whole_words_and_pdf_fallback_and_settings_target() {
        let text = "cat scatter catch cat.";
        let mut find = FindSession::new(false);
        find.whole_words = true;
        find.search_in_text("cat", text);
        assert_eq!(find.matches, 2);

        // On a PDF, whole_words is disabled (`FEATURES.md` §5.1).
        let mut pdf_find = FindSession::new(true);
        pdf_find.whole_words = true;
        assert!(!pdf_find.whole_words_available());
        pdf_find.search_in_text("cat", text);
        assert_eq!(pdf_find.matches, 4);

        let settings_url = PageUrl::parse("search://settings/privacy").expect("settings url");
        assert_eq!(
            FindTarget::for_url(&settings_url),
            FindTarget::SettingsSearch
        );
        let pdf_url = PageUrl::parse("https://example.com/paper.pdf").expect("pdf url");
        assert_eq!(
            FindTarget::for_url(&pdf_url),
            FindTarget::Page { is_pdf: true }
        );
    }
}
