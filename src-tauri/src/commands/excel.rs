use calamine::{Reader, open_workbook, Xlsx};

/// Read watermark texts from Excel file (first column), synchronous core implementation.
///
/// # Behavior
/// - Reads first worksheet
/// - Extracts first column values
/// - Skips row 0 (treated as header)
/// - Stops at first empty cell
pub(crate) fn read_excel_core(excel_path: &str) -> Result<Vec<String>, String> {
    let mut workbook: Xlsx<_> = open_workbook(excel_path)
        .map_err(|e| format!("打开 Excel 失败: {}", e))?;

    let worksheet_names = workbook.sheet_names();
    if worksheet_names.is_empty() {
        return Err("Excel 文件没有工作表".to_string());
    }

    let first_sheet_name = worksheet_names[0].clone();
    let range = workbook
        .worksheet_range(&first_sheet_name)
        .map_err(|e| format!("读取工作表失败: {}", e))?;

    let mut watermarks = Vec::new();

    // 从第 1 行开始（跳过第 0 行表头）
    for row_idx in 1..range.height() {
        if let Some(cell) = range.get((row_idx, 0)) {
            let text = cell.to_string();
            if text.trim().is_empty() {
                break;
            }
            watermarks.push(text);
        } else {
            break;
        }
    }

    if watermarks.is_empty() {
        return Err("Excel 第一列未找到水印文本（第 0 行视为表头，从第 1 行读取）".to_string());
    }

    Ok(watermarks)
}

/// Read watermark texts from Excel file (Tauri command, wraps `read_excel_core`)
#[tauri::command]
pub async fn read_excel_watermarks(excel_path: String) -> Result<Vec<String>, String> {
    read_excel_core(&excel_path)
}
