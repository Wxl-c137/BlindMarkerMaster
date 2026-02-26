#!/usr/bin/env bash
# ──────────────────────────────────────────────
# release.sh  —  一键打版本号并触发 GitHub Release
# 用法：./release.sh <版本号>
#   示例：./release.sh 0.2.0
# ──────────────────────────────────────────────
set -e

VERSION="${1}"

# ── 参数检查 ──────────────────────────────────
if [[ -z "$VERSION" ]]; then
  echo "用法: ./release.sh <版本号>"
  echo "示例: ./release.sh 0.2.0"
  exit 1
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "错误: 版本号格式应为 x.x.x（如 0.2.0）"
  exit 1
fi

TAG="v${VERSION}"

# ── 确认工作区干净 ────────────────────────────
if [[ -n "$(git status --porcelain)" ]]; then
  echo "错误: 工作区有未提交的修改，请先 commit 或 stash"
  git status --short
  exit 1
fi

# ── 检查 tag 是否已存在 ───────────────────────
if git rev-parse "$TAG" &>/dev/null; then
  echo "错误: tag $TAG 已存在"
  exit 1
fi

echo "→ 准备发布 $TAG"

# ── 更新版本号 ────────────────────────────────
echo "  更新 package.json ..."
sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"${VERSION}\"/" package.json

echo "  更新 src-tauri/tauri.conf.json ..."
sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"${VERSION}\"/" src-tauri/tauri.conf.json

echo "  更新 src-tauri/Cargo.toml ..."
# [package] 段的 version 行格式为 `version = "x.y.z"`（独立一行）
# 依赖版本格式为 `crate = { version = "..." }`，不会被匹配
sed -i '' "s/^version = \"[^\"]*\"/version = \"${VERSION}\"/" src-tauri/Cargo.toml

echo "  更新 src/App.tsx ..."
# 落地页显示版本号，格式为 >v0.0.0<
sed -i '' "s/>v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*</>v${VERSION}</" src/App.tsx

# ── Cargo.lock 同步 ───────────────────────────
# (如未被 .gitignore 忽略则更新)
if [[ -f "src-tauri/Cargo.lock" ]] && ! git check-ignore -q src-tauri/Cargo.lock; then
  echo "  更新 Cargo.lock ..."
  (cd src-tauri && cargo update --workspace --quiet 2>/dev/null || true)
fi

# ── 提交 + 打 tag + 推送 ─────────────────────
echo "  提交版本变更 ..."
# 校验三处版本号已对齐
pkg_ver=$(grep '"version"' package.json | head -1 | sed 's/.*"\([0-9.]*\)".*/\1/')
tauri_ver=$(grep '"version"' src-tauri/tauri.conf.json | head -1 | sed 's/.*"\([0-9.]*\)".*/\1/')
cargo_ver=$(grep '^version = ' src-tauri/Cargo.toml | head -1 | sed 's/version = "\([0-9.]*\)".*/\1/')
app_ver=$(grep -o '>v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*<' src/App.tsx | head -1 | sed 's/>v\(.*\)</\1/')
if [[ "$pkg_ver" != "$VERSION" || "$tauri_ver" != "$VERSION" || "$cargo_ver" != "$VERSION" || "$app_ver" != "$VERSION" ]]; then
  echo "错误: 版本号未对齐！"
  echo "  package.json       : $pkg_ver"
  echo "  tauri.conf.json    : $tauri_ver"
  echo "  Cargo.toml         : $cargo_ver"
  echo "  src/App.tsx        : $app_ver"
  echo "  期望               : $VERSION"
  exit 1
fi
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src/App.tsx
git commit -m "chore: bump version to ${VERSION}"

echo "  创建 tag $TAG ..."
git tag -a "$TAG" -m "Release ${TAG}"

echo "  推送到 GitHub ..."
git push origin main
git push origin "$TAG"

echo ""
echo "✓ 已推送 $TAG，GitHub Actions 正在构建中"
echo "  查看进度: https://github.com/Wxl-c137/BlindMarkerMaster/actions"
echo "  发布页面: https://github.com/Wxl-c137/BlindMarkerMaster/releases"
