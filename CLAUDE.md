# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**BlindMark Master** is a desktop application for batch blind watermarking of images within compressed archives (.zip, .7z, .rar). The application uses DWT (Discrete Wavelet Transform) + DCT (Discrete Cosine Transform) watermarking algorithms to embed MD5-encoded watermarks into images while preserving visual quality.

### Key Features
- Batch watermark embedding for all images in compressed archives
- Support for .zip, .7z, and .rar archive formats
- Watermark input via single text or Excel file (sequential row-to-file mapping)
- Real-time preview with adjustable strength (0.1-1.0)
- Robust DWT+DCT frequency-domain watermarking
- Parallel processing using all CPU cores (Rayon)
- Disk-based temporary workspace to handle large archives
- Dark/light theme support

## Tech Stack

### Backend (Rust)
- **Framework**: Tauri 2.0
- **Image Processing**: `image`, `ndarray`
- **Watermarking**: `omni-wave` (DWT), `rustdct` (DCT), `md-5`
- **Compression**: `zip`, `sevenz-rust`, `unrar`
- **Utilities**: `rayon`, `tempfile`, `walkdir`, `calamine`, `thiserror`

### Frontend (TypeScript/React)
- **Framework**: Vite + React + TypeScript
- **Styling**: Tailwind CSS
- **Components**: Shadcn UI
- **Icons**: Lucide React
- **Animation**: Framer Motion

## Build Commands

### Development
```bash
# Install frontend dependencies
npm install

# Run development server
npm run tauri dev

# Frontend only (without Tauri)
npm run dev
```

### Production
```bash
# Build production binary
npm run tauri build

# Frontend build only
npm run build
```

### Testing
```bash
# Run Rust tests
cd src-tauri && cargo test

# Run Rust tests with output
cd src-tauri && cargo test -- --nocapture
```

## Prerequisites

### Required Tools
- **Rust**: Install from https://rustup.rs/
- **Node.js**: Version 18+ recommended
- **System Dependencies**:
  - macOS: `brew install 7zip`
  - Linux: `sudo apt-get install p7zip-full unrar`
  - Windows: Install 7-Zip and WinRAR

### Development Setup
1. Clone repository
2. Install Rust toolchain: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
3. Install Node.js dependencies: `npm install`
4. Install system dependencies (7zip, unrar)
5. Run development server: `npm run tauri dev`

## Architecture

### Project Structure
```
blindmarktool/
├── src/                          # React frontend
│   ├── components/               # UI components
│   ├── hooks/                    # Custom React hooks
│   ├── lib/                      # TypeScript types and utilities
│   ├── App.tsx                   # Main application component
│   └── main.tsx                  # React entry point
├── src-tauri/                    # Rust backend
│   ├── src/
│   │   ├── commands/             # Tauri command handlers
│   │   │   ├── watermark.rs      # Watermark embed/extract commands
│   │   │   ├── archive.rs        # Archive processing command
│   │   │   └── excel.rs          # Excel reading command
│   │   ├── core/
│   │   │   ├── watermark/        # Watermarking algorithms
│   │   │   │   ├── encoder.rs    # ✅ MD5 encoding/decoding (IMPLEMENTED)
│   │   │   │   ├── dwt.rs        # ⏳ DWT decomposition (TODO)
│   │   │   │   ├── dct.rs        # ⏳ DCT transform (TODO)
│   │   │   │   ├── embedder.rs   # ⏳ Complete embedding pipeline (TODO)
│   │   │   │   └── extractor.rs  # ⏳ Extraction pipeline (TODO)
│   │   │   ├── compression/      # Archive handling
│   │   │   │   ├── common.rs     # ✅ ArchiveHandler trait (DEFINED)
│   │   │   │   └── zip_handler.rs # ⏳ ZIP implementation (TODO)
│   │   │   └── file_ops/         # File management
│   │   │       ├── temp_manager.rs # ⏳ Disk-based workspace (TODO)
│   │   │       └── scanner.rs     # ⏳ Recursive file discovery (TODO)
│   │   ├── models/               # Data structures
│   │   │   ├── error.rs          # ✅ Custom error types (IMPLEMENTED)
│   │   │   ├── task.rs           # ✅ FileTask, ProcessingStatus (IMPLEMENTED)
│   │   │   └── config.rs         # ✅ WatermarkConfig (IMPLEMENTED)
│   │   ├── utils/                # Utilities
│   │   │   ├── progress.rs       # ⏳ Progress events (TODO)
│   │   │   └── parallel.rs       # ⏳ Rayon parallel processing (TODO)
│   │   ├── main.rs               # ✅ Entry point (IMPLEMENTED)
│   │   └── lib.rs                # ✅ Tauri setup (IMPLEMENTED)
│   ├── Cargo.toml                # ✅ Rust dependencies (CONFIGURED)
│   └── tauri.conf.json           # ✅ Tauri configuration (CONFIGURED)
├── package.json                  # ✅ Node dependencies (CONFIGURED)
├── vite.config.ts                # ✅ Vite configuration (CONFIGURED)
├── tailwind.config.js            # ✅ Tailwind configuration (CONFIGURED)
└── tsconfig.json                 # ✅ TypeScript configuration (CONFIGURED)
```

