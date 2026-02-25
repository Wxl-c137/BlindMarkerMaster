use std::sync::Arc;
use std::path::Path;
use tauri::AppHandle;
use serde::Serialize;
use crate::models::{WatermarkConfig, WatermarkSource};
use super::excel::read_excel_core;
use crate::core::{
    compression::ArchiveProcessor,
    file_ops::{temp_manager::TempWorkspace, scanner::FileScanner},
    watermark::{JsonWatermarker, json_marker::DEFAULT_WATERMARK_KEY},
};
use crate::utils::{progress::ProgressEmitter, parallel::ParallelProcessor};
use crate::core::watermark::extractor::WatermarkExtractor;

/// 单个文件的水印提取结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatermarkFinding {
    /// 文件在压缩包中的相对路径
    pub file: String,
    /// 解码后的显示值（明文/MD5哈希/解密原文）
    pub value: String,
    /// 水印编码模式："md5" / "plaintext" / "aes" / "unknown"
    pub mode: String,
    /// AES 模式下是否成功解密；其他模式始终为 true
    pub decrypted: bool,
}

/// 图片盲水印提取结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageWatermarkFinding {
    /// 图片在压缩包中的相对路径
    pub file: String,
    /// 提取的原始文本水印内容
    pub text: String,
}

/// 处理压缩包，批量添加水印
///
/// # 流程
/// 1. 读取全部水印文本（单条 或 Excel 所有行）
/// 2. 解压到临时工作区（仅一次）
/// 3. 扫描文件（仅一次）
/// 4. 对每个水印文本：
///    a. 处理图片 / JSON / VAJ / VMI（写入独立临时目录）
///    b. 打包输出：
///       - 单水印 → output_dir/<archive>_watermarked.<ext>
///       - 多水印 → output_dir/<水印文本>/<archive>_watermarked.<ext>
/// 5. 清理临时文件
#[tauri::command]
pub async fn process_archive(
    app: AppHandle,
    archive_path: String,
    config: WatermarkConfig,
    process_images: bool,
    process_json: bool,
    process_vaj: bool,
    process_vmi: bool,
    output_dir: Option<String>,
    obfuscate: bool,
    watermark_mode: String,
    aes_key: Option<String>,
    selected_images: Option<Vec<String>>,
    fast_mode: bool,
) -> Result<String, String> {
    let archive_path_buf = std::path::PathBuf::from(&archive_path);
    let progress = Arc::new(ProgressEmitter::new(app));

    // === 读取全部水印文本 ===
    let watermarks: Vec<String> = match &config.watermark_source {
        WatermarkSource::SingleText { content } => vec![content.clone()],
        WatermarkSource::ExcelFile { path } => read_excel_core(path)?,
    };
    let is_batch = watermarks.len() > 1;
    let total_watermarks = watermarks.len();

    // 解析水印字段名（未设置时使用默认值 "_watermark"）
    let wm_key: String = config
        .watermark_key
        .as_deref()
        .filter(|k| !k.trim().is_empty())
        .unwrap_or(DEFAULT_WATERMARK_KEY)
        .to_string();

    let archive_name = archive_path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");

    // 输出文件名与原始包名保持一致
    let archive_output_filename = archive_path_buf
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("archive")
        .to_string();

    // 输出基础目录（未指定时与源文件同目录）
    let base_output_dir: std::path::PathBuf = match &output_dir {
        Some(dir) => std::path::PathBuf::from(dir),
        None => archive_path_buf
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from(".")),
    };

    // === Step 1: 创建工作区并解压（仅一次）===
    progress
        .emit_status("initializing".to_string(), "正在创建工作区...".to_string())
        .map_err(|e| format!("Progress error: {}", e))?;

    let workspace = TempWorkspace::new(archive_name)
        .map_err(|e| format!("创建工作区失败: {}", e))?;

    progress
        .emit_status("extracting".to_string(), format!("正在解压 {}...", archive_name))
        .map_err(|e| format!("Progress error: {}", e))?;

    let archive_processor = ArchiveProcessor::new();
    archive_processor
        .extract(&archive_path_buf, workspace.extracted_path())
        .map_err(|e| format!("解压失败: {}", e))?;

    // === Step 2: 扫描文件（仅一次）===
    let scanner = FileScanner::new();

    let images = if process_images {
        progress
            .emit_status("scanning".to_string(), "正在扫描图片...".to_string())
            .map_err(|e| format!("Progress error: {}", e))?;
        let all_images = scanner
            .scan(workspace.extracted_path())
            .map_err(|e| format!("扫描图片失败: {}", e))?;
        // 若前端指定了选中图片，则只处理选中的
        if let Some(ref sel) = selected_images {
            if !sel.is_empty() {
                all_images.into_iter().filter(|f| sel.contains(&f.relative_path)).collect()
            } else {
                all_images
            }
        } else {
            all_images
        }
    } else {
        vec![]
    };

    let json_files = if process_json {
        scanner
            .scan_json_files(workspace.extracted_path())
            .map_err(|e| format!("扫描 JSON 失败: {}", e))?
    } else {
        vec![]
    };

    let vaj_files = if process_vaj {
        scanner
            .scan_vaj_files(workspace.extracted_path())
            .map_err(|e| format!("扫描 VAJ 失败: {}", e))?
    } else {
        vec![]
    };

    let vmi_files = if process_vmi {
        scanner
            .scan_vmi_files(workspace.extracted_path())
            .map_err(|e| format!("扫描 VMI 失败: {}", e))?
    } else {
        vec![]
    };

    // 预计算用于 copy_other_files 的引用切片（扫描结果整个函数内有效）
    let image_rel_strs: Vec<&str> = images.iter().map(|f| f.relative_path.as_str()).collect();
    let json_rel_paths: Vec<&Path> = json_files.iter().map(|(_, r)| r.as_path()).collect();
    let vaj_rel_paths: Vec<&Path> = vaj_files.iter().map(|(_, r)| r.as_path()).collect();
    let vmi_rel_paths: Vec<&Path> = vmi_files.iter().map(|(_, r)| r.as_path()).collect();

    // 扫描完成后发送汇总，让前端知道各类型文件数量
    progress
        .emit_scan_summary(json_files.len(), vaj_files.len(), vmi_files.len(), images.len())
        .map_err(|e| format!("Progress error: {}", e))?;

    let mut final_output = String::new();

    // === Step 3: 对每个水印文本处理并打包 ===
    for (idx, watermark_text) in watermarks.iter().enumerate() {
        if is_batch {
            let label: String = if watermark_text.chars().count() > 24 {
                watermark_text.chars().take(24).collect::<String>() + "…"
            } else {
                watermark_text.clone()
            };
            progress
                .emit_status(
                    "processing".to_string(),
                    format!("[{}/{}] 正在处理：{}", idx + 1, total_watermarks, label),
                )
                .map_err(|e| format!("Progress error: {}", e))?;
        }

        // 为当前水印创建独立的临时 processed 目录
        let processed_dir = tempfile::tempdir()
            .map_err(|e| format!("创建临时目录失败: {}", e))?;
        let processed_path = processed_dir.path();

        // --- 处理图片 ---
        if process_images && !images.is_empty() {
            if !is_batch {
                progress
                    .emit_status(
                        "processing_images".to_string(),
                        format!("正在处理 {} 张图片...", images.len()),
                    )
                    .map_err(|e| format!("Progress error: {}", e))?;
            }
            let parallel_processor = ParallelProcessor::new();
            parallel_processor
                .process_batch_single(
                    &images,
                    watermark_text,
                    config.strength,
                    processed_path,
                    Some(Arc::clone(&progress)),
                    fast_mode,
                )
                .map_err(|e| format!("图片处理失败: {}", e))?;
        }

        // --- 处理 JSON ---
        let json_total = json_files.len();
        for (file_idx, (abs_path, rel_path)) in json_files.iter().enumerate() {
            let fname = rel_path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            progress
                .emit_detail_progress(idx + 1, total_watermarks, "json", file_idx + 1, json_total, fname)
                .map_err(|e| format!("Progress error: {}", e))?;
            let content = std::fs::read_to_string(abs_path)
                .map_err(|e| format!("读取 JSON 失败 {}: {}", rel_path.display(), e))?;
            let watermarked = if obfuscate {
                JsonWatermarker::embed_obfuscated(&content, watermark_text, &watermark_mode, aes_key.as_deref())
            } else {
                JsonWatermarker::embed(&content, watermark_text, &wm_key, &watermark_mode, aes_key.as_deref())
            }.map_err(|e| format!("JSON 水印注入失败 {}: {}", rel_path.display(), e))?;
            let dest = processed_path.join(rel_path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录失败: {}", e))?;
            }
            std::fs::write(&dest, watermarked.as_bytes())
                .map_err(|e| format!("写入 JSON 失败 {}: {}", rel_path.display(), e))?;
        }

        // --- 处理 VAJ ---
        let vaj_total = vaj_files.len();
        for (file_idx, (abs_path, rel_path)) in vaj_files.iter().enumerate() {
            let fname = rel_path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            progress
                .emit_detail_progress(idx + 1, total_watermarks, "vaj", file_idx + 1, vaj_total, fname)
                .map_err(|e| format!("Progress error: {}", e))?;
            let content = std::fs::read_to_string(abs_path)
                .map_err(|e| format!("读取 VAJ 失败 {}: {}", rel_path.display(), e))?;
            let watermarked = if obfuscate {
                JsonWatermarker::embed_obfuscated(&content, watermark_text, &watermark_mode, aes_key.as_deref())
            } else {
                JsonWatermarker::embed(&content, watermark_text, &wm_key, &watermark_mode, aes_key.as_deref())
            }.map_err(|e| format!("VAJ 水印注入失败 {}: {}", rel_path.display(), e))?;
            let dest = processed_path.join(rel_path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录失败: {}", e))?;
            }
            std::fs::write(&dest, watermarked.as_bytes())
                .map_err(|e| format!("写入 VAJ 失败 {}: {}", rel_path.display(), e))?;
        }

        // --- 处理 VMI ---
        let vmi_total = vmi_files.len();
        for (file_idx, (abs_path, rel_path)) in vmi_files.iter().enumerate() {
            let fname = rel_path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            progress
                .emit_detail_progress(idx + 1, total_watermarks, "vmi", file_idx + 1, vmi_total, fname)
                .map_err(|e| format!("Progress error: {}", e))?;
            let content = std::fs::read_to_string(abs_path)
                .map_err(|e| format!("读取 VMI 失败 {}: {}", rel_path.display(), e))?;
            let watermarked = if obfuscate {
                JsonWatermarker::embed_obfuscated(&content, watermark_text, &watermark_mode, aes_key.as_deref())
            } else {
                JsonWatermarker::embed(&content, watermark_text, &wm_key, &watermark_mode, aes_key.as_deref())
            }.map_err(|e| format!("VMI 水印注入失败 {}: {}", rel_path.display(), e))?;
            let dest = processed_path.join(rel_path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录失败: {}", e))?;
            }
            std::fs::write(&dest, watermarked.as_bytes())
                .map_err(|e| format!("写入 VMI 失败 {}: {}", rel_path.display(), e))?;
        }

        // --- 复制其他文件 ---
        copy_other_files(
            workspace.extracted_path(),
            processed_path,
            &image_rel_strs,
            &json_rel_paths,
            &vaj_rel_paths,
            &vmi_rel_paths,
        )
        .map_err(|e| format!("复制文件失败: {}", e))?;

        // --- 确定输出路径（始终输出到以水印文本命名的子文件夹）---
        let folder_name = sanitize_path_component(watermark_text);
        let subfolder = base_output_dir.join(&folder_name);
        std::fs::create_dir_all(&subfolder)
            .map_err(|e| format!("创建输出目录失败 {}: {}", subfolder.display(), e))?;
        let output_path = subfolder.join(&archive_output_filename);

        // --- 打包 ---
        progress
            .emit_status("packaging".to_string(), format!("正在打包：{}...", &archive_output_filename))
            .map_err(|e| format!("Progress error: {}", e))?;

        archive_processor
            .create(processed_path, &output_path)
            .map_err(|e| format!("打包失败: {}", e))?;

        final_output = output_path.to_string_lossy().to_string();

        if is_batch {
            progress
                .emit_status(
                    "batch_item_done".to_string(),
                    format!("已完成 {}/{}", idx + 1, total_watermarks),
                )
                .map_err(|e| format!("Progress error: {}", e))?;
        }
        // processed_dir 在此处 drop，自动清理
    }

    // 批量模式返回输出基础目录，单条模式返回输出文件路径
    let result = if is_batch {
        base_output_dir.to_string_lossy().to_string()
    } else {
        final_output
    };

    progress
        .emit_complete(result.clone())
        .map_err(|e| format!("Progress error: {}", e))?;

    Ok(result)
}

