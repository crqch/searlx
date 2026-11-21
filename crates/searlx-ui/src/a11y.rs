//! Explicit focus model and chrome accessibility tree (`AGENTS.md` §3.3, P2-20).
//!
//! Rule (`AGENTS.md` §3.3):
//! > Use one focus model: the page that has the keyboard, the chrome field, or
//! > a panel. Make it explicit and testable.

/// Which surface currently owns keyboard focus (`AGENTS.md` §3.3, P2-20).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    /// The active web page (or focused split pane `0..4`).
    Page {
        /// Focused split pane index (`0` when not split).
        pane: usize,
    },
    /// The address / search field.
    AddressField,
    /// The tab column / strip at `tab_index`.
    TabList {
        /// Focused tab index in the row.
        tab_index: usize,
    },
    /// The floating find-on-page pill (`⌘F`).
    FindBar,
    /// The site card (`Click active tab`).
    SiteCard,
    /// A link peek panel (`Shift-click`).
    PeekPanel,
    /// The side panel.
    SidePanel,
    /// The `⌃Tab` recent-tab switcher.
    TabSwitcher,
}

impl Default for FocusTarget {
    fn default() -> Self {
        Self::Page { pane: 0 }
    }
}

/// Explicit keyboard focus controller (`AGENTS.md` §3.3, `FEATURES.md` §22, P2-20).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusController {
    current: FocusTarget,
    last_page_pane: usize,
}

impl FocusController {
    /// Create a focus controller starting on the primary page pane.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current focus owner.
    #[must_use]
    pub const fn current(&self) -> FocusTarget {
        self.current
    }

    /// Focus the address field (`⌘L`).
    pub fn focus_address_field(&mut self) {
        self.remember_pane();
        self.current = FocusTarget::AddressField;
    }

    /// Focus a specific page pane (`0..4`).
    pub fn focus_page_pane(&mut self, pane: usize) {
        self.last_page_pane = pane;
        self.current = FocusTarget::Page { pane };
    }

    /// Focus a panel or overlay (`FindBar`, `SiteCard`, `PeekPanel`, `SidePanel`, `TabSwitcher`).
    pub fn focus_target(&mut self, target: FocusTarget) {
        self.remember_pane();
        self.current = target;
    }

    /// Move focus through the keyboard accessibility ring (`TabList -> AddressField -> Page`).
    pub fn cycle_chrome_focus(&mut self, tab_count: usize) -> FocusTarget {
        let max_tab = tab_count.saturating_sub(1);
        self.current = match self.current {
            FocusTarget::Page { pane } => {
                self.last_page_pane = pane;
                FocusTarget::TabList { tab_index: 0 }
            }
            FocusTarget::TabList { tab_index } if tab_index < max_tab => FocusTarget::TabList {
                tab_index: tab_index + 1,
            },
            FocusTarget::TabList { .. } => FocusTarget::AddressField,
            FocusTarget::AddressField
            | FocusTarget::FindBar
            | FocusTarget::SiteCard
            | FocusTarget::PeekPanel
            | FocusTarget::SidePanel
            | FocusTarget::TabSwitcher => FocusTarget::Page {
                pane: self.last_page_pane,
            },
        };
        self.current
    }

    /// Handle `Escape`: dismisses any focused overlay (`FindBar`, `PeekPanel`,
    /// `SiteCard`, `AddressField`, `TabSwitcher`) and returns keyboard focus to
    /// the page (`FEATURES.md` §22).
    pub fn dismiss_overlay_to_page(&mut self) -> FocusTarget {
        self.current = FocusTarget::Page {
            pane: self.last_page_pane,
        };
        self.current
    }

    fn remember_pane(&mut self) {
        if let FocusTarget::Page { pane } = self.current {
            self.last_page_pane = pane;
        }
    }
}

/// Semantic accessibility role for a chrome node (`P2-20`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yRole {
    /// Root browser window.
    Window,
    /// Tab strip or sidebar column.
    TabList,
    /// Individual tab or pin item.
    Tab,
    /// Address and search input box.
    SearchBox,
    /// Action button (`New Tab`, `Close Tab`).
    Button,
    /// Main web page content area.
    WebArea,
}

