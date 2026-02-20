import { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { Archive, FileSpreadsheet, Zap, Lock, Unlock, X, Check, CornerDownRight, Sun, Moon, Loader2 } from 'lucide-react';
import {
  WatermarkConfig,
  WatermarkSource,
  StatusEvent,
  ProgressEvent,
  formatMD5,
  getFilename,
  getStatusLabel,
} from './lib/types';

type Tab = 'embed' | 'extract';

interface ScanSummary {
  jsonCount: number;
  vajCount: number;
  vmiCount: number;
  imageCount: number;
}

interface DetailProgress {
  batchCurrent: number;
  batchTotal: number;
  fileType: string;
  typeCurrent: number;
  typeTotal: number;
  filename: string;
}

// Tracks how far each file type has progressed (persists across detail events)
interface TypeCounters {
  json: number;
  vaj: number;
  vmi: number;
  image: number;
}

interface EmbedState {
  archivePath: string | null;
  sourceType: 'singleText' | 'excelFile';
  singleText: string;
  excelPath: string | null;
  processImages: boolean;
  processJson: boolean;
  processVaj: boolean;
  processVmi: boolean;
  watermarkKey: string;
  processObfuscation: boolean;
  watermarkMode: 'md5' | 'plaintext' | 'aes';
  aesKey: string;
  fastMode: boolean;
  outputDir: string | null;
  isProcessing: boolean;
  statusMessage: string;
  statusCode: string;
  // image-level progress (from parallel processor)
  progressCurrent: number;
  progressTotal: number;
  progressFilename: string;
  // scan summary
  scan: ScanSummary | null;
  // per-file detail progress
  detail: DetailProgress | null;
  // per-type counters (reset each watermark round)
  typeCounters: TypeCounters;
  outputPath: string | null;
  error: string | null;
  // image selection
  imageList: string[];
  selectedImages: string[];
  imageListLoading: boolean;
}

interface WatermarkFinding {
  file: string;
  value: string;
  mode: string;
  decrypted: boolean;
}

interface ImageWatermarkFinding {
  file: string;
  text: string;
}

interface ExtractState {
  archivePath: string | null;
  isExtracting: boolean;
  result: WatermarkFinding[] | null;
  imageFindings: ImageWatermarkFinding[] | null;
  scannedPngCount: number | null;
  error: string | null;
  aesKey: string;
  scanImages: boolean;
}

function formatElapsed(sec: number): string {
  if (sec < 60) return `${sec}s`;
  return `${Math.floor(sec / 60)}m ${sec % 60}s`;
}

function App() {
  const [activeTab, setActiveTab] = useState<Tab>('embed');
  const [cpuCount, setCpuCount] = useState<number | null>(null);
  const [isDark, setIsDark] = useState(true);
  const [isDragging, setIsDragging] = useState(false);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  const [embed, setEmbed] = useState<EmbedState>({
    archivePath: null,
    sourceType: 'singleText',
    singleText: '',
    excelPath: null,
    processImages: true,
    processJson: true,
    processVaj: true,
    processVmi: true,
    watermarkKey: '',
    processObfuscation: false,
    watermarkMode: 'md5',
    aesKey: '',
    fastMode: false,
    outputDir: null,
    isProcessing: false,
    statusMessage: '',
    statusCode: 'idle',
    progressCurrent: 0,
    progressTotal: 0,
    progressFilename: '',
    scan: null,
    detail: null,
    typeCounters: { json: 0, vaj: 0, vmi: 0, image: 0 },
    outputPath: null,
    error: null,
    imageList: [],
    selectedImages: [],
    imageListLoading: false,
  });

  const [extract, setExtract] = useState<ExtractState>({
    archivePath: null,
    isExtracting: false,
    result: null,
    imageFindings: null,
    scannedPngCount: null,
    error: null,
    aesKey: '',
    scanImages: true,
  });

  const startTimeRef = useRef<number | null>(null);
  const [elapsedSec, setElapsedSec] = useState(0);
  const activeTabRef = useRef<Tab>(activeTab);
  useEffect(() => { activeTabRef.current = activeTab; }, [activeTab]);

  useEffect(() => {
    let unlistenStatus: UnlistenFn | null = null;
    let unlistenProgress: UnlistenFn | null = null;
    let unlistenScanSummary: UnlistenFn | null = null;
    let unlistenDetailProgress: UnlistenFn | null = null;
    let unlistenDragEnter: UnlistenFn | null = null;
    let unlistenDragLeave: UnlistenFn | null = null;
    let unlistenDragDrop: UnlistenFn | null = null;

    const setupListeners = async () => {
      unlistenStatus = await listen<StatusEvent>('watermark-status', (event) => {
        const { status, message } = event.payload;
        setEmbed((prev) => ({
          ...prev,
          statusCode: status,
          statusMessage: message,
          error: status === 'error' ? message : prev.error,
        }));
      });

      unlistenProgress = await listen<ProgressEvent>('watermark-progress', (event) => {
        const { currentFile, totalFiles, filename } = event.payload;
        setEmbed((prev) => ({
          ...prev,
          progressCurrent: Math.max(prev.progressCurrent, currentFile),
          progressTotal: totalFiles,
          progressFilename: filename,
          // Also drive the per-type image counter in the scan summary grid
          typeCounters: {
            ...prev.typeCounters,
            image: Math.max(prev.typeCounters.image, currentFile),
          },
        }));
      });

      unlistenScanSummary = await listen<ScanSummary>('watermark-scan-summary', (event) => {
        setEmbed((prev) => ({
          ...prev,
          scan: event.payload,
          typeCounters: { json: 0, vaj: 0, vmi: 0, image: 0 },
        }));
      });

      unlistenDetailProgress = await listen<DetailProgress>('watermark-detail-progress', (event) => {
        const d = event.payload;
        setEmbed((prev) => {
          const tc = { ...prev.typeCounters };
          const t = d.fileType as keyof TypeCounters;
          if (t in tc) tc[t] = d.typeCurrent;
          return { ...prev, detail: d, typeCounters: tc };
        });
      });

      unlistenDragEnter = await listen('tauri://drag-enter', () => {
        setIsDragging(true);
      });

      unlistenDragLeave = await listen('tauri://drag-leave', () => {
        setIsDragging(false);
      });

      unlistenDragDrop = await listen('tauri://drag-drop', (event) => {
        setIsDragging(false);
        const payload = event.payload as { paths?: string[] } | string[];
        const paths: string[] = Array.isArray(payload) ? payload : (payload as { paths?: string[] })?.paths ?? [];
        if (paths.length === 0) return;
        const path = paths[0];
        const ext = path.split('.').pop()?.toLowerCase() ?? '';
        if (!['zip', '7z', 'var', 'rar'].includes(ext)) return;

        if (activeTabRef.current === 'embed') {
          setEmbed((prev) => ({
            ...prev,
            archivePath: path,
            outputPath: null,
            error: null,
            statusCode: 'idle',
            statusMessage: '',
          }));
        } else {
          setExtract((prev) => ({ ...prev, archivePath: path, result: null, error: null }));
        }
      });
    };

    setupListeners();
    // Fetch CPU core count once on mount
    invoke<number>('get_cpu_count').then(setCpuCount).catch(() => {});
    return () => {
      unlistenStatus?.();
      unlistenProgress?.();
      unlistenScanSummary?.();
      unlistenDetailProgress?.();
      unlistenDragEnter?.();
      unlistenDragLeave?.();
      unlistenDragDrop?.();
    };
  }, []);

  // Sync theme class on <html> for body bg + CSS var overrides
  useEffect(() => {
    if (isDark) {
      document.documentElement.classList.remove('light');
    } else {
      document.documentElement.classList.add('light');
    }
  }, [isDark]);

  const handleSelectArchive = useCallback(async () => {
    const selected = await open({
      title: '选择压缩包',
      multiple: false,
      filters: [{ name: 'Archives', extensions: ['zip', '7z', 'var', 'rar'] }],
    });
    if (selected && typeof selected === 'string') {
      setEmbed((prev) => ({
        ...prev,
        archivePath: selected,
        outputPath: null,
        error: null,
        statusCode: 'idle',
        statusMessage: '',
      }));
    }
  }, []);

  const handleSelectExcel = useCallback(async () => {
    const selected = await open({
      title: '选择 Excel 文件',
      multiple: false,
      filters: [{ name: 'Excel Files', extensions: ['xlsx', 'xls'] }],
    });
    if (selected && typeof selected === 'string') {
      setEmbed((prev) => ({ ...prev, excelPath: selected }));
    }
  }, []);

  const handleSelectOutputDir = useCallback(async () => {
    const selected = await open({ title: '选择输出目录', directory: true, multiple: false });
    if (selected && typeof selected === 'string') {
      setEmbed((prev) => ({ ...prev, outputDir: selected }));
    }
  }, []);

  const handleCopy = useCallback((key: string, text: string) => {
    navigator.clipboard.writeText(text).then(() => {
      setCopiedKey(key);
      setTimeout(() => setCopiedKey((prev) => (prev === key ? null : prev)), 2000);
    });
  }, []);

  const handleProcess = useCallback(async () => {
    const { archivePath, sourceType, singleText, excelPath, processImages, processJson, processVaj, processVmi, watermarkKey, outputDir, processObfuscation, watermarkMode, aesKey, selectedImages, fastMode } = embed;

    if (!archivePath) { setEmbed((prev) => ({ ...prev, error: '请先选择压缩包' })); return; }
    if (!processImages && !processJson && !processVaj && !processVmi) {
      setEmbed((prev) => ({ ...prev, error: '请至少选择一种水印类型' })); return;
    }
    if (sourceType === 'singleText' && !singleText.trim()) {
      setEmbed((prev) => ({ ...prev, error: '请输入水印文本' })); return;
    }
    if (sourceType === 'excelFile' && !excelPath) {
      setEmbed((prev) => ({ ...prev, error: '请选择 Excel 文件' })); return;
    }
    if (watermarkMode === 'aes' && !aesKey.trim()) {
      setEmbed((prev) => ({ ...prev, error: 'AES 模式需要输入密钥' })); return;
    }

    const watermarkSource: WatermarkSource =
      sourceType === 'singleText'
        ? { type: 'singleText', content: singleText.trim() }
        : { type: 'excelFile', path: excelPath! };

    const config: WatermarkConfig = {
      strength: 0.5,
      watermarkSource,
      watermarkKey: watermarkKey.trim() || null,
    };

    setEmbed((prev) => ({
      ...prev,
      isProcessing: true,
      outputPath: null,
      error: null,
      statusCode: 'initializing',
      statusMessage: '准备中...',
      progressCurrent: 0,
      progressTotal: 0,
      scan: null,
      detail: null,
      typeCounters: { json: 0, vaj: 0, vmi: 0, image: 0 },
    }));

    try {
      const outputPath = await invoke<string>('process_archive', {
        archivePath, config, processImages, processJson, processVaj, processVmi,
        outputDir: outputDir ?? null,
        obfuscate: processObfuscation,
        watermarkMode,
        aesKey: aesKey.trim() || null,
        selectedImages: processImages && selectedImages.length > 0 ? selectedImages : null,
        fastMode,
      });
      setEmbed((prev) => ({ ...prev, isProcessing: false, outputPath, statusCode: 'complete', statusMessage: '处理完成' }));
    } catch (err) {
      setEmbed((prev) => ({ ...prev, isProcessing: false, error: String(err), statusCode: 'error', statusMessage: String(err) }));
    }
  }, [embed]);

  const handleSelectExtractArchive = useCallback(async () => {
    const selected = await open({
      title: '选择压缩包',
      multiple: false,
      filters: [{ name: 'Archives', extensions: ['zip', '7z', 'var', 'rar'] }],
    });
    if (selected && typeof selected === 'string') {
      setExtract((prev) => ({ ...prev, archivePath: selected, result: null, error: null }));
    }
  }, []);

  const handleExtract = useCallback(async () => {
    const { archivePath, aesKey, scanImages } = extract;
    if (!archivePath) { setExtract((prev) => ({ ...prev, error: '请先选择压缩包' })); return; }

    setExtract((prev) => ({ ...prev, isExtracting: true, result: null, imageFindings: null, scannedPngCount: null, error: null }));
    try {
      const { jsonFindings, imageFindings, scannedPngCount } = await invoke<{
        jsonFindings: WatermarkFinding[];
        imageFindings: ImageWatermarkFinding[];
        scannedPngCount: number;
      }>('scan_all_watermarks_in_archive', {
        archivePath,
        aesKey: aesKey.trim() || null,
        scanImages,
      });
      setExtract((prev) => ({ ...prev, isExtracting: false, result: jsonFindings, imageFindings, scannedPngCount }));
    } catch (err) {
      setExtract((prev) => ({ ...prev, isExtracting: false, error: String(err) }));
    }
  }, [extract]);

  // Load image list when processImages is enabled and archive is selected
  useEffect(() => {
    if (!embed.processImages || !embed.archivePath) {
      setEmbed((prev) => ({ ...prev, imageList: [], selectedImages: [] }));
      return;
    }
    setEmbed((prev) => ({ ...prev, imageListLoading: true, imageList: [], selectedImages: [] }));
    invoke<string[]>('list_images_in_archive', { archivePath: embed.archivePath })
      .then((list) => setEmbed((prev) => ({
        ...prev,
        imageList: list,
        // Auto-select only PNG files; JPEG/JPG are not supported for image watermarking
        selectedImages: list.filter((img) => img.toLowerCase().endsWith('.png')),
        imageListLoading: false,
      })))
      .catch(() => setEmbed((prev) => ({ ...prev, imageListLoading: false })));
  }, [embed.processImages, embed.archivePath]);

  // Timer: starts when processing begins, stops (and retains final value) on completion/error
  useEffect(() => {
    if (embed.isProcessing) {
      startTimeRef.current = Date.now();
      setElapsedSec(0);
      const id = setInterval(() => {
        setElapsedSec(Math.floor((Date.now() - startTimeRef.current!) / 1000));
      }, 500);
      return () => clearInterval(id);
    }
  }, [embed.isProcessing]);

  // Theme color tokens — swap all neutrals between dark gaming and light mode
  const t = useMemo(() => isDark ? {
    bg: '#07071a', card: '#0d0d23',
    cardBorderC: 'rgba(0,245,255,0.1)', cardBorderP: 'rgba(168,85,247,0.12)',
    labelC: 'rgba(0,245,255,0.5)', labelP: 'rgba(168,85,247,0.55)',
    text: 'rgba(255,255,255,0.87)', textMuted: 'rgba(255,255,255,0.65)',
    textDim: 'rgba(255,255,255,0.55)', textFaint: 'rgba(255,255,255,0.42)',
    inputBg: '#07071a', inputBorder: 'rgba(255,255,255,0.1)',
    focusC: 'rgba(0,245,255,0.4)', focusP: 'rgba(168,85,247,0.4)',
    divider: 'rgba(255,255,255,0.05)',
    listBg: '#07071a', listChecked: 'rgba(0,245,255,0.05)',
    tabBg: '#0d0d23', tabBorder: 'rgba(0,245,255,0.1)',
    toggleOff: 'rgba(255,255,255,0.1)',
    clearBtn: 'rgba(255,255,255,0.3)',
    dirBtnBg: 'rgba(255,255,255,0.06)', dirBtnBorder: 'rgba(255,255,255,0.1)', dirBtnColor: 'rgba(255,255,255,0.6)',
    emptyBg: 'rgba(255,255,255,0.04)',
    themeBtnBg: 'rgba(255,255,255,0.06)', themeBtnBorder: 'rgba(255,255,255,0.12)', themeBtnColor: 'rgba(255,255,255,0.6)',
  } : {
    bg: '#f0f4ff', card: '#ffffff',
    cardBorderC: 'rgba(99,102,241,0.15)', cardBorderP: 'rgba(139,92,246,0.18)',
    labelC: '#6366f1', labelP: '#8b5cf6',
    text: '#0f172a', textMuted: '#334155',
    textDim: '#475569', textFaint: '#64748b',
    inputBg: '#f8f9ff', inputBorder: 'rgba(99,102,241,0.15)',
    focusC: 'rgba(99,102,241,0.4)', focusP: 'rgba(139,92,246,0.4)',
    divider: 'rgba(99,102,241,0.08)',
    listBg: '#f8f9ff', listChecked: 'rgba(99,102,241,0.06)',
    tabBg: '#ffffff', tabBorder: 'rgba(99,102,241,0.15)',
    toggleOff: 'rgba(99,102,241,0.15)',
    clearBtn: 'rgba(15,23,42,0.4)',
    dirBtnBg: 'rgba(99,102,241,0.06)', dirBtnBorder: 'rgba(99,102,241,0.15)', dirBtnColor: 'rgba(15,23,42,0.6)',
    emptyBg: 'rgba(99,102,241,0.04)',
    themeBtnBg: 'rgba(99,102,241,0.08)', themeBtnBorder: 'rgba(99,102,241,0.2)', themeBtnColor: '#6366f1',
  }, [isDark]);

  const embedProgress = embed.progressTotal > 0
    ? Math.round((embed.progressCurrent / embed.progressTotal) * 100)
    : embed.isProcessing ? 30 : 0;

  const isComplete = embed.statusCode === 'complete';
  const isError = embed.statusCode === 'error';

  return (
    <div className="min-h-screen font-chakra" style={{ background: t.bg, color: t.text }}>
      <div className="max-w-2xl mx-auto px-4 py-8">

        {/* Header */}
        <div className="flex items-start justify-between mb-8">
          <div className="flex items-start gap-3">
            {/* Pixel art isometric cube icon */}
            <div className="cube-icon-wrap shrink-0 mt-1">
              <svg className="px-cube" width="24" height="24" viewBox="0 0 24 24" style={{display:'block',margin:'6px'}}>
                <polygon className="px-top"       points="12,0 24,6 12,12 0,6"    stroke="rgba(180,255,255,.6)"  strokeWidth="1"/>
                <polygon className="px-left"      points="0,6 12,12 12,24 0,18"   stroke="rgba(200,160,255,.35)" strokeWidth="1"/>
                <polygon className="px-right"     points="12,12 24,6 24,18 12,24" stroke="rgba(140,90,220,.28)"  strokeWidth="1"/>
                <polygon className="px-shine"     points="12,1 18,4 15,7 9,4"/>
                <polygon className="px-edge-glow" points="12,0 24,6 24,18 12,24 0,18 0,6"/>
              </svg>
            </div>
            <div>
            <h1 className="font-orbitron text-2xl font-bold tracking-tight mb-0.5">
              <span style={{ color: '#00f5ff', textShadow: '0 0 10px rgba(0,245,255,0.8), 0 0 20px rgba(0,245,255,0.4)' }}>BLIND</span>
              <span style={{ color: t.text }}>MARK </span>
              <span style={{ color: '#ff0080', textShadow: '0 0 10px rgba(255,0,128,0.8), 0 0 20px rgba(255,0,128,0.4)' }}>MASTER</span>
            </h1>
            <p className="text-xs mb-1" style={{ color: t.textFaint, letterSpacing: '0.06em' }}>by lulu</p>
            <div className="flex items-center gap-3 mt-0.5">
              <p className="text-sm" style={{ color: t.textMuted }}>
                JSON / VAJ / VMI · 图片盲水印 · 支持 .zip .7z .var .rar
              </p>
              {cpuCount !== null && (
                <span
                  className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium border"
                  style={{ color: '#00f5ff', background: 'rgba(0,245,255,0.08)', borderColor: 'rgba(0,245,255,0.3)', boxShadow: '0 0 8px rgba(0,245,255,0.1)' }}
                >
                  <Zap className="w-3 h-3" /> 多线程并行 · {cpuCount} 核
                </span>
              )}
            </div>
            </div>
          </div>
          {/* Theme toggle */}
          <button
            onClick={() => setIsDark((d) => !d)}
            className="shrink-0 mt-1 p-2 rounded-lg transition-all cursor-pointer focus:outline-none"
            style={{ background: t.themeBtnBg, border: `1px solid ${t.themeBtnBorder}`, color: t.themeBtnColor }}
            title={isDark ? '切换浅色模式' : '切换深色模式'}
          >
            {isDark ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
          </button>
        </div>

        {/* Tab bar */}
        <div className="flex gap-1 rounded-xl p-1 mb-6 border" style={{ background: t.tabBg, borderColor: t.tabBorder }}>
          {(['embed', 'extract'] as Tab[]).map((tab) => {
            const active = activeTab === tab;
            const isCyan = tab === 'embed';
            return (
              <button
                key={tab}
                onClick={() => setActiveTab(tab)}
                className="flex-1 py-2 text-sm font-medium rounded-lg transition-all cursor-pointer focus:outline-none font-orbitron tracking-wide border"
                style={active ? {
                  color: isCyan ? '#00f5ff' : '#a855f7',
                  background: isCyan ? 'rgba(0,245,255,0.08)' : 'rgba(168,85,247,0.08)',
                  borderColor: isCyan ? 'rgba(0,245,255,0.35)' : 'rgba(168,85,247,0.35)',
                  boxShadow: isCyan
                    ? '0 0 14px rgba(0,245,255,0.15), inset 0 0 14px rgba(0,245,255,0.04)'
                    : '0 0 14px rgba(168,85,247,0.15), inset 0 0 14px rgba(168,85,247,0.04)',
                } : { color: t.textMuted, borderColor: 'transparent', background: 'transparent' }}
              >
                {tab === 'embed' ? '添加水印' : '提取水印'}
              </button>
            );
          })}
        </div>

        {/* ===== Embed Tab ===== */}
        {activeTab === 'embed' && (
          <div className="space-y-3">

            {/* Archive picker */}
            <div className="rounded-2xl border p-5 transition-colors" style={{ background: t.card, borderColor: t.cardBorderC }}>
              <p className="text-xs font-orbitron uppercase tracking-widest mb-3" style={{ color: t.labelC }}>压缩包</p>
              <button
                onClick={handleSelectArchive}
                disabled={embed.isProcessing}
                className="w-full flex items-center gap-3 px-4 py-3 rounded-xl border-2 border-dashed transition-all text-left cursor-pointer focus:outline-none disabled:opacity-50 disabled:cursor-not-allowed"
                style={embed.archivePath
                  ? { borderColor: 'rgba(0,245,255,0.4)', background: 'rgba(0,245,255,0.05)' }
                  : isDragging
                    ? { borderColor: 'rgba(0,245,255,0.6)', background: 'rgba(0,245,255,0.08)' }
                    : { borderColor: t.cardBorderC }}
              >
                <Archive className="w-6 h-6 shrink-0" style={{ color: embed.archivePath || isDragging ? '#00f5ff' : t.textMuted }} />
                <div className="min-w-0">
                  {embed.archivePath ? (
                    <>
                      <p className="text-sm font-medium truncate" style={{ color: '#00f5ff' }}>{getFilename(embed.archivePath)}</p>
                      <p className="text-xs truncate" style={{ color: t.textFaint }} title={embed.archivePath}>{embed.archivePath}</p>
                    </>
                  ) : (
                    <p className="text-sm" style={{ color: isDragging ? '#00f5ff' : t.textDim }}>
                      {isDragging ? '松开以选择文件' : '点击选择或拖入文件 — .zip / .7z / .var / .rar'}
                    </p>
                  )}
                </div>
              </button>
            </div>

            {/* Watermark config */}
            <div className="rounded-2xl border p-5 space-y-4 transition-colors" style={{ background: t.card, borderColor: t.cardBorderC }}>
              <p className="text-xs font-orbitron uppercase tracking-widest" style={{ color: t.labelC }}>水印配置</p>

              {/* Source type */}
              <div className="flex gap-2">
                {(['singleText', 'excelFile'] as const).map((s) => (
                  <button
                    key={s}
                    onClick={() => setEmbed((prev) => ({ ...prev, sourceType: s }))}
                    disabled={embed.isProcessing}
                    className="flex-1 py-2 text-sm rounded-lg border transition-all cursor-pointer focus:outline-none disabled:opacity-50"
                    style={embed.sourceType === s
                      ? { borderColor: 'rgba(0,245,255,0.45)', background: 'rgba(0,245,255,0.08)', color: '#00f5ff', fontWeight: 500 }
                      : { borderColor: t.cardBorderC, color: t.textMuted }}
                  >
                    {s === 'singleText' ? '固定文本' : 'Excel 批量'}
                  </button>
                ))}
              </div>

              {/* Text / Excel input */}
              {embed.sourceType === 'singleText' ? (
                <input
                  type="text"
                  aria-label="水印文本"
                  value={embed.singleText}
                  onChange={(e) => setEmbed((prev) => ({ ...prev, singleText: e.target.value }))}
                  placeholder="输入水印文本"
                  disabled={embed.isProcessing}
                  className="w-full px-3 py-2.5 rounded-lg text-sm focus:outline-none disabled:opacity-50"
                  style={{ background: t.inputBg, border: `1px solid ${t.inputBorder}`, color: t.text }}
                  onFocus={(e) => (e.currentTarget.style.borderColor = t.focusC)}
                  onBlur={(e) => (e.currentTarget.style.borderColor = t.inputBorder)}
                />
              ) : (
                <div
                  onClick={handleSelectExcel}
                  className="flex items-center gap-3 px-4 py-3 rounded-xl border-2 border-dashed cursor-pointer transition-all"
                  style={embed.excelPath ? { borderColor: 'rgba(0,255,136,0.4)', background: 'rgba(0,255,136,0.05)' } : { borderColor: t.cardBorderC }}
                >
                  <FileSpreadsheet className="w-5 h-5 shrink-0" style={{ color: embed.excelPath ? '#00ff88' : t.textMuted }} />
                  <div className="min-w-0">
                    {embed.excelPath ? (
                      <>
                        <p className="text-sm font-medium truncate" style={{ color: '#00ff88' }}>{getFilename(embed.excelPath)}</p>
                        <p className="text-xs" style={{ color: t.textFaint }}>每行对应一个文件的水印，首行为表头自动跳过</p>
                      </>
                    ) : (
                      <p className="text-sm" style={{ color: t.textDim }}>点击选择 Excel 文件 — .xlsx / .xls</p>
                    )}
                  </div>
                </div>
              )}

              {/* Watermark encoding mode */}
              <div>
                <p className="text-xs mb-2" style={{ color: t.textDim }}>水印编码方式</p>
                <div className="grid grid-cols-3 gap-2">
                  {([
                    { value: 'md5',       label: 'MD5 哈希',    desc: '不可逆哈希，隐藏原始内容' },
                    { value: 'plaintext', label: '明文',         desc: '原文直接写入' },
                    { value: 'aes',       label: 'AES-256-GCM', desc: '加密后写入，提取需密钥' },
                  ] as { value: EmbedState['watermarkMode']; label: string; desc: string }[]).map(({ value, label, desc }) => (
                    <button
                      key={value}
                      onClick={() => setEmbed((prev) => ({ ...prev, watermarkMode: value }))}
                      disabled={embed.isProcessing}
                      title={desc}
                      className="py-2 px-1 text-xs rounded-lg border transition-all cursor-pointer focus:outline-none disabled:opacity-50"
                      style={embed.watermarkMode === value
                        ? { borderColor: 'rgba(168,85,247,0.5)', background: 'rgba(168,85,247,0.1)', color: '#a855f7', fontWeight: 500 }
                        : { borderColor: t.cardBorderC, color: t.textMuted }}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>

              {/* AES key input */}
              {embed.watermarkMode === 'aes' && (
                <div>
                  <p className="text-xs mb-1.5" style={{ color: t.textDim }}>
                    AES 密钥 <span style={{ color: t.textFaint }}>（提取水印时需要输入相同密钥）</span>
                  </p>
                  <input
                    type="password"
                    aria-label="AES 密钥"
                    value={embed.aesKey}
                    onChange={(e) => setEmbed((prev) => ({ ...prev, aesKey: e.target.value }))}
                    placeholder="输入自定义密钥"
                    disabled={embed.isProcessing}
                    className="w-full px-3 py-2 rounded-lg text-sm focus:outline-none disabled:opacity-50"
                    style={{ background: t.inputBg, border: `1px solid ${t.inputBorder}`, color: t.text }}
                    onFocus={(e) => (e.currentTarget.style.borderColor = t.focusP)}
                    onBlur={(e) => (e.currentTarget.style.borderColor = t.inputBorder)}
                  />
                </div>
              )}

              {/* File type toggles */}
              <div>
                <p className="text-xs mb-2" style={{ color: t.textDim }}>处理类型</p>
                <div className="grid grid-cols-4 gap-2">
                  {([
                    { key: 'processJson', label: 'JSON' },
                    { key: 'processVaj',  label: 'VAJ' },
                    { key: 'processVmi',  label: 'VMI' },
                    { key: 'processImages', label: '图片*' },
                  ] as { key: keyof EmbedState; label: string }[]).map(({ key, label }) => {
                    const checked = embed[key] as boolean;
                    return (
                      <label
                        key={key}
                        className="flex items-center justify-center gap-1.5 py-2 rounded-lg border cursor-pointer transition-all text-sm select-none"
                        style={checked ? {
                          borderColor: 'rgba(0,245,255,0.5)', background: 'rgba(0,245,255,0.08)',
                          color: '#00f5ff', fontWeight: 500,
                          opacity: embed.isProcessing ? 0.5 : 1, cursor: embed.isProcessing ? 'not-allowed' : 'pointer',
                        } : {
                          borderColor: t.cardBorderC, color: t.textMuted,
                          opacity: embed.isProcessing ? 0.5 : 1, cursor: embed.isProcessing ? 'not-allowed' : 'pointer',
                        }}
                      >
                        <input
                          type="checkbox"
                          checked={checked}
                          onChange={(e) => setEmbed((prev) => ({ ...prev, [key]: e.target.checked }))}
                          disabled={embed.isProcessing}
                          className="sr-only"
                        />
                        {checked && <Check className="w-3 h-3" />}
                        {label}
                      </label>
                    );
                  })}
                </div>
                <p className="text-xs mt-1.5" style={{ color: t.textFaint }}>* 图片水印仅支持 PNG 格式，JPG/JPEG 文件将原样保留</p>
              </div>


              {/* Fast mode toggle */}
              {embed.processImages && (
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-sm" style={{ color: t.text }}>高速模式</p>
                    <p className="text-xs mt-0.5" style={{ color: t.textDim }}>
                      对 &gt;512px 大图仅处理左上角 512×512 区域，速度可提升 4–10 倍
                    </p>
                  </div>
                  <button
                    role="switch"
                    aria-checked={embed.fastMode}
                    onClick={() => setEmbed((prev) => ({ ...prev, fastMode: !prev.fastMode }))}
                    disabled={embed.isProcessing}
                    className="relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors cursor-pointer focus:outline-none disabled:opacity-50"
                    style={{ background: embed.fastMode ? '#00f5ff' : t.toggleOff, boxShadow: embed.fastMode ? '0 0 10px rgba(0,245,255,0.5)' : 'none' }}
                  >
                    <span className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${embed.fastMode ? 'translate-x-6' : 'translate-x-1'}`} />
                  </button>
                </div>
              )}

              {/* Image selection list */}
              {embed.processImages && (
                <div>
                  <div className="flex items-center justify-between mb-2">
                    <p className="text-xs" style={{ color: t.textDim }}>
                      选择图片
                      {embed.imageList.length > 0 && (
                        <span className="ml-1" style={{ color: t.textFaint }}>（已选 {embed.selectedImages.length}/{embed.imageList.length}）</span>
                      )}
                    </p>
                    {embed.imageList.length > 0 && (
                      <div className="flex gap-2">
                        <button
                          onClick={() => setEmbed((prev) => ({ ...prev, selectedImages: prev.imageList.filter((img) => img.toLowerCase().endsWith('.png')) }))}
                          disabled={embed.isProcessing}
                          className="text-xs cursor-pointer focus:outline-none rounded disabled:opacity-50"
                          style={{ color: '#00f5ff' }}
                        >全选 PNG</button>
                        <button
                          onClick={() => setEmbed((prev) => ({ ...prev, selectedImages: [] }))}
                          disabled={embed.isProcessing}
                          className="text-xs cursor-pointer focus:outline-none rounded disabled:opacity-50 opacity-60 hover:opacity-100 transition-opacity"
                          style={{ color: t.textMuted }}
                        >取消全选</button>
                      </div>
                    )}
                  </div>
                  {embed.imageListLoading ? (
                    <div className="space-y-1.5">
                      {[...Array(4)].map((_, i) => (
                        <div key={i} className="h-8 rounded-lg animate-pulse" style={{ background: t.emptyBg }} />
                      ))}
                    </div>
                  ) : embed.imageList.length === 0 ? (
                    <p className="text-xs py-2" style={{ color: t.textDim }}>
                      {embed.archivePath ? '压缩包中未找到图片（PNG/JPEG）' : '请先选择压缩包'}
                    </p>
                  ) : (
                    <div className="max-h-40 overflow-y-auto rounded-lg border divide-y" style={{ borderColor: t.cardBorderC }}>
                      {[...embed.imageList]
                        .sort((a, b) => {
                          const aPng = a.toLowerCase().endsWith('.png');
                          const bPng = b.toLowerCase().endsWith('.png');
                          if (aPng && !bPng) return -1;
                          if (!aPng && bPng) return 1;
                          return a.localeCompare(b);
                        })
                        .map((img) => {
                          const isPng = img.toLowerCase().endsWith('.png');
                          const checked = embed.selectedImages.includes(img);
                          return (
                            <label
                              key={img}
                              className={`flex items-center gap-2 px-3 py-2 transition-colors text-xs select-none ${embed.isProcessing ? 'pointer-events-none' : ''}`}
                              style={{
                                background: !isPng ? 'transparent' : checked ? t.listChecked : t.listBg,
                                opacity: !isPng ? 0.35 : 1,
                                cursor: !isPng ? 'not-allowed' : 'pointer',
                                borderColor: t.divider,
                              }}
                            >
                              <input
                                type="checkbox"
                                checked={checked}
                                onChange={(e) => {
                                  if (!isPng) return;
                                  setEmbed((prev) => ({
                                    ...prev,
                                    selectedImages: e.target.checked
                                      ? [...prev.selectedImages, img]
                                      : prev.selectedImages.filter((s) => s !== img),
                                  }));
                                }}
                                disabled={embed.isProcessing || !isPng}
                              />
                              <span className="font-mono truncate" style={{ color: isPng ? t.textMuted : t.textFaint }} title={img}>{img}</span>
                              {!isPng && <span className="ml-auto shrink-0" style={{ color: t.textFaint }}>仅限 PNG</span>}
                            </label>
                          );
                        })}
                    </div>
                  )}
                </div>
              )}

              {/* Watermark field name */}
              {!embed.processObfuscation && (
                <div>
                  <p className="text-xs mb-1.5" style={{ color: t.textDim }}>
                    水印字段名 <span style={{ color: t.textFaint }}>（留空默认 <span className="font-mono">_watermark</span>）</span>
                  </p>
                  <input
                    type="text"
                    aria-label="水印字段名"
                    value={embed.watermarkKey}
                    onChange={(e) => setEmbed((prev) => ({ ...prev, watermarkKey: e.target.value }))}
                    placeholder="_watermark"
                    disabled={embed.isProcessing}
                    className="w-full px-3 py-2 rounded-lg text-sm font-mono focus:outline-none disabled:opacity-50"
                    style={{ background: t.inputBg, border: `1px solid ${t.inputBorder}`, color: t.text }}
                    onFocus={(e) => (e.currentTarget.style.borderColor = t.focusC)}
                    onBlur={(e) => (e.currentTarget.style.borderColor = t.inputBorder)}
                  />
                </div>
              )}

              {/* Obfuscation toggle */}
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-sm" style={{ color: t.text }}>水印混淆</p>
                  <p className="text-xs mt-0.5" style={{ color: t.textDim }}>随机命名水印字段并插入既有字段旁，提高隐蔽性</p>
                </div>
                <button
                  role="switch"
                  aria-checked={embed.processObfuscation}
                  onClick={() => setEmbed((prev) => ({ ...prev, processObfuscation: !prev.processObfuscation }))}
                  disabled={embed.isProcessing}
                  className="relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors cursor-pointer focus:outline-none disabled:opacity-50"
                  style={{ background: embed.processObfuscation ? '#00f5ff' : t.toggleOff, boxShadow: embed.processObfuscation ? '0 0 10px rgba(0,245,255,0.5)' : 'none' }}
                >
                  <span className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${embed.processObfuscation ? 'translate-x-6' : 'translate-x-1'}`} />
                </button>
              </div>
            </div>

            {/* Output & Run */}
            <div className="rounded-2xl border p-5 space-y-3 transition-colors" style={{ background: t.card, borderColor: t.cardBorderC }}>
              <p className="text-xs font-orbitron uppercase tracking-widest" style={{ color: t.labelC }}>输出目录</p>

              <div className="flex items-center gap-2">
                <button
                  onClick={handleSelectOutputDir}
                  disabled={embed.isProcessing}
                  className="shrink-0 px-3 py-2 rounded-lg text-sm transition-colors cursor-pointer focus:outline-none disabled:opacity-50"
                  style={{ background: t.dirBtnBg, border: `1px solid ${t.dirBtnBorder}`, color: t.dirBtnColor }}
                >
                  选择目录
                </button>
                <span className="text-sm truncate flex-1" style={{ color: t.textMuted }} title={embed.outputDir ?? ''}>
                  {embed.outputDir ?? '默认：与源文件同目录'}
                </span>
                {embed.outputDir && (
                  <button
                    onClick={() => setEmbed((prev) => ({ ...prev, outputDir: null }))}
                    disabled={embed.isProcessing}
                    className="shrink-0 hover:text-red-400 transition-colors cursor-pointer focus:outline-none rounded disabled:opacity-50"
                    style={{ color: t.clearBtn }}
                    title="清除"
                  >
                    <X className="w-4 h-4" />
                  </button>
                )}
              </div>

              <button
                onClick={handleProcess}
                disabled={embed.isProcessing || !embed.archivePath}
                className="w-full py-3 rounded-xl text-sm font-semibold font-orbitron transition-all cursor-pointer focus:outline-none disabled:cursor-not-allowed flex items-center justify-center gap-2"
                style={{
                  background: embed.isProcessing || !embed.archivePath ? t.emptyBg : 'linear-gradient(135deg, #ff0080, #a855f7)',
                  color: embed.isProcessing || !embed.archivePath ? t.textDim : 'white',
                  boxShadow: embed.isProcessing || !embed.archivePath ? 'none' : '0 0 20px rgba(255,0,128,0.3), 0 0 40px rgba(168,85,247,0.15)',
                }}
              >
                {embed.isProcessing && <Loader2 className="w-4 h-4 animate-spin" />}
                {embed.isProcessing ? '处理中…' : '开始添加水印'}
              </button>

              {/* Progress */}
              {(embed.isProcessing || isComplete || isError) && (
                <div className="space-y-2 pt-1">
                  <div className="flex items-center justify-between text-xs">
                    <div className="flex items-center gap-1.5 shrink-0">
                      <span className="font-medium" style={{ color: isError ? '#f87171' : isComplete ? '#00ff88' : '#00f5ff' }}>
                        {getStatusLabel(embed.statusCode)}
                      </span>
                      <span className="tabular-nums" style={{ color: t.textDim }}>· {formatElapsed(elapsedSec)}</span>
                    </div>
                    <span className="truncate max-w-[160px] ml-2" style={{ color: t.textDim }} title={embed.statusMessage}>
                      {embed.statusMessage}
                    </span>
                  </div>

                  {!isError && (
                    <div className="h-1.5 w-full rounded-full overflow-hidden" style={{ background: t.divider }}>
                      <div
                        className="h-full rounded-full transition-all duration-300"
                        style={{
                          width: `${isComplete ? 100 : embedProgress}%`,
                          background: isComplete ? 'linear-gradient(90deg, #00ff88, #00f5ff)' : 'linear-gradient(90deg, #ff0080, #a855f7)',
                          boxShadow: isComplete ? '0 0 8px rgba(0,255,136,0.6)' : '0 0 8px rgba(255,0,128,0.6)',
                        }}
                      />
                    </div>
                  )}

                  {embed.detail && embed.detail.batchTotal > 1 && (
                    <div className="flex items-center gap-2 text-xs" style={{ color: t.textDim }}>
                      <span className="shrink-0">批次</span>
                      <div className="flex-1 h-1 rounded-full overflow-hidden" style={{ background: t.divider }}>
                        <div
                          className="h-full rounded-full transition-all duration-300"
                          style={{ width: `${Math.round((embed.detail.batchCurrent / embed.detail.batchTotal) * 100)}%`, background: '#a855f7', boxShadow: '0 0 6px rgba(168,85,247,0.6)' }}
                        />
                      </div>
                      <span className="tabular-nums shrink-0">{embed.detail.batchCurrent}/{embed.detail.batchTotal}</span>
                    </div>
                  )}

                  {embed.scan && (
                    <div className="grid grid-cols-2 gap-x-4 gap-y-1">
                      {([
                        { key: 'json' as keyof TypeCounters, label: 'JSON', total: embed.scan.jsonCount, show: embed.processJson },
                        { key: 'vaj' as keyof TypeCounters, label: 'VAJ',  total: embed.scan.vajCount,  show: embed.processVaj },
                        { key: 'vmi' as keyof TypeCounters, label: 'VMI',  total: embed.scan.vmiCount,  show: embed.processVmi },
                        { key: 'image' as keyof TypeCounters, label: '图片', total: embed.scan.imageCount, show: embed.processImages },
                      ])
                        .filter(({ show, total }) => show && total > 0)
                        .map(({ key, label, total }) => {
                          const done = embed.typeCounters[key];
                          const pct = Math.round((done / total) * 100);
                          return (
                            <div key={key} className="flex items-center gap-1.5 text-xs" style={{ color: t.textDim }}>
                              <span className="w-7 shrink-0">{label}</span>
                              <div className="flex-1 h-1 rounded-full overflow-hidden" style={{ background: t.divider }}>
                                <div className="h-full rounded-full transition-all duration-300" style={{ width: `${pct}%`, background: '#00f5ff', boxShadow: '0 0 6px rgba(0,245,255,0.6)' }} />
                              </div>
                              <span className="tabular-nums shrink-0">{done}/{total}</span>
                            </div>
                          );
                        })}
                    </div>
                  )}

                  {embed.detail && !isComplete && (
                    <p className="flex items-center gap-1 text-xs truncate" style={{ color: t.textDim }} title={embed.detail.filename}>
                      <CornerDownRight className="w-3 h-3 shrink-0" />{embed.detail.filename}
                    </p>
                  )}

                  {embed.progressTotal > 0 && !isComplete && (
                    <p className="text-xs" style={{ color: t.textDim }}>
                      图片 {embed.progressCurrent}/{embed.progressTotal}
                      {embed.progressFilename && <span className="ml-1">· {getFilename(embed.progressFilename)}</span>}
                    </p>
                  )}
                </div>
              )}

              {embed.error && (
                <div className="p-3 rounded-xl" style={{ background: 'rgba(239,68,68,0.08)', border: '1px solid rgba(239,68,68,0.25)' }}>
                  <p className="text-red-500 text-sm">{embed.error}</p>
                </div>
              )}

              {embed.outputPath && (
                <div className="p-3 rounded-xl" style={{ background: 'rgba(0,255,136,0.06)', border: '1px solid rgba(0,255,136,0.2)' }}>
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <p className="text-xs mb-1" style={{ color: 'rgba(0,255,136,0.7)' }}>
                        {embed.sourceType === 'excelFile' ? '批量输出目录' : '输出文件'}
                      </p>
                      <p className="text-xs font-mono break-all" style={{ color: '#00ff88' }}>{embed.outputPath}</p>
                    </div>
                    <button
                      onClick={() => handleCopy('output-path', embed.outputPath!)}
                      className="shrink-0 px-3 py-1 text-xs rounded-lg transition-all cursor-pointer focus:outline-none font-orbitron"
                      style={{ background: 'rgba(0,255,136,0.15)', color: '#00ff88', border: '1px solid rgba(0,255,136,0.3)' }}
                    >
                      {copiedKey === 'output-path' ? '已复制!' : '复制'}
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        )}

        {/* ===== Extract Tab ===== */}
        {activeTab === 'extract' && (
          <div className="space-y-3">

            {/* Archive picker */}
            <div className="rounded-2xl border p-5 transition-colors" style={{ background: t.card, borderColor: t.cardBorderP }}>
              <p className="text-xs font-orbitron uppercase tracking-widest mb-3" style={{ color: t.labelP }}>压缩包</p>
              <button
                onClick={handleSelectExtractArchive}
                disabled={extract.isExtracting}
                className="w-full flex items-center gap-3 px-4 py-3 rounded-xl border-2 border-dashed transition-all text-left cursor-pointer focus:outline-none disabled:opacity-50 disabled:cursor-not-allowed"
                style={extract.archivePath
                  ? { borderColor: 'rgba(168,85,247,0.4)', background: 'rgba(168,85,247,0.05)' }
                  : isDragging
                    ? { borderColor: 'rgba(168,85,247,0.6)', background: 'rgba(168,85,247,0.08)' }
                    : { borderColor: t.cardBorderP }}
              >
                <Archive className="w-6 h-6 shrink-0" style={{ color: extract.archivePath || isDragging ? '#a855f7' : t.textMuted }} />
                <div className="min-w-0">
                  {extract.archivePath ? (
                    <>
                      <p className="text-sm font-medium truncate" style={{ color: '#a855f7' }}>{getFilename(extract.archivePath)}</p>
                      <p className="text-xs truncate" style={{ color: t.textFaint }} title={extract.archivePath}>{extract.archivePath}</p>
                    </>
                  ) : (
                    <p className="text-sm" style={{ color: isDragging ? '#a855f7' : t.textDim }}>
                      {isDragging ? '松开以选择文件' : '点击选择或拖入已添加水印的压缩包'}
                    </p>
                  )}
                </div>
              </button>
            </div>

            {/* Action */}
            <div className="rounded-2xl border p-5 space-y-3 transition-colors" style={{ background: t.card, borderColor: t.cardBorderP }}>

              {/* AES key input */}
              <div>
                <p className="text-xs mb-1.5" style={{ color: t.textDim }}>
                  AES 密钥 <span style={{ color: t.textFaint }}>（仅在水印使用 AES 加密时需要）</span>
                </p>
                <input
                  type="password"
                  aria-label="AES 密钥"
                  value={extract.aesKey}
                  onChange={(e) => setExtract((prev) => ({ ...prev, aesKey: e.target.value }))}
                  placeholder="留空则仅识别明文和 MD5 水印"
                  disabled={extract.isExtracting}
                  className="w-full px-3 py-2 rounded-lg text-sm focus:outline-none disabled:opacity-50"
                  style={{ background: t.inputBg, border: `1px solid ${t.inputBorder}`, color: t.text }}
                  onFocus={(e) => (e.currentTarget.style.borderColor = t.focusP)}
                  onBlur={(e) => (e.currentTarget.style.borderColor = t.inputBorder)}
                />
              </div>

              {/* Scan images toggle */}
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-sm" style={{ color: t.text }}>扫描图片水印</p>
                  <p className="text-xs mt-0.5" style={{ color: t.textDim }}>
                    关闭后跳过图片盲水印提取，仅扫描 JSON/VAJ/VMI 水印
                  </p>
                </div>
                <button
                  role="switch"
                  aria-checked={extract.scanImages}
                  onClick={() => setExtract((prev) => ({ ...prev, scanImages: !prev.scanImages }))}
                  disabled={extract.isExtracting}
                  className="relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors cursor-pointer focus:outline-none disabled:opacity-50"
                  style={{ background: extract.scanImages ? '#a855f7' : t.toggleOff, boxShadow: extract.scanImages ? '0 0 10px rgba(168,85,247,0.5)' : 'none' }}
                >
                  <span className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${extract.scanImages ? 'translate-x-6' : 'translate-x-1'}`} />
                </button>
              </div>

              <button
                onClick={handleExtract}
                disabled={extract.isExtracting || !extract.archivePath}
                className="w-full py-3 rounded-xl text-sm font-semibold font-orbitron transition-all cursor-pointer focus:outline-none disabled:cursor-not-allowed flex items-center justify-center gap-2"
                style={{
                  background: extract.isExtracting || !extract.archivePath ? t.emptyBg : 'linear-gradient(135deg, #a855f7, #00f5ff)',
                  color: extract.isExtracting || !extract.archivePath ? t.textDim : 'white',
                  boxShadow: extract.isExtracting || !extract.archivePath ? 'none' : '0 0 20px rgba(168,85,247,0.3), 0 0 40px rgba(0,245,255,0.15)',
                }}
              >
                {extract.isExtracting && <Loader2 className="w-4 h-4 animate-spin" />}
                {extract.isExtracting ? '提取中…' : '提取水印'}
              </button>

              {extract.error && (
                <div className="p-3 rounded-xl" style={{ background: 'rgba(239,68,68,0.08)', border: '1px solid rgba(239,68,68,0.25)' }}>
                  <p className="text-red-500 text-sm">{extract.error}</p>
                </div>
              )}

              {extract.result && (() => {
                const groups = new Map<string, { files: string[]; mode: string; decrypted: boolean }>();
                for (const f of extract.result) {
                  const existing = groups.get(f.value);
                  if (existing) { existing.files.push(f.file); }
                  else { groups.set(f.value, { files: [f.file], mode: f.mode, decrypted: f.decrypted }); }
                }
                const entries = Array.from(groups.entries());
                return entries.length === 0 ? (
                  <div className="p-3 rounded-xl" style={{ background: t.emptyBg }}>
                    <p className="text-sm" style={{ color: t.textMuted }}>未在压缩包中找到水印字段</p>
                  </div>
                ) : (
                  <div className="space-y-2">
                    <p className="text-xs" style={{ color: t.textDim }}>
                      共扫描到 {extract.result.length} 个含水印文件，{entries.length} 个不同值
                    </p>
                    {entries.map(([value, { files, mode, decrypted }]) => {
                      const modeLabel = mode === 'md5' ? 'MD5' : mode === 'plaintext' ? '明文' : mode === 'aes' ? 'AES' : '未知';
                      const accent = mode === 'md5' ? '#a855f7' : mode === 'plaintext' ? '#00ff88' : decrypted ? '#00f5ff' : '#f97316';
                      return (
                        <div key={value} className="p-4 rounded-xl" style={{ background: `${accent}08`, border: `1px solid ${accent}28`, boxShadow: `0 0 12px ${accent}12` }}>
                          <div className="flex items-start justify-between gap-2 mb-2">
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center gap-2 mb-1">
                                <span className="text-xs px-1.5 py-0.5 rounded font-medium font-orbitron" style={{ background: `${accent}20`, color: accent, border: `1px solid ${accent}40` }}>
                                  {modeLabel}
                                </span>
                                {mode === 'aes' && !decrypted && (
                                  <span className="flex items-center gap-1 text-xs text-orange-400"><Lock className="w-3 h-3" /> 未解密（密钥错误或未提供）</span>
                                )}
                                {mode === 'aes' && decrypted && (
                                  <span className="flex items-center gap-1 text-xs" style={{ color: '#00f5ff' }}><Unlock className="w-3 h-3" /> 已解密</span>
                                )}
                              </div>
                              <p className={`text-sm font-mono break-all ${mode === 'md5' ? 'tracking-wide' : ''}`} style={{ color: accent }}>
                                {mode === 'md5' ? formatMD5(value) : value}
                              </p>
                            </div>
                            <button
                              onClick={() => handleCopy(`wm-${value}`, value)}
                              className="shrink-0 px-3 py-1 text-white text-xs rounded-lg transition-all cursor-pointer focus:outline-none font-orbitron"
                              style={{ background: accent, boxShadow: `0 0 10px ${accent}50` }}
                            >
                              {copiedKey === `wm-${value}` ? '已复制!' : '复制'}
                            </button>
                          </div>
                          <div className="space-y-0.5">
                            {files.map((f) => (
                              <p key={f} className="text-xs font-mono truncate" style={{ color: `${accent}70` }} title={f}>· {f}</p>
                            ))}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                );
              })()}

              {/* Image watermark findings */}
              {extract.scanImages && extract.scannedPngCount !== null && extract.scannedPngCount === 0 && (
                <div className="p-3 rounded-xl" style={{ background: 'rgba(255,200,0,0.06)', border: '1px solid rgba(255,200,0,0.2)' }}>
                  <p className="text-xs" style={{ color: 'rgba(255,200,0,0.7)' }}>
                    压缩包内无 PNG 图片，图片水印仅支持 PNG（JPEG 有损压缩会破坏水印）
                  </p>
                </div>
              )}
              {extract.scanImages && extract.scannedPngCount !== null && extract.scannedPngCount > 0 && extract.imageFindings !== null && extract.imageFindings.length === 0 && (
                <div className="p-3 rounded-xl" style={{ background: 'rgba(255,200,0,0.06)', border: '1px solid rgba(255,200,0,0.2)' }}>
                  <p className="text-xs" style={{ color: 'rgba(255,200,0,0.7)' }}>
                    找到 {extract.scannedPngCount} 张 PNG 图片，但均未检测到图片盲水印（可能未嵌入水印，或图片尺寸过小）
                  </p>
                </div>
              )}
              {extract.imageFindings && extract.imageFindings.length > 0 && (
                <div className="space-y-2">
                  <p className="text-xs font-orbitron uppercase tracking-widest" style={{ color: 'rgba(0,245,255,0.55)' }}>
                    图片盲水印（{extract.imageFindings.length} 张）
                  </p>
                  {extract.imageFindings.map((f) => (
                    <div
                      key={f.file}
                      className="p-3 rounded-xl"
                      style={{ background: 'rgba(0,245,255,0.05)', border: '1px solid rgba(0,245,255,0.2)', boxShadow: '0 0 10px rgba(0,245,255,0.08)' }}
                    >
                      <div className="flex items-start justify-between gap-2">
                        <div className="min-w-0 flex-1">
                          <p className="text-xs font-mono truncate mb-1" style={{ color: 'rgba(0,245,255,0.55)' }} title={f.file}>· {f.file}</p>
                          <p className="text-sm break-all" style={{ color: t.text }}>{f.text}</p>
                        </div>
                        <button
                          onClick={() => handleCopy(`img-${f.file}`, f.text)}
                          className="shrink-0 px-3 py-1 text-xs rounded-lg transition-all cursor-pointer focus:outline-none font-orbitron"
                          style={{ background: '#00f5ff', color: t.bg, fontWeight: 600, boxShadow: '0 0 12px rgba(0,245,255,0.5)' }}
                        >
                          {copiedKey === `img-${f.file}` ? '已复制!' : '复制'}
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              )}

            </div>
          </div>
        )}

      </div>
    </div>
  );
}

export default App;
