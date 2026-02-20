# BlindMark Master

> 桌面端批量盲水印工具，支持对压缩包内文件批量嵌入不可见水印

[![Release](https://img.shields.io/github/v/release/Wxl-c137/BlindMarkerMaster)](https://github.com/Wxl-c137/BlindMarkerMaster/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-blue)](#下载)

**官网**：[https://wxl-c137.github.io/BlindMarkerMaster/](https://wxl-c137.github.io/BlindMarkerMaster/)

---

## 功能特性

- **批量处理**：对 `.zip` / `.7z` / `.var` / `.rar` 压缩包内所有目标文件一键嵌入水印
- **多文件类型**：同时支持 JSON / VAJ / VMI 数据文件与 PNG 图片盲水印
- **三种编码**：MD5 哈希（不可逆）、明文、AES-256-GCM 加密
- **Excel 批量**：Excel 按行映射，第 N 行对应第 N 个文件，自动顺序处理
- **水印混淆**：随机字段名并插入既有字段旁，提高隐蔽性
- **多核并行**：基于 Rayon，自动利用全部 CPU 核心
- **高速模式**：大图仅处理左上角 512×512 区域，速度提升 4–10 倍
- **水印提取**：从已处理压缩包中还原水印内容

## 下载

前往 [Releases 页面](https://github.com/Wxl-c137/BlindMarkerMaster/releases) 下载对应平台安装包：

| 文件 | 平台 |
|------|------|
| `*_aarch64.dmg` | macOS Apple Silicon（M1/M2/M3/M4） |
| `*_x64.dmg` | macOS Intel |
| `*_x64-setup.exe` | Windows 64 位（推荐） |
| `*_x64_en-US.msi` | Windows 64 位（MSI） |

> **macOS 首次启动**：若提示「无法验证开发者」，前往 **系统设置 → 隐私与安全性** 点击「仍要打开」。

## 快速上手

### 嵌入水印

1. 切换到 **添加水印** 标签页
2. 拖入或点击选择压缩包（`.zip` / `.7z` / `.var` / `.rar`）
3. 选择水印来源：**固定文本** 或 **Excel 批量**
4. 选择编码方式（默认 MD5）
5. 勾选需要处理的文件类型（JSON / VAJ / VMI / 图片）
6. 点击 **开始添加水印**

### 提取水印

1. 切换到 **提取水印** 标签页
2. 拖入已处理的压缩包
3. 点击 **开始提取**，查看水印内容

### 输出文件结构

输出文件保存在以水印文本命名的子文件夹内，原始文件不受影响：

```
输出目录/
└── <水印文本>/
    └── 原文件名.zip
```

Excel 批量模式下每行水印对应一个独立子文件夹：

```
输出目录/
├── 张三/
│   └── data.zip
├── 李四/
│   └── data.zip
└── 王五/
    └── data.zip
```

## 技术栈

| 层级 | 技术 |
|------|------|
| 后端 | Rust · Tauri 2.0 |
| 前端 | React · TypeScript · Tailwind CSS |
| 图片水印 | DWT（Haar 小波）+ DCT（8×8 块） |
| 数据水印 | 字段注入（MD5 / 明文 / AES-256-GCM） |
| 并行 | Rayon |
| 压缩包 | zip · sevenz-rust · unrar |

## 本地开发

### 环境要求

- Rust（[rustup.rs](https://rustup.rs/)）
- Node.js 18+
- macOS：`brew install 7zip`
- Linux：`sudo apt-get install p7zip-full unrar`
- Windows：安装 7-Zip 和 WinRAR

### 启动开发环境

```bash
git clone https://github.com/Wxl-c137/BlindMarkerMaster.git
cd BlindMarkerMaster
npm install
npm run tauri dev
```

### 构建生产包

```bash
npm run tauri build
```

### 发布新版本

```bash
# 自动更新版本号、打 tag、推送，触发 GitHub Actions 自动构建
./release.sh 0.2.0
```

## 支持格式

### 压缩包
✅ ZIP · ✅ 7Z · ✅ VAR · ✅ RAR

### 数据文件
✅ JSON · ✅ VAJ · ✅ VMI

### 图片
✅ PNG（盲水印）· JPG/JPEG（原样保留，不支持频域水印）

---

*by lulu*