/// 将水印文本转换为合法的文件夹名（替换操作系统禁止的字符）
fn sanitize_path_component(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c => c,
        })
        .collect();
    let trimmed = sanitized.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "watermark".to_string()
    } else {
        trimmed.chars().take(100).collect()
    }
}

/// 将解压目录中不属于图片、JSON、VAJ 或 VMI 的文件原样复制到 processed 目录
fn copy_other_files(
    src_root: &Path,
    dst_root: &Path,
    image_rel_paths: &[&str],
    json_rel_paths: &[&Path],
    vaj_rel_paths: &[&Path],
    vmi_rel_paths: &[&Path],
) -> Result<(), std::io::Error> {
    use walkdir::WalkDir;

    for entry in WalkDir::new(src_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let rel = path.strip_prefix(src_root).unwrap_or(path);
        let rel_str = rel.to_string_lossy();

        // 跳过已处理的图片、JSON、VAJ 和 VMI
        let is_image = image_rel_paths.iter().any(|r| *r == rel_str.as_ref());
        let is_json = json_rel_paths.iter().any(|r| *r == rel);
        let is_vaj = vaj_rel_paths.iter().any(|r| *r == rel);
        let is_vmi = vmi_rel_paths.iter().any(|r| *r == rel);
        if is_image || is_json || is_vaj || is_vmi {
            continue;
        }

        let dst = dst_root.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(path, &dst)?;
    }

    Ok(())
}

