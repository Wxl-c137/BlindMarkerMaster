// TypeScript types for the BlindMark Master application

// --- Watermark Configuration ---

/** Watermark source: single text or Excel file */
export type WatermarkSourceType = 'singleText' | 'excelFile';

/** Matches Rust WatermarkSource enum (tagged union with "type" field) */
export type WatermarkSource =
  | { type: 'singleText'; content: string }
  | { type: 'excelFile'; path: string };

/** Matches Rust WatermarkConfig struct */
export interface WatermarkConfig {
  strength: number; // 0.1 - 1.0
  watermarkSource: WatermarkSource;
  /** Custom JSON field name for watermark; null/undefined → backend uses "_watermark" */
  watermarkKey?: string | null;
}

// --- Archive Processing ---

/** Archive file types supported */
export type ArchiveType = 'zip' | '7z' | 'var' | 'rar';

/** Processing status for archive workflow */
export type ArchiveStatus =
  | 'idle'
  | 'initializing'
  | 'extracting'
  | 'scanning'
  | 'processing_images'
  | 'processing_json'
  | 'copying'
  | 'packaging'
  | 'complete'
  | 'error';

/** Matches Rust StatusEvent (emitted via "watermark-status" event) */
export interface StatusEvent {
  status: string;
  message: string;
}

/** Matches Rust ProgressEvent (emitted via "watermark-progress" event) */
export interface ProgressEvent {
  currentFile: number;
  totalFiles: number;
  filename: string;
  progress: number;
  status: string;
}

// --- Preview Image ---

export interface PreviewImage {
  original: string; // file path
  watermarked: string; // base64 data URL
}

// --- Processing Task ---

export interface ProcessingStatus {
  status: 'idle' | 'loading' | 'processing' | 'completed' | 'error';
  progress: number; // 0-100
  currentFile?: string;
  totalFiles?: number;
  processedFiles?: number;
  error?: string;
}

export interface FileTask {
  id: string;
  filename: string;
  originalSize: number;
  md5Content: string;
  status: 'waiting' | 'processing' | 'completed' | 'error';
  progress: number;
  error?: string;
}

export interface ImageFile {
  relativePath: string;
  tempPath: string;
  isSupported: boolean;
}

export interface ExtractionResult {
  md5Hash: string;
  confidence: number;
}

// --- Helper Functions ---

/** Convert Uint8Array to base64 data URL for image preview */
export function uint8ArrayToDataUrl(data: Uint8Array): string {
  let binary = '';
  const chunkSize = 8192;
  for (let i = 0; i < data.length; i += chunkSize) {
    binary += String.fromCharCode(...data.subarray(i, i + chunkSize));
  }
  return `data:image/png;base64,${btoa(binary)}`;
}

/** Validate strength value */
export function isValidStrength(strength: number): boolean {
  return strength >= 0.1 && strength <= 1.0;
}

/** Format file size for display */
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 Bytes';
  const k = 1024;
  const sizes = ['Bytes', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return Math.round((bytes / Math.pow(k, i)) * 100) / 100 + ' ' + sizes[i];
}

/** Format MD5 hash with spaces for readability */
export function formatMD5(hash: string): string {
  return hash.match(/.{1,8}/g)?.join(' ') || hash;
}

/** Get filename from a full path */
export function getFilename(path: string): string {
  return path.replace(/\\/g, '/').split('/').pop() || path;
}

/** Map archive status to Chinese label */
export function getStatusLabel(status: string): string {
  const labels: Record<string, string> = {
    idle: '就绪',
    initializing: '初始化',
    extracting: '解压中',
    scanning: '扫描文件',
    processing_images: '处理图片',
    images_done: '图片完成',
    processing_json: '处理JSON',
    json_done: 'JSON完成',
    copying: '复制文件',
    packaging: '打包中',
    complete: '完成',
    error: '错误',
  };
  return labels[status] || status;
}
