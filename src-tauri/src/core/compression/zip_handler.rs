use std::path::Path;
use std::fs::{self, File};
use std::io;
use zip::{ZipArchive, ZipWriter, write::FileOptions, CompressionMethod};
use rayon::prelude::*;
use walkdir::WalkDir;
use crate::core::compression::common::ArchiveHandler;
use crate::models::BlindMarkError;

/// ZIP archive handler
///
/// Handles extraction and creation of ZIP archives while preserving directory hierarchy.
pub struct ZipHandler;

impl ZipHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ArchiveHandler for ZipHandler {
    /// Extract ZIP archive to destination directory
    ///
    /// # Arguments
    /// * `archive_path` - Path to ZIP file
    /// * `dest_dir` - Destination directory for extraction
    ///
    /// # Behavior
    /// - Preserves directory hierarchy
    /// - Creates parent directories as needed
    /// - Sets file permissions on Unix systems
    fn extract(&self, archive_path: &Path, dest_dir: &Path) -> Result<(), BlindMarkError> {
        let file = File::open(archive_path)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to open ZIP archive {}: {}", archive_path.display(), e)
            ))?;

        let mut archive = ZipArchive::new(file)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to read ZIP archive: {}", e)
            ))?;

        // Create destination directory if it doesn't exist
        fs::create_dir_all(dest_dir)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to create destination directory: {}", e)
            ))?;

        // Extract each file
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to read file at index {}: {}", i, e)
                ))?;

            let file_path = match file.enclosed_name() {
                Some(path) => path.to_owned(),
                None => continue, // Skip files with invalid names
            };

            let output_path = dest_dir.join(&file_path);

            if file.is_dir() {
                // Create directory
                fs::create_dir_all(&output_path)
                    .map_err(|e| BlindMarkError::Archive(
                        format!("Failed to create directory {}: {}", output_path.display(), e)
                    ))?;
            } else {
                // Create parent directories
                if let Some(parent) = output_path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| BlindMarkError::Archive(
                            format!("Failed to create parent directory {}: {}", parent.display(), e)
                        ))?;
                }

                // Extract file
                let mut output_file = File::create(&output_path)
                    .map_err(|e| BlindMarkError::Archive(
                        format!("Failed to create output file {}: {}", output_path.display(), e)
                    ))?;

                std::io::copy(&mut file, &mut output_file)
                    .map_err(|e| BlindMarkError::Archive(
                        format!("Failed to extract file {}: {}", file_path.display(), e)
                    ))?;

                // Set permissions on Unix systems
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(mode) = file.unix_mode() {
                        fs::set_permissions(&output_path, fs::Permissions::from_mode(mode))
                            .map_err(|e| BlindMarkError::Archive(
                                format!("Failed to set permissions: {}", e)
                            ))?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Create ZIP archive from source directory
    ///
    /// # Arguments
    /// * `source_dir` - Directory to archive
    /// * `output_path` - Path for output ZIP file
    ///
    /// # Behavior
    /// - Enumerates entries in a single pass, then reads all files in parallel with Rayon
    /// - Already-compressed formats (PNG, JPG, MP3…) are stored without re-compression
    /// - Text/data files use Deflate level 1 (fastest) for quick compression
    fn create(&self, source_dir: &Path, output_path: &Path) -> Result<(), BlindMarkError> {
        // === Step 1: Enumerate entries (single-threaded walk) ===
        let mut dir_names: Vec<String> = Vec::new();
        let mut file_infos: Vec<(std::path::PathBuf, String)> = Vec::new();

        for entry in WalkDir::new(source_dir).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            let relative = path.strip_prefix(source_dir)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to calculate relative path: {}", e)
                ))?;
            if relative.as_os_str().is_empty() {
                continue;
            }
            let name = relative.to_string_lossy().to_string();
            if path.is_dir() {
                dir_names.push(name);
            } else if path.is_file() {
                file_infos.push((path.to_path_buf(), name));
            }
        }

        // === Step 2: Read all files in parallel ===
        let file_data: Vec<(String, Vec<u8>)> = file_infos
            .into_par_iter()
            .map(|(path, name)| {
                let data = fs::read(&path)
                    .map_err(|e| BlindMarkError::Archive(
                        format!("Failed to read file {}: {}", path.display(), e)
                    ))?;
                Ok((name, data))
            })
            .collect::<Result<Vec<_>, BlindMarkError>>()?;

        // === Step 3: Write to ZIP (sequential — ZipWriter is not thread-safe) ===
        let file = File::create(output_path)
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to create ZIP file {}: {}", output_path.display(), e)
            ))?;
        let mut zip = ZipWriter::new(file);

        let stored_opts = FileOptions::<()>::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o755);

        for name in dir_names {
            zip.add_directory(&name, stored_opts)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to add directory {} to archive: {}", name, e)
                ))?;
        }

        for (name, data) in file_data {
            // Already-compressed formats: store as-is (zero CPU cost)
            // Text/binary formats: fast Deflate level 1
            let opts = if is_already_compressed(&name) {
                stored_opts
            } else {
                FileOptions::<()>::default()
                    .compression_method(CompressionMethod::Deflated)
                    .compression_level(Some(1))
                    .unix_permissions(0o755)
            };

            zip.start_file(&name, opts)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to start file {} in archive: {}", name, e)
                ))?;

            let mut cursor = io::Cursor::new(&data);
            io::copy(&mut cursor, &mut zip)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to write file {} to archive: {}", name, e)
                ))?;
        }

        zip.finish()
            .map_err(|e| BlindMarkError::Archive(
                format!("Failed to finalize ZIP archive: {}", e)
            ))?;

        Ok(())
    }

    /// Check if this handler supports the given archive
    ///
    /// Returns true for ZIP-compatible formats: .zip, .var (VaM package)
    fn supports(&self, archive_path: &Path) -> bool {
        archive_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                let lower = e.to_ascii_lowercase();
                lower == "zip" || lower == "var"
            })
            .unwrap_or(false)
    }
}

