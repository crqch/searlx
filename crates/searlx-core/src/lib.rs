//! Platform-free domain types, data formats, and pure logic for searlx.
//!
//! This crate must compile and test on every supported OS. It owns URLs,
//! history, bookmarks, sessions, imports, fuzzy matching, rules, data-file
//! codecs, and migrations. It must not depend on a UI toolkit or a web engine.

pub mod address;
pub mod blocking;
pub mod bookmarks;
pub mod commands;
pub mod data;
pub mod domain;
pub mod downloads;
pub mod error;
pub mod find;
pub mod history;
pub mod ids;
pub mod import;
pub mod navigation;
pub mod page_tools;
pub mod reader;
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
pub use blocking::{
    COSMETIC_AD_SELECTORS, DomElementDescriptor, FIRST_LOAD_BLOCKER_WAIT_MS, HideElementsManager,
    SiteCssManager, TRACKER_DOMAINS, YOUTUBE_ISOLATED_FALLBACK_JS, YOUTUBE_PAGE_WORLD_JS,
    compile_webkit_content_rules, normalize_site_key, sanitize_user_css,
    should_block_third_party_request, synthesize_selector, youtube_ad_scripts,
};
pub use bookmarks::{BookmarkMenuAction, BookmarkSearchHit, BookmarksManager};
pub use commands::{
    CommandDescriptor, CommandId, CommandRegistry, KeyChord, KeyModifiers, MenuCategory,
    ShortcutConflictError, is_reserved_shortcut,
};
pub use domain::{
    BookmarkItem, BookmarkTree, CURRENT_SCHEMA_VERSION, EngineId, HistoryVisit, PermissionDecision,
    PermissionKey, PermissionKind, PinItem, SessionShape, SpaceColor, SplitAxis, SplitItem,
    TabGroupItem, TabSnapshot,
};
pub use downloads::{
    DOWNLOADS_HISTORY_CAP, DownloadEntry, DownloadState, DownloadsManager, HttpMethod,
    number_duplicate_filename, sanitize_extension_subfolder, sanitize_suggested_filename,
};
pub use error::{CoreError, Result};
pub use find::{
    FindSession, FindTarget, ISOLATED_WORLD_FIND_JS, MAX_FIND_MATCHES, count_matches_in_text,
};
pub use history::{
    ARCHIVE_CAP, ClearHistoryOutcome, ClearHistoryScope, HISTORY_DISK_CAP, HISTORY_MEMORY_CAP,
    HistoryDayGroup, HistoryManager, RECENTLY_CLOSED_CAP, RecentlyClosedItem,
};
pub use ids::{CollectionId, GroupId, PinId, ProfileId, SpaceId, TabId, TabIdAllocator, WindowId};
pub use import::{MAX_IMPORT_FILE_BYTES, SearchImportReport, import_search_directory};
pub use navigation::{InPageError, NavDirection, NavEntry, TabNavigation};
pub use page_tools::{
    CAPTURE_MAX_HEIGHT_PT, CAPTURE_MAX_PIXELS, CAPTURE_MIN_PT, CapturePlan, CaptureRequest,
    DUSK_FILTER_FALLBACK_CSS, DUSK_MAX_HELD_CHARS, DUSK_MAX_MADE_CHARS, DUSK_MAX_SHEETS,
    DuskBudget, DuskController, DuskDecision, TranslatableNode, TranslationManager,
    is_measured_light_page, plan_page_capture,
};
pub use reader::{
    ReaderArticle, ReaderExitReload, ReaderTheme, detect_markdown_twin_link, exit_reader,
    extract_article,
};
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
