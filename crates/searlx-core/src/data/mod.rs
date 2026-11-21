//! Versioned JSON data files from the original app.
//!
//! See [`model`] for the inferred schemas and [`file`] for the quarantine and
//! atomic-write rules.

pub mod file;
pub mod model;
pub mod store;

pub use file::{DataError, quarantine, read_json, write_json};
pub use model::*;
pub use store::{
    CURRENT_BOOKMARKS_VERSION, DataStore, LoadOutcome, migrate_bookmarks_file,
    migrate_legacy_office_browser_dir, migrate_session_file,
};
