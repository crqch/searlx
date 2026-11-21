//! Errors at the core boundary.
//!
//! Every I/O and decode failure is a value. Nothing in this crate panics on
//! user input, network data, or file data.

use std::path::PathBuf;

/// A failure in a core operation.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// The input could not be interpreted as a page URL.
    #[error("invalid address: {0}")]
    InvalidUrl(String),

    /// A file operation failed.
    #[error("i/o error on {path}: {source}")]
    Io {
        /// The path involved, when known.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// A data file did not contain the expected JSON shape.
    #[error("could not decode {path}: {source}")]
    Decode {
        /// The file that failed to decode.
        path: PathBuf,
        /// The serde error.
        #[source]
        source: serde_json::Error,
    },

    /// A data file declared a newer schema than this build understands.
    #[error("unsupported data version {found} in {path} (this build supports {supported})")]
    UnsupportedVersion {
        /// The file that declared the version.
        path: PathBuf,
        /// The version found in the file.
        found: u32,
        /// The newest version this build understands.
        supported: u32,
    },

    /// A file was larger than the safe read limit.
    #[error("{path} is {size} bytes, above the {limit}-byte limit")]
    FileTooLarge {
        /// The offending file.
        path: PathBuf,
        /// Its size in bytes.
        size: u64,
        /// The limit in bytes.
        limit: u64,
    },

    /// A path that must be a regular file was a symlink or something else.
    #[error("{path} is not a regular file")]
    NotARegularFile {
        /// The offending path.
        path: PathBuf,
    },

    /// A corrupt file could not be moved aside.
    #[error("could not quarantine {path}: {source}")]
    Quarantine {
        /// The corrupt file.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// A value was outside its permitted range.
    #[error("{0}")]
    Invalid(String),
}

impl CoreError {
    /// Wrap an I/O error with the path that produced it.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// A core result.
pub type Result<T> = std::result::Result<T, CoreError>;
