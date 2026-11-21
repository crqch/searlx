//! Command registry, default keyboard shortcuts, user overrides, and menu
//! routing (`FEATURES.md` §22, §29, P1-11).
//!
//! Important product rules (`FEATURES.md` §22, §29):
//! - `⌘S` (`Ctrl+S` on Windows/Linux) folds/hides the sidebar (`ToggleSidebar`),
//!   **not** Save Page (`SavePage` has no default shortcut).
//! - `⌘.` (`StopLoading`) has a real command handler, fixing the missing
//!   handler noted in `FEATURES.md` §29.
//! - Conflicts with another command or with reserved OS/text-editing shortcuts
//!   (`⌘C`, `⌘V`, `⌘X`, `⌘A`, `⌘Z`, `⌃⌘F`) are rejected.
//! - Extension commands can be adopted into the registry unless their shortcut
//!   conflicts with a reserved or already-bound shortcut (`FEATURES.md` §14.2).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Menu categories for routing commands into the menu bar and command palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MenuCategory {
    /// Application / Search menu.
    App,
    /// File menu.
    File,
    /// Edit menu.
    Edit,
    /// View menu.
    View,
    /// Tabs menu.
    Tabs,
    /// Bookmarks menu.
    Bookmarks,
    /// History menu.
    History,
    /// Spaces menu.
    Spaces,
    /// Extension command.
    Extension,
}

/// Every built-in browser command (`FEATURES.md` §22), plus extension commands.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CommandId {
    // --- App / File / Edit ---
    OpenSettings,
    OpenPasswords,
    NewWindow,
    NewTab,
    NewPrivateTab,
    ReopenClosed,
    OpenAddress,
    CloseTabOrPanel,
    CloseWindow,
    SavePage,
    Print,
    CopyCleanLink,
    FindOnPage,
    FindNext,
    FindPrevious,
    StopLoading,
    StopAgent,
    Quit,

    // --- View ---
    ShowTabsInSidebar,
    ToggleSidebar,
    FoldAllGroups,
    UnfoldAllGroups,
    Reload,
    ReloadFromOrigin,
    ReadingMode,
    FloatVideo,
    DarkenThisSite,
    PauseDarkeningEverywhere,
    HideElements,
    ReviewHiddenElements,
    ZoomIn,
    ZoomOut,
    ActualSize,
    WebInspector,
    JavaScriptConsole,
    InspectElement,
    ToggleSidePanel,

    // --- Tabs ---
    GoBack,
    GoForward,
    NextTab,
    PreviousTab,
    SearchTabs,
    ToggleSplitView,
    FocusLeftPane,
    FocusRightPane,
    DuplicateTab,
    CopyAddress,
    PasteAndGo,
    ToggleMuteTab,
    PutTabToSleep,
    RecentTabSwitcher,
    SelectTab(u8),

    // --- Bookmarks / History ---
    AddBookmark,
    ShowBookmarks,
    ToggleBookmarksBar,
    ShowHistory,
    ShowDownloads,
    ShowArchive,
    ClearBrowsingData,

    // --- Spaces ---
    SelectSpace(u8),
    NewSpace,

    // --- Extensions ---
    Extension {
        extension_id: String,
        command_name: String,
    },
}

/// Modifier flags on a keyboard shortcut.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct KeyModifiers {
    /// `⌘` on macOS, primary command modifier (`Ctrl` on Windows/Linux when normalized).
    pub cmd: bool,
    /// `⌃` (`Control`).
    pub ctrl: bool,
    /// `⌥` (`Alt` / `Option`).
    pub alt: bool,
    /// `⇧` (`Shift`).
    pub shift: bool,
}

impl KeyModifiers {
    /// Primary shortcut modifier (`⌘`).
    pub const CMD: Self = Self {
        cmd: true,
        ctrl: false,
        alt: false,
        shift: false,
    };

    /// `⇧⌘`.
    pub const SHIFT_CMD: Self = Self {
        cmd: true,
        ctrl: false,
        alt: false,
        shift: true,
    };

    /// `⌥⌘`.
    pub const ALT_CMD: Self = Self {
        cmd: true,
        ctrl: false,
        alt: true,
        shift: false,
    };