/// 从压缩包中提取指定 JSON 文件的水印
#[tauri::command]
pub async fn extract_json_watermark_from_archive(
    archive_path: String,
    json_path_in_archive: Option<String>,
    watermark_key: Option<String>,
) -> Result<String, String> {
    let archive_path_buf = std::path::PathBuf::from(&archive_path);
    let archive_name = archive_path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");

    let workspace = TempWorkspace::new(archive_name)
        .map_err(|e| format!("创建工作区失败: {}", e))?;

    let archive_processor = ArchiveProcessor::new();
    archive_processor
        .extract(&archive_path_buf, workspace.extracted_path())
        .map_err(|e| format!("解压失败: {}", e))?;

    // 默认读取 meta.json；也可指定路径
    let target = json_path_in_archive.unwrap_or_else(|| "meta.json".to_string());
    let json_abs = workspace.extracted_path().join(&target);

    let content = std::fs::read_to_string(&json_abs)
        .map_err(|e| format!("读取 {} 失败: {}", target, e))?;

    let key = watermark_key
        .as_deref()
        .filter(|k| !k.trim().is_empty())
        .unwrap_or(DEFAULT_WATERMARK_KEY);

    JsonWatermarker::extract(&content, key)
        .map_err(|e| e.to_string())
}

/// 扫描压缩包中所有 JSON / VAJ / VMI 文件，提取其中的水印字段
///
/// 与 extract_json_watermark_from_archive 不同：
/// - 不需要指定单一文件路径
/// - 遍历全部受支持格式的文件
/// - 返回所有找到水印的文件列表（文件路径 + 水印值 + 模式）
/// - 找不到水印字段的文件直接跳过（不报错）
#[tauri::command]
pub async fn scan_watermarks_in_archive(
    archive_path: String,
    watermark_key: Option<String>,
    aes_key: Option<String>,
) -> Result<Vec<WatermarkFinding>, String> {
    let archive_path_buf = std::path::PathBuf::from(&archive_path);
    let archive_name = archive_path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");

    let workspace = TempWorkspace::new(archive_name)
        .map_err(|e| format!("创建工作区失败: {}", e))?;

    let archive_processor = ArchiveProcessor::new();
    archive_processor
        .extract(&archive_path_buf, workspace.extracted_path())
        .map_err(|e| format!("解压失败: {}", e))?;

    let key = watermark_key
        .as_deref()
        .filter(|k| !k.trim().is_empty())
        .unwrap_or(DEFAULT_WATERMARK_KEY);
    let _ = key; // 保留参数兼容性；提取现通过值扫描实现，无需指定键名

    let scanner = FileScanner::new();
    let extracted = workspace.extracted_path();
    let aes_key_ref = aes_key.as_deref();

    // 收集所有 JSON / VAJ / VMI 文件（忽略各类扫描错误）
    let mut all_files: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
    for scan_result in [
        scanner.scan_json_files(extracted),
        scanner.scan_vaj_files(extracted),
        scanner.scan_vmi_files(extracted),
    ] {
        if let Ok(files) = scan_result {
            all_files.extend(files);
        }
    }

    // 逐文件扫描所有格式的水印值（兼容明文、MD5、AES 三种模式）
    let mut findings: Vec<WatermarkFinding> = Vec::new();
    for (abs_path, rel_path) in &all_files {
        if let Ok(content) = std::fs::read_to_string(abs_path) {
            for (value, mode, decrypted) in JsonWatermarker::scan_watermark_values(&content, aes_key_ref) {
                findings.push(WatermarkFinding {
                    file: rel_path.to_string_lossy().to_string(),
                    value,
                    mode,
                    decrypted,
                });
            }
        }
    }

    Ok(findings)
}

