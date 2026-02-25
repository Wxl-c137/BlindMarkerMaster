use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use crate::models::ImageFile;

/// Recursive file scanner for finding supported images
///
/// Scans directories recursively and filters for PNG/JPEG/JPG files.
/// Maintains relative paths for preserving directory hierarchy.
pub struct FileScanner {
    supported_extensions: Vec<&'static str>,
}

impl FileScanner {
    /// Create a new file scanner with default supported formats (PNG, JPEG, JPG)
    pub fn new() -> Self {
        Self {
            supported_extensions: vec!["png", "jpg", "jpeg"],
        }
    }

    /// Create a file scanner with custom supported extensions
    pub fn with_extensions(extensions: Vec<&'static str>) -> Self {
        Self {
            supported_extensions: extensions,
        }
    }

    /// Scan a directory recursively for supported image files
    ///
    /// # Arguments
    /// * `root_path` - Root directory to scan
    ///
    /// # Returns
    /// * Vector of `ImageFile` sorted by relative path (for Excel sequential mapping)
    ///
    /// # Example
    /// ```ignore
    /// let scanner = FileScanner::new();
    /// let images = scanner.scan(Path::new("/tmp/extracted"))?;
    /// // images[0] corresponds to Excel row 1, images[1] to row 2, etc.
    /// ```
    pub fn scan(&self, root_path: &Path) -> Result<Vec<ImageFile>, std::io::Error> {
        let mut images = Vec::new();

        // Walk directory tree
        for entry in WalkDir::new(root_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip directories
            if !path.is_file() {
                continue;
            }

            // Check if file has supported extension
            if let Some(extension) = path.extension() {
                let ext_str = extension.to_string_lossy().to_lowercase();

                if self.supported_extensions.contains(&ext_str.as_str()) {
                    // Calculate relative path from root
                    let relative_path = path.strip_prefix(root_path)
                        .map_err(|e| std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to calculate relative path: {}", e)
                        ))?
                        .to_path_buf();

                    images.push(ImageFile::new(
                        relative_path.to_string_lossy().to_string(),
                        path.to_path_buf(),
                    ));
                }
            }
        }

        // Sort by relative path for consistent ordering (critical for Excel mapping)
        // Row 1 in Excel → images[0], Row 2 → images[1], etc.
        images.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        Ok(images)
    }

    /// Count total number of supported images in a directory
    pub fn count_images(&self, root_path: &Path) -> Result<usize, std::io::Error> {
        Ok(self.scan(root_path)?.len())
    }

    /// Check if a file is a supported image based on extension
    pub fn is_supported(&self, path: &Path) -> bool {
        if let Some(extension) = path.extension() {
            let ext_str = extension.to_string_lossy().to_lowercase();
            self.supported_extensions.contains(&ext_str.as_str())
        } else {
            false
        }
    }

    /// Get list of supported extensions
    pub fn supported_extensions(&self) -> &[&'static str] {
        &self.supported_extensions
    }

    /// Scan and group images by directory
    ///
    /// Returns a map of directory path to list of images in that directory
    pub fn scan_grouped(&self, root_path: &Path) -> Result<std::collections::HashMap<PathBuf, Vec<ImageFile>>, std::io::Error> {
        let images = self.scan(root_path)?;
        let mut grouped = std::collections::HashMap::new();

        for image in images {
            let dir = Path::new(&image.relative_path)
                .parent()
                .unwrap_or(Path::new(""))
                .to_path_buf();

            grouped.entry(dir).or_insert_with(Vec::new).push(image);
        }

        Ok(grouped)
    }

    /// 扫描目录中指定扩展名的所有文本文件
    ///
    /// 返回按相对路径排序的 (绝对路径, 相对路径) 列表
    pub fn scan_files_by_extension(&self, root_path: &Path, extension: &str) -> Result<Vec<(PathBuf, PathBuf)>, std::io::Error> {
        let mut files = Vec::new();

        for entry in WalkDir::new(root_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            if let Some(ext) = path.extension() {
                let ext_lower = ext.to_string_lossy().to_lowercase();
                if ext_lower == extension {
                    let relative = path
                        .strip_prefix(root_path)
                        .map_err(|e| std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("相对路径计算失败: {}", e),
                        ))?
                        .to_path_buf();
                    files.push((path.to_path_buf(), relative));
                }
            }
        }

        files.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(files)
    }

    /// 扫描目录中的所有 JSON 文件（.json 扩展名）
    pub fn scan_json_files(&self, root_path: &Path) -> Result<Vec<(PathBuf, PathBuf)>, std::io::Error> {
        self.scan_files_by_extension(root_path, "json")
    }

    /// 扫描目录中的所有 VAJ 文件（.vaj 扩展名，VaM 场景/资源 JSON）
    pub fn scan_vaj_files(&self, root_path: &Path) -> Result<Vec<(PathBuf, PathBuf)>, std::io::Error> {
        self.scan_files_by_extension(root_path, "vaj")
    }

    /// 扫描目录中的所有 VMI 文件（.vmi 扩展名，VaM 形态 JSON）
    pub fn scan_vmi_files(&self, root_path: &Path) -> Result<Vec<(PathBuf, PathBuf)>, std::io::Error> {
        self.scan_files_by_extension(root_path, "vmi")
    }
}