    /// `⌃⌘`.
    pub const CTRL_CMD: Self = Self {
        cmd: true,
        ctrl: true,
        alt: false,
        shift: false,
    };

    /// `⌥⇧⌘`.
    pub const ALT_SHIFT_CMD: Self = Self {
        cmd: true,
        ctrl: false,
        alt: true,
        shift: true,
    };

    /// `⌃⌥⌘`.
    pub const CTRL_ALT_CMD: Self = Self {
        cmd: true,
        ctrl: true,
        alt: true,
        shift: false,
    };

    /// `⌃`.
    pub const CTRL: Self = Self {
        cmd: false,
        ctrl: true,
        alt: false,
        shift: false,
    };

    /// Whether at least one non-Shift modifier is present.
    #[must_use]
    pub const fn has_primary_modifier(self) -> bool {
        self.cmd || self.ctrl || self.alt
    }
}

/// A keyboard shortcut chord (`modifiers` + lowercase key token).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KeyChord {
    /// Active modifiers.
    pub modifiers: KeyModifiers,
    /// Lowercase key token (e.g. `"t"`, `"s"`, `"tab"`, `"["`, `"]"`, `"backspace"`).
    pub key: String,
}

impl KeyChord {
    /// Construct a key chord.
    #[must_use]
    pub fn new(modifiers: KeyModifiers, key: impl Into<String>) -> Self {
        Self {
            modifiers,
            key: key.into().to_ascii_lowercase(),
        }
    }

    /// Parse a hyphen-separated chord specification such as `"cmd-s"`,
    /// `"shift-cmd-t"`, `"ctrl-tab"`, or `"alt-cmd-["`.
    pub fn parse(spec: &str) -> Result<Self> {
        let trimmed = spec.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            return Err(CoreError::Invalid("empty shortcut specification".into()));
        }
        let mut modifiers = KeyModifiers::default();
        let parts: Vec<&str> = trimmed.split('-').collect();
        if parts.is_empty() {
            return Err(CoreError::Invalid(format!("invalid shortcut: {spec:?}")));
        }
        for token in &parts[..parts.len() - 1] {
            match *token {
                "cmd" | "meta" | "super" => modifiers.cmd = true,
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "opt" | "option" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                other => {
                    return Err(CoreError::Invalid(format!(
                        "unknown modifier {other:?} in {spec:?}"
                    )));
                }
            }
        }
        let key = parts[parts.len() - 1];
        if key.is_empty() {
            return Err(CoreError::Invalid(format!(
                "missing key in shortcut {spec:?}"
            )));
        }
        let is_function_key =
            key.starts_with('f') && key.len() >= 2 && key[1..].chars().all(|c| c.is_ascii_digit());
        if !modifiers.has_primary_modifier() && !is_function_key {
            return Err(CoreError::Invalid(format!(
                "shortcut {spec:?} requires cmd, ctrl, alt, or a function key"
            )));
        }
        Ok(Self::new(modifiers, key))
    }

    /// Canonical hyphenated string (`"shift-cmd-t"`).
    #[must_use]
    pub fn to_spec(&self) -> String {
        let mut out = String::new();
        if self.modifiers.ctrl {
            out.push_str("ctrl-");
        }
        if self.modifiers.alt {
            out.push_str("alt-");
        }
        if self.modifiers.shift {
            out.push_str("shift-");
        }
        if self.modifiers.cmd {
            out.push_str("cmd-");
        }
        out.push_str(&self.key);
        out
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_spec())
    }
}

/// Metadata for a registered command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandDescriptor {
    /// Command identifier.
    pub id: CommandId,
    /// Menu category.
    pub category: MenuCategory,
    /// Human-readable menu title.
    pub title: String,
    /// Default shortcut, if any.
    pub default_shortcut: Option<KeyChord>,
}

/// Reserved OS and standard text-editing shortcuts that user overrides may not steal (`FEATURES.md` §18.2, §22).
#[must_use]
pub fn is_reserved_shortcut(chord: &KeyChord) -> bool {
    // Standard editing keys (⌘C/V/X/A/Z) and macOS full screen (⌃⌘F).
    (chord.modifiers == KeyModifiers::CMD
        && matches!(chord.key.as_str(), "c" | "v" | "x" | "a" | "z"))
        || (chord.modifiers == KeyModifiers::CTRL_CMD && chord.key == "f")
}

