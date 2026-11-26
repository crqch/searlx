//! Explicit focus model and chrome accessibility tree (`AGENTS.md` §3.3, P2-20).
//!
//! Rule (`AGENTS.md` §3.3):
//! > Use one focus model: the page that has the keyboard, the chrome field, or
//! > a panel. Make it explicit and testable.

use searlx_core::{AddressInvalidReason, SuggestionRow, UiSnapshot};

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
    /// Popover or modal dialog (`Site Card`, `Find Bar`).
    Popover,
    /// List container (`Suggestions`, `Spaces`).
    List,
    /// Item inside a list container.
    ListItem,
    /// Status announcement (`status_banner`, invalid-input shake).
    Status,
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
    /// Additional accessible state/description metadata.
    pub description: Option<String>,
    /// Whether this node is selected (e.g. active tab).
    pub selected: bool,
    /// Whether this node currently holds keyboard focus.
    pub focused: bool,
}

/// Live chrome accessibility tree synchronized from [`UiSnapshot`] (`AGENTS.md` §3.3, P2-20).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccessibilityTree {
    nodes: Vec<A11yNode>,
}

impl AccessibilityTree {
    /// Create an empty accessibility tree.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a synchronized accessibility tree from a live [`UiSnapshot`].
    #[must_use]
    pub fn from_snapshot(
        snapshot: &UiSnapshot,
        focus: FocusTarget,
        address_overlay_open: bool,
        site_card_open: bool,
    ) -> Self {
        let mut tree = Self::new();
        tree.sync(snapshot, focus, address_overlay_open, site_card_open);
        tree
    }

    /// Rebuild all nodes from a live [`UiSnapshot`] and focus state.
    pub fn sync(
        &mut self,
        snapshot: &UiSnapshot,
        focus: FocusTarget,
        address_overlay_open: bool,
        site_card_open: bool,
    ) {
        self.nodes.clear();
        let active_space_name = snapshot
            .spaces
            .iter()
            .find(|s| s.active)
            .map(|s| s.name.clone());
        self.nodes.push(A11yNode {
            role: A11yRole::Window,
            name: "searlx".into(),
            value: active_space_name,
            description: snapshot.sidebar_folded.then(|| "sidebar folded".to_owned()),
            selected: false,
            focused: false,
        });

        self.nodes.push(A11yNode {
            role: A11yRole::TabList,
            name: "Tabs".into(),
            value: Some(format!("{} tabs", snapshot.tabs.len())),
            description: Some(format!("{:?}", snapshot.sidebar_layout)),
            selected: false,
            focused: matches!(focus, FocusTarget::TabList { .. }),
        });

        for (index, tab) in snapshot.tabs.iter().enumerate() {
            let mut flags = Vec::new();
            if tab.pinned {
                flags.push("pinned");
            }
            if tab.private {
                flags.push("private");
            }
            if tab.sleeping {
                flags.push("sleeping");
            }
            if tab.put_away {
                flags.push("put away");
            }
            if tab.in_split {
                flags.push("split");
            }
            if tab.audible && !tab.muted {
                flags.push("audible");
            } else if tab.muted {
                flags.push("muted");
            }
            if tab.is_lookalike {
                flags.push("lookalike host");
            }
            let description = (!flags.is_empty()).then(|| flags.join(", "));
            self.nodes.push(A11yNode {
                role: A11yRole::Tab,
                name: tab.title.clone(),
                value: Some(tab.url.as_str().to_owned()),
                description,
                selected: tab.active,
                focused: focus == FocusTarget::TabList { tab_index: index },
            });
        }

        self.nodes.push(A11yNode {
            role: A11yRole::Button,
            name: "New Tab".into(),
            value: None,
            description: None,
            selected: false,
            focused: false,
        });
        self.nodes.push(A11yNode {
            role: A11yRole::Button,
            name: if snapshot.sidebar_folded {
                "Show Sidebar".into()
            } else {
                "Hide Sidebar".into()
            },
            value: None,
            description: None,
            selected: snapshot.sidebar_folded,
            focused: false,
        });

        let address_value = if address_overlay_open || snapshot.address.focused {
            snapshot.address.text.clone()
        } else {
            snapshot.address.current_url_display.clone()
        };
        let mut address_notes = Vec::new();
        if snapshot.address.is_lookalike {
            address_notes.push(format!("lookalike: {}", snapshot.address.safe_host));
        }
        if let Some(chip) = &snapshot.address.site_search_chip {
            address_notes.push(format!("site search: {}", chip.name));
        }
        if let Some(comp) = &snapshot.address.inline_completion {
            address_notes.push(format!("completion: {comp}"));
        }
        if let Some(reason) = &snapshot.address.shake_reason {
            address_notes.push(format!("invalid: {}", describe_invalid_reason(reason)));
        }
        self.nodes.push(A11yNode {
            role: A11yRole::SearchBox,
            name: "Address and search".into(),
            value: Some(address_value),
            description: (!address_notes.is_empty()).then(|| address_notes.join("; ")),
            selected: address_overlay_open,
            focused: focus == FocusTarget::AddressField || snapshot.address.focused,
        });

        if address_overlay_open && !snapshot.address.suggestions.is_empty() {
            self.nodes.push(A11yNode {
                role: A11yRole::List,
                name: "Suggestions".into(),
                value: Some(format!(
                    "{} suggestions",
                    snapshot.address.suggestions.len()
                )),
                description: None,
                selected: false,
                focused: false,
            });
            for row in &snapshot.address.suggestions {
                self.nodes.push(A11yNode {
                    role: A11yRole::ListItem,
                    name: suggestion_a11y_label(row),
                    value: suggestion_a11y_value(row),
                    description: None,
                    selected: false,
                    focused: false,
                });
            }
        }

        if site_card_open && let Some(card) = &snapshot.site_card {
            self.nodes.push(A11yNode {
                role: A11yRole::Popover,
                name: "Site Card".into(),
                value: Some(card.header.clone()),
                description: Some(format!("{}% zoom", card.zoom_percent)),
                selected: true,
                focused: focus == FocusTarget::SiteCard,
            });
        }

        if let Some(find) = &snapshot.find {
            self.nodes.push(A11yNode {
                role: A11yRole::Popover,
                name: "Find on page".into(),
                value: Some(find.query.clone()),
                description: Some(find.status_text.clone()),
                selected: true,
                focused: focus == FocusTarget::FindBar,
            });
        }

        if let Some(reason) = &snapshot.address.shake_reason {
            self.nodes.push(A11yNode {
                role: A11yRole::Status,
                name: "Invalid address".into(),
                value: Some(describe_invalid_reason(reason)),
                description: None,
                selected: false,
                focused: false,
            });
        }

        if let Some(banner) = &snapshot.status_banner {
            self.nodes.push(A11yNode {
                role: A11yRole::Status,
                name: "Status".into(),
                value: Some(banner.clone()),
                description: None,
                selected: false,
                focused: false,
            });
        }

        if snapshot.split_panes.is_empty() {
            let active_tab = snapshot.tabs.iter().find(|t| t.active);
            self.nodes.push(A11yNode {
                role: A11yRole::WebArea,
                name: active_tab.map_or_else(|| "Page".to_owned(), |t| t.title.clone()),
                value: active_tab.map(|t| t.url.as_str().to_owned()),
                description: None,
                selected: true,
                focused: matches!(focus, FocusTarget::Page { .. }),
            });
        } else {
            for (idx, pane) in snapshot.split_panes.iter().enumerate() {
                self.nodes.push(A11yNode {
                    role: A11yRole::WebArea,
                    name: pane.title.clone(),
                    value: Some(pane.url.as_str().to_owned()),
                    description: Some(format!("split pane {}", idx + 1)),
                    selected: pane.focused,
                    focused: focus == FocusTarget::Page { pane: idx },
                });
            }
        }
    }