/// 合并扫描结果（JSON/VAJ/VMI 水印 + 图片盲水印）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedScanResult {
    pub json_findings: Vec<WatermarkFinding>,
    pub image_findings: Vec<ImageWatermarkFinding>,
    /// 本次扫描实际处理的 PNG 图片数量（JPEG 已过滤，0 表示压缩包内无 PNG）
    pub scanned_png_count: usize,
}

/// 一次性扫描压缩包中所有水印（JSON/VAJ/VMI + 图片盲水印）
///
/// 相比分别调用两个命令，此命令只解压一次，图片扫描并行处理，速度更快。
///
/// # 参数
/// * `scan_images` - 是否扫描图片盲水印。设为 false 可跳过 DWT+DCT 提取，
///                   大幅缩短仅含 JSON 水印的压缩包的提取时间。
///                   即使为 true，也只处理 PNG（JPEG 经有损压缩无法保留水印）。
#[tauri::command]
pub async fn scan_all_watermarks_in_archive(
    archive_path: String,
    aes_key: Option<String>,
    scan_images: Option<bool>,
) -> Result<CombinedScanResult, String> {
    use rayon::prelude::*;

    let archive_path_buf = std::path::PathBuf::from(&archive_path);
    let archive_name = archive_path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");

    let workspace = TempWorkspace::new(archive_name)
        .map_err(|e| format!("创建工作区失败: {}", e))?;

    let archive_processor = ArchiveProcessor::new();
    archive_processor
        .extract(&archive_path_buf, workspace.extracted_path())
        .map_err(|e| format!("解压失败: {}", e))?;

    let scanner = FileScanner::new();
    let extracted = workspace.extracted_path();
    let aes_key_ref = aes_key.as_deref();

    // ── 扫描 JSON / VAJ / VMI 文件（通常数量少，顺序处理即可）──────────────
    let mut all_text_files: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
    for scan_result in [
        scanner.scan_json_files(extracted),
        scanner.scan_vaj_files(extracted),
        scanner.scan_vmi_files(extracted),
    ] {
        if let Ok(files) = scan_result {
            all_text_files.extend(files);
        }
    }

    let mut json_findings: Vec<WatermarkFinding> = Vec::new();
    for (abs_path, rel_path) in &all_text_files {
        if let Ok(content) = std::fs::read_to_string(abs_path) {
            for (value, mode, decrypted) in JsonWatermarker::scan_watermark_values(&content, aes_key_ref) {
                json_findings.push(WatermarkFinding {
                    file: rel_path.to_string_lossy().to_string(),
                    value,
                    mode,
                    decrypted,
                });
            }
        }
    }

    // ── 并行扫描图片盲水印 ────────────────────────────────────────────────
    // 仅在 scan_images=true（默认）时执行；
    // 只处理 PNG（无损），JPEG 经有损压缩无法保留 DWT+DCT 水印，自动过滤。
    let should_scan_images = scan_images.unwrap_or(true);
    let all_images = if should_scan_images {
        scanner.scan(extracted).unwrap_or_default()
    } else {
        vec![]
    };
    // 过滤出 PNG：JPEG 必定提取失败，提前排除可减少无效 IO 和解码开销
    let png_images: Vec<_> = all_images
        .into_iter()
        .filter(|f| f.relative_path.to_lowercase().ends_with(".png"))
        .collect();

    let mut image_findings: Vec<ImageWatermarkFinding> = if png_images.is_empty() {
        // 无 PNG 图片（或用户关闭了图片扫描）→ 直接返回空结果，跳过 DWT+DCT 计算
        vec![]
    } else {
        let extractor = WatermarkExtractor::new();
        png_images
            .par_iter()
            .filter_map(|image_file| {
                let img = image::open(&image_file.temp_path).ok()?;
                let text = extractor.try_extract_text(&img).ok()??;
                Some(ImageWatermarkFinding {
                    file: image_file.relative_path.clone(),
                    text,
                })
            })
            .collect()
    };

    // 按文件路径排序，保证结果顺序稳定
    image_findings.sort_by(|a, b| a.file.cmp(&b.file));

    Ok(CombinedScanResult { json_findings, image_findings, scanned_png_count: png_images.len() })
}