### Watermarking Algorithm Flow

#### Embedding Pipeline
1. **Input**: User text or Excel row → MD5 hash → 128-bit binary sequence
2. **Image Preprocessing**: RGB → Grayscale (ITU-R BT.601: 0.299R + 0.587G + 0.114B)
3. **DWT Decomposition**: 2-level Haar wavelet → Extract LH or HL mid-frequency subband
4. **DCT Transform**: Apply DCT to 8x8 blocks of selected subband
5. **Embedding**: Modify mid-frequency DCT coefficients using strength factor (0.1-1.0)
6. **Reconstruction**: IDCT → IDWT → Reconstruct RGB image
7. **Output**: Watermarked image preserving original format

#### Extraction Pipeline
1. **Input**: Watermarked image
2. **DWT Decomposition**: 2-level decomposition → Extract mid-frequency subband
3. **DCT Transform**: Apply DCT to 8x8 blocks
4. **Extraction**: Detect binary sequence from coefficient magnitudes
5. **Decoding**: Binary sequence → MD5 hash string
6. **Output**: Extracted MD5 hash

### Archive Processing Workflow
1. **Upload**: User drops .zip/.7z/.rar file via drag-drop or file picker
2. **Extraction**: Extract to temporary disk directory (using tempfile)
3. **Scanning**: Recursively scan for PNG/JPEG/JPG images, maintain relative paths
4. **Watermarking**: Process images in parallel using Rayon (one thread per image)
   - Single text mode: Same watermark for all images
   - Excel mode: Sequential mapping (Row N → File N by discovery order)
5. **Repackaging**: Create new archive with `_watermarked` suffix, preserve directory structure
6. **Cleanup**: Automatic temporary directory cleanup on completion

## Implementation Status

### ✅ Completed
- Project initialization (Tauri + Vite + React)
- Configuration files (Cargo.toml, package.json, tsconfig, tailwind)
- Rust data models (error types, task structures, config types)
- Watermark encoder (MD5 encoding/decoding with comprehensive tests)
- Module structure and placeholders

### ⏳ In Progress / TODO

#### Priority 1: Core Watermarking Algorithm
1. **DWT Processor** [src-tauri/src/core/watermark/dwt.rs](src-tauri/src/core/watermark/dwt.rs)
   - Implement using `omni-wave` crate with ndarray integration
   - `decompose()`: 2-level Haar wavelet decomposition
   - `reconstruct()`: Inverse DWT from modified components
   - Return `DWTComponents` struct with LL, LH, HL, HH subbands

2. **DCT Processor** [src-tauri/src/core/watermark/dct.rs](src-tauri/src/core/watermark/dct.rs)
   - Implement using `rustdct` crate with `DctPlanner`
   - `forward()`: Apply DCT to entire subband in 8x8 blocks
   - `inverse()`: Apply IDCT for reconstruction
   - `embed_watermark()`: Modify mid-frequency coefficients (positions: (2,3), (3,2), (3,3), (4,2), (4,3))
   - `extract_watermark()`: Detect bits from coefficient magnitudes

3. **Embedder** [src-tauri/src/core/watermark/embedder.rs](src-tauri/src/core/watermark/embedder.rs)
   - Integrate DWT, DCT, and encoder modules
   - RGB → Grayscale conversion
   - Complete embedding pipeline
   - Grayscale → RGB reconstruction

4. **Extractor** [src-tauri/src/core/watermark/extractor.rs](src-tauri/src/core/watermark/extractor.rs)
   - Reverse embedding pipeline
   - Extract binary sequence and decode to MD5

#### Priority 2: File Operations
5. **Temp Manager** [src-tauri/src/core/file_ops/temp_manager.rs](src-tauri/src/core/file_ops/temp_manager.rs)
   - Use `tempfile::Builder` for disk-based workspace
   - Create `extracted/` and `processed/` subdirectories
   - Implement `copy_processed()` to maintain hierarchy

