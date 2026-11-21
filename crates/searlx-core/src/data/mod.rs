//! Versioned JSON data files from the original app.
//!
//! See [`model`] for the inferred schemas and [`file`] for the quarantine and
//! atomic-write rules.

pub mod file;
pub mod model;

pub use file::{DataError, quarantine, read_json, write_json};
pub use model::*;