impl Default for FileScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_structure() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create directory structure with various files
        fs::create_dir_all(base.join("images/photos")).unwrap();
        fs::create_dir_all(base.join("images/screenshots")).unwrap();
        fs::create_dir_all(base.join("documents")).unwrap();

        // Create image files
        fs::write(base.join("image1.png"), b"fake png").unwrap();
        fs::write(base.join("image2.jpg"), b"fake jpg").unwrap();
        fs::write(base.join("image3.JPEG"), b"fake jpeg").unwrap();
        fs::write(base.join("images/photo.png"), b"photo").unwrap();
        fs::write(base.join("images/photos/vacation.jpg"), b"vacation").unwrap();
        fs::write(base.join("images/screenshots/screen.PNG"), b"screen").unwrap();

        // Create non-image files (should be ignored)
        fs::write(base.join("readme.txt"), b"text file").unwrap();
        fs::write(base.join("data.json"), b"{}").unwrap();
        fs::write(base.join("documents/report.pdf"), b"pdf").unwrap();

        temp_dir
    }

    #[test]
    fn test_scan_finds_all_images() {
        let temp_dir = create_test_structure();
        let scanner = FileScanner::new();

        let images = scanner.scan(temp_dir.path()).unwrap();

        // Should find 6 image files
        assert_eq!(images.len(), 6);
    }

    #[test]
    fn test_scan_sorts_by_path() {
        let temp_dir = create_test_structure();
        let scanner = FileScanner::new();

        let images = scanner.scan(temp_dir.path()).unwrap();

        // Images should be sorted alphabetically by relative path
        for i in 0..images.len() - 1 {
            assert!(images[i].relative_path <= images[i + 1].relative_path);
        }
    }

    #[test]
    fn test_scan_preserves_relative_paths() {
        let temp_dir = create_test_structure();
        let scanner = FileScanner::new();

        let images = scanner.scan(temp_dir.path()).unwrap();

        // Find the nested image
        let vacation = images.iter()
            .find(|img| img.relative_path.contains("vacation.jpg"))
            .expect("Should find vacation.jpg");

        // Relative path should preserve directory structure
        assert!(vacation.relative_path.contains("images"));
        assert!(vacation.relative_path.contains("photos"));
    }

    #[test]
    fn test_scan_case_insensitive_extensions() {
        let temp_dir = create_test_structure();
        let scanner = FileScanner::new();

        let images = scanner.scan(temp_dir.path()).unwrap();

        // Should find both .JPEG and .PNG (uppercase extensions)
        let has_uppercase = images.iter().any(|img| {
            img.temp_path.to_string_lossy().contains(".JPEG") ||
            img.temp_path.to_string_lossy().contains(".PNG")
        });
        assert!(has_uppercase, "Should handle uppercase extensions");
    }

    #[test]
    fn test_scan_ignores_non_images() {
        let temp_dir = create_test_structure();
        let scanner = FileScanner::new();

        let images = scanner.scan(temp_dir.path()).unwrap();

        // Should not include .txt, .json, .pdf files
        for image in &images {
            let path_str = image.temp_path.to_string_lossy();
            assert!(!path_str.ends_with(".txt"));
            assert!(!path_str.ends_with(".json"));
            assert!(!path_str.ends_with(".pdf"));
        }
    }

    #[test]
    fn test_count_images() {
        let temp_dir = create_test_structure();
        let scanner = FileScanner::new();

        let count = scanner.count_images(temp_dir.path()).unwrap();
        assert_eq!(count, 6);
    }

    #[test]
    fn test_is_supported() {
        let scanner = FileScanner::new();

        assert!(scanner.is_supported(Path::new("image.png")));
        assert!(scanner.is_supported(Path::new("photo.jpg")));
        assert!(scanner.is_supported(Path::new("pic.JPEG")));
        assert!(!scanner.is_supported(Path::new("document.pdf")));
        assert!(!scanner.is_supported(Path::new("file.txt")));
        assert!(!scanner.is_supported(Path::new("no_extension")));
    }

    #[test]
    fn test_custom_extensions() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("image.gif"), b"gif").unwrap();
        fs::write(temp_dir.path().join("image.webp"), b"webp").unwrap();
        fs::write(temp_dir.path().join("image.png"), b"png").unwrap();

        let scanner = FileScanner::with_extensions(vec!["gif", "webp"]);
        let images = scanner.scan(temp_dir.path()).unwrap();

        // Should only find .gif and .webp
        assert_eq!(images.len(), 2);
    }

    #[test]
    fn test_scan_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let scanner = FileScanner::new();

        let images = scanner.scan(temp_dir.path()).unwrap();
        assert_eq!(images.len(), 0);
    }

    #[test]
    fn test_scan_grouped() {
        let temp_dir = create_test_structure();
        let scanner = FileScanner::new();

        let grouped = scanner.scan_grouped(temp_dir.path()).unwrap();

        // Should have multiple directories
        assert!(grouped.len() > 1);

        // Root directory should have some images
        let root_images = grouped.get(Path::new("")).unwrap();
        assert!(root_images.len() > 0);
    }

    #[test]
    fn test_supported_extensions() {
        let scanner = FileScanner::new();
        let extensions = scanner.supported_extensions();

        assert_eq!(extensions.len(), 3);
        assert!(extensions.contains(&"png"));
        assert!(extensions.contains(&"jpg"));
        assert!(extensions.contains(&"jpeg"));
    }
}