6. **Scanner** [src-tauri/src/core/file_ops/scanner.rs](src-tauri/src/core/file_ops/scanner.rs)
   - Use `walkdir` for recursive traversal
   - Filter for PNG/JPEG/JPG
   - Sort by relative path (critical for Excel mapping)
   - Return `Vec<ImageFile>` with paths

#### Priority 3: Archive Handling
7. **ZIP Handler** [src-tauri/src/core/compression/zip_handler.rs](src-tauri/src/core/compression/zip_handler.rs)
   - Implement `ArchiveHandler` trait using `zip` crate
   - `extract()`: Decompress preserving structure
   - `create()`: Walk directory and create archive maintaining hierarchy
   - Use `CompressionMethod::Deflated`

8. **7z and RAR Handlers** (Future)
   - Similar implementations using `sevenz-rust` and `unrar`

#### Priority 4: Parallel Processing & Progress
9. **Progress Emitter** [src-tauri/src/utils/progress.rs](src-tauri/src/utils/progress.rs)
   - Wrap Tauri's `AppHandle`
   - Emit `watermark-progress` and `watermark-status` events
   - Use `emit_all()` for broadcasting

10. **Parallel Processor** [src-tauri/src/utils/parallel.rs](src-tauri/src/utils/parallel.rs)
    - Configure Rayon thread pool based on `num_cpus::get()`
    - Process images in parallel (one per thread)
    - Handle single text vs Excel mode
    - Track progress with `Arc<Mutex<>>`

#### Priority 5: Tauri Commands
11. **Watermark Commands** [src-tauri/src/commands/watermark.rs](src-tauri/src/commands/watermark.rs)
    - `embed_watermark_single()`: For preview (returns image bytes)
    - `extract_watermark()`: Returns extracted MD5 hash

12. **Archive Command** [src-tauri/src/commands/archive.rs](src-tauri/src/commands/archive.rs)
    - `process_archive()`: Main batch processing command
    - Integrate with `ArchiveProcessor` and `ProgressEmitter`

13. **Excel Command** [src-tauri/src/commands/excel.rs](src-tauri/src/commands/excel.rs)
    - Use `calamine` to read first column from .xlsx
    - Return `Vec<String>` of watermark texts

14. **Update lib.rs**
    - Register all commands with Tauri builder
    - Use `generate_handler![]` macro

#### Priority 6: Frontend
15. **TypeScript Types** [src/lib/types.ts](src/lib/types.ts)
    - Mirror Rust structs with camelCase
    - `FileTask`, `ProcessingStatus`, `WatermarkConfig`, `ProgressEvent`

16. **Hooks** [src/hooks/](src/hooks/)
    - `useTauriCommand.ts`: Generic command invocation hook
    - `useProgress.ts`: Event listener for progress updates
    - `useTheme.ts`: Dark/light mode detection

17. **UI Components** [src/components/](src/components/)
    - `DragDropZone.tsx`: File upload with drag-drop
    - `ImageComparisonViewer.tsx`: Split-view with slider
    - `WatermarkInput.tsx`: Text/Excel mode toggle
    - `StrengthControl.tsx`: Slider (0.1-1.0)
    - `TaskList.tsx`: Processing status list
    - `ExtractionResult.tsx`: MD5 hash display

18. **Main App** [src/App.tsx](src/App.tsx)
    - State management for config, tasks, preview
    - Wire up all components
    - Handle command invocations
    - Progress event listeners

## Critical Implementation Notes

### DWT Implementation
- **Recommended**: Use `omni-wave` crate for ndarray integration
- **Alternative**: Manual Haar wavelet implementation if library issues arise
- Store both level 1 and level 2 components for accurate reconstruction

### DCT Coefficient Selection
- Mid-frequency positions in 8x8 blocks: (2,3), (3,2), (3,3), (4,2), (4,3), etc.
- May need tuning for optimal robustness vs. invisibility trade-off
- Start conservatively and adjust based on PSNR measurements

### Excel Sequential Mapping
- Sort scanned files by relative path BEFORE mapping to Excel rows
- Row 1 → First file in sorted order
- If more files than rows: reuse last row
- If more rows than files: ignore extra rows

### Error Handling Patterns
Always use `Result<T, BlindMarkError>` for fallible operations:
```rust
// Good
pub fn process_image(&self, path: &Path) -> Result<Image, BlindMarkError> {
    let img = image::open(path)
        .map_err(|e| BlindMarkError::ImageProcessing(e.to_string()))?;
    Ok(img)
}

// For Tauri commands, convert to String
#[tauri::command]
pub async fn my_command() -> Result<Data, String> {
    do_work().map_err(|e| e.to_string())
}
```

