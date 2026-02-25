use image::open;
use crate::core::watermark::{embedder::WatermarkEmbedder, extractor::WatermarkExtractor};

/// Embed watermark into a single image (for preview)
///
/// # Arguments
/// * `image_path` - Path to input image
/// * `watermark_text` - Text to embed
/// * `strength` - Embedding strength (0.1 - 1.0)
///
/// # Returns
/// * PNG encoded bytes of watermarked image
#[tauri::command]
pub async fn embed_watermark_single(
    image_path: String,
    watermark_text: String,
    strength: f32,
) -> Result<Vec<u8>, String> {
    // Validate strength
    if !(0.1..=1.0).contains(&strength) {
        return Err(format!("Strength must be between 0.1 and 1.0, got {}", strength));
    }

    // Load image
    let image = open(&image_path)
        .map_err(|e| format!("Failed to load image {}: {}", image_path, e))?;

    // Create embedder
    let embedder = WatermarkEmbedder::new();

    // Embed watermark and return as PNG bytes
    let watermarked_bytes = embedder.embed_to_bytes(&image, &watermark_text, strength)
        .map_err(|e| format!("Failed to embed watermark: {}", e))?;

    Ok(watermarked_bytes)
}

/// Extract watermark from an image
///
/// # Arguments
/// * `image_path` - Path to watermarked image
///
/// # Returns
/// * Extracted MD5 hash string
#[tauri::command]
pub async fn extract_watermark(image_path: String) -> Result<String, String> {
    // Load image
    let image = open(&image_path)
        .map_err(|e| format!("Failed to load image {}: {}", image_path, e))?;

    // Create extractor
    let extractor = WatermarkExtractor::new();

    // Extract watermark
    let md5_hash = extractor.extract(&image)
        .map_err(|e| format!("Failed to extract watermark: {}", e))?;

    Ok(md5_hash)
}

/// Get image dimensions
///
/// # Arguments
/// * `image_path` - Path to image
///
/// # Returns
/// * (width, height) tuple
#[tauri::command]
pub async fn get_image_dimensions(image_path: String) -> Result<(u32, u32), String> {
    let image = open(&image_path)
        .map_err(|e| format!("Failed to load image: {}", e))?;

    let (width, height) = (image.width(), image.height());
    Ok((width, height))
}

/// Get number of logical CPU cores available for parallel processing
///
/// # Returns
/// * Number of logical CPU cores (used by Rayon thread pool)
#[tauri::command]
pub fn get_cpu_count() -> usize {
    num_cpus::get()
}
