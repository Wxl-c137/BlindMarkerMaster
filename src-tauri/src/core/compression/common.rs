// Archive handler trait for different compression formats

use std::path::Path;
use crate::models::BlindMarkError;

/// Trait for handling different archive formats
pub trait ArchiveHandler: Send + Sync {
    /// Extract archive to specified directory preserving hierarchy
    fn extract(&self, archive_path: &Path, dest_dir: &Path) -> Result<(), BlindMarkError>;

    /// Create archive from directory preserving hierarchy
    fn create(&self, source_dir: &Path, output_path: &Path) -> Result<(), BlindMarkError>;

    /// Check if this handler supports the given file
    fn supports(&self, archive_path: &Path) -> bool;
}
