// Archive compression modules
pub mod common;
pub mod zip_handler;

#[path = "7z_handler.rs"]
pub mod sevenz_handler;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::models::BlindMarkError;
use common::ArchiveHandler;
use zip_handler::ZipHandler;
use sevenz_handler::SevenZHandler;

/// Archive processor that orchestrates the complete workflow
///
/// Workflow:
/// 1. Auto-detect archive type based on extension
/// 2. Extract archive to temporary workspace
/// 3. Return extraction path for further processing
/// 4. Create new archive from processed files
pub struct ArchiveProcessor {
    handlers: Vec<Arc<dyn ArchiveHandler>>,
}

impl ArchiveProcessor {
    /// Create a new archive processor with all supported handlers
    pub fn new() -> Self {
        let handlers: Vec<Arc<dyn ArchiveHandler>> = vec![
            Arc::new(ZipHandler::new()),
            Arc::new(SevenZHandler::new()),
        ];

        Self { handlers }
    }

    /// Auto-detect and get appropriate handler for an archive
    ///
    /// # Arguments
    /// * `archive_path` - Path to the archive file
    ///
    /// # Returns
    /// * Handler that supports this archive type
    fn get_handler(&self, archive_path: &Path) -> Result<Arc<dyn ArchiveHandler>, BlindMarkError> {
        for handler in &self.handlers {
            if handler.supports(archive_path) {
                return Ok(Arc::clone(handler));
            }
        }

        let ext = archive_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown");

        Err(BlindMarkError::UnsupportedArchive(
            format!("Unsupported archive format: .{}", ext)
        ))
    }

    /// Extract archive to destination directory
    ///
    /// # Arguments
    /// * `archive_path` - Path to archive file
    /// * `dest_dir` - Destination directory for extraction
    ///
    /// # Returns
    /// * Path to extracted directory
    pub fn extract(&self, archive_path: &Path, dest_dir: &Path) -> Result<PathBuf, BlindMarkError> {
        let handler = self.get_handler(archive_path)?;
        handler.extract(archive_path, dest_dir)?;
        Ok(dest_dir.to_path_buf())
    }

    /// Create archive from source directory
    ///
    /// # Arguments
    /// * `source_dir` - Directory containing files to archive
    /// * `output_path` - Path for output archive
    /// * `format` - Optional archive format (auto-detected from extension if None)
    ///
    /// # Returns
    /// * Path to created archive
    pub fn create(
        &self,
        source_dir: &Path,
        output_path: &Path,
    ) -> Result<PathBuf, BlindMarkError> {
        let handler = self.get_handler(output_path)?;
        handler.create(source_dir, output_path)?;
        Ok(output_path.to_path_buf())
    }

    /// Generate output filename with "_watermarked" suffix
    ///
    /// # Example
    /// ```ignore
    /// "archive.zip" -> "archive_watermarked.zip"
    /// "data.7z" -> "data_watermarked.7z"
    /// ```
    pub fn generate_output_name(input_path: &Path) -> PathBuf {
        let parent = input_path.parent();
        let stem = input_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("archive");
        let extension = input_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let new_name = if extension.is_empty() {
            format!("{}_watermarked", stem)
        } else {
            format!("{}_watermarked.{}", stem, extension)
        };

        match parent {
            Some(p) => p.join(new_name),
            None => PathBuf::from(new_name),
        }
    }

    /// Check if a file is a supported archive format
    pub fn is_supported(&self, path: &Path) -> bool {
        self.handlers.iter().any(|h| h.supports(path))
    }

    /// Get list of supported archive extensions
    ///
    /// 当前支持 ZIP 和 7z。RAR 因 `unrar` 需要系统库（许可证限制），
    /// 暂未实现，传入 .rar 文件会返回错误。
    pub fn supported_extensions() -> Vec<&'static str> {
        vec!["zip", "7z"]
    }
}

