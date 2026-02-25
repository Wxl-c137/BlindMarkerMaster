use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Processing status for each file
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ProcessingStatus {
    Waiting,
    Extracting,
    Processing,
    Repackaging,
    Complete,
    Error(String),
}

/// Represents a file task with processing state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTask {
    pub id: String,
    pub filename: String,
    pub original_size: u64,
    pub md5_content: String,
    pub status: ProcessingStatus,
    pub progress: f32,  // 0.0 - 1.0
}

impl FileTask {
    pub fn new(id: String, filename: String, original_size: u64) -> Self {
        Self {
            id,
            filename,
            original_size,
            md5_content: String::new(),
            status: ProcessingStatus::Waiting,
            progress: 0.0,
        }
    }

    pub fn set_status(&mut self, status: ProcessingStatus) {
        self.status = status;
    }

    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
    }

    pub fn set_md5(&mut self, md5: String) {
        self.md5_content = md5;
    }
}

/// Represents an image file found in the archive
#[derive(Debug, Clone)]
pub struct ImageFile {
    /// Relative path from archive root (preserves hierarchy)
    pub relative_path: String,
    /// Temporary path on disk
    pub temp_path: PathBuf,
}

impl ImageFile {
    pub fn new(relative_path: String, temp_path: PathBuf) -> Self {
        Self {
            relative_path,
            temp_path,
        }
    }
}
