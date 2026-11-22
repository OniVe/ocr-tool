# Pixel Poetry GUI screenshots

Captured from the Linux build under headless Xvfb (llvmpipe) with the in-app
egui screenshot harness; the host GUI was not started.

- Window: 1180×840 (default); `08-min-1000x720.png` and
  `11-min-zh-1000x720.png` are the minimum size.
- `01`–`08` use the Russian interface. `09-ready-en.png` is the ready state in
  English, `10-ready-zh.png` is the ready state in Chinese,
  `11-min-zh-1000x720.png` is the Chinese UI at the minimum size, and
  `12-advanced-zh.png` is the Advanced window in Chinese.
- `icon-preview.png` shows the pixel-art application icon (cream `#F2EFE7`,
  ink `#111111`), not a GUI state.
- The native Windows decoration is disabled in the app. The top row is the
  custom pixel title bar: brand on the left; the language button (`EN` / `RU` /
  `中文`), `LOCAL` badge, version, `?` and the `—` / `□` / `×` window controls on
  the right. Under Xvfb there is no window manager, so these shots show the
  custom bar as the whole window chrome.
- At the 1000×720 minimum, the language button, badge, version, `?` and all
  three window controls remain fully visible. The scrollable left rail can
  clip its last hint line; it scrolls to reach it. The footer stays localized
  and single-line.
- Brand and technical names stay as-is in every language: `OCR TOOL`,
  `PDF/PNG/JPEG/TIFF/WebP/BMP`, `HTML/JSON/MD`, `DPI`, `PDFium`,
  `llama-server`, `CTRL/ESC` and the raw log.
- Demo data only: `$HOME/demo/...` paths, fake PDF/PNG files, a sparse
  pinned-size model pair and a stub runtime. No recognition was run.
- `raw` states use `OCR_GUI_DEMO_STATE=done|running|error` (screenshot-only
  fields; no socket, job thread or document access).

Environment used for the ready/result shots:

```text
OCR_MODEL_DIR=$HOME/demo/model
OCR_LLAMA_SERVER=$HOME/demo/runtime/llama-vulkan/llama-server
OCR_PDFIUM=$HOME/demo/runtime/pdfium/bin/libpdfium.so
OCR_GUI_DEMO_INPUTS=invoice.pdf;scan.png;contract.pdf
OCR_GUI_DEMO_OUTPUT=$HOME/demo/out
OCR_GUI_SHOT=/tmp/shot.png OCR_GUI_SHOT_DELAY_MS=2500
OCR_GUI_LANG=en|ru|zh
```

For the minimum-size shot, the app is started on a private Xvfb display and
resized with `xdotool windowsize <id> 1000 720` before the in-app capture.

Command shape:

```bash
cd $HOME/demo
xvfb-run -a -s "-screen 0 1400x900x24" \
  env LIBGL_ALWAYS_SOFTWARE=1 OCR_GUI_LANG=zh ... ./ocr-tool-gui
```
