pub mod error;
pub mod task;
pub mod config;

// Re-export commonly used types
pub use error::BlindMarkError;
pub use task::ImageFile;
pub use config::{WatermarkConfig, WatermarkSource, WatermarkData};
