//! Opening URLs and files in the user's other applications.

use crate::PlatformResult;
use searlx_core::PageUrl;
use std::path::Path;

/// The system opener.
pub trait Opener: Send + Sync {
    /// Open a page in the default browser.
    fn open_url(&self, url: &PageUrl) -> PlatformResult<()>;

    /// Open a file with its default application.
    fn open_path(&self, path: &Path) -> PlatformResult<()>;
}
