//! Platform-free domain types, data formats, and pure logic for searlx.
//!
//! This crate must compile and test on every supported OS. It owns URLs,
//! history, bookmarks, sessions, imports, fuzzy matching, rules, data-file
//! codecs, and migrations. It must not depend on a UI toolkit or a web engine.

pub mod address;
pub mod commands;
pub mod data;
pub mod domain;
pub mod error;
pub mod ids;
pub mod import;
pub mod session;
pub mod settings;
pub mod tabs;
pub mod url;

pub use address::{
    AddressCommand, AddressInvalidReason, AddressResolution, HostLookalikeInfo, OpenTabCandidate,
    ParseOptions, SearchEngine, SiteSearch, SubmitDisposition, SuggestionContext, SuggestionRow,
    build_suggestions, builtin_site_searches, expand_search_template, inline_completion,
    inspect_host_lookalike, match_site_search_offer, resolve_address, resolve_address_with_fs,
};
pub use commands::{
    CommandDescriptor, CommandId, CommandRegistry, KeyChord, KeyModifiers, MenuCategory,
    ShortcutConflictError, is_reserved_shortcut,
};
pub use domain::{
    BookmarkItem, BookmarkTree, CURRENT_SCHEMA_VERSION, EngineId, HistoryVisit, PermissionDecision,
    PermissionKey, PermissionKind, PinItem, SessionShape, SpaceColor, SplitAxis, SplitItem,
    TabGroupItem, TabSnapshot,
};
pub use error::{CoreError, Result};
pub use ids::{CollectionId, GroupId, PinId, ProfileId, SpaceId, TabId, TabIdAllocator, WindowId};
pub use import::{MAX_IMPORT_FILE_BYTES, SearchImportReport, import_search_directory};
pub use session::{
    DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, RestoreMode,
    RestoredSession, RestoredTab, RestoredWindow, WindowFrame, default_window_session,
    restore_session, save_additional_windows, save_window_session,
};
pub use settings::{
    AddressBarWidth, AutoCloseAfter, DownloadsOpenMode, FloatStyle, HighRefreshMode,
    LookPreference, NewTabPlace, ProxyConfig, Settings, SettingsPage, SettingsSearchHit, SleepPace,
    TabGlyphMode, TabLayoutMode, WebmailProvider, search_settings,
};
pub use tabs::{DragPayload, DragSession, DropTarget, SplitDropSide, TabEntry, TabModel};
pub use url::PageUrl;