/// 列出压缩包中所有图片文件的相对路径
///
/// 用于前端展示图片列表，供用户选择要添加盲水印的图片。
#[tauri::command]
pub async fn list_images_in_archive(
    archive_path: String,
) -> Result<Vec<String>, String> {
    let archive_path_buf = std::path::PathBuf::from(&archive_path);
    let archive_name = archive_path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");

    let workspace = TempWorkspace::new(archive_name)
        .map_err(|e| format!("创建工作区失败: {}", e))?;

    let archive_processor = ArchiveProcessor::new();
    archive_processor
        .extract(&archive_path_buf, workspace.extracted_path())
        .map_err(|e| format!("解压失败: {}", e))?;

    let scanner = FileScanner::new();
    let images = scanner
        .scan(workspace.extracted_path())
        .map_err(|e| format!("扫描图片失败: {}", e))?;

    Ok(images.into_iter().map(|f| f.relative_path).collect())
}

/// 扫描压缩包中所有图片，提取含有原始文本盲水印的图片及其水印内容
#[tauri::command]
pub async fn scan_image_watermarks_in_archive(
    archive_path: String,
) -> Result<Vec<ImageWatermarkFinding>, String> {
    let archive_path_buf = std::path::PathBuf::from(&archive_path);
    let archive_name = archive_path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");

    let workspace = TempWorkspace::new(archive_name)
        .map_err(|e| format!("创建工作区失败: {}", e))?;

    let archive_processor = ArchiveProcessor::new();
    archive_processor
        .extract(&archive_path_buf, workspace.extracted_path())
        .map_err(|e| format!("解压失败: {}", e))?;

    let scanner = FileScanner::new();
    let images = scanner
        .scan(workspace.extracted_path())
        .map_err(|e| format!("扫描图片失败: {}", e))?;

    let extractor = WatermarkExtractor::new();
    let mut findings: Vec<ImageWatermarkFinding> = Vec::new();

    for image_file in &images {
        let img = match image::open(&image_file.temp_path) {
            Ok(img) => img,
            Err(_) => continue,
        };
        if let Ok(Some(text)) = extractor.try_extract_text(&img) {
            findings.push(ImageWatermarkFinding {
                file: image_file.relative_path.clone(),
                text,
            });
        }
    }

    Ok(findings)
}
