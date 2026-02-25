use std::path::Path;
use std::fs::{self, File};
use std::io;
use std::path::PathBuf;
use encoding_rs::GBK;
use zip::{ZipArchive, ZipWriter, write::FullFileOptions, CompressionMethod, HasZipMetadata};
use rayon::prelude::*;
use walkdir::WalkDir;
use crate::core::compression::common::ArchiveHandler;
use crate::models::BlindMarkError;

/// Detect and decode a ZIP entry filename from its raw bytes.
///
/// ZIP archives may store filenames in several encodings depending on which
/// tool created them:
///
/// | Scenario                          | `is_utf8` | Raw bytes       | Action           |
/// |-----------------------------------|-----------|-----------------|------------------|
/// | Modern ZIP (EFS flag or 0x7075)   | true      | UTF-8           | Use directly     |
/// | Modern ZIP, no EFS flag           | false     | UTF-8           | Valid UTF-8 → OK |
/// | Old Chinese Windows ZIP           | false     | GBK / GB2312    | GBK decode       |
/// | Other / mixed                     | false     | CP437 / unknown | Lossy UTF-8      |
fn decode_zip_filename(raw: &[u8], is_utf8: bool) -> String {
    // EFS flag set or pure ASCII → trust the zip crate's decoding.
    if is_utf8 || raw.is_ascii() {
        return String::from_utf8_lossy(raw).into_owned();
    }

    // Non-ASCII bytes without EFS flag.
    // 1. Try strict UTF-8 first: many modern tools omit the EFS flag but still
    //    write UTF-8 filenames (e.g. some macOS / Linux tools).
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_owned();
    }

    // 2. Not valid UTF-8 → try GBK.  This is the standard encoding for
    //    filenames in ZIPs created by Windows Explorer and many older Chinese
    //    Windows tools (WinZip, 好压, 360压缩, etc.).
    let (cow, _enc, had_errors) = GBK.decode(raw);
    if !had_errors {
        return cow.into_owned();
    }

    // 3. GBK also had replacement characters → last resort: lossy UTF-8.
    String::from_utf8_lossy(raw).into_owned()
}

/// Sanitize a decoded ZIP filename to prevent path-traversal attacks.
///
/// Replicates the logic of `ZipFile::enclosed_name()` but works on an
/// arbitrary `&str` (needed after we re-decode the filename ourselves).
fn sanitize_zip_path(name: &str) -> Option<PathBuf> {
    if name.contains('\0') {
        return None;
    }
    let path = PathBuf::from(name);
    let mut depth: usize = 0;
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => return None,
            std::path::Component::ParentDir => depth = depth.checked_sub(1)?,
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::CurDir => {}
        }
    }
    Some(path)
}

/// Build `FullFileOptions` for a ZIP entry.
///
/// zip 2.x 对任何非 ASCII 文件名自动设置 EFS 标志（通用标志位 bit 11），
/// 直接将文件名以 UTF-8 写入本地文件头，所有现代工具均能正确识别。
/// 无需手动注入 Unicode Path Extra Field（0x7075），否则 zip 库内部会
/// 校验 payload 里的 CRC32 与实际写入的文件名字节是否一致，对目录条目
/// 等特殊情况容易产生不匹配而报错。
fn file_opts(method: CompressionMethod, level: Option<i64>, _name: &str) -> Result<FullFileOptions<'static>, BlindMarkError> {
    // EFS 标志由 zip 库自动处理，此处不再手动添加 0x7075 字段
    let mut opts = FullFileOptions::default().compression_method(method);
    #[cfg(unix)]
    {
        opts = opts.unix_permissions(0o755);
    }
    if let Some(lvl) = level {
        opts = opts.compression_level(Some(lvl));
    }
    Ok(opts)
}

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

            // --- Encoding-aware filename decoding ---
            // Copy needed fields before borrowing `file` for I/O.
            // zip 2.x already handles the EFS flag (bit 11) and the Unicode
            // Path Extra Field (0x7075); is_utf8 reflects both.
            let (is_utf8, raw_name) = {
                let meta = file.get_metadata();
                (meta.is_utf8, meta.file_name_raw.to_vec())
            };
            let decoded_name = decode_zip_filename(&raw_name, is_utf8);

            // Sanitize to prevent path-traversal (replaces enclosed_name()).
            let file_path = match sanitize_zip_path(&decoded_name) {
                Some(p) => p,
                None => continue, // Skip invalid / unsafe paths
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
            let name = relative.to_string_lossy().replace('\\', "/");
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

        for name in dir_names {
            let stored_name = if name.ends_with('/') {
                name.clone()
            } else {
                format!("{}/", name)
            };
            let opts = file_opts(CompressionMethod::Stored, None, &stored_name)?;
            zip.add_directory(&stored_name, opts)
                .map_err(|e| BlindMarkError::Archive(
                    format!("Failed to add directory {} to archive: {}", stored_name, e)
                ))?;
        }

        for (name, data) in file_data {
            // Already-compressed formats: store as-is (zero CPU cost)
            // Text/binary formats: fast Deflate level 1
            let opts = if is_already_compressed(&name) {
                file_opts(CompressionMethod::Stored, None, &name)?
            } else {
                file_opts(CompressionMethod::Deflated, Some(1), &name)?
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
