use rayon::prelude::*;
use std::sync::{Arc, Mutex};
use image::open;
use crate::core::watermark::embedder::WatermarkEmbedder;
use crate::models::{ImageFile, BlindMarkError};
use crate::utils::progress::ProgressEmitter;

/// Parallel processor for batch watermarking
///
/// Uses Rayon for CPU-bound parallel processing of images.
pub struct ParallelProcessor {
    thread_count: usize,
}

impl ParallelProcessor {
    /// Create a new parallel processor
    ///
    /// Uses all available CPU cores by default
    pub fn new() -> Self {
        Self {
            thread_count: num_cpus::get(),
        }
    }

    /// Create a parallel processor with custom thread count
    pub fn with_threads(thread_count: usize) -> Self {
        Self { thread_count }
    }

    /// Process batch of images in parallel with single watermark text
    ///
    /// # Arguments
    /// * `images` - List of images to process
    /// * `watermark_text` - Single watermark text for all images
    /// * `strength` - Embedding strength
    /// * `output_dir` - Output directory path
    /// * `progress` - Optional progress emitter
    /// * `fast_mode` - When true, images with both dimensions > 512px are processed
    ///                 only in their top-left 512×512 ROI for faster throughput.
    ///
    /// # Returns
    /// * Number of successfully processed images
    pub fn process_batch_single(
        &self,
        images: &[ImageFile],
        watermark_text: &str,
        strength: f32,
        output_dir: &std::path::Path,
        progress: Option<Arc<ProgressEmitter>>,
        fast_mode: bool,
    ) -> Result<usize, BlindMarkError> {
        let total_files = images.len();
        let processed_count = Arc::new(Mutex::new(0usize));
        let embedder = WatermarkEmbedder::new();

        // Configure Rayon thread pool
        rayon::ThreadPoolBuilder::new()
            .num_threads(self.thread_count)
            .build()
            .map_err(|e| BlindMarkError::ImageProcessing(
                format!("Failed to create thread pool: {}", e)
            ))?
            .install(|| {
                images.par_iter().try_for_each(|image_file| {
                    let output_path = output_dir.join(&image_file.relative_path);
                    if let Some(parent) = output_path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| BlindMarkError::ImageProcessing(
                                format!("Failed to create output directory: {}", e)
                            ))?;
                    }

                    // Image watermark only supports PNG (lossless).
                    // JPEG files are copied as-is without watermarking.
                    let is_jpeg = output_path.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_lowercase())
                        .map(|e| e == "jpg" || e == "jpeg")
                        .unwrap_or(false);

                    if is_jpeg {
                        std::fs::copy(&image_file.temp_path, &output_path)
                            .map_err(|e| BlindMarkError::ImageProcessing(
                                format!("Failed to copy {}: {}", image_file.relative_path, e)
                            ))?;
                    } else {
                        // Load image, embed watermark, save
                        let img = open(&image_file.temp_path)
                            .map_err(|e| BlindMarkError::ImageProcessing(
                                format!("Failed to load {}: {}", image_file.relative_path, e)
                            ))?;
                        let watermarked = embedder.embed_raw_text(&img, watermark_text, strength, fast_mode)?;
                        watermarked.save(&output_path)
                            .map_err(|e| BlindMarkError::ImageProcessing(
                                format!("Failed to save {}: {}", output_path.display(), e)
                            ))?;
                    }

                    // Update processed count and emit progress after completion (1-based, monotonically increasing)
                    let completed = {
                        let mut count = processed_count.lock().unwrap_or_else(|e| e.into_inner());
                        *count += 1;
                        *count
                    };
                    if let Some(ref emitter) = progress {
                        let _ = emitter.emit_progress(
                            completed,
                            total_files,
                            image_file.relative_path.clone(),
                            (completed as f32 / total_files as f32) * 100.0,
                            "processing".to_string(),
                        );
                    }

                    Ok::<(), BlindMarkError>(())
                })
            })?;

        let final_count = *processed_count.lock().unwrap_or_else(|e| e.into_inner());
        Ok(final_count)
    }

    /// Process batch of images with Excel watermark mapping
    ///
    /// # Arguments
    /// * `images` - List of images (sorted by relative path)
    /// * `watermarks` - List of watermark texts from Excel
    /// * `strength` - Embedding strength
    /// * `output_dir` - Output directory path
    /// * `progress` - Optional progress emitter
    /// * `fast_mode` - When true, large images (both dims > 512px) use ROI processing.
    ///
    /// # Behavior
    /// Maps watermarks sequentially: images[0] → watermarks[0], images[1] → watermarks[1], etc.
    /// If there are more images than watermarks, remaining images get the last watermark.
    pub fn process_batch_excel(
        &self,
        images: &[ImageFile],
        watermarks: &[String],
        strength: f32,
        output_dir: &std::path::Path,
        progress: Option<Arc<ProgressEmitter>>,
        fast_mode: bool,
    ) -> Result<usize, BlindMarkError> {
        if watermarks.is_empty() {
            return Err(BlindMarkError::InvalidConfig(
                "No watermarks provided".to_string()
            ));
        }

        let total_files = images.len();
        let processed_count = Arc::new(Mutex::new(0usize));
        let embedder = WatermarkEmbedder::new();

        // Configure Rayon thread pool
        rayon::ThreadPoolBuilder::new()
            .num_threads(self.thread_count)
            .build()
            .map_err(|e| BlindMarkError::ImageProcessing(
                format!("Failed to create thread pool: {}", e)
            ))?
            .install(|| {
                images.par_iter().enumerate().try_for_each(|(index, image_file)| {
                    // Get watermark text (use last one if index exceeds watermarks)
                    let watermark_index = index.min(watermarks.len() - 1);
                    let watermark_text = &watermarks[watermark_index];

                    let output_path = output_dir.join(&image_file.relative_path);
                    if let Some(parent) = output_path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| BlindMarkError::ImageProcessing(
                                format!("Failed to create output directory: {}", e)
                            ))?;
                    }

                    // Image watermark only supports PNG (lossless).
                    // JPEG files are copied as-is without watermarking.
                    let is_jpeg = output_path.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_lowercase())
                        .map(|e| e == "jpg" || e == "jpeg")
                        .unwrap_or(false);

                    if is_jpeg {
                        std::fs::copy(&image_file.temp_path, &output_path)
                            .map_err(|e| BlindMarkError::ImageProcessing(
                                format!("Failed to copy {}: {}", image_file.relative_path, e)
                            ))?;
                    } else {
                        let img = open(&image_file.temp_path)
                            .map_err(|e| BlindMarkError::ImageProcessing(
                                format!("Failed to load {}: {}", image_file.relative_path, e)
                            ))?;
                        let watermarked = embedder.embed_raw_text(&img, watermark_text, strength, fast_mode)?;
                        watermarked.save(&output_path)
                            .map_err(|e| BlindMarkError::ImageProcessing(
                                format!("Failed to save {}: {}", output_path.display(), e)
                            ))?;
                    }

                    // Update processed count and emit progress after completion (1-based, monotonically increasing)
                    let completed = {
                        let mut count = processed_count.lock().unwrap_or_else(|e| e.into_inner());
                        *count += 1;
                        *count
                    };
                    if let Some(ref emitter) = progress {
                        let _ = emitter.emit_progress(
                            completed,
                            total_files,
                            format!("{} -> {}", image_file.relative_path, watermark_text),
                            (completed as f32 / total_files as f32) * 100.0,
                            "processing".to_string(),
                        );
                    }

                    Ok::<(), BlindMarkError>(())
                })
            })?;

        let final_count = *processed_count.lock().unwrap_or_else(|e| e.into_inner());
        Ok(final_count)
    }

    /// Get configured thread count
    pub fn thread_count(&self) -> usize {
        self.thread_count
    }
}