/// Returns true for formats that are already compressed and won't benefit from Deflate.
/// Storing them avoids wasting CPU trying to compress incompressible data.
fn is_already_compressed(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp"
            | "mp3" | "mp4" | "ogg" | "wav" | "aac" | "flac"
            | "zip" | "7z" | "rar" | "var"
    )
}

impl Default for ZipHandler {
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
        let handler = ZipHandler::new();
        assert!(handler.supports(Path::new("archive.zip")));
        assert!(handler.supports(Path::new("ARCHIVE.ZIP")));
        assert!(handler.supports(Path::new("package.var")));
        assert!(handler.supports(Path::new("Package.VAR")));
        assert!(!handler.supports(Path::new("archive.7z")));
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

        // Create ZIP
        let handler = ZipHandler::new();
        let zip_path = temp_archive.path().join("test.zip");
        handler.create(temp_source.path(), &zip_path).unwrap();

        assert!(zip_path.exists());

        // Extract ZIP
        handler.extract(&zip_path, temp_dest.path()).unwrap();

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
        let handler = ZipHandler::new();
        let zip_path = temp_archive.path().join("nested.zip");
        handler.create(temp_source.path(), &zip_path).unwrap();
        handler.extract(&zip_path, temp_dest.path()).unwrap();

        // Verify hierarchy
        assert!(temp_dest.path().join("a/b/c/deep.txt").exists());
        let content = fs::read_to_string(temp_dest.path().join("a/b/c/deep.txt")).unwrap();
        assert_eq!(content, "deep file");
    }

    #[test]
    fn test_extract_nonexistent_archive() {
        let handler = ZipHandler::new();
        let temp_dest = TempDir::new().unwrap();

        let result = handler.extract(Path::new("/nonexistent.zip"), temp_dest.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_create_empty_directory() {
        let temp_source = TempDir::new().unwrap();
        let temp_archive = TempDir::new().unwrap();

        let handler = ZipHandler::new();
        let zip_path = temp_archive.path().join("empty.zip");

        // Should succeed even with empty directory
        let result = handler.create(temp_source.path(), &zip_path);
        assert!(result.is_ok());
        assert!(zip_path.exists());
    }
}
