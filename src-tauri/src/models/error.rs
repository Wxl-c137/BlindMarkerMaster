use thiserror::Error;

/// Custom error types for BlindMark Master application
#[derive(Error, Debug)]
pub enum BlindMarkError {
    #[error("Archive error: {0}")]
    Archive(String),

    #[error("Unsupported archive format: {0}")]
    UnsupportedArchive(String),

    #[error("Image processing error: {0}")]
    ImageProcessing(String),

    #[error("Unsupported image format: {0}")]
    UnsupportedImage(String),

    #[error("Watermark embedding failed: {0}")]
    EmbeddingFailed(String),

    #[error("Watermark extraction failed: {0}")]
    ExtractionFailed(String),

    #[error("Excel reading error: {0}")]
    ExcelError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Corrupted archive: {0}")]
    CorruptedArchive(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

// Convert to string for Tauri (commands must return Result<T, String>)
impl From<BlindMarkError> for String {
    fn from(err: BlindMarkError) -> String {
        err.to_string()
    }
}