### Performance Optimizations
- Use Rayon thread pool for parallel image processing
- Process images independently (one thread per image)
- For preview mode, consider downsampling images before watermarking
- Cache DWT/DCT planners across images if possible
- Always use `--release` flag for production builds (~10x faster)

### Memory Management
- Use disk-based temporary workspace (tempfile) for large archives
- Avoid loading entire archive into memory
- Process images one at a time within parallel workers
- For very large images (>20MP), consider tile-based processing

## Testing Strategy

### Unit Tests
Test each module independently:
```bash
# Test specific module
cargo test --package blindmark-master --lib core::watermark::encoder

# Test with output
cargo test -- --nocapture

# Test specific function
cargo test test_encode_known_text
```

### Integration Tests
- End-to-end watermarking: embed known text → extract → verify MD5 match
- Archive processing: create test.zip → process → verify directory structure preserved
- Excel mapping: create test.xlsx → process archive → verify sequential mapping

### Manual Testing Checklist
- [ ] Drag-drop .zip file
- [ ] Single text watermarking
- [ ] Excel file watermarking (5 images, 5 rows)
- [ ] Preview with different strength values (0.1, 0.5, 1.0)
- [ ] Extract watermark and verify MD5
- [ ] Dark/light theme switching
- [ ] Progress updates during batch processing
- [ ] Error handling (corrupted archive, unsupported format, missing Excel sheet)

## Common Development Tasks

### Adding a New Tauri Command
1. Create command function in `src-tauri/src/commands/`
2. Add `#[tauri::command]` attribute
3. Register in `src-tauri/src/lib.rs` using `generate_handler![]`
4. Call from frontend using `invoke('command_name', { args })`

### Modifying Watermark Algorithm
- **Strength parameter**: Adjust in [dct.rs](src-tauri/src/core/watermark/dct.rs) `embed_watermark()` method
- **DWT level**: Change in [dwt.rs](src-tauri/src/core/watermark/dwt.rs) `DWTProcessor::new()`
- **DCT block size**: Modify in [dct.rs](src-tauri/src/core/watermark/dct.rs) (currently 8x8)

### Debugging Tips
- Enable Rust backtraces: `RUST_BACKTRACE=1 npm run tauri dev`
- Check Tauri console in browser DevTools (Cmd+Option+I on macOS)
- Use `println!()` or `eprintln!()` in Rust code (appears in terminal)
- Frontend logs appear in browser console
- For release builds, check logs in system console app

## Security Considerations
- Validate file paths to prevent directory traversal
- Limit archive extraction size to prevent zip bombs
- Sanitize Excel input (treat all cells as text)
- Use Tauri's capability system to restrict file system access
- Never execute arbitrary code from archive contents

## Known Limitations
- RAR extraction requires system `unrar` library (licensing constraints)
- Very large images (>20MP) may require significant memory
- Watermark robustness varies with JPEG compression level
- Sequential Excel mapping requires stable file system ordering

## Useful Resources
- [Tauri Documentation](https://tauri.app/v2/guides/)
- [omni-wave Documentation](https://docs.rs/omni-wave/latest/omni_wave/)
- [rustdct Documentation](https://docs.rs/rustdct/latest/rustdct/)
- [Watermarking Theory](https://en.wikipedia.org/wiki/Digital_watermarking)
- [DWT Tutorial](https://en.wikipedia.org/wiki/Discrete_wavelet_transform)
- [DCT Tutorial](https://en.wikipedia.org/wiki/Discrete_cosine_transform)

## Next Steps for Implementation
1. **Start with encoder tests**: Verify MD5 encoding/decoding works correctly
2. **Implement DWT processor**: Critical for watermarking algorithm
3. **Implement DCT processor**: Second critical component
4. **Integrate in embedder**: Combine DWT + DCT + encoder
5. **Implement extractor**: Reverse pipeline
6. **Test watermarking**: Embed and extract on single image
7. **Implement file operations**: Temp manager and scanner
8. **Implement ZIP handler**: Archive extraction and creation
9. **Implement parallel processor**: Batch processing
10. **Wire up Tauri commands**: Connect backend to frontend
11. **Build frontend UI**: Components and App.tsx
12. **End-to-end testing**: Full workflow from archive to watermarked archive

## Implementation Plan Reference
Detailed implementation plan is located at: [/Users/wxl/.claude/plans/distributed-churning-aurora.md](/Users/wxl/.claude/plans/distributed-churning-aurora.md)
