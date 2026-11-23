#!/usr/bin/env bash
# Sign macOS release binaries with a Developer ID Application identity and,
# when notary credentials are present, submit them to Apple's notary service.
#
# Both targets are bare Mach-O executables, not .app bundles, so the notarization
# ticket cannot be stapled: there is nowhere to attach it. Gatekeeper checks the
# ticket online on first launch, so users need network access once.
set -euo pipefail

: "${MACOS_SIGNING_IDENTITY:?MACOS_SIGNING_IDENTITY must be set to a Developer ID Application identity}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARIES=(
  "$ROOT/target/release/ocr-tool"
  "$ROOT/target/release/ocr-tool-gui"
)

for p in "${BINARIES[@]}"; do
  if [[ ! -f "$p" ]]; then
    echo "Missing binary: $p" >&2
    exit 1
  fi
  echo "Signing $p"
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$p"
  codesign --verify --strict --verbose=2 "$p"
done

if [[ -n "${MACOS_NOTARY_APPLE_ID:-}" && -n "${MACOS_NOTARY_PASSWORD:-}" && -n "${MACOS_NOTARY_TEAM_ID:-}" ]]; then
  workdir="$(mktemp -d)"
  trap 'rm -rf "$workdir"' EXIT

  staging="$workdir/ocr-tool"
  mkdir -p "$staging"
  cp "${BINARIES[@]}" "$staging/"
  zip_path="$workdir/ocr-tool-macos.zip"
  ditto -c -k --keepParent "$staging" "$zip_path"

  echo "Submitting $zip_path to Apple notary service"
  xcrun notarytool submit "$zip_path" \
    --apple-id "$MACOS_NOTARY_APPLE_ID" \
    --password "$MACOS_NOTARY_PASSWORD" \
    --team-id "$MACOS_NOTARY_TEAM_ID" \
    --wait
  # No `xcrun stapler staple`: bare executables cannot hold a stapled ticket.
  echo "Notarization accepted (ticket is not stapled for bare binaries)."
else
  echo "Notary credentials not set; skipping notarization."
fi