    /// All nodes in traversal order.
    #[must_use]
    pub fn nodes(&self) -> &[A11yNode] {
        &self.nodes
    }

    /// Number of nodes in the tree.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Currently focused node, if any.
    #[must_use]
    pub fn focused_node(&self) -> Option<&A11yNode> {
        self.nodes.iter().find(|n| n.focused)
    }

    /// All tab nodes in the active space.
    #[must_use]
    pub fn tab_nodes(&self) -> Vec<&A11yNode> {
        self.nodes_with_role(A11yRole::Tab)
    }

    /// All nodes matching `role`.
    #[must_use]
    pub fn nodes_with_role(&self, role: A11yRole) -> Vec<&A11yNode> {
        self.nodes.iter().filter(|n| n.role == role).collect()
    }

    /// Find the first node matching exact accessible `name`.
    #[must_use]
    pub fn find_by_name(&self, name: &str) -> Option<&A11yNode> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// All status announcement nodes.
    #[must_use]
    pub fn status_nodes(&self) -> Vec<&A11yNode> {
        self.nodes_with_role(A11yRole::Status)
    }

    /// All web area nodes (one per active tab, or one per split pane).
    #[must_use]
    pub fn web_areas(&self) -> Vec<&A11yNode> {
        self.nodes_with_role(A11yRole::WebArea)
    }
}

fn describe_invalid_reason(reason: &AddressInvalidReason) -> String {
    match reason {
        AddressInvalidReason::Empty => "Empty address".to_owned(),
        AddressInvalidReason::EmailOrUserinfo => "Email addresses cannot be opened".to_owned(),
        AddressInvalidReason::UnsupportedScheme(scheme) => {
            format!("Unsupported scheme: {scheme}")
        }
        AddressInvalidReason::MissingLocalFile(path) => {
            format!("File not found: {}", path.display())
        }
        AddressInvalidReason::MalformedAddress(raw) => {
            format!("Malformed address: {raw}")
        }
    }
}