/// Why a shortcut assignment was rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ShortcutConflictError {
    /// The chord is reserved by the OS or text editing.
    #[error("shortcut {0} is reserved for system or text editing")]
    Reserved(KeyChord),
    /// The chord is already assigned to another command.
    #[error("shortcut {chord} is already used by {existing:?}")]
    AlreadyAssigned {
        /// Conflicting chord.
        chord: KeyChord,
        /// Command that owns the chord.
        existing: CommandId,
    },
    /// The command is not registered.
    #[error("unknown command {0:?}")]
    UnknownCommand(CommandId),
}

/// Central registry of browser commands, menu items, and keyboard shortcuts (`P1-11`).
#[derive(Clone, Debug)]
pub struct CommandRegistry {
    descriptors: BTreeMap<CommandId, CommandDescriptor>,
    overrides: BTreeMap<CommandId, Option<KeyChord>>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRegistry {
    /// Build a registry pre-populated with every `FEATURES.md` §22 command and default shortcut.
    #[must_use]
    pub fn new() -> Self {
        let mut descriptors = BTreeMap::new();
        for desc in builtin_commands() {
            descriptors.insert(desc.id.clone(), desc);
        }
        Self {
            descriptors,
            overrides: BTreeMap::new(),
        }
    }

    /// All registered command descriptors in deterministic order.
    pub fn commands(&self) -> impl Iterator<Item = &CommandDescriptor> {
        self.descriptors.values()
    }

    /// Look up a command descriptor by ID.
    #[must_use]
    pub fn descriptor(&self, id: &CommandId) -> Option<&CommandDescriptor> {
        self.descriptors.get(id)
    }

    /// Effective shortcut for `id`, taking user overrides into account.
    #[must_use]
    pub fn shortcut_for(&self, id: &CommandId) -> Option<KeyChord> {
        if let Some(overridden) = self.overrides.get(id) {
            return overridden.clone();
        }
        self.descriptors
            .get(id)
            .and_then(|d| d.default_shortcut.clone())
    }

    /// Resolve a pressed [`KeyChord`] to its [`CommandId`].
    #[must_use]
    pub fn command_for_shortcut(&self, chord: &KeyChord) -> Option<CommandId> {
        for id in self.descriptors.keys() {
            if self.shortcut_for(id).as_ref() == Some(chord) {
                return Some(id.clone());
            }
        }
        None
    }

    /// List commands belonging to a menu category.
    #[must_use]
    pub fn menu_items(&self, category: MenuCategory) -> Vec<&CommandDescriptor> {
        self.descriptors
            .values()
            .filter(|d| d.category == category)
            .collect()
    }

    /// Rebind `command` to `new_chord` (or `None` to clear its shortcut),
    /// rejecting reserved chords and conflicts with other commands.
    pub fn set_override(
        &mut self,
        command: CommandId,
        new_chord: Option<KeyChord>,
    ) -> std::result::Result<(), ShortcutConflictError> {
        if !self.descriptors.contains_key(&command) {
            return Err(ShortcutConflictError::UnknownCommand(command));
        }
        if let Some(ref chord) = new_chord {
            if is_reserved_shortcut(chord) {
                return Err(ShortcutConflictError::Reserved(chord.clone()));
            }
            if let Some(existing) = self.command_for_shortcut(chord)
                && existing != command
            {
                return Err(ShortcutConflictError::AlreadyAssigned {
                    chord: chord.clone(),
                    existing,
                });
            }
        }
        self.overrides.insert(command, new_chord);
        Ok(())
    }

    /// Remove any user override for `command`, restoring its default shortcut.
    pub fn reset_override(&mut self, command: &CommandId) {
        self.overrides.remove(command);
    }

    /// Register an extension command (`FEATURES.md` §14.2), adopting its
    /// suggested shortcut only if it is not reserved or already taken.
    pub fn register_extension_command(
        &mut self,
        extension_id: impl Into<String>,
        command_name: impl Into<String>,
        title: impl Into<String>,
        suggested_shortcut: Option<KeyChord>,
    ) -> CommandId {
        let id = CommandId::Extension {
            extension_id: extension_id.into(),
            command_name: command_name.into(),
        };
        let default_shortcut = suggested_shortcut.filter(|chord| {
            !is_reserved_shortcut(chord) && self.command_for_shortcut(chord).is_none()
        });
        self.descriptors.insert(
            id.clone(),
            CommandDescriptor {
                id: id.clone(),
                category: MenuCategory::Extension,
                title: title.into(),
                default_shortcut,
            },
        );
        id
    }
}

#[allow(clippy::too_many_lines)]
fn builtin_commands() -> Vec<CommandDescriptor> {
    use CommandId::*;
    use MenuCategory::*;

    let mut list = vec![
        // App / File / Edit
        CommandDescriptor {
            id: OpenSettings,
            category: App,
            title: "Settings…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, ",")),
        },
        CommandDescriptor {
            id: OpenPasswords,
            category: App,
            title: "Passwords…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::ALT_CMD, "l")),
        },
        CommandDescriptor {
            id: Quit,
            category: App,
            title: "Quit Search".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "q")),
        },
        CommandDescriptor {
            id: NewWindow,
            category: File,
            title: "New Window".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "n")),
        },
        CommandDescriptor {
            id: NewTab,
            category: File,
            title: "New Tab".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "t")),
        },
        CommandDescriptor {
            id: NewPrivateTab,
            category: File,
            title: "New Private Tab".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "n")),
        },
        CommandDescriptor {
            id: ReopenClosed,
            category: File,
            title: "Reopen Closed Tab".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "t")),
        },
        CommandDescriptor {
            id: OpenAddress,
            category: File,
            title: "Open Address…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "l")),
        },
        CommandDescriptor {
            id: CloseTabOrPanel,
            category: File,
            title: "Close Tab".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "w")),
        },
        CommandDescriptor {
            id: CloseWindow,
            category: File,
            title: "Close Window".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "w")),
        },
        // Save Page has NO shortcut because ⌘S folds the sidebar (`FEATURES.md` §22, §29).
        CommandDescriptor {
            id: SavePage,
            category: File,
            title: "Save Page As…".into(),
            default_shortcut: None,
        },
        CommandDescriptor {
            id: Print,
            category: File,
            title: "Print…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "p")),
        },
        CommandDescriptor {
            id: StopAgent,
            category: File,
            title: "Stop Agent".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CTRL_ALT_CMD, ".")),
        },
        CommandDescriptor {
            id: CopyCleanLink,
            category: Edit,
            title: "Copy Clean Link".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "c")),
        },
        CommandDescriptor {
            id: FindOnPage,
            category: Edit,
            title: "Find…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "f")),
        },
        CommandDescriptor {
            id: FindNext,
            category: Edit,
            title: "Find Next".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "g")),
        },
        CommandDescriptor {
            id: FindPrevious,
            category: Edit,
            title: "Find Previous".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "g")),
        },
        // View
        CommandDescriptor {
            id: ShowTabsInSidebar,
            category: View,
            title: "Show Tabs in Sidebar".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "s")),
        },
        CommandDescriptor {
            id: ToggleSidebar,
            category: View,
            title: "Hide Sidebar".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "s")),
        },
        CommandDescriptor {
            id: FoldAllGroups,
            category: View,
            title: "Fold All Groups".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::ALT_CMD, "[")),
        },
        CommandDescriptor {
            id: UnfoldAllGroups,
            category: View,
            title: "Unfold All Groups".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::ALT_CMD, "]")),
        },
        CommandDescriptor {
            id: Reload,
            category: View,
            title: "Reload Page".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "r")),
        },
        CommandDescriptor {
            id: ReloadFromOrigin,
            category: View,
            title: "Reload Page from Origin".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::ALT_CMD, "r")),
        },
        CommandDescriptor {
            id: StopLoading,
            category: View,
            title: "Stop Loading".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, ".")),
        },
        CommandDescriptor {
            id: ReadingMode,
            category: View,
            title: "Reading Mode".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "r")),
        },
        CommandDescriptor {
            id: FloatVideo,
            category: View,
            title: "Float Video".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "p")),
        },
        CommandDescriptor {
            id: DarkenThisSite,
            category: View,
            title: "Darken This Site".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "d")),
        },
        CommandDescriptor {
            id: PauseDarkeningEverywhere,
            category: View,
            title: "Pause Darkening Everywhere".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::ALT_SHIFT_CMD, "d")),
        },
        CommandDescriptor {
            id: HideElements,
            category: View,
            title: "Hide Elements…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "h")),
        },
        CommandDescriptor {
            id: ReviewHiddenElements,
            category: View,
            title: "Hidden on This Site…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "u")),
        },
        CommandDescriptor {
            id: ZoomIn,
            category: View,
            title: "Zoom In".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "+")),
        },
        CommandDescriptor {
            id: ZoomOut,
            category: View,
            title: "Zoom Out".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "-")),
        },
        CommandDescriptor {
            id: ActualSize,
            category: View,
            title: "Actual Size".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "0")),
        },
        CommandDescriptor {
            id: WebInspector,
            category: View,
            title: "Web Inspector".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::ALT_CMD, "i")),
        },
        CommandDescriptor {
            id: JavaScriptConsole,
            category: View,
            title: "JavaScript Console".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::ALT_CMD, "j")),
        },
        CommandDescriptor {
            id: InspectElement,
            category: View,
            title: "Inspect Element".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::ALT_CMD, "c")),
        },
        CommandDescriptor {
            id: ToggleSidePanel,
            category: View,
            title: "Side Panel".into(),
            default_shortcut: None,
        },
        // Tabs
        CommandDescriptor {
            id: GoBack,
            category: Tabs,
            title: "Back".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "[")),
        },
        CommandDescriptor {
            id: GoForward,
            category: Tabs,
            title: "Forward".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "]")),
        },
        CommandDescriptor {
            id: NextTab,
            category: Tabs,
            title: "Select Next Tab".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "]")),
        },
        CommandDescriptor {
            id: PreviousTab,
            category: Tabs,
            title: "Select Previous Tab".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "[")),
        },
        CommandDescriptor {
            id: SearchTabs,
            category: Tabs,
            title: "Search Tabs…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "k")),
        },
        CommandDescriptor {
            id: ToggleSplitView,
            category: Tabs,
            title: "Split Current Page".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::ALT_CMD, "n")),
        },
        CommandDescriptor {
            id: FocusLeftPane,
            category: Tabs,
            title: "Focus Left Page".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CTRL_CMD, "left")),
        },
        CommandDescriptor {
            id: FocusRightPane,
            category: Tabs,
            title: "Focus Right Page".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CTRL_CMD, "right")),
        },
        CommandDescriptor {
            id: DuplicateTab,
            category: Tabs,
            title: "Duplicate Tab".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "d")),
        },
        CommandDescriptor {
            id: CopyAddress,
            category: Tabs,
            title: "Copy Address".into(),
            default_shortcut: None,
        },
        CommandDescriptor {
            id: PasteAndGo,
            category: Tabs,
            title: "Paste and Go".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "v")),
        },
        CommandDescriptor {
            id: ToggleMuteTab,
            category: Tabs,
            title: "Mute Tab".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "m")),
        },
        CommandDescriptor {
            id: PutTabToSleep,
            category: Tabs,
            title: "Put Tab to Sleep".into(),
            default_shortcut: None,
        },
        CommandDescriptor {
            id: RecentTabSwitcher,
            category: Tabs,
            title: "Switch Recent Tabs".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CTRL, "tab")),
        },
        // Bookmarks / History
        CommandDescriptor {
            id: AddBookmark,
            category: Bookmarks,
            title: "Add This Page…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "b")),
        },
        CommandDescriptor {
            id: ShowBookmarks,
            category: Bookmarks,
            title: "Manage Bookmarks…".into(),
            default_shortcut: None,
        },
        CommandDescriptor {
            id: ToggleBookmarksBar,
            category: Bookmarks,
            title: "Show Bookmarks Bar".into(),
            default_shortcut: None,
        },
        CommandDescriptor {
            id: ShowHistory,
            category: History,
            title: "Show History".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, "y")),
        },
        CommandDescriptor {
            id: ShowDownloads,
            category: History,
            title: "Downloads".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "j")),
        },
        CommandDescriptor {
            id: ShowArchive,
            category: History,
            title: "Archived Tabs…".into(),
            default_shortcut: None,
        },
        CommandDescriptor {
            id: ClearBrowsingData,
            category: History,
            title: "Clear Browsing Data…".into(),
            default_shortcut: Some(KeyChord::new(KeyModifiers::SHIFT_CMD, "backspace")),
        },
        // Spaces
        CommandDescriptor {
            id: NewSpace,
            category: Spaces,
            title: "New Space…".into(),
            default_shortcut: None,
        },
    ];

    for digit in 1..=9_u8 {
        list.push(CommandDescriptor {
            id: SelectTab(digit),
            category: Tabs,
            title: format!("Select Tab {digit}"),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CMD, digit.to_string())),
        });
        list.push(CommandDescriptor {
            id: SelectSpace(digit),
            category: Spaces,
            title: format!("Select Space {digit}"),
            default_shortcut: Some(KeyChord::new(KeyModifiers::CTRL, digit.to_string())),
        });
    }

    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_s_folds_sidebar_and_does_not_save_page() {
        let registry = CommandRegistry::new();
        let cmd_s = KeyChord::parse("cmd-s").expect("parse cmd-s");
        assert_eq!(
            registry.command_for_shortcut(&cmd_s),
            Some(CommandId::ToggleSidebar)
        );
        assert_eq!(registry.shortcut_for(&CommandId::SavePage), None);
    }

    #[test]
    fn every_phase_1_action_is_reachable_by_menu_and_keyboard() {
        let registry = CommandRegistry::new();
        for (cmd, spec) in [
            (CommandId::NewTab, "cmd-t"),
            (CommandId::NewPrivateTab, "shift-cmd-n"),
            (CommandId::CloseTabOrPanel, "cmd-w"),
            (CommandId::ReopenClosed, "shift-cmd-t"),
            (CommandId::OpenAddress, "cmd-l"),
            (CommandId::OpenSettings, "cmd-,"),
            (CommandId::ToggleSidebar, "cmd-s"),
            (CommandId::Reload, "cmd-r"),
            (CommandId::ReloadFromOrigin, "alt-cmd-r"),
            (CommandId::StopLoading, "cmd-."),
            (CommandId::GoBack, "cmd-["),
            (CommandId::GoForward, "cmd-]"),
            (CommandId::NextTab, "shift-cmd-]"),
            (CommandId::PreviousTab, "shift-cmd-["),
            (CommandId::FindOnPage, "cmd-f"),
            (CommandId::RecentTabSwitcher, "ctrl-tab"),
            (CommandId::SelectTab(1), "cmd-1"),
            (CommandId::SelectSpace(1), "ctrl-1"),
        ] {
            let chord = KeyChord::parse(spec).expect("parse chord");
            assert_eq!(registry.shortcut_for(&cmd), Some(chord.clone()));
            assert_eq!(registry.command_for_shortcut(&chord), Some(cmd.clone()));
            assert!(registry.descriptor(&cmd).is_some());
        }
    }

    #[test]
    fn rejects_conflicting_and_reserved_shortcuts() {
        let mut registry = CommandRegistry::new();

        // Trying to bind SavePage to cmd-s conflicts with ToggleSidebar.
        let cmd_s = KeyChord::parse("cmd-s").expect("cmd-s");
        let conflict = registry
            .set_override(CommandId::SavePage, Some(cmd_s))
            .expect_err("must reject duplicate shortcut");
        assert!(matches!(
            conflict,
            ShortcutConflictError::AlreadyAssigned {
                existing: CommandId::ToggleSidebar,
                ..
            }
        ));

        // Trying to bind SavePage to cmd-c conflicts with reserved copy shortcut.
        let cmd_c = KeyChord::parse("cmd-c").expect("cmd-c");
        let reserved = registry
            .set_override(CommandId::SavePage, Some(cmd_c))
            .expect_err("must reject reserved shortcut");
        assert!(matches!(reserved, ShortcutConflictError::Reserved(_)));

        // Rebinding SavePage to an unused shortcut succeeds.
        let alt_cmd_s = KeyChord::parse("alt-cmd-s").expect("alt-cmd-s");
        registry
            .set_override(CommandId::SavePage, Some(alt_cmd_s.clone()))
            .expect("bind alt-cmd-s");
        assert_eq!(
            registry.command_for_shortcut(&alt_cmd_s),
            Some(CommandId::SavePage)
        );
    }
}
