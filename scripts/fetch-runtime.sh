#!/usr/bin/env bash
# Download pinned llama.cpp b12535 and PDFium 8165 into runtime/<target>/.
# Usage: scripts/fetch-runtime.sh [linux-x64|macos-arm64|macos-x64]
# The model weights are NOT downloaded here; use `ocr-tool models download`.
set -euo pipefail

LLAMA_VERSION="b12535"
PDFIUM_TAG="chromium/8165"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT/runtime}"

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux) echo "linux-x64" ;;
    Darwin)
      case "$arch" in
        arm64|aarch64) echo "macos-arm64" ;;
        *) echo "macos-x64" ;;
      esac
      ;;
    *) echo "unsupported" ;;
  esac
}

TARGET="${1:-$(detect_target)}"
LLAMA_BASE="https://github.com/ggml-org/llama.cpp/releases/download/$LLAMA_VERSION"
PDFIUM_BASE="https://github.com/bblanchon/pdfium-binaries/releases/download/$PDFIUM_TAG"

case "$TARGET" in
  linux-x64)
    GPU_URL="$LLAMA_BASE/llama-$LLAMA_VERSION-bin-ubuntu-vulkan-x64.tar.gz"
    CPU_URL="$LLAMA_BASE/llama-$LLAMA_VERSION-bin-ubuntu-x64.tar.gz"
    PDFIUM_URL="$PDFIUM_BASE/pdfium-linux-x64.tgz"
    ;;
  macos-arm64)
    GPU_URL="$LLAMA_BASE/llama-$LLAMA_VERSION-bin-macos-arm64.tar.gz"
    CPU_URL="$GPU_URL"
    PDFIUM_URL="$PDFIUM_BASE/pdfium-mac-univ.tgz"
    ;;
  macos-x64)
    GPU_URL="$LLAMA_BASE/llama-$LLAMA_VERSION-bin-macos-x64.tar.gz"
    CPU_URL="$GPU_URL"
    PDFIUM_URL="$PDFIUM_BASE/pdfium-mac-univ.tgz"
    ;;
  *)
    echo "Unsupported target '$TARGET' (use linux-x64, macos-arm64 or macos-x64; Windows: fetch-runtime.ps1)" >&2
    exit 2
    ;;
esac

CACHE="$ROOT/.tmp/runtime-download"
TARGET_DIR="$OUT_DIR/$TARGET"
mkdir -p "$CACHE" "$TARGET_DIR"

download() {
  local url="$1" destination="$2"
  if [[ ! -f "$destination" ]]; then
    echo "  downloading $url"
    curl -fL --retry 3 --retry-delay 2 -o "$destination" "$url"
  fi
}

GPU_ARCHIVE="$CACHE/$(basename "$GPU_URL")"
CPU_ARCHIVE="$CACHE/$(basename "$CPU_URL")"
PDFIUM_ARCHIVE="$CACHE/$(basename "$PDFIUM_URL")"

echo "ocr-tool runtime: $TARGET -> $TARGET_DIR"
download "$GPU_URL" "$GPU_ARCHIVE"
download "$CPU_URL" "$CPU_ARCHIVE"
download "$PDFIUM_URL" "$PDFIUM_ARCHIVE"

mkdir -p "$TARGET_DIR/llama-vulkan" "$TARGET_DIR/llama-cpu" "$TARGET_DIR/pdfium"
echo "  extracting Vulkan/Metal build"
tar -xzf "$GPU_ARCHIVE" -C "$TARGET_DIR/llama-vulkan"
echo "  extracting CPU build"
if [[ "$GPU_URL" == "$CPU_URL" ]]; then
  cp -R "$TARGET_DIR/llama-vulkan/." "$TARGET_DIR/llama-cpu/"
else
  tar -xzf "$CPU_ARCHIVE" -C "$TARGET_DIR/llama-cpu"
fi

for dir in "$TARGET_DIR/llama-vulkan" "$TARGET_DIR/llama-cpu"; do
  if [[ ! -x "$dir/llama-server" ]]; then
    chmod +x "$dir/llama-server" 2>/dev/null || true
  fi
  [[ -x "$dir/llama-server" ]] || { echo "llama-server missing in $dir" >&2; exit 1; }
done

echo "  extracting PDFium"
tar -xzf "$PDFIUM_ARCHIVE" -C "$TARGET_DIR/pdfium"
if [[ ! -f "$TARGET_DIR/pdfium/lib/libpdfium.so" && ! -f "$TARGET_DIR/pdfium/lib/libpdfium.dylib" ]]; then
  echo "PDFium library not found under $TARGET_DIR/pdfium" >&2
  exit 1
fi

echo "Runtime ready: $TARGET_DIR"
