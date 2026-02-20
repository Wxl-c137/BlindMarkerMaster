# BlindMark Master

A desktop application for batch blind watermarking of images within compressed archives using DWT+DCT frequency-domain algorithms.

## Features

- 🎯 **Batch Processing**: Watermark all images in .zip, .7z, or .rar archives
- 🔐 **Secure Watermarking**: MD5-encoded watermarks embedded using DWT+DCT algorithms
- 📊 **Flexible Input**: Single text or Excel file for sequential watermark mapping
- 👁️ **Real-time Preview**: Adjustable strength (0.1-1.0) with before/after comparison
- ⚡ **Parallel Processing**: Utilizes all CPU cores for fast batch operations
- 🎨 **Modern UI**: Clean interface with dark/light theme support

## Tech Stack

- **Backend**: Rust + Tauri 2.0
- **Frontend**: React + TypeScript + Tailwind CSS
- **Algorithms**: DWT (Discrete Wavelet Transform) + DCT (Discrete Cosine Transform)

## Prerequisites

### Required Software
- **Rust**: Install from https://rustup.rs/
- **Node.js**: Version 18 or higher
- **System Dependencies**:
  ```bash
  # macOS
  brew install 7zip

  # Ubuntu/Debian
  sudo apt-get install p7zip-full unrar

  # Windows
  # Install 7-Zip and WinRAR manually
  ```

## Installation

1. **Clone the repository**
   ```bash
   git clone <repository-url>
   cd blindmarktool
   ```

2. **Install dependencies**
   ```bash
   npm install
   ```

3. **Run development server**
   ```bash
   npm run tauri dev
   ```

## Development

### Project Structure
```
blindmarktool/
├── src/                    # React frontend
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── commands/       # Tauri commands
│   │   ├── core/           # Core algorithms
│   │   ├── models/         # Data structures
│   │   └── utils/          # Utilities
├── CLAUDE.md               # Development guide for Claude Code
└── README.md               # This file
```

### Build Commands

```bash
# Development mode
npm run tauri dev

# Production build
npm run tauri build

# Run tests
cd src-tauri && cargo test

# Frontend only (without Tauri)
npm run dev
```

## Usage

### Single Text Watermark
1. Open the application
2. Select "Single Text" mode
3. Enter your watermark text
4. Drag and drop a .zip/.7z/.rar file
5. Adjust strength slider (0.1-1.0)
6. Click "Process"
7. Output file will be saved as `filename_watermarked.zip`

### Excel Watermark Mapping
1. Prepare an Excel file (.xlsx) with watermark texts in the first column
2. Select "Excel File" mode
3. Choose your Excel file
4. Drag and drop an archive
5. Files will be watermarked sequentially (Row 1 → File 1, Row 2 → File 2, etc.)

### Watermark Extraction
1. Select "Extract" mode
2. Choose a watermarked image
3. The embedded MD5 hash will be displayed

## How It Works

### Watermarking Algorithm
1. **Encoding**: Input text → MD5 hash → 128-bit binary sequence
2. **Decomposition**: Apply 2-level Haar wavelet transform (DWT) to image
3. **Transform**: Apply DCT to mid-frequency subband in 8x8 blocks
4. **Embedding**: Modify mid-frequency DCT coefficients based on binary sequence
5. **Reconstruction**: Inverse DCT → Inverse DWT → Watermarked image

The watermark is embedded in the frequency domain, making it robust against:
- JPEG compression
- Scaling/resizing
- Noise addition
- Minor image modifications

### Archive Processing
1. Extract archive to temporary disk directory
2. Recursively scan for PNG/JPEG/JPG images
3. Process images in parallel using all CPU cores
4. Repackage into new archive with `_watermarked` suffix
5. Preserve exact directory structure

## Supported Formats

### Archive Formats
- ✅ ZIP (.zip)
- 🔄 7-Zip (.7z) - In development
- 🔄 RAR (.rar) - In development

### Image Formats
- ✅ PNG (.png)
- ✅ JPEG (.jpg, .jpeg)

## Development Status

This project is currently under active development. See [CLAUDE.md](CLAUDE.md) for detailed implementation status and developer guidance.

### Completed ✅
- Project initialization and configuration
- Data models and type definitions
- MD5 encoder/decoder with tests
- Project structure and module organization

### In Progress ⏳
- DWT processor implementation
- DCT processor implementation
- Complete watermark embedding pipeline
- Archive handling (ZIP/7z/RAR)
- Frontend UI components

## Performance

- **Parallel Processing**: Utilizes all CPU cores via Rayon
- **Memory Efficient**: Disk-based temporary workspace for large archives
- **Fast**: Processes typical images (2-5MP) in ~1-2 seconds per image

## Security

- Validates file paths to prevent directory traversal
- Limits archive extraction size to prevent zip bombs
- Sanitizes Excel input
- Uses Tauri's capability system for file system access control

## License

[Add your license here]

## Contributing

Contributions are welcome! Please see [CLAUDE.md](CLAUDE.md) for development guidelines.

## Acknowledgments

- DWT implementation using [omni-wave](https://crates.io/crates/omni-wave)
- DCT implementation using [rustdct](https://crates.io/crates/rustdct)
- Built with [Tauri](https://tauri.app/)
