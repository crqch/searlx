//! Platform-free domain types, data formats, and pure logic for searlx.
//!
//! This crate must compile and test on every supported OS. It owns URLs,
//! history, bookmarks, sessions, imports, fuzzy matching, rules, data-file
//! codecs, and migrations. It must not depend on a UI toolkit or a web engine.

pub mod error;
pub mod ids;
pub mod url;

pub use error::{CoreError, Result};
pub use ids::{CollectionId, GroupId, PinId, ProfileId, SpaceId, TabId, TabIdAllocator, WindowId};
pub use url::PageUrl;
