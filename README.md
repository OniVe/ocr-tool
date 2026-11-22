# ocr-tool

[![CI](https://github.com/onive/ocr-tool/actions/workflows/ci.yml/badge.svg)](https://github.com/onive/ocr-tool/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/onive/ocr-tool?label=release)](https://github.com/onive/ocr-tool/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**English** · [Русский](README.ru.md)

Local OCR for PDF and images (PNG/JPEG/TIFF/WebP/BMP), optimized for Russian
documents. The app renders pages with PDFium, sends each page to
[Chandra OCR 2](https://github.com/datalab-to/chandra) served by a local
`llama-server` (llama.cpp), and writes Markdown, standalone UTF-8 HTML and/or
layout JSON.

There is a CLI (`ocr-tool`) and a desktop GUI (`ocr-tool-gui`). Everything runs
offline after the runtime and model are downloaded.

![ocr-tool GUI](docs/screenshots/pixel-ui/02-ready.png)

More screens: [docs/screenshots/pixel-ui](docs/screenshots/pixel-ui/) (setup,
running, result, error, advanced settings, help and the 1000×720 minimum).

> **Not a single executable.** A working installation is:
> `ocr-tool` binary + PDFium library + `llama-server` binary + 3.4 GB model files.
> Model weights are never bundled; this is not a hosted service and no network
> request is made during recognition.

## Status

Version **0.1.0**. Locally built and tested on **Windows 11 x64**, Rust 1.99
MSVC, NVIDIA RTX 3060 12 GB (Vulkan build), llama.cpp **b12535**, PDFium
**chromium/8165**, Chandra OCR 2 **Q4_K_M** + **mmproj Q8_0**. The **200**
workspace tests pass on Windows; Linux and macOS are covered by the CI matrix
(`.github/workflows/ci.yml`) rather than local hardware. Release archives are
**not code-signed** (no Authenticode, notarization or installer).

| Component | Pinned version |
|---|---|
| llama.cpp | b12535 |
| PDFium (pdfium-binaries) | chromium/8165 (PDFium 160.0.8165.0) |
| Model | `mradermacher/chandra-ocr-2-GGUF` Q4_K_M 3.07 GB + mmproj Q8_0 367 MB |
| Rust | 1.82+ (tested with 1.99) |

## Quick start (Windows)

1. Download `ocr-tool-0.1.0-windows-x64.zip` from
   [Releases](https://github.com/onive/ocr-tool/releases).
2. Unpack it anywhere — there is no installer and no admin rights are needed.
3. Run `ocr-tool-gui.exe`.

On first launch the GUI downloads the pinned model (~3.4 GB). The release
archive already contains `runtime/`; the GUI downloads the engine only if it is
missing — for example after a source build or if the folder was deleted
(~70 MB: `llama-server` + PDFium). Recognition then runs locally, with no CLI
and no further network access.

Build from source instead:

```powershell
git clone https://github.com/onive/ocr-tool.git
cd ocr-tool
cargo build --release
.\target\release\ocr-tool-gui.exe
```

The public repository **does not contain test documents**. `samples/SOURCES.md`
records only the provenance of the local fixtures used during development; the
files themselves contain personal data and are never published. Use your own
file, for example `C:\docs\document.pdf` (Windows) or `path/to/document.pdf`
(Linux/macOS).

On the first run the GUI shows one **ПОДГОТОВКА** panel with two independent
rows:

1. **СКАЧАТЬ МОДЕЛЬ (~3,4 ГБ)** — the pinned Chandra Q4_K_M + mmproj pair,
   resumable and SHA-256 verified, into the app-data model folder (or a folder
   chosen with **ПАПКА МОДЕЛИ**).
2. **СКАЧАТЬ ДВИЖОК (~70 МБ)** — llama-vulkan, llama-cpu and PDFium into the
   editable engine folder (defaults to
   `%LOCALAPPDATA%\ocr-tool\runtime\windows-x64`). The sizes are per platform
   (~25 MB on macOS).

Both downloads have progress and cancel; **РАСПОЗНАТЬ** stays disabled while
either is running. To keep everything in one place, choose both folders under
the same parent with **ПАПКА МОДЕЛИ** / **ПАПКА ДВИЖКА**.

On Windows the release GUI is linked as a Windows-subsystem executable:
double-clicking `ocr-tool-gui.exe` opens only the GUI window, with no separate
console. Startup, preflight/download errors and panics (with a backtrace) are
appended to `%LOCALAPPDATA%\ocr-tool\logs\gui.log`
(`<app-data>/ocr-tool/logs/gui.log` on Linux/macOS) and stay visible in the
in-window Log panel; debug builds keep a console for logs. `ocr-tool.exe`
remains a console program. Model/runtime downloads run on background threads,
and `llama-server` is a separate, console-hidden sidecar managed by the GUI
(in-process inference is out of scope).

Both Windows executables carry the same pixel-art app icon
(`assets/ocr-tool.ico`, sizes 16–256), so Explorer, the taskbar and Alt-Tab show
it; the GUI window icon is loaded from `assets/icon-256.png`. The artwork is
cream `#F2EFE7` and ink `#111111`: a document with a scan line. The editable SVG
source is `assets/ocr-tool.svg`; regenerate the ICO/PNG set with
`bun scripts/gen-icon.ts`. Preview:
[docs/screenshots/pixel-ui/icon-preview.png](docs/screenshots/pixel-ui/icon-preview.png).
The PNG is also used for window icons on Linux/macOS; a macOS `.icns` can be
generated in CI from the same source.

CLI parity (for robots/scripts):

```powershell
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
cargo run --release -p ocr-cli -- ocr C:\docs\document.pdf -o out
cargo run --release -p ocr-cli -- ocr C:\docs\contract.pdf -o out --pages 1-2
```

The GUI is self-sufficient: if the model is missing it offers **СКАЧАТЬ МОДЕЛЬ
(~3,4 ГБ)** with progress and cancel plus **ПАПКА МОДЕЛИ**; if the engine is
missing it offers the engine folder plus **СКАЧАТЬ ДВИЖОК**. Both downloads are
resumable and recognise an already present installation, so nothing is fetched
twice. When model files exist but have the wrong size, **ЗАМЕНИТЬ** (with a
confirmation step) deletes them and fetches the pinned pair again; a ready
runtime gets **ПЕРЕУСТАНОВИТЬ**.

Linux/macOS:

```bash
cargo build --release
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
cargo run --release -p ocr-cli -- ocr document.pdf -o out
```

If you already unpacked runtime and model elsewhere, point the app at them:

```powershell
$env:OCR_LLAMA_SERVER = "C:\tools\llama\llama-server.exe"
$env:OCR_MODEL_DIR    = "C:\models\chandra-ocr-2"
$env:OCR_PDFIUM       = "C:\tools\pdfium\pdfium.dll"
```

or use `--llama-dir`, `--model-dir`, `--pdfium` on the CLI / the model dir,
llama dir and PDFium fields under **ДОПОЛНИТЕЛЬНО** in the GUI. The engine
folder in the setup panel (and the picker under **ДОПОЛНИТЕЛЬНО**) fills the
llama directory and the PDFium library from
`runtime/<os>-<arch>/{llama-*,pdfium}`; the same folder is where **СКАЧАТЬ
ДВИЖОК** installs the pinned archives. Model and runtime folders are independent
on purpose — set both to keep everything in one place.

## CLI

```
ocr-tool ocr <inputs...> -o <dir>
    [--format md|html|json|both|all]
    [--extract-blocks none|images|all]
    [--device auto|gpu|cpu]
    [--dpi 192]
    [--pages 1-5,7]
    [--password PASS]
    [--model-dir PATH] [--llama-dir PATH] [--pdfium PATH]
    [--recursive] [--json]
    [--max-pixels N] [--max-tokens N] [--timeout SECONDS]
    [--fast | --image-max-tokens N] [--parallel-pages 1|2]

ocr-tool doctor [--model-dir PATH] [--llama-dir PATH] [--json]
ocr-tool models download [--dir PATH] [--force]
ocr-tool models verify [--dir PATH] [--deep]
ocr-tool runtime download [--dir PATH] [--force]
ocr-tool runtime verify [--dir PATH]
ocr-tool gui
```

- Inputs may be files and/or directories. Directories are scanned one level deep
  unless `--recursive` is given.
- Pages are 1-based; ranges are clamped to the document length. A malformed
  selection (including `0`) is rejected during preflight with exit code `2`.
- `--password` unlocks encrypted PDFs. It is passed only to PDFium and is never
  written to logs, `_meta.json` or the GUI log.
- Progress is written to **stderr**; the final result (or `--json` report) goes to
  **stdout**. A selected-page list shorter than the document prints
  `page 9/11 (selected 1/3)`, not `page 9/3`.
- Exit codes: `0` success, `2` bad arguments/configuration/preflight (missing
  input, model, PDFium; an explicit `--llama-dir` that does not exist is a
  warning and falls back to the discovered runtime), `3` processing/runtime
  failure or cancellation. Partial page/document failures are reported and keep
  the successful outputs, but exit with `3`.
- A page failure does not abort the document; other pages are still recognized.
- `models verify` checks the pinned file sizes without reading the weights;
  `--deep` additionally computes SHA-256 (reads ~3.4 GB). `models download`
  recognises the same renamed pairs as OCR (`chandra-q4.gguf` + `mmproj.gguf`)
  and reuses them instead of downloading again.
- `runtime download` installs the pinned llama.cpp b12535 builds (Vulkan/Metal
  GPU + CPU fallback) and PDFium 8165 into `--dir` (default:
  `%LOCALAPPDATA%\ocr-tool\runtime\<tag>`). Archives are size/SHA-256 verified
  before extraction, `.part` files resume, cancel keeps them, and a component is
  unpacked into a temporary directory and renamed into place. `runtime verify`
  checks an installed folder offline; it exits `2` when a component is missing.
  A failed `runtime download` exits `3`. Both accept `--dir PATH`; the model and
  runtime commands are independent.

### Speed profiles

- **Quality (default):** 4096 image tokens, one page at a time. Best table and
  small-text fidelity. On the RTX 3060 CUDA reference it measured **19.65 s**
  per A4 page.
- **Fast:** `--fast` (same as `--image-max-tokens 2048`). Image prefill drops
  from 4.1 s to 1.7 s and the page from **19.65 s to 16.53 s (−16%)**; the
  invoice spot check kept its key fields. Other documents can differ: on a
  two-page contract the fast output changed one order/date line, so Quality
  stays the default for legal and financial work. Caveat: table labels such as
  `БИК` / `Сч. №` can shift one row down, so verify bank and table data before
  using it for form automation. `--image-max-tokens N` accepts 1–4096 for
  experiments; `--fast` wins when both are given. The token cap is passed to
  llama-server as `LLAMA_ARG_IMAGE_MAX_TOKENS`, which also works with builds
  that lack the command-line flag.
- **Two pages at once:** `--parallel-pages 2` starts `llama-server` with
  `-np 2 -c 20000` and recognizes PDF pages in pairs. On the same CUDA
  reference it measured **13.18 s/page** at 4096 image tokens and
  **10.22 s/page** at 2048 (two pages in 20.4 s at 5.1 GB VRAM): a 1.5–1.9×
  throughput gain for multi-page PDFs. A single page is not faster, because one
  request still occupies one slot; output pages stay in document order. On CPU
  both requests share the same cores, so expect little or no speedup. Images
  (non-PDF inputs) are always recognized one at a time.

Output files are named `<source-stem>.md`, `<source-stem>.html` and
`<source-stem>.json` (only the requested formats), with `-2`, `-3` suffixes on
collision. `--format both` keeps md+html (the previous default); `all` writes
md+html+json; `json` writes only the layout JSON. Windows device names (`CON`,
`NUL`, `COM1`, …) get a trailing `_`. Each document gets `<stem>_meta.json`;
the job merges its documents into `_meta.json` under a file lock, so two jobs
writing to the same `-o` directory keep both sets of entries.
PDF pages are separated by `<!-- page N -->` in Markdown and
`<section data-page="N">` in HTML. HTML is sanitized with ammonia and wrapped
with `<meta charset="utf-8">`, `lang="ru"`.

### JSON, block files and embedding

`<stem>.json` follows Chandra's `_chunks.json` shape (bboxes in page pixels,
block label, inner HTML) and adds a header, stable block ids and asset paths:

```json
{
  "schema": "chandra-json/v1",
  "source": "contract_ru.pdf",
  "generator": "ocr-tool",
  "pages": [{
    "page_num": 1,
    "page_box": [0, 0, 2480, 3508],
    "chunks": [{
      "block_id": "p1-01",
      "bbox": [100, 200, 2400, 500],
      "label": "Text",
      "content": "<p>…</p>",
      "text": "…",
      "assets": {"png": "contract_ru_blocks/page-1-01-text.png",
                 "md": "contract_ru_blocks/page-1-01-text.md"}
    }]
  }]
}
```

- The normalized 0–1000 `data-bbox` is converted to page pixels; `page_box` is
  `[0, 0, width, height]` of the JPEG the model saw (after PDF DPI rendering or
  image downscaling).
- `content` is the sanitized inner HTML of the layout div; nested layout
  attributes are removed. `text` is its plain text (including `img alt`).
- `block_id` is `p<page>-<NN>`; every top-level block gets one.
- If the model returned no layout divs, one `block` chunk covers the whole page.
  `Blank-Page` blocks are skipped.
- `assets` lists only files that were actually created, relative to the JSON.
  Invalid or tiny bboxes keep the JSON chunk but produce no PNG.

`--extract-blocks` controls per-block files (default `none`):

| mode | files |
|---|---|
| `none` | no per-block files; JSON still has chunks/bbox/text when `--format json\|all` |
| `images` | PNG crops for `Image`/`Figure` only (the old `--extract-images`) |
| `all` | PNG for every non-blank block + the typed files below |

`--extract-images` is kept as a deprecated alias for `--extract-blocks images`.

Typed files are written for `all` under `<stem>_blocks/page-<n>-<idx>-<label>.<ext>`:

| label | files |
|---|---|
| `Text`, `Section-Header`, `Caption`, `Footnote`, `List-Group`, `Table-Of-Contents`, `Bibliography`, `Page-Header`, `Page-Footer`, `Complex-Block` | `.md`, `.png` |
| `Table` | `.html` (block HTML), `.csv` (expanded rows), `.png` |
| `Equation-Block` | `.tex` (text from `<math>`), `.png` |
| `Code-Block` | `.txt`, `.png` |
| `Form` | `.html` (inputs/checkboxes), `.png` |
| `Image`, `Figure`, `Diagram` | `.png`, `.txt` (alt/description) |
| `Chemical-Block` | `.html`, `.txt` (SMILES/chem text), `.png` |
| `Blank-Page` | skipped |

CSV uses the first top-level table; `colspan`/`rowspan` are expanded with empty
cells and fields are RFC 4180 quoted. Nested tables are flattened best-effort.
PNG crops smaller than 8×8 px or with invalid bboxes are skipped. A document
stops after 2000 block files with a warning; JSON chunks remain.

When block extraction is enabled, HTML/Markdown are annotated for readability:
- HTML wraps each embedded block in
  `<section class="ocr-block" id="block-p1-01" data-block-id="p1-01" data-label="Text">`
  and adds compact `<a class="ocr-asset">PNG</a>` links; `Image`/`Figure`/`Diagram`
  get a real `<img src="…png" alt="…">`.
- Markdown adds `<!-- block: p1-01 Text -->` and asset links; visual blocks use
  `![alt](…png)`.
- `images` annotates only `Image`/`Figure`; `none` keeps the previous Markdown
  and HTML byte-for-byte.

Stamps, logos and signatures are extracted **only when Chandra labels them
`Image`/`Figure`**. A small stamp may be recognized as `Text`/`Form`, and a
handwritten signature may become text; there is no guarantee. In `all` mode,
whatever label the model chose still gets its PNG and typed file. Extracted
files are listed in `<stem>_meta.json` (`images`, plus `assets` for all files).

## GUI

Build/run:

```powershell
cargo run --release -p ocr-gui
# or target\release\ocr-tool-gui.exe, no arguments
```

The GUI is self-sufficient on the first run; no CLI or environment variable is
required.

- **Russian pixel-art interface.** Every visible label is Russian (brand and
  technical names such as `OCR TOOL`, `PDFium`, `DPI`, `HTML/JSON/MD` stay
  as-is), with cream `#F2EFE7` and ink `#111111` palette and pixel fonts. See
  the [screenshot gallery](docs/screenshots/pixel-ui/).
- **Frameless window with its own header.** Native decoration is disabled; the
  custom pixel header is the title bar. Drag it to move the window, double-click
  to maximize/restore, resize from any edge, and use the `—` / `□` / `×`
  controls. Minimum size is **1000×720** (default 1180×840); the window stays
  resizable, keeps its icon and appears on the taskbar.
- **No console window** — the release binary uses the Windows GUI subsystem, so
  starting it from Explorer creates only the app window. Diagnostics are written
  to `%LOCALAPPDATA%\ocr-tool\logs\gui.log` (Linux/macOS:
  `<app-data>/ocr-tool/logs/gui.log`); a fatal error before the window exists
  also opens a dialog that names the log file. Debug builds keep the console.
- On start, and whenever a path changes, it runs cheap checks (no OCR): the
  model directory is resolved and the two GGUF sizes are compared with the
  pinned download; PDFium is located and `llama-server` candidates are
  enumerated.
- **Model missing or incomplete** — the **ПОДГОТОВКА** panel marks the model
  `НЕТ` or `НЕПОЛНАЯ` and offers **СКАЧАТЬ МОДЕЛЬ (~3,4 ГБ)** (background
  thread, per-file bytes/percent, already-present files skipped, cancel keeps
  the `.part` file for resume), **ПАПКА МОДЕЛИ** (a folder picker, rechecked
  immediately), and **ЗАМЕНИТЬ**. Replace asks for confirmation, deletes the
  existing model files and their `.part` remainders, and downloads the pinned
  pair again; it is the GUI exit from the incomplete/wrong-size state that a
  plain download deliberately refuses to overwrite.
- **Runtime missing** — the same panel shows the editable engine folder
  (placeholder: the platform default), **ПАПКА ДВИЖКА**, and **СКАЧАТЬ ДВИЖОК**
  (~70 MB; ~25 MB on macOS). The download fetches llama-vulkan, llama-cpu and
  PDFium for the current platform, verifies their pinned SHA-256, and installs
  them into the chosen folder; progress, per-component status and cancel are
  shown, and a partial `.part` archive resumes. On success the folder fills
  llama dir (`llama-vulkan` / `llama-cpu`) and PDFium
  (`pdfium/bin|lib/<lib>`) and the status says the engine is ready. In the
  release package the `runtime` folder next to the executable is still
  discovered automatically. A missing runtime never appears as a raw
  `os error 3` in the main status.
- **Independent downloads** — model and engine each have their own button and
  cancel; while either runs, **РАСПОЗНАТЬ** and the other download button are
  disabled. **ПЕРЕУСТАНОВИТЬ** (with confirmation) lives in the engine row once
  the runtime is ready.
- **PDFium source** — the ready status names where the library came from: the
  chosen runtime folder/path, `OCR_PDFIUM`, or auto-discovery next to the app.
  A visible warning is shown when `OCR_PDFIUM` overrides the bundled runtime.
- **Advanced paths** (**ДОПОЛНИТЕЛЬНО**) opens automatically while setup is
  incomplete (model, llama-server or PDFium missing) and keeps optional model
  dir / llama dir / PDFium / engine folder overrides. The engine folder is the
  same field as in the setup panel.
- Recognition: **ДОБАВИТЬ ФАЙЛЫ** (drag and drop works), output folder
  (**ПАПКА СОХРАНЕНИЯ**), format (`MD`, `HTML`, `JSON`, `ОБА`, `ВСЁ`), device,
  DPI, page range, PDF password (masked field), recurse toggle and **Block
  extraction** (off / images only / all blocks). A **СКОРОСТЬ** card selects
  **КАЧЕСТВО** (4096 image tokens) or **БЫСТРО** (2048; ~15% faster, table rows
  may shift) and the **2 pages at once (PDF; GPU)** option. **РАСПОЗНАТЬ** runs
  in a background thread with progress, per-page status, log and cancel.
- **Result actions** — **ОТКРЫТЬ ПАПКУ**, **ОТКРЫТЬ HTML**, **ОТКРЫТЬ MD** and
  **ОТКРЫТЬ JSON**; each file button appears only when that file exists, so
  **ОТКРЫТЬ MD** is absent for an HTML-only or JSON-only job. Cancel kills the
  request and the `llama-server` process tree; closing the window cancels the
  job and waits for the sidecar to stop.
- **Model preload** (`preload_model`, **on by default**) — after setup the GUI
  starts `llama-server` and keeps the model in memory, so the first document
  skips engine startup. It reuses the same server across jobs, shows
  `ЗАГРУЖАЕМ МОДЕЛЬ…` and then `МОДЕЛЬ В ПАМЯТИ`, and reserves roughly **5 GB
  of VRAM/RAM**. Turn it off with **ПРЕДЗАГРУЗКА МОДЕЛИ** in **СКОРОСТЬ** when
  memory is needed elsewhere; the setting is remembered. A failed preload is
  reported but does not block recognition — the next job starts its own engine.
- **Saved preferences** — output folder, format, device, DPI, block extraction,
  fast mode, parallel pages, `model_dir`, `runtime_dir`, `llama_dir`, `pdfium`,
  page range, recurse, `max_pixels`, `max_tokens`, request timeout
  (`timeout_secs`) and `preload_model` are stored in `gui-prefs.json` in the
  app-data folder. Passwords, document names and document contents are never
  saved.
- **Approximate ETA from the start.** Before the first page finishes the GUI
  shows a deliberately rough estimate: ~12 min per selected page plus ~20 s of
  startup for CPU (Fast: ~10 min per page); ~30 s per page plus ~10 s of startup
  for GPU (Fast: ~25 s). With two pages at once the ETA counts chunks of two,
  because throughput rises while one-page latency does not. Auto follows
  runtime discovery: the GPU estimate when a GPU candidate is present, otherwise
  the cautious CPU estimate. After pages complete it switches to the measured
  average. For multi-file jobs the label names the current file only; while the
  page count is still unknown no ETA is shown. This is an estimate, not a
  promise of exact timing.

## How it works

```
input ─┬─ image ──────────────► JPEG (downscale to --max-pixels)
       └─ PDF → PDFium (DPI) ──► JPEG per selected page
                                     │
                         llama-server /v1/chat/completions
                         official Chandra ocr_layout prompt
                         temperature=0, max_tokens=4096
                                     │
                     HTML blocks ──► ammonia ──► .html
                              ├────► htmd ─────► .md
                              └────► layout ───► .json (+ _blocks/*.png|md|…)
```

- One `llama-server` per job, reused for all pages; it is started on a free
  localhost port with a unique `--alias`, health-checked for up to 180 s and
  killed with `taskkill /T /F` and a kill-on-close Windows job object (Unix: a
  process group plus `PR_SET_PDEATHSIG`) on Drop/cancel/crash. `/props` or
  `/v1/models` must confirm the alias or model path before the server is
  accepted, so a process that wins the free-port race is detected. The sidecar
  always gets `-np 1` or `-np 2`; two slots use `-c 20000`, and the vision token
  budget is passed as `LLAMA_ARG_IMAGE_MAX_TOKENS`.
- `--device auto` tries the Vulkan/Metal build first and automatically falls back
  to the CPU build if startup fails. With `--llama-dir` pointing at a parent of
  `llama-vulkan` and `llama-cpu`, both builds are enumerated.
- `--reasoning off` is passed to llama-server; if a build nevertheless puts the
  answer in `message.reasoning_content`, the client reads it.
- Request timeout is 600 s/page by default (`--timeout`).
- `doctor` prints the `llama-server --version` banner (5 s timeout) for every
  candidate and the PDFium DLL file version alongside the component paths.
- Sidecar logs live in `%TEMP%/ocr-tool`; empty logs are removed and only the 20
  newest are kept.

### Search order

`llama-server`:

1. `OCR_LLAMA_SERVER` (file or directory)
2. `--llama-dir` (file, directory, or parent of `llama-vulkan`/`llama-cpu`)
3. `<exe-dir>/runtime/<os>-<arch>/llama-vulkan`, `llama-metal`, `llama-cuda`, `llama-cpu`
4. same under the current directory
5. `PATH`

Model directory:

1. `--model-dir`
2. `OCR_MODEL_DIR`
3. app-data: `%LOCALAPPDATA%\ocr-tool\models\chandra-ocr-2` (Windows),
   `~/.local/share/ocr-tool/models/chandra-ocr-2` (Linux),
   `~/Library/Application Support/ocr-tool/models/chandra-ocr-2` (macOS)

PDFium:

1. `--pdfium` / the GUI runtime folder (library file or directory)
2. `OCR_PDFIUM` (library file or directory)
3. beside the executable
4. `runtime/<os>-<arch>/pdfium/{bin,lib}`, `runtime/<os>/...`
5. current directory

If `OCR_PDFIUM` is set but invalid, the app reports an error instead of silently
loading another library. A missing PDFium is a normal error, not a panic. The
GUI runtime status and `doctor` show which source won (runtime folder/path,
`OCR_PDFIUM`, or auto-discovery); the GUI warns when `OCR_PDFIUM` overrides the
bundled runtime.

## Build, test, package

```bash
cargo fmt --all -- --check
cargo test --workspace   # 200 tests
cargo build --release
```

Binaries: `target/release/ocr-tool` and `target/release/ocr-tool-gui`
(`.exe` on Windows). Prerequisites and platform dependencies are listed in
[CONTRIBUTING.md](CONTRIBUTING.md).

Packaging (model not included):

```powershell
.\scripts\package.ps1
```

```bash
./scripts/package.sh
```

`scripts/fetch-runtime.ps1|.sh` download the pinned llama.cpp and PDFium archives
into `runtime/<target>/` for packaging; GUI and CLI users do not need them
(**СКАЧАТЬ ДВИЖОК** / `runtime download` install the same layout).
`scripts/package.ps1|.sh` copy the executables, `runtime/<target>`,
`README.md`/`README.ru.md`, `LICENSE`, `CHANGELOG.md` and `NOTICE` into `dist/`.
GitHub Actions runs the test/build matrix on Windows,
Ubuntu and macOS and produces the release archives; code signing/notarization
is not configured, so Windows SmartScreen may warn on first launch.

`scripts/acceptance.ps1` is the local acceptance test used during development.
It expects `.tmp/models`, `.tmp/llama`, `.tmp/llama-cpu` and `.tmp/pdfium` and
the sample files; it checks the recognized substrings, PDF pages, CPU fallback,
`doctor`, GUI startup, the PE subsystem of both executables
(`scripts/check-gui-subsystem.ps1`: GUI=2 Windows GUI, CLI=3 console), the GUI
file log, and the offline `runtime --help` / `runtime verify` commands.
`scripts/acceptance-runtime.ps1` performs the real ~69 MB runtime download E2E
(archive SHA-256, `llama-server --version`, `doctor`, CPU OCR).

## Performance

Reference machine: RTX 3060 12 GB, Vulkan b12535, Q4_K_M.

- GPU: roughly **20–50 s per A4 page** (image prompt processing dominates).
  Measured locally with Vulkan: the 2496×3508 invoice page took **25–27 s**; two
  A4 contract pages took **43–48 s** after a ~7 s engine start.
- CUDA reference (WSL2, same GPU/model): **19.65 s/page** in Quality,
  **16.53 s/page** with Fast, **13.18 s/page** with two pages at once (4096
  image tokens) and **10.22 s/page** with Fast + two pages. Two-page mode used
  ~5.1 GB VRAM. CUDA was 4–14% faster than Vulkan on warm jobs, not 2×.
- CPU: approximately **3.3 generated tokens/s**; a normal page can take
  **10–15 minutes**. A 1248×351 crop with 96 output tokens completed in
  **39–45 s** (plus engine startup). Use `--pages`, `--dpi 120`, or a crop for
  quick checks.
- Download: the 367 MB mmproj took ~30 s on the test connection and resumed from
  a 50 MB `.part` file; the 3.07 GB Q4 file follows the same path.
- Peak disk: ~3.4 GB model + ~250 MB runtime + outputs.

CPU mode is a compatibility fallback, not a practical bulk-processing mode on
typical laptops.

## Limitations

- **Not a single exe.** PDFium, llama-server and the model must be present.
- CPU OCR is very slow; there is no quantized CPU-specific optimization.
- PDF text layers are ignored: every selected page is rasterized and OCRed.
  That is slower than a text extractor but handles scans uniformly.
- Tables are preserved as HTML tables and converted to Markdown by `htmd`;
  complex merged cells may not round-trip perfectly into Markdown.
- Images up to twice `--max-pixels` are decoded and downscaled; anything larger
  is rejected before the bitmap is allocated (a 6000×6000 PNG does not
  materialise a 354 MB buffer under a small `--max-pixels`).
- Encrypted PDFs need `--password` (or the GUI’s masked field). PDFium decides
  which encryption schemes are supported.
- Printed/handwritten Russian quality depends on Chandra; stamps, handwriting
  and poor scans can contain errors. Always review financial/legal output.
- `--extract-blocks images` only crops blocks the model labelled `Image`/`Figure`;
  stamps or signatures classified as `Text`/`Form` are not cropped. `all` crops
  whatever label exists, but the PNG is only a rectangle around the model's
  bbox, not a semantic mask.
- Fast mode (2048 image tokens) can move table labels one row down. It is a
  speed profile, not a table-fidelity guarantee; review bank details and totals.
- `--parallel-pages 2` overlaps requests, not PDF rendering, and needs roughly
  5 GB of VRAM. It does not make a single page faster and may not help on CPU.
  Images (non-PDF inputs) are still processed one at a time.
- Linux/macOS are exercised by the CI matrix, not on local hardware in this
  snapshot; only Windows 11 x64 was verified locally.
- The GUI and CLI download both the model and the runtime (llama-server +
  PDFium) themselves; the release package also ships `runtime/` next to the
  executable. Nothing is installed by a hidden script.
- The model is licensed OpenRAIL-M; commercial use is not promised. See NOTICE.
- The public repository contains no OCR test documents. `samples/SOURCES.md`
  documents only the provenance of local fixtures; the files contain personal
  data and are never published (the whole folder except `SOURCES.md` is
  gitignored).

## Privacy

All recognition is local. The only network operations are explicit model
downloads (the GUI **СКАЧАТЬ МОДЕЛЬ** button or `models download`, both to
Hugging Face) and explicit runtime downloads (the GUI **СКАЧАТЬ ДВИЖОК**
button or `runtime download`, both to GitHub), plus the packaging
`fetch-runtime` scripts. No telemetry. Output files can contain sensitive text;
protect them accordingly.

## Development and support

- [CONTRIBUTING.md](CONTRIBUTING.md) — prerequisites, checks, PR rules.
- [SECURITY.md](SECURITY.md) — how to report a vulnerability privately.
- [CHANGELOG.md](CHANGELOG.md) — release history; see also
  [Releases](https://github.com/onive/ocr-tool/releases).

## License

The Rust code in this repository is **MIT** (see [LICENSE](LICENSE)). Bundled
fonts are under the SIL Open Font License (see `assets/fonts/OFL*.txt`). The
Chandra OCR 2 model weights are **OpenRAIL-M**, and llama.cpp/PDFium keep their
own licenses; downloaded components are not part of this repository's source
license. See [NOTICE](NOTICE) for details.
