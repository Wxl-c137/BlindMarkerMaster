use serde::{Deserialize, Serialize};

/// Watermark configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatermarkConfig {
    /// Strength factor (0.1 - 1.0)
    pub strength: f32,
    /// Source of watermark data
    pub watermark_source: WatermarkSource,
    /// Custom JSON field name for the watermark (default: "_watermark")
    #[serde(default)]
    pub watermark_key: Option<String>,
}

impl WatermarkConfig {
    pub fn new(strength: f32, watermark_source: WatermarkSource) -> Self {
        Self {
            strength: strength.clamp(0.1, 1.0),
            watermark_source,
            watermark_key: None,
        }
    }
}

/// Source of watermark data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WatermarkSource {
    /// Single text watermark for all images
    SingleText { content: String },
    /// Excel file with one watermark per row (sequential mapping)
    ExcelFile { path: String },
}

/// Watermark data after encoding
#[derive(Debug, Clone)]
pub struct WatermarkData {
    /// MD5 hash of the original text
    pub md5_hash: String,
    /// Binary sequence (128 bits) derived from MD5
    pub binary_sequence: Vec<u8>,
}

impl WatermarkData {
    pub fn new(md5_hash: String, binary_sequence: Vec<u8>) -> Self {
        Self {
            md5_hash,
            binary_sequence,
        }
    }
}
