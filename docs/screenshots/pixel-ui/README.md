# Pixel Poetry GUI screenshots

Captured from the Linux build under headless Xvfb (llvmpipe) with the in-app
egui screenshot harness; the host GUI was not started.

- Window: 1180×840 (default); `08-min-1000x720.png` is the minimum size.
- The native Windows decoration is disabled in the app. The top row is the
  custom pixel title bar: brand on the left; `ЛОКАЛЬНО`, version, `?` and the
  `—` / `□` / `×` window controls on the right. Under Xvfb there is no window
  manager, so these shots show the custom bar as the whole window chrome.
- All visible labels are Russian. Brand and technical names stay as-is:
  `OCR TOOL`, `PDF/PNG/JPEG/TIFF/WebP/BMP`, `HTML/JSON/MD`, `DPI`, `PDFium`,
  `llama-server`, `CTRL/ESC` and the raw log.
- Demo data only: `/home/onive/demo/...` paths, fake PDF/PNG files, a sparse
  pinned-size model pair and a stub runtime. No recognition was run.
- `raw` states use `OCR_GUI_DEMO_STATE=done|running|error` (screenshot-only
  fields; no socket, job thread or document access).

Environment used for the ready/result shots:

```text
OCR_MODEL_DIR=/home/onive/demo/model
OCR_LLAMA_SERVER=/home/onive/demo/runtime/llama-vulkan/llama-server
OCR_PDFIUM=/home/onive/demo/runtime/pdfium/bin/libpdfium.so
OCR_GUI_DEMO_INPUTS=invoice.pdf;scan.png;contract.pdf
OCR_GUI_DEMO_OUTPUT=/home/onive/demo/out
OCR_GUI_SHOT=/tmp/shot.png OCR_GUI_SHOT_DELAY_MS=2500
```

Command shape:

```bash
cd /home/onive/demo
xvfb-run -a -s "-screen 0 1400x900x24" \
  env LIBGL_ALWAYS_SOFTWARE=1 ... ./ocr-tool-gui
```