/// One node in the chrome accessibility tree (`P2-20`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A11yNode {
    /// Semantic role.
    pub role: A11yRole,
    /// Accessible name read by screen readers.
    pub name: String,
    /// Current value (e.g. typed text or page URL).
    pub value: Option<String>,
    /// Whether this node is selected (e.g. active tab).
    pub selected: bool,
    /// Whether this node currently holds keyboard focus.
    pub focused: bool,
}

/// Build a flat, ordered accessibility tree for the window chrome and active page (`P2-20`).
#[must_use]
pub fn build_chrome_a11y_tree(
    tabs: &[(String, String)],
    active_tab: usize,
    field_text: &str,
    focus: FocusTarget,
) -> Vec<A11yNode> {
    let mut nodes = Vec::with_capacity(tabs.len() + 4);
    nodes.push(A11yNode {
        role: A11yRole::Window,
        name: "searlx".into(),
        value: None,
        selected: false,
        focused: false,
    });
    nodes.push(A11yNode {
        role: A11yRole::TabList,
        name: "Tabs".into(),
        value: Some(format!("{} tabs", tabs.len())),
        selected: false,
        focused: matches!(focus, FocusTarget::TabList { .. }),
    });
    for (index, (title, url)) in tabs.iter().enumerate() {
        nodes.push(A11yNode {
            role: A11yRole::Tab,
            name: title.clone(),
            value: Some(url.clone()),
            selected: index == active_tab,
            focused: focus == FocusTarget::TabList { tab_index: index },
        });
    }
    nodes.push(A11yNode {
        role: A11yRole::Button,
        name: "New Tab".into(),
        value: None,
        selected: false,
        focused: false,
    });
    nodes.push(A11yNode {
        role: A11yRole::SearchBox,
        name: "Address and search".into(),
        value: Some(field_text.to_owned()),
        selected: false,
        focused: focus == FocusTarget::AddressField,
    });
    let active_title = tabs
        .get(active_tab)
        .map_or_else(|| "Page".to_owned(), |(t, _)| t.clone());
    let active_url = tabs.get(active_tab).map(|(_, u)| u.clone());
    nodes.push(A11yNode {
        role: A11yRole::WebArea,
        name: active_title,
        value: active_url,
        selected: true,
        focused: matches!(focus, FocusTarget::Page { .. }),
    });
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_cycles_through_tabs_field_and_page_and_escape_returns_to_page() {
        let mut focus = FocusController::new();
        assert_eq!(focus.current(), FocusTarget::Page { pane: 0 });

        assert_eq!(
            focus.cycle_chrome_focus(2),
            FocusTarget::TabList { tab_index: 0 }
        );
        assert_eq!(
            focus.cycle_chrome_focus(2),
            FocusTarget::TabList { tab_index: 1 }
        );
        assert_eq!(focus.cycle_chrome_focus(2), FocusTarget::AddressField);
        assert_eq!(focus.cycle_chrome_focus(2), FocusTarget::Page { pane: 0 });

        focus.focus_page_pane(1);
        focus.focus_target(FocusTarget::FindBar);
        assert_eq!(focus.current(), FocusTarget::FindBar);
        assert_eq!(
            focus.dismiss_overlay_to_page(),
            FocusTarget::Page { pane: 1 }
        );
    }

    #[test]
    fn accessibility_tree_exposes_tabs_address_field_and_page_content() {
        let tabs = vec![
            ("New Tab".to_owned(), "search://newtab".to_owned()),
            ("Example".to_owned(), "https://example.com/".to_owned()),
        ];
        let tree = build_chrome_a11y_tree(&tabs, 1, "example.com", FocusTarget::AddressField);
        assert!(
            tree.iter()
                .any(|n| n.role == A11yRole::Tab && n.name == "Example" && n.selected)
        );
        assert!(
            tree.iter()
                .any(|n| n.role == A11yRole::SearchBox && n.focused)
        );
        assert!(
            tree.iter()
                .any(|n| n.role == A11yRole::WebArea && n.name == "Example")
        );
    }
}
