use tempfile::TempDir;
use std::path::{Path, PathBuf};
use std::fs;
use crate::models::BlindMarkError;

/// Temporary workspace manager for archive processing
///
/// Creates a disk-based temporary workspace with subdirectories:
/// - `extracted/` - Files extracted from archive
/// - `processed/` - Files after watermarking
///
/// Automatically cleaned up when dropped.
pub struct TempWorkspace {
    temp_dir: TempDir,
    extracted_path: PathBuf,
    processed_path: PathBuf,
}

impl TempWorkspace {
    /// Create a new temporary workspace
    ///
    /// # Arguments
    /// * `archive_name` - Name of the archive (used for debugging/logging)
    ///
    /// # Returns
    /// * `TempWorkspace` with extracted/ and processed/ subdirectories
    pub fn new(archive_name: &str) -> Result<Self, BlindMarkError> {
        // Create temporary directory with prefix
        let temp_dir = tempfile::Builder::new()
            .prefix(&format!("blindmark_{}_", archive_name))
            .tempdir()
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to create temporary directory: {}", e)
            ))?;

        let base_path = temp_dir.path();
        let extracted_path = base_path.join("extracted");
        let processed_path = base_path.join("processed");

        // Create subdirectories
        fs::create_dir_all(&extracted_path)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to create extracted directory: {}", e)
            ))?;

        fs::create_dir_all(&processed_path)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to create processed directory: {}", e)
            ))?;

        Ok(Self {
            temp_dir,
            extracted_path,
            processed_path,
        })
    }

    /// Get path to extracted files directory
    pub fn extracted_path(&self) -> &Path {
        &self.extracted_path
    }

    /// Get path to processed files directory
    pub fn processed_path(&self) -> &Path {
        &self.processed_path
    }

    /// Get base temporary directory path
    pub fn base_path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Copy a processed file while maintaining relative directory structure
    ///
    /// # Arguments
    /// * `src_path` - Source file path (relative to extracted_path)
    /// * `dest_relative_path` - Destination path (relative to processed_path)
    ///
    /// # Example
    /// ```ignore
    /// workspace.copy_processed("images/photo.png", "images/photo.png")?;
    /// ```
    pub fn copy_processed(&self, src_path: &Path, dest_relative_path: &Path) -> Result<(), BlindMarkError> {
        let src_full = self.extracted_path.join(src_path);
        let dest_full = self.processed_path.join(dest_relative_path);

        // Create parent directories if needed
        if let Some(parent) = dest_full.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to create directory {}: {}", parent.display(), e)
                ))?;
        }

        // Copy file
        fs::copy(&src_full, &dest_full)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to copy {} to {}: {}", src_full.display(), dest_full.display(), e)
            ))?;

        Ok(())
    }

    /// Write processed content directly to a file in the processed directory
    ///
    /// # Arguments
    /// * `relative_path` - Path relative to processed_path
    /// * `content` - File content to write
    pub fn write_processed(&self, relative_path: &Path, content: &[u8]) -> Result<(), BlindMarkError> {
        let dest_full = self.processed_path.join(relative_path);

        // Create parent directories if needed
        if let Some(parent) = dest_full.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to create directory {}: {}", parent.display(), e)
                ))?;
        }

        // Write file
        fs::write(&dest_full, content)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to write to {}: {}", dest_full.display(), e)
            ))?;

        Ok(())
    }

    /// Get total size of all files in extracted directory (in bytes)
    pub fn extracted_size(&self) -> Result<u64, BlindMarkError> {
        self.dir_size(&self.extracted_path)
    }

    /// Get total size of all files in processed directory (in bytes)
    pub fn processed_size(&self) -> Result<u64, BlindMarkError> {
        self.dir_size(&self.processed_path)
    }

    /// Calculate total size of all files in a directory recursively
    fn dir_size(&self, path: &Path) -> Result<u64, BlindMarkError> {
        let mut total_size = 0u64;

        if path.is_dir() {
            let entries = fs::read_dir(path)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to read directory {}: {}", path.display(), e)
                ))?;

            for entry in entries {
                let entry = entry.map_err(|e| BlindMarkError::Archive(
                    format!("Failed to read directory entry: {}", e)
                ))?;

                let metadata = entry.metadata()
                    .map_err(|e| BlindMarkError::Archive(
                        format!("Failed to get metadata: {}", e)
                    ))?;

                if metadata.is_dir() {
                    total_size += self.dir_size(&entry.path())?;
                } else {
                    total_size += metadata.len();
                }
            }
        }

        Ok(total_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_create_workspace() {
        let workspace = TempWorkspace::new("test_archive").unwrap();

        assert!(workspace.extracted_path().exists());
        assert!(workspace.processed_path().exists());
        assert!(workspace.base_path().exists());
    }

    #[test]
    fn test_copy_processed() {
        let workspace = TempWorkspace::new("test_copy").unwrap();

        // Create a test file in extracted directory
        let test_file = workspace.extracted_path().join("test.txt");
        let mut file = File::create(&test_file).unwrap();
        file.write_all(b"test content").unwrap();

        // Copy to processed directory
        workspace.copy_processed(Path::new("test.txt"), Path::new("test.txt")).unwrap();

        // Verify file exists in processed directory
        let processed_file = workspace.processed_path().join("test.txt");
        assert!(processed_file.exists());

        let content = fs::read_to_string(processed_file).unwrap();
        assert_eq!(content, "test content");
    }

    #[test]
    fn test_copy_processed_with_subdirs() {
        let workspace = TempWorkspace::new("test_subdirs").unwrap();

        // Create nested directory structure
        let subdir = workspace.extracted_path().join("images").join("photos");
        fs::create_dir_all(&subdir).unwrap();

        let test_file = subdir.join("photo.jpg");
        let mut file = File::create(&test_file).unwrap();
        file.write_all(b"fake image data").unwrap();

        // Copy with relative path
        workspace.copy_processed(
            Path::new("images/photos/photo.jpg"),
            Path::new("images/photos/photo.jpg")
        ).unwrap();

        // Verify nested structure is preserved
        let processed_file = workspace.processed_path()
            .join("images")
            .join("photos")
            .join("photo.jpg");
        assert!(processed_file.exists());

        let content = fs::read(processed_file).unwrap();
        assert_eq!(content, b"fake image data");
    }

    #[test]
    fn test_write_processed() {
        let workspace = TempWorkspace::new("test_write").unwrap();

        let content = b"direct write content";
        workspace.write_processed(Path::new("output.txt"), content).unwrap();

        let written_file = workspace.processed_path().join("output.txt");
        assert!(written_file.exists());

        let read_content = fs::read(written_file).unwrap();
        assert_eq!(read_content, content);
    }

    #[test]
    fn test_write_processed_with_subdirs() {
        let workspace = TempWorkspace::new("test_write_subdirs").unwrap();

        let content = b"nested write";
        workspace.write_processed(Path::new("data/results/output.bin"), content).unwrap();

        let written_file = workspace.processed_path()
            .join("data")
            .join("results")
            .join("output.bin");
        assert!(written_file.exists());
    }

    #[test]
    fn test_dir_size() {
        let workspace = TempWorkspace::new("test_size").unwrap();

        // Create some files
        let file1 = workspace.extracted_path().join("file1.txt");
        fs::write(&file1, b"12345").unwrap(); // 5 bytes

        let file2 = workspace.extracted_path().join("file2.txt");
        fs::write(&file2, b"1234567890").unwrap(); // 10 bytes

        let total_size = workspace.extracted_size().unwrap();
        assert_eq!(total_size, 15);
    }

    #[test]
    fn test_cleanup_on_drop() {
        let base_path;
        {
            let workspace = TempWorkspace::new("test_cleanup").unwrap();
            base_path = workspace.base_path().to_path_buf();
            assert!(base_path.exists());
        }
        // After drop, directory should be cleaned up
        assert!(!base_path.exists());
    }
}
