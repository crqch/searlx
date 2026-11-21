//! File dialogs.

use crate::PlatformResult;
use std::path::{Path, PathBuf};

/// A file-type filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    /// The label shown in the dialog.
    pub name: String,
    /// Extensions without a dot.
    pub extensions: Vec<String>,
}

/// Open/save dialogs.
pub trait FileDialog: Send + Sync {
    /// Pick one file.
    fn pick_file(&self, title: &str, filters: &[FileFilter]) -> PlatformResult<Option<PathBuf>>;

    /// Pick several files.
    fn pick_files(&self, title: &str, filters: &[FileFilter]) -> PlatformResult<Vec<PathBuf>>;

    /// Pick a directory.
    fn pick_directory(&self, title: &str) -> PlatformResult<Option<PathBuf>>;

    /// Pick a save destination.
    fn save_file(
        &self,
        title: &str,
        suggested_name: &str,
        filters: &[FileFilter],
    ) -> PlatformResult<Option<PathBuf>>;

    /// Reveal a path in the platform file manager.
    fn reveal(&self, path: &Path) -> PlatformResult<()>;
}