impl Default for ParallelProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_image(path: &std::path::Path, width: u32, height: u32) {
        let img = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([
                (x * 4) as u8,
                (y * 4) as u8,
                128u8,
            ])
        });
        img.save(path).unwrap();
    }

    #[test]
    fn test_parallel_processor_creation() {
        let processor = ParallelProcessor::new();
        assert!(processor.thread_count() > 0);

        let custom_processor = ParallelProcessor::with_threads(4);
        assert_eq!(custom_processor.thread_count(), 4);
    }

    #[test]
    fn test_process_batch_single() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = TempDir::new().unwrap();

        // Create test images - 256×256 for sufficient LH2 capacity (544-bit text watermarks)
        let img1_path = temp_dir.path().join("img1.png");
        let img2_path = temp_dir.path().join("img2.png");
        create_test_image(&img1_path, 256, 256);
        create_test_image(&img2_path, 256, 256);

        let images = vec![
            ImageFile::new("img1.png".to_string(), img1_path),
            ImageFile::new("img2.png".to_string(), img2_path),
        ];

        let processor = ParallelProcessor::new();
        let result = processor.process_batch_single(
            &images,
            "Test watermark",
            0.5,
            output_dir.path(),
            None,
            false,
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);

        // Verify output files exist
        assert!(output_dir.path().join("img1.png").exists());
        assert!(output_dir.path().join("img2.png").exists());
    }

    #[test]
    fn test_process_batch_jpeg_copied_as_is() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = TempDir::new().unwrap();

        // Create a PNG first, save as JPEG to simulate JPEG input
        let img_src = temp_dir.path().join("img1_src.png");
        create_test_image(&img_src, 256, 256);
        let src_img = image::open(&img_src).unwrap();
        let jpg_path = temp_dir.path().join("img1.jpg");
        src_img.save(&jpg_path).unwrap();

        let images = vec![
            ImageFile::new("img1.jpg".to_string(), jpg_path),
        ];

        let processor = ParallelProcessor::new();
        let result = processor.process_batch_single(
            &images,
            "JPEG test watermark",
            0.5,
            output_dir.path(),
            None,
            false,
        );

        assert!(result.is_ok(), "JPEG processing should succeed: {:?}", result.err());
        // JPEG should be copied as-is (no format change, no watermark)
        assert!(output_dir.path().join("img1.jpg").exists(), "JPEG should be copied as-is with .jpg extension");
        assert!(!output_dir.path().join("img1.png").exists(), "No .png conversion should occur");
    }

    #[test]
    fn test_process_batch_excel() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = TempDir::new().unwrap();

        // Create test images - 256×256 for sufficient LH2 capacity (544-bit text watermarks)
        let img1_path = temp_dir.path().join("img1.png");
        let img2_path = temp_dir.path().join("img2.png");
        create_test_image(&img1_path, 256, 256);
        create_test_image(&img2_path, 256, 256);

        let images = vec![
            ImageFile::new("img1.png".to_string(), img1_path),
            ImageFile::new("img2.png".to_string(), img2_path),
        ];

        let watermarks = vec!["Mark 1".to_string(), "Mark 2".to_string()];

        let processor = ParallelProcessor::new();
        let result = processor.process_batch_excel(
            &images,
            &watermarks,
            0.5,
            output_dir.path(),
            None,
            false,
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);
    }

    #[test]
    fn test_process_batch_excel_more_images_than_watermarks() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = TempDir::new().unwrap();

        // Create 3 test images
        let img_paths: Vec<_> = (0..3)
            .map(|i| {
                let path = temp_dir.path().join(format!("img{}.png", i));
                create_test_image(&path, 256, 256);
                path
            })
            .collect();

        let images: Vec<_> = img_paths
            .iter()
            .enumerate()
            .map(|(i, path)| ImageFile::new(format!("img{}.png", i), path.clone()))
            .collect();

        // Only 2 watermarks for 3 images
        let watermarks = vec!["Mark 1".to_string(), "Mark 2".to_string()];

        let processor = ParallelProcessor::new();
        let result = processor.process_batch_excel(
            &images,
            &watermarks,
            0.5,
            output_dir.path(),
            None,
            false,
        );

        // Should succeed, 3rd image gets last watermark
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 3);
    }
}
