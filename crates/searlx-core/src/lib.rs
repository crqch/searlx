//! Platform-free domain types, data formats, and pure logic for searlx.
//!
//! This crate must compile and test on every supported OS. It owns URLs,
//! history, bookmarks, sessions, imports, fuzzy matching, rules, data-file
//! codecs, and migrations. It must not depend on a UI toolkit or a web engine.

pub mod address;
pub mod data;
pub mod domain;
pub mod error;
pub mod ids;
pub mod url;

pub use address::{
    AddressCommand, AddressInvalidReason, AddressResolution, HostLookalikeInfo, OpenTabCandidate,
    ParseOptions, SearchEngine, SiteSearch, SubmitDisposition, SuggestionContext, SuggestionRow,
    build_suggestions, builtin_site_searches, expand_search_template, inline_completion,
    inspect_host_lookalike, match_site_search_offer, resolve_address, resolve_address_with_fs,
};
pub use domain::{
    BookmarkItem, BookmarkTree, CURRENT_SCHEMA_VERSION, EngineId, HistoryVisit, PermissionDecision,
    PermissionKey, PermissionKind, PinItem, SessionShape, SpaceColor, SplitAxis, SplitItem,
    TabGroupItem, TabSnapshot,
};
pub use error::{CoreError, Result};
pub use ids::{CollectionId, GroupId, PinId, ProfileId, SpaceId, TabId, TabIdAllocator, WindowId};
pub use url::PageUrl;
