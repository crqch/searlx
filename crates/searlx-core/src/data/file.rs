//! Reading and writing the versioned JSON data files.
//!
//! A file that fails to decode is renamed to `*.unreadable-<stamp>.json` and
//! never overwritten. The original bytes stay available for a later recovery
//! tool; the app keeps running with defaults.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use time::OffsetDateTime;
use time::macros::format_description;
use uuid::Uuid;

/// A data-file failure. Paths are safe to show to a person and to log; the
/// contents of the file are never included.
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    /// The file could not be read. Missing files are not quarantined.
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// The file was not valid JSON for the expected shape. It was moved aside
    /// when `quarantined_to` is set.
    #[error("could not decode {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
        quarantined_to: Option<PathBuf>,
    },

    /// The value could not be serialised.
    #[error("could not encode data for {path}: {source}")]
    Encode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// The file or its temporary sibling could not be written.
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// A corrupt file could not be moved aside.
    #[error("could not quarantine {path}: {source}")]
    Quarantine {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Read one JSON data file.
///
/// # Errors
/// Returns [`DataError::Read`] for I/O failures and [`DataError::Decode`] when
/// the bytes do not match `T`. A decode failure also attempts to quarantine the
/// file, recording where it went in the error.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, DataError> {
    let bytes = fs::read(path).map_err(|source| DataError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(value),
        Err(source) => {
            let quarantined_to = quarantine(path).ok();
            Err(DataError::Decode {
                path: path.to_path_buf(),
                source,
                quarantined_to,
            })
        }
    }
}

/// Write one JSON data file atomically: a temporary sibling, then a rename.
///
/// # Errors
/// Returns [`DataError::Encode`] if serialisation fails or
/// [`DataError::Write`] if the temp file or rename fails.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), DataError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| DataError::Encode {
        path: path.to_path_buf(),
        source,
    })?;
    bytes.push(b'\n');

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| DataError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("json");
    let temp = path.with_extension(format!("{extension}.tmp-{}", Uuid::new_v4()));
    if let Err(source) = fs::write(&temp, bytes) {
        let _ = fs::remove_file(&temp);
        return Err(DataError::Write {
            path: path.to_path_buf(),
            source,
        });
    }
    if let Err(source) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(DataError::Write {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

/// Move a corrupt file aside without ever replacing an earlier quarantine.
///
/// # Errors
/// Returns the rename error if the file cannot be moved.
pub fn quarantine(path: &Path) -> Result<PathBuf, io::Error> {
    let stamp = OffsetDateTime::now_utc()
        .format(&format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .unwrap_or_else(|_| "unknown".to_owned());
    quarantine_with_stamp(path, &stamp)
}

fn quarantine_with_stamp(path: &Path, stamp: &str) -> Result<PathBuf, io::Error> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("data");
    let mut attempt = 0_u32;
    loop {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{attempt}")
        };
        let name = format!("{stem}.unreadable-{stamp}{suffix}.json");
        let candidate = match directory {
            Some(directory) => directory.join(&name),
            None => PathBuf::from(&name),
        };
        if candidate.exists() {
            attempt = attempt.saturating_add(1);
            continue;
        }
        fs::rename(path, &candidate)?;
        return Ok(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarantine_never_overwrites() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("session.json");
        fs::write(&path, b"{").expect("write first");
        let first = quarantine_with_stamp(&path, "20261121T010203Z").expect("first quarantine");

        fs::write(&path, b"{").expect("write second");
        let second = quarantine_with_stamp(&path, "20261121T010203Z").expect("second quarantine");

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
        assert!(!path.exists());
        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some("session.unreadable-20261121T010203Z.json")
        );
        assert_eq!(
            second.file_name().and_then(|name| name.to_str()),
            Some("session.unreadable-20261121T010203Z-1.json")
        );
    }

    #[test]
    fn write_then_read_round_trips() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("spaces.json");
        let file = crate::data::SpacesFile {
            spaces: vec![crate::data::Space {
                id: "space-1".into(),
                name: "Personal".into(),
                ..crate::data::Space::default()
            }],
            ..crate::data::SpacesFile::default()
        };
        write_json(&path, &file).expect("write");
        let decoded: crate::data::SpacesFile = read_json(&path).expect("read");
        assert_eq!(decoded, file);
        let leftovers: Vec<_> = fs::read_dir(directory.path())
            .expect("list")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
    }
}
