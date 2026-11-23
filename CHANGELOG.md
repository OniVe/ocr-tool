# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Notes

- The release workflow now publishes a `SHA256SUMS` file and GitHub build
  provenance attestations for every archive (see `docs/verification.md`).
  Authenticode and Developer ID signing remain opt-in; archives built without
  the signing secrets are still unsigned.

## [0.1.0] - 2026-11-22

First public release.

### Added

- CLI (`ocr-tool`) and desktop GUI (`ocr-tool-gui`) for local OCR of PDF and
  PNG/JPEG/TIFF/WebP/BMP images, with Russian document support.
- PDF rendering through PDFium and OCR through Chandra OCR 2 served by a local
  `llama-server` (llama.cpp); recognition works offline after setup.
- Markdown, standalone UTF-8 HTML and layout JSON output, plus optional
  per-block assets (PNG crops, typed HTML/CSV/TEX/TXT files).
- Quality (4096 image tokens) and Fast (2048) profiles, and recognition of two
  PDF pages in parallel.
- Pinned runtime and model downloads with resumable `.part` files and SHA-256
  verification; `doctor`, `runtime` and `models` CLI commands for setup and
  verification.
- GUI setup panel, advanced paths, saved preferences and model preload (on by
  default) to keep the model in memory between jobs.
- Russian pixel-art interface with a frameless custom header (drag,
  double-click maximize/restore, edge resize, `—` / `□` / `×` controls) and a
  1000×720 minimum window size.
- Pixel-art application icons (cream `#F2EFE7` / ink `#111111`) for Windows
  executables, the GUI window and documentation.
- 216 workspace tests.

### Notes

- The `v0.1.0` release archives are not code-signed; Linux/macOS builds are
  covered by CI. Model weights are not bundled and remain under OpenRAIL-M.
- A code-signing pipeline (Windows Authenticode, macOS Developer ID and
  notarization) is available in `.github/workflows/release.yml` and
  `docs/signing.md`. It is enabled by repository secrets; the `v0.1.0` assets
  were built without it and are unsigned.

[Unreleased]: https://github.com/onive/ocr-tool/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/onive/ocr-tool/releases/tag/v0.1.0
