use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Progress event for image-level updates (existing, used by parallel processor)
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub current_file: usize,
    pub total_files: usize,
    pub filename: String,
    pub progress: f32,
    pub status: String,
}

/// Status update event for overall processing
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusEvent {
    pub status: String,
    pub message: String,
}

/// Emitted once after scanning, before processing begins.
/// Tells the frontend how many files of each type were found.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummaryEvent {
    pub json_count: usize,
    pub vaj_count: usize,
    pub vmi_count: usize,
    pub image_count: usize,
}

/// Emitted for each individual file as it starts being processed.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailProgressEvent {
    /// Current watermark index (1-based); equals 1 for single-watermark runs
    pub batch_current: usize,
    /// Total watermarks to process
    pub batch_total: usize,
    /// File category: "json" | "vaj" | "vmi" | "image"
    pub file_type: String,
    /// Index of current file within its category (1-based)
    pub type_current: usize,
    /// Total files in this category
    pub type_total: usize,
    /// Filename (not full path) of the file being processed
    pub filename: String,
}

pub struct ProgressEmitter {
    app: AppHandle,
}

impl ProgressEmitter {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// Emit image-level progress (used by parallel processor)
    pub fn emit_progress(
        &self,
        current_file: usize,
        total_files: usize,
        filename: String,
        progress: f32,
        status: String,
    ) -> Result<(), String> {
        let event = ProgressEvent { current_file, total_files, filename, progress, status };
        self.app
            .emit("watermark-progress", event)
            .map_err(|e| format!("Failed to emit progress event: {}", e))
    }

    /// Emit overall status update
    pub fn emit_status(&self, status: String, message: String) -> Result<(), String> {
        let event = StatusEvent { status, message };
        self.app
            .emit("watermark-status", event)
            .map_err(|e| format!("Failed to emit status event: {}", e))
    }

    /// Emit scan summary (once per archive run, after scanning)
    pub fn emit_scan_summary(
        &self,
        json_count: usize,
        vaj_count: usize,
        vmi_count: usize,
        image_count: usize,
    ) -> Result<(), String> {
        let event = ScanSummaryEvent { json_count, vaj_count, vmi_count, image_count };
        self.app
            .emit("watermark-scan-summary", event)
            .map_err(|e| format!("Failed to emit scan summary: {}", e))
    }

    /// Emit per-file detail progress
    pub fn emit_detail_progress(
        &self,
        batch_current: usize,
        batch_total: usize,
        file_type: &str,
        type_current: usize,
        type_total: usize,
        filename: &str,
    ) -> Result<(), String> {
        let event = DetailProgressEvent {
            batch_current,
            batch_total,
            file_type: file_type.to_string(),
            type_current,
            type_total,
            filename: filename.to_string(),
        };
        self.app
            .emit("watermark-detail-progress", event)
            .map_err(|e| format!("Failed to emit detail progress: {}", e))
    }

    /// Emit completion event
    pub fn emit_complete(&self, output_path: String) -> Result<(), String> {
        self.emit_status("complete".to_string(), format!("Processing complete: {}", output_path))
    }

    /// Emit error event
    pub fn emit_error(&self, error: String) -> Result<(), String> {
        self.emit_status("error".to_string(), error)
    }
}