impl Default for ArchiveProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_files(dir: &Path) {
        fs::create_dir_all(dir.join("subdir")).unwrap();
        fs::write(dir.join("file1.txt"), b"test content 1").unwrap();
        fs::write(dir.join("subdir/file2.txt"), b"test content 2").unwrap();
    }

    #[test]
    fn test_generate_output_name() {
        let input = Path::new("/path/to/archive.zip");
        let output = ArchiveProcessor::generate_output_name(input);
        assert_eq!(output, Path::new("/path/to/archive_watermarked.zip"));

        let input2 = Path::new("data.7z");
        let output2 = ArchiveProcessor::generate_output_name(input2);
        assert_eq!(output2, Path::new("data_watermarked.7z"));

        let input3 = Path::new("noext");
        let output3 = ArchiveProcessor::generate_output_name(input3);
        assert_eq!(output3, Path::new("noext_watermarked"));
    }

    #[test]
    fn test_is_supported() {
        let processor = ArchiveProcessor::new();
        assert!(processor.is_supported(Path::new("test.zip")));
        assert!(processor.is_supported(Path::new("test.7z")));
        assert!(processor.is_supported(Path::new("TEST.ZIP")));
        assert!(!processor.is_supported(Path::new("test.rar")));
        assert!(!processor.is_supported(Path::new("test.tar.gz")));
    }

    #[test]
    fn test_supported_extensions() {
        let extensions = ArchiveProcessor::supported_extensions();
        assert!(extensions.contains(&"zip"));
        assert!(extensions.contains(&"7z"));
    }

    #[test]
    fn test_extract_and_create_zip() {
        let temp_source = TempDir::new().unwrap();
        let temp_extract = TempDir::new().unwrap();
        let temp_output = TempDir::new().unwrap();

        // Create test files
        create_test_files(temp_source.path());

        let processor = ArchiveProcessor::new();

        // Create ZIP
        let zip_path = temp_output.path().join("test.zip");
        processor.create(temp_source.path(), &zip_path).unwrap();
        assert!(zip_path.exists());

        // Extract ZIP
        let extract_path = temp_extract.path();
        processor.extract(&zip_path, extract_path).unwrap();

        // Verify extracted files
        assert!(extract_path.join("file1.txt").exists());
        assert!(extract_path.join("subdir/file2.txt").exists());

        let content = fs::read_to_string(extract_path.join("file1.txt")).unwrap();
        assert_eq!(content, "test content 1");
    }

    #[test]
    fn test_extract_and_create_7z() {
        let temp_source = TempDir::new().unwrap();
        let temp_extract = TempDir::new().unwrap();
        let temp_output = TempDir::new().unwrap();

        // Create test files
        create_test_files(temp_source.path());

        let processor = ArchiveProcessor::new();

        // Create 7z
        let archive_path = temp_output.path().join("test.7z");
        processor.create(temp_source.path(), &archive_path).unwrap();
        assert!(archive_path.exists());

        // Extract 7z
        let extract_path = temp_extract.path();
        processor.extract(&archive_path, extract_path).unwrap();

        // Verify extracted files
        assert!(extract_path.join("file1.txt").exists());
        assert!(extract_path.join("subdir/file2.txt").exists());
    }

    #[test]
    fn test_unsupported_format() {
        let processor = ArchiveProcessor::new();
        let temp_dest = TempDir::new().unwrap();

        let result = processor.extract(Path::new("archive.rar"), temp_dest.path());
        assert!(result.is_err());

        if let Err(BlindMarkError::UnsupportedArchive(msg)) = result {
            assert!(msg.contains("rar"));
        } else {
            panic!("Expected UnsupportedArchive error");
        }
    }

    #[test]
    fn test_get_handler() {
        let processor = ArchiveProcessor::new();

        // Should succeed for supported formats
        assert!(processor.get_handler(Path::new("test.zip")).is_ok());
        assert!(processor.get_handler(Path::new("test.7z")).is_ok());

        // Should fail for unsupported formats
        assert!(processor.get_handler(Path::new("test.rar")).is_err());
        assert!(processor.get_handler(Path::new("test.tar.gz")).is_err());
    }
}
