use std::path::Path;
use std::fs::{self, File};
use sevenz_rust::{SevenZReader, SevenZWriter, Password};
use walkdir::WalkDir;
use crate::core::compression::common::ArchiveHandler;
use crate::models::BlindMarkError;

/// 7z archive handler
///
/// Handles extraction and creation of 7z archives using sevenz-rust.
pub struct SevenZHandler;

impl SevenZHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ArchiveHandler for SevenZHandler {
    /// Extract 7z archive to destination directory
    ///
    /// # Arguments
    /// * `archive_path` - Path to 7z file
    /// * `dest_dir` - Destination directory for extraction
    ///
    /// # Behavior
    /// - Preserves directory hierarchy
    /// - Creates parent directories as needed
    /// - Does not support password-protected archives
    fn extract(&self, archive_path: &Path, dest_dir: &Path) -> Result<(), BlindMarkError> {
        let file = File::open(archive_path)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to open 7z archive {}: {}", archive_path.display(), e)
            ))?;

        // Get file size
        let metadata = file.metadata()
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to get file metadata: {}", e)
            ))?;
        let file_size = metadata.len();

        let mut reader = SevenZReader::new(file, file_size, Password::empty())
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to read 7z archive: {}", e)
            ))?;

        // Create destination directory
        fs::create_dir_all(dest_dir)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to create destination directory: {}", e)
            ))?;

        // Extract all entries
        reader.for_each_entries(|entry, reader| {
            let entry_path = entry.name();
            let output_path = dest_dir.join(entry_path);

            if entry.is_directory() {
                // Create directory
                fs::create_dir_all(&output_path)
                    .map_err(|e| sevenz_rust::Error::io(e))?;
            } else {
                // Create parent directories
                if let Some(parent) = output_path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| sevenz_rust::Error::io(e))?;
                }

                // Extract file
                let mut output_file = File::create(&output_path)
                    .map_err(|e| sevenz_rust::Error::io(e))?;

                std::io::copy(reader, &mut output_file)
                    .map_err(|e| sevenz_rust::Error::io(e))?;
            }

            Ok(true) // Continue processing
        })
        .map_err(|e| BlindMarkError::Archive(
            format!("Failed to extract 7z archive: {}", e)
        ))?;

        Ok(())
    }

    /// Create 7z archive from source directory
    ///
    /// # Arguments
    /// * `source_dir` - Directory to archive
    /// * `output_path` - Path for output 7z file
    ///
    /// # Behavior
    /// - Preserves directory hierarchy
    /// - Uses LZMA2 compression
    fn create(&self, source_dir: &Path, output_path: &Path) -> Result<(), BlindMarkError> {
        let file = File::create(output_path)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to create 7z file {}: {}", output_path.display(), e)
            ))?;

        let mut writer = SevenZWriter::new(file)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to create 7z writer: {}", e)
            ))?;

        // Walk source directory
        let walker = WalkDir::new(source_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok());

        for entry in walker {
            let path = entry.path();
            let relative_path = path.strip_prefix(source_dir)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to calculate relative path: {}", e)
                ))?;

            // Skip root directory
            if relative_path.as_os_str().is_empty() {
                continue;
            }

            // Convert path to string for 7z entry name
            let name = relative_path.to_string_lossy().to_string();

            if path.is_file() {
                // Add file to archive (stream directly from disk, no intermediate buffer)
                let mut file = File::open(path)
                    .map_err(|e| BlindMarkError::Archive(
                        format!("Failed to open file {}: {}", path.display(), e)
                    ))?;

                writer.push_archive_entry(
                    sevenz_rust::SevenZArchiveEntry::from_path(&path, name),
                    Some(&mut file),
                )
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to add file to archive: {}", e)
                ))?;
            } else if path.is_dir() {
                // Add directory entry
                writer.push_archive_entry::<&[u8]>(
                    sevenz_rust::SevenZArchiveEntry::from_path(&path, name),
                    None,
                )
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to add directory to archive: {}", e)
                ))?;
            }
        }

        writer.finish()
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to finalize 7z archive: {}", e)
            ))?;

        Ok(())
    }

    /// Check if this handler supports the given archive
    ///
    /// Returns true for files with .7z extension (case-insensitive)
    fn supports(&self, archive_path: &Path) -> bool {
        archive_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("7z"))
            .unwrap_or(false)
    }
}

