#!/usr/bin/env bash
# Build and package ocr-tool for the current host as a .tar.gz.
# The GGUF model is deliberately NOT included.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-}"
if [[ -z "$TARGET" ]]; then
  case "$(uname -s)" in
    Linux) TARGET="linux-x64" ;;
    Darwin)
      if [[ "$(uname -m)" == "arm64" ]]; then TARGET="macos-arm64"; else TARGET="macos-x64"; fi
      ;;
    *) echo "Unsupported host; on Windows use scripts/package.ps1" >&2; exit 2 ;;
  esac
fi

VERSION="$(grep -m1 '^version = "' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
RUNTIME="$ROOT/runtime/$TARGET"
if [[ ! -d "$RUNTIME" ]]; then
  echo "Runtime not found: $RUNTIME. Run scripts/fetch-runtime.sh $TARGET first." >&2
  exit 1
fi

cd "$ROOT"
cargo build --release

STAGE="$ROOT/dist/ocr-tool-$VERSION-$TARGET"
DIST="$ROOT/dist"
rm -rf "$STAGE"
mkdir -p "$STAGE/runtime"
install -m 755 target/release/ocr-tool "$STAGE/"
install -m 755 target/release/ocr-tool-gui "$STAGE/"
cp README.md README.ru.md LICENSE CHANGELOG.md NOTICE SECURITY.md CONTRIBUTING.md "$STAGE/"
mkdir -p "$STAGE/docs/screenshots"
cp -R docs/screenshots/pixel-ui "$STAGE/docs/screenshots/"
mkdir -p "$STAGE/samples"
cp samples/SOURCES.md "$STAGE/samples/"
cp -R "$RUNTIME" "$STAGE/runtime/$TARGET"

ARCHIVE="$DIST/ocr-tool-$VERSION-$TARGET.tar.gz"
rm -f "$ARCHIVE"
tar -czf "$ARCHIVE" -C "$STAGE" .
echo "Package: $ARCHIVE"
echo "Contents: executables + runtime/$TARGET + docs; model not included."
