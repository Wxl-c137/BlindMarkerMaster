// Module declarations
mod models;
mod core;
mod commands;
mod utils;

use commands::watermark::{embed_watermark_single, extract_watermark, get_image_dimensions, get_cpu_count};
use commands::excel::read_excel_watermarks;
use commands::archive::{process_archive, extract_json_watermark_from_archive, scan_watermarks_in_archive, list_images_in_archive, scan_image_watermarks_in_archive, scan_all_watermarks_in_archive};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_icon(tauri::include_image!("icons/icon.png"));
            }
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            embed_watermark_single,
            extract_watermark,
            get_image_dimensions,
            get_cpu_count,
            read_excel_watermarks,
            process_archive,
            extract_json_watermark_from_archive,
            scan_watermarks_in_archive,
            list_images_in_archive,
            scan_image_watermarks_in_archive,
            scan_all_watermarks_in_archive,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