fn suggestion_a11y_label(row: &SuggestionRow) -> String {
    match row {
        SuggestionRow::OpenTab { title, .. } => format!("Switch to tab: {title}"),
        SuggestionRow::SiteSearchOffer { site } => format!("Search {}", site.name),
        SuggestionRow::PastSiteSearch {
            site_name, query, ..
        } => format!("{site_name}: {query}"),
        SuggestionRow::History { title, .. } => title.clone(),
        SuggestionRow::Bookmark { title, .. } => title.clone(),
        SuggestionRow::ICloudTab {
            device_name, title, ..
        } => format!("{title} ({device_name})"),
        SuggestionRow::KnownSite { title, .. } => title.clone(),
        SuggestionRow::EngineSuggestion { query, .. } => query.clone(),
        SuggestionRow::InternalPage { label, .. } => label.clone(),
        SuggestionRow::Command(cmd) => cmd.label().to_owned(),
        SuggestionRow::SearchWords { engine, query, .. } => format!("Search {engine} for {query}"),
    }
}

fn suggestion_a11y_value(row: &SuggestionRow) -> Option<String> {
    match row {
        SuggestionRow::OpenTab { url, .. }
        | SuggestionRow::PastSiteSearch { url, .. }
        | SuggestionRow::History { url, .. }
        | SuggestionRow::ICloudTab { url, .. }
        | SuggestionRow::KnownSite { url, .. }
        | SuggestionRow::EngineSuggestion { url, .. }
        | SuggestionRow::InternalPage { url, .. }
        | SuggestionRow::SearchWords { url, .. } => Some(url.display()),
        SuggestionRow::Bookmark { url, .. } => url.as_ref().map(searlx_core::PageUrl::display),
        SuggestionRow::SiteSearchOffer { site } => Some(site.host.clone()),
        SuggestionRow::Command(_) => None,
    }
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
        description: None,
        selected: false,
        focused: false,
    });
    nodes.push(A11yNode {
        role: A11yRole::TabList,
        name: "Tabs".into(),
        value: Some(format!("{} tabs", tabs.len())),
        description: None,
        selected: false,
        focused: matches!(focus, FocusTarget::TabList { .. }),
    });
    for (index, (title, url)) in tabs.iter().enumerate() {
        nodes.push(A11yNode {
            role: A11yRole::Tab,
            name: title.clone(),
            value: Some(url.clone()),
            description: None,
            selected: index == active_tab,
            focused: focus == FocusTarget::TabList { tab_index: index },
        });
    }
    nodes.push(A11yNode {
        role: A11yRole::Button,
        name: "New Tab".into(),
        value: None,
        description: None,
        selected: false,
        focused: false,
    });
    nodes.push(A11yNode {
        role: A11yRole::SearchBox,
        name: "Address and search".into(),
        value: Some(field_text.to_owned()),
        description: None,
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
        description: None,
        selected: true,
        focused: matches!(focus, FocusTarget::Page { .. }),
    });
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use searlx_core::{AppCommand, BrowserController, EngineCapabilities, PageUrl, RestoreMode};

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

    #[test]
    fn accessibility_tree_syncs_from_live_controller_snapshot() {
        let dir = std::env::temp_dir().join(format!("searlx-a11y-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (mut ctrl, _) = BrowserController::open(
            &dir,
            EngineCapabilities::baseline("Test"),
            RestoreMode::FreshStart,
        )
        .expect("open controller");
        ctrl.dispatch(AppCommand::NewPrivateTab {
            url: Some(PageUrl::parse("https://example.com/docs").expect("url")),
            activate: true,
        })
        .expect("new private tab");
        ctrl.dispatch(AppCommand::OpenFindBar).expect("open find");
        ctrl.dispatch(AppCommand::FindInPage {
            query: "docs".into(),
            case_sensitive: false,
            whole_words: false,
            text_hint: Some("docs example docs".into()),
        })
        .expect("find");
        ctrl.dispatch(AppCommand::FocusAddressBar).expect("focus");
        ctrl.dispatch(AppCommand::SetAddressText { text: "git".into() })
            .expect("type");

        let snap = ctrl.ui_snapshot();
        let tree = AccessibilityTree::from_snapshot(&snap, FocusTarget::AddressField, true, true);
        assert_eq!(tree.tab_nodes().len(), 2);
        assert!(
            tree.tab_nodes()
                .iter()
                .any(|n| n.selected && n.description.as_deref() == Some("private"))
        );
        assert!(tree.find_by_name("Site Card").is_some());
        assert!(tree.find_by_name("Find on page").is_some());
        assert!(tree.find_by_name("Suggestions").is_some());
        assert!(
            tree.focused_node()
                .is_some_and(|n| n.role == A11yRole::SearchBox)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
