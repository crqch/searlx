//! Shared UI action messages sent by GTK chrome widgets into the shell coordinator.

use searlx_core::{
    AppCommand, DropTarget, InternalPageKind, PageUrl, SpaceColor, SpaceStorePolicy, TabId,
};

/// High-level UI intent emitted by sidebar, stage, overlays, and keyboard shortcuts.
#[derive(Clone, Debug)]
pub enum ShellAction {
    /// Forward a domain command directly to [`searlx_core::BrowserController::dispatch`].
    Command(AppCommand),

    /// Open or focus the floating overlay address bar (`Ctrl+L` / `Ctrl+T` when floating).
    OpenAddressOverlay {
        prefill: Option<String>,
        for_new_tab: bool,
    },
    /// Close the floating overlay address bar without navigating.
    CloseAddressOverlay,

    /// Show or hide the Site Card popover anchored to a chrome widget.
    ToggleSiteCard,
    /// Open or close the Downloads popover.
    ToggleDownloadsPopover,

    /// Toggle the Hide Elements interactive DOM picker (`Shift+Ctrl+H`).
    ToggleHideElementsPicker,
    /// Open the "Hidden on this site" review popover (`Shift+Ctrl+U`).
    OpenHiddenElementsDialog,
    /// Open the floating per-site CSS editor (`Edit Site Style…`).
    OpenSiteCssDialog,

    /// Trigger OS print on the active tab.
    PrintActiveTab,
    /// Show the Create New Space modal/popover.
    OpenNewSpaceDialog,
    /// Create a new space and switch to it.
    CreateAndSwitchSpace {
        name: String,
        color: SpaceColor,
        store_policy: SpaceStorePolicy,
    },
    /// Rename an existing space.
    PromptRenameSpace(searlx_core::SpaceId),
    /// Prompt to rename a tab inline/dialog.
    PromptRenameTab(TabId),
    /// Prompt to create a named tab group around `tab`.
    PromptGroupTab(TabId),

    /// Start a pointer drag on a tab row.
    StartTabDrag { tab: TabId, x: f64, y: f64 },
    /// Update pointer position & drop target during drag.
    UpdateTabDrag {
        x: f64,
        y: f64,
        target: Option<DropTarget>,
    },
    /// Finish pointer drag.
    FinishTabDrag,
    /// Cancel pointer drag.
    CancelTabDrag,

    /// Open a link in a specific target (used by bookmarks bar / context menus).
    OpenUrlInCurrentTab(PageUrl),
    /// Open internal page via shortcut/menu.
    OpenInternal(InternalPageKind, Option<String>),

    /// Toggle Reader Mode on the active tab.
    ToggleReaderMode,
    /// Update `Ctrl`-held visual shortcut hint state without tearing down widgets mid-key-event.
    SetCtrlHeld(bool),
    /// Close folded peek drawer.
    HideFoldedPeek,
    /// Request quit (respecting `settings.quit_confirm`).
    RequestQuit,
}

/// Callback handle passed down to pure GTK view modules.
pub type ActionSink = std::rc::Rc<dyn Fn(ShellAction)>;
