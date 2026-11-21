//! Platform-free domain types, data formats, and pure logic for searlx.
//!
//! This crate must compile and test on every supported OS. It owns URLs,
//! history, bookmarks, sessions, imports, fuzzy matching, rules, data-file
//! codecs, and migrations. It must not depend on a UI toolkit or a web engine.

pub mod address;
pub mod ai_agents_bench;
pub mod blocking;
pub mod bookmarks;
pub mod commands;
pub mod data;
pub mod domain;
pub mod downloads;
pub mod error;
pub mod extensions_security;
pub mod find;
pub mod gestures_lifecycle;
pub mod groups_splits_switcher;
pub mod history;
pub mod ids;
pub mod import;
pub mod menus_and_pages;
pub mod navigation;
pub mod page_tools;
pub mod peek_small_float;
pub mod pins_and_folders;
pub mod privacy;
pub mod reader;
pub mod recovery;
pub mod session;
pub mod settings;
pub mod spaces_profiles;
pub mod sync_panel_launcher_routes;
pub mod tabs;
pub mod url;
pub mod vault;

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
pub use groups_splits_switcher::{
    ClosedTabGroup, MoonWindowCandidate, SPLIT_GUTTER_PT, SPLIT_MIN_PANE_PT, SPLIT_SNAP_PT,
    SPLIT_UNSNAP_PT, SWITCHER_COLUMNS, SWITCHER_HOLD_DELAY_MS, SWITCHER_HOVER_GUARD_PT,
    SWITCHER_MAX_CARDS, SWITCHER_MAX_MOONS, SplitPaneFrame, SwitcherHeldAction, TabGroupsManager,
    TabSwitcherState, drag_two_pane_divider, layout_split_stage, move_split_focus,
};
pub use history::{
    ARCHIVE_CAP, ClearHistoryOutcome, ClearHistoryScope, HISTORY_DISK_CAP, HISTORY_MEMORY_CAP,
    HistoryDayGroup, HistoryManager, RECENTLY_CLOSED_CAP, RecentlyClosedItem,
};
pub use ids::{CollectionId, GroupId, PinId, ProfileId, SpaceId, TabId, TabIdAllocator, WindowId};
pub use import::{MAX_IMPORT_FILE_BYTES, SearchImportReport, import_search_directory};
pub use menus_and_pages::{
    ConnectionState, ContextMenuItem, InternalPageKind, KNOWN_VIDEO_SITES, SiteCardInput,
    SiteCardModel, build_page_context_menu, build_tab_context_menu, clean_link_url,
    open_internal_page, step_zoom_percent,
};
pub use navigation::{InPageError, NavDirection, NavEntry, TabNavigation};
pub use page_tools::{
    CAPTURE_MAX_HEIGHT_PT, CAPTURE_MAX_PIXELS, CAPTURE_MIN_PT, CapturePlan, CaptureRequest,
    DUSK_FILTER_FALLBACK_CSS, DUSK_MAX_HELD_CHARS, DUSK_MAX_MADE_CHARS, DUSK_MAX_SHEETS,
    DuskBudget, DuskController, DuskDecision, TranslatableNode, TranslationManager,
    is_measured_light_page, plan_page_capture,
};
pub use pins_and_folders::{
    LIVE_FEED_LAUNCH_DELAY_SECS, LIVE_FEED_REFRESH_SECS, LiveFeedPost, MAX_LIVE_FEED_BYTES,
    MAX_LIVE_FEED_POSTS, MAX_PIN_FOLDER_DEPTH, MAX_PIN_FOLDER_NAME_CHARS, PinFolderNode,
    PinNavigationAction, PinsAndFoldersManager, compute_pin_grid_rows, evaluate_pin_link_click,
    handle_command_click_on_pin, parse_rss_or_atom_feed,
};
pub use privacy::{PermissionStore, StoreCompositor};
pub use reader::{
    ReaderArticle, ReaderExitReload, ReaderTheme, detect_markdown_twin_link, exit_reader,
    extract_article,
};
pub use recovery::{
    AppcastRelease, CrashRecoveryAction, CrashRecoveryManager, EXPECTED_APPCAST_ID,
    EXPECTED_TEAM_ID, MAX_AUTO_RELOADS_PER_MINUTE, StagedInstallOutcome, UPDATE_CHECK_GUARD_SECS,
    append_crash_log, install_verified_bundle_swap, sweep_old_bundle, verify_update_release,
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
pub use spaces_profiles::{
    ProfileEntry, ProfileWindow, SPACE_STORE_ERASURE_RETRY_SECS, SpaceEntry, SpaceGradientPreset,
    SpaceStorePolicy, SpacesAndProfilesManager, WashStrength,
};
pub use tabs::{DragPayload, DragSession, DropTarget, SplitDropSide, TabEntry, TabModel};
pub use url::PageUrl;
pub use vault::{
    AUTOFILL_CLICK_GUARD_MS, CLIPBOARD_CLEAR_SECS, ConcealedClipboardCopy, MASKED_PASSWORD_DOTS,
    MAX_VAULT_OFFERS, PasswordVault, PostSignInAction, SAVE_OFFER_EXPIRY_SECS, SHOW_AUTO_HIDE_SECS,
    VaultAccount, VaultSecret,
};