impl Default for SevenZHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_files(dir: &Path) {
        fs::create_dir_all(dir.join("subdir")).unwrap();
        fs::write(dir.join("file1.txt"), b"content1").unwrap();
        fs::write(dir.join("file2.txt"), b"content2").unwrap();
        fs::write(dir.join("subdir/file3.txt"), b"content3").unwrap();
    }

    #[test]
    fn test_supports() {
        let handler = SevenZHandler::new();
        assert!(handler.supports(Path::new("archive.7z")));
        assert!(handler.supports(Path::new("ARCHIVE.7Z")));
        assert!(!handler.supports(Path::new("archive.zip")));
        assert!(!handler.supports(Path::new("archive.rar")));
        assert!(!handler.supports(Path::new("noextension")));
    }

    #[test]
    fn test_create_and_extract() {
        let temp_source = TempDir::new().unwrap();
        let temp_dest = TempDir::new().unwrap();
        let temp_archive = TempDir::new().unwrap();

        // Create test files
        create_test_files(temp_source.path());

        // Create 7z
        let handler = SevenZHandler::new();
        let archive_path = temp_archive.path().join("test.7z");
        handler.create(temp_source.path(), &archive_path).unwrap();

        assert!(archive_path.exists());

        // Extract 7z
        handler.extract(&archive_path, temp_dest.path()).unwrap();

        // Verify extracted files
        assert!(temp_dest.path().join("file1.txt").exists());
        assert!(temp_dest.path().join("file2.txt").exists());
        assert!(temp_dest.path().join("subdir/file3.txt").exists());

        let content1 = fs::read_to_string(temp_dest.path().join("file1.txt")).unwrap();
        assert_eq!(content1, "content1");

        let content3 = fs::read_to_string(temp_dest.path().join("subdir/file3.txt")).unwrap();
        assert_eq!(content3, "content3");
    }

    #[test]
    fn test_extract_preserves_hierarchy() {
        let temp_source = TempDir::new().unwrap();
        let temp_dest = TempDir::new().unwrap();
        let temp_archive = TempDir::new().unwrap();

        // Create nested structure
        fs::create_dir_all(temp_source.path().join("a/b/c")).unwrap();
        fs::write(temp_source.path().join("a/b/c/deep.txt"), b"deep file").unwrap();

        // Create and extract
        let handler = SevenZHandler::new();
        let archive_path = temp_archive.path().join("nested.7z");
        handler.create(temp_source.path(), &archive_path).unwrap();
        handler.extract(&archive_path, temp_dest.path()).unwrap();

        // Verify hierarchy
        assert!(temp_dest.path().join("a/b/c/deep.txt").exists());
        let content = fs::read_to_string(temp_dest.path().join("a/b/c/deep.txt")).unwrap();
        assert_eq!(content, "deep file");
    }

    #[test]
    fn test_extract_nonexistent_archive() {
        let handler = SevenZHandler::new();
        let temp_dest = TempDir::new().unwrap();

        let result = handler.extract(Path::new("/nonexistent.7z"), temp_dest.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_create_empty_directory() {
        let temp_source = TempDir::new().unwrap();
        let temp_archive = TempDir::new().unwrap();

        let handler = SevenZHandler::new();
        let archive_path = temp_archive.path().join("empty.7z");

        // Should succeed even with empty directory
        let result = handler.create(temp_source.path(), &archive_path);
        assert!(result.is_ok());
        assert!(archive_path.exists());
    }
}
