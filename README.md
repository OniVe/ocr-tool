<a id="readme-top"></a>

<!-- Badges -->
[![CI][ci-shield]][ci-url]
[![Release][release-shield]][release-url]
[![License: MIT][license-shield]][license-url]
[![Rust 1.82+][rust-shield]][rust-url]
[![Platform][platform-shield]][platform-url]
[![100% local][offline-shield]][offline-url]

<br />

<div align="center">
  <a href="https://github.com/onive/ocr-tool">
    <img src="assets/icon-256.png" alt="ocr-tool logo" width="96" height="96">
  </a>

  <h3 align="center">ocr-tool</h3>

  <p align="center">
    Local OCR for PDF and images — offline, tuned for Russian documents.
    <br />
    PDFium renders pages, a local llama.cpp server runs Chandra OCR 2, and the result is Markdown, HTML and JSON.
    <br /><br />
    <strong>English</strong> · <a href="README.ru.md">Russian</a>
    <br /><br />
    <a href="#getting-started">Quick Start</a>
    ·
    <a href="docs/screenshots/pixel-ui/">Screenshots</a>
    ·
    <a href="https://github.com/onive/ocr-tool/releases">Download</a>
    ·
    <a href="https://github.com/onive/ocr-tool/issues/new?labels=bug">Report Bug</a>
    ·
    <a href="https://github.com/onive/ocr-tool/issues/new?labels=enhancement">Request Feature</a>
  </p>
</div>

<details>
  <summary>Table of Contents</summary>
  <ol>
    <li>
      <a href="#about-the-project">About The Project</a>
      <ul>
        <li><a href="#built-with">Built With</a></li>
      </ul>
    </li>
    <li>
      <a href="#getting-started">Getting Started</a>
      <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#installation">Installation</a></li>
      </ul>
    </li>
    <li><a href="#usage">Usage</a></li>
    <li><a href="#how-it-works">How It Works</a></li>
    <li><a href="#roadmap">Roadmap</a></li>
    <li><a href="#contributing">Contributing</a></li>
    <li><a href="#license">License</a></li>
    <li><a href="#contact">Contact</a></li>
    <li><a href="#acknowledgments">Acknowledgments</a></li>
  </ol>
</details>

<!-- ABOUT THE PROJECT -->
## About The Project

[![ocr-tool GUI in the ready state][hero-screenshot]](docs/screenshots/pixel-ui/)

`ocr-tool` is a local OCR application for PDF and images
(PNG/JPEG/TIFF/WebP/BMP) with a focus on Russian documents. PDFium renders each
selected page, [Chandra OCR 2](https://github.com/datalab-to/chandra) served by
a local `llama-server` (llama.cpp) recognizes it, and the app writes Markdown,
standalone UTF-8 HTML and/or layout JSON. There is a CLI (`ocr-tool`) and a
desktop GUI (`ocr-tool-gui`). After the runtime and model are downloaded,
everything runs offline.

> **Not a single executable.** A working installation is:
> `ocr-tool` binary + PDFium library + `llama-server` binary + 3.4 GB model files.
> Model weights are never bundled; this is not a hosted service and no network
> request is made during recognition.

### Key features

- **100% local.** Recognition never touches the network; there is no telemetry.
  Only explicit model/runtime downloads use it.
- **CLI and GUI.** The GUI is self-sufficient on first launch; the CLI offers the
  same pipeline for scripts and batch jobs.
- **PDF and images.** PDF, PNG, JPEG, TIFF, WebP and BMP; pages are rendered with
  PDFium at the chosen DPI.
- **Markdown, HTML and JSON.** Standalone UTF-8 HTML (`lang="ru"`, sanitized with
  ammonia), Markdown with page markers, and layout JSON with bboxes and block
  ids.
- **Blocks and assets.** Optional per-block PNG crops and typed files (`.md`,
  `.csv`, `.tex`, `.html`, `.txt`), linked from the document output.
- **Quality and Fast profiles.** 4096 or 2048 image tokens; Quality is the
  default for legal and financial work.
- **Two pages at once.** `--parallel-pages 2` overlaps PDF requests on a GPU for
  a 1.5–1.9× throughput gain on multi-page documents.
- **Model preload.** The GUI keeps `llama-server` and the model in memory
  between jobs (on by default, ~5 GB of VRAM/RAM).
- **Russian pixel-art GUI.** Frameless window, custom header, Russian labels and
  a cream/ink palette.
- **English, Russian and Chinese UI.** The interface language follows the OS
  locale on first launch and can be switched from the header or the Advanced
  window; the chosen language is saved.
- **Saved preferences.** Paths, format, device, DPI, speed options and preload
  are restored on the next launch. Passwords and document contents are never
  saved.

### Status

Version **0.1.0**. Locally built and tested on **Windows 11 x64**, Rust 1.99
MSVC, NVIDIA RTX 3060 12 GB (Vulkan build), llama.cpp **b12535**, PDFium
**chromium/8165**, Chandra OCR 2 **Q4_K_M** + **mmproj Q8_0**. The **216**
workspace tests pass on Windows; Linux and macOS are covered by the CI matrix
(`.github/workflows/ci.yml`) rather than local hardware. Release archives are
**not code-signed** (no Authenticode, notarization or installer).

### Built With

[![Rust][rust-stack-shield]][rust-stack-url]
[![egui/eframe][egui-shield]][egui-url]
[![llama.cpp b12535][llama-shield]][llama-url]
[![PDFium 8165][pdfium-shield]][pdfium-url]
[![Chandra OCR 2][chandra-shield]][chandra-url]

The app is written in Rust with **egui/eframe 0.31**. Inference runs in a pinned
**llama.cpp b12535** sidecar; PDF rendering uses **PDFium chromium/8165**
(PDFium 160.0.8165.0) from pdfium-binaries; the model is **Chandra OCR 2**
(Q4_K_M + mmproj Q8_0).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- GETTING STARTED -->
## Getting Started

### Prerequisites

- **Windows 11 x64, Linux or macOS.** Windows 11 x64 is verified locally; Linux
  and macOS are exercised by the CI matrix.
- **Rust 1.82+** for source builds. Windows needs the MSVC toolchain; Linux
  needs the usual GTK/eframe development packages. Platform packages and checks
  are listed in [CONTRIBUTING.md](CONTRIBUTING.md).
- **A GPU is optional.** A Vulkan/Metal build is used when available, with an
  automatic CPU fallback. CPU OCR is very slow (see
  [Performance](#performance)).
- **Disk:** ~3.4 GB for the model plus ~70 MB for the runtime (~25 MB on macOS);
  peak usage is roughly 3.4 GB model + 250 MB runtime + outputs.
- **No installer and no admin rights** are needed for the release archive.

The model is not part of the repository or the release archive. The GUI and CLI
download it explicitly on request.

### Installation

1. Download the latest release from
   [Releases](https://github.com/onive/ocr-tool/releases):
   `ocr-tool-0.1.0-windows-x64.zip` for Windows, a `.tar.gz` archive for
   Linux/macOS.
2. Unpack it anywhere — there is no installer and no admin rights are needed.
3. Run `ocr-tool-gui.exe` (Windows) or the GUI binary from the unpacked folder.

On first launch the GUI shows one **SETUP** panel with two independent rows:

1. **DOWNLOAD MODEL (~3.4 GB)** — the pinned Chandra Q4_K_M + mmproj pair,
   resumable and SHA-256 verified, into the app-data model folder (or a folder
   chosen with **MODEL FOLDER**).
2. **DOWNLOAD ENGINE (~70 MB)** — llama-vulkan, llama-cpu and PDFium into the
   editable engine folder (defaults to
   `%LOCALAPPDATA%\ocr-tool\runtime\windows-x64`). The sizes are per platform
   (~25 MB on macOS).

The release archive already contains `runtime/`; the GUI downloads the engine
only if it is missing — for example after a source build or if the folder was
deleted. The model row marks its state `MISSING` or `INCOMPLETE`. Both
downloads have progress and cancel; **RECOGNIZE** and the other download
button stay disabled while either is running. Downloads resume from `.part`
files and recognise an already present installation, so nothing is fetched
twice. When model files exist but have the wrong size, **REPLACE** (with a
confirmation step) deletes them, including `.part` remainders, and fetches the
pinned pair again; a ready runtime gets **REINSTALL**. A missing runtime is
shown as a clear setup state, not as a raw `os error 3` in the main status. To
keep everything in one place, choose both folders under the same parent with
**MODEL FOLDER** / **ENGINE FOLDER**.

Recognition then runs locally, with no CLI and no further network access.

#### Build from source

```powershell
git clone https://github.com/onive/ocr-tool.git
cd ocr-tool
cargo build --release
cargo run -p ocr-gui
```

The binaries are `target/release/ocr-tool` and `target/release/ocr-tool-gui`
(`.exe` on Windows). A source build has no `runtime/` folder: download the
engine and the model with the CLI (or the GUI buttons) before the first job.

```powershell
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
```

```bash
# Linux/macOS
cargo build --release
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
cargo run --release -p ocr-cli -- ocr path/to/document.pdf -o out
```

The public repository **does not contain test documents**. `samples/SOURCES.md`
records only the provenance of the local fixtures used during development; the
files themselves contain personal data and are never published. Use your own
file, for example `C:\docs\document.pdf` (Windows) or `path/to/document.pdf`
(Linux/macOS).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- USAGE -->
## Usage

### GUI

Run `ocr-tool-gui.exe` (release) or `cargo run --release -p ocr-gui` from a
source checkout; no CLI or environment variable is required.

- **Add files.** **ADD FILES** opens a file picker, **ADD FOLDER**
  adds a folder, and drag and drop works too. Directories are scanned one level
  deep unless recursion is enabled.
- **Job settings.** Output folder (**OUTPUT FOLDER**), format (`MD`, `HTML`,
  `JSON`, `BOTH`, `ALL`), device, DPI, page range, PDF password (masked field),
  recurse toggle and **Block extraction** (off / images only / all blocks).
- **Speed.** The **SPEED** card selects **QUALITY** (4096 image tokens) or
  **FAST** (2048; ~15% faster, table rows may shift), the **2 pages at once
  (PDF; GPU)** option, and **MODEL PRELOAD**.
- **Preload.** On by default: after setup the GUI starts `llama-server` and
  keeps the model in memory, so the first document skips engine startup. The
  same server is reused across jobs; statuses are `LOADING MODEL…` and
  `MODEL IN MEMORY`, and roughly **5 GB of VRAM/RAM** stays reserved. Turn it
  off when memory is needed elsewhere; a failed preload is reported but does not
  block recognition.
- **Run.** **RECOGNIZE** runs in a background thread with progress, per-page
  status, a log and cancel. Cancel kills the request and the `llama-server`
  process tree; closing the window cancels the job and waits for the sidecar to
  stop.
- **Results.** **OPEN FOLDER**, **OPEN HTML**, **OPEN MD**,
  **OPEN JSON**. Each file button appears only when that file exists, so
  **OPEN MD** is absent for an HTML-only or JSON-only job.
- **Shortcuts.** `Ctrl+O` adds files, `Ctrl+Enter` starts recognition, `Esc`
  cancels.
- **Saved preferences.** Output folder, format, device, DPI, block extraction,
  fast mode, parallel pages, `model_dir`, `runtime_dir`, `llama_dir`, `pdfium`,
  page range, recurse, `max_pixels`, `max_tokens`, request timeout
  (`timeout_secs`) and `preload_model` are stored in `gui-prefs.json` in the
  app-data folder. Passwords, document names and document contents are never
  saved.
- **Cheap preflight.** On start, and whenever a path changes, the GUI resolves
  the model directory, compares the two GGUF sizes with the pinned download,
  locates PDFium and enumerates `llama-server` candidates — without running OCR.
- **Frameless window.** Native decoration is disabled; the custom pixel header
  is the title bar. Drag it to move the window, double-click to
  maximize/restore, resize from any edge, and use the `—` / `□` / `×` controls.
  Minimum size is **1000×720** (default 1180×840); the window stays resizable,
  keeps its icon and appears on the taskbar.
- **No console window.** The Windows release binary uses the GUI subsystem
  (PE subsystem 2; the CLI is 3), so starting it from Explorer creates only the
  app window. Startup, preflight/download errors and panics (with a backtrace)
  go to `%LOCALAPPDATA%\ocr-tool\logs\gui.log`
  (`<app-data>/ocr-tool/logs/gui.log` on Linux/macOS) and stay visible in the
  in-window Log panel; a fatal error before the window exists also opens a
  dialog naming the log file. Debug builds keep a console, and `ocr-tool.exe`
  remains a console program.
- **Approximate ETA from the start.** Before the first page finishes the GUI
  shows a deliberately rough estimate: ~12 min per selected page plus ~20 s of
  startup for CPU (Fast: ~10 min per page); ~30 s per page plus ~10 s of startup
  for GPU (Fast: ~25 s). With two pages at once the ETA counts chunks of two.
  Auto follows runtime discovery: the GPU estimate when a GPU candidate is
  present, otherwise the cautious CPU estimate. After pages complete it switches
  to the measured average. For multi-file jobs the label names the current file
  only; no ETA is shown while the page count is unknown.
- **Pixel-art identity.** Every visible label is Russian (brand and technical
  names such as `OCR TOOL`, `PDFium`, `DPI`, `HTML/JSON/MD` stay as-is), with a
  cream `#F2EFE7` and ink `#111111` palette and pixel fonts. Both Windows
  executables carry the same icon (`assets/ocr-tool.ico`, sizes 16–256); the GUI
  window uses `assets/icon-256.png`. The editable SVG is `assets/ocr-tool.svg`;
  regenerate the ICO/PNG set with `bun scripts/gen-icon.ts`. The PNG is also
  used for window icons on Linux/macOS; a macOS `.icns` can be generated in CI
  from the same source. More screens:
  [docs/screenshots/pixel-ui](docs/screenshots/pixel-ui/).

### CLI

```powershell
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
cargo run --release -p ocr-cli -- ocr C:\docs\document.pdf -o out --format all
```

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
- Progress is written to **stderr**; the final result (or `--json` report) goes
  to **stdout**. A selected-page list shorter than the document prints
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

### Output formats and block files

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

### Speed profiles

- **Quality (default):** 4096 image tokens, one page at a time. Best table and
  small-text fidelity. On the RTX 3060 CUDA reference it measured **19.65 s**
  per A4 page.
- **Fast:** `--fast` (same as `--image-max-tokens 2048`). Image prefill drops
  from 4.1 s to 1.7 s and the page from **19.65 s to 16.53 s (−16%)**; the
  invoice spot check kept its key fields. Other documents can differ: on a
  two-page contract the fast output changed one order/date line, so Quality
  stays the default for legal and financial work. Caveat: Russian bank labels
  (`BIK`, account number) can shift one row down, so verify bank and table data
  before using it for form automation. `--image-max-tokens N` accepts 1–4096
  for experiments; `--fast` wins when both are given. The token cap is passed to
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

### Advanced paths

If runtime and model already exist elsewhere, point the app at them:

```powershell
$env:OCR_LLAMA_SERVER = "C:\tools\llama\llama-server.exe"
$env:OCR_MODEL_DIR    = "C:\models\chandra-ocr-2"
$env:OCR_PDFIUM       = "C:\tools\pdfium\pdfium.dll"
```

or use `--llama-dir`, `--model-dir`, `--pdfium` on the CLI / the model dir,
llama dir and PDFium fields under **ADVANCED** in the GUI. The engine
folder in the setup panel (and the picker under **ADVANCED**) fills the
llama directory and the PDFium library from
`runtime/<os>-<arch>/{llama-vulkan,llama-cpu,pdfium}`; the GUI then uses
`llama-vulkan`/`llama-cpu` and `pdfium/bin|lib/<lib>`. The same folder is where
**DOWNLOAD ENGINE** installs the pinned archives. Model and runtime folders are
independent
on purpose — set both to keep everything in one place. **ADVANCED** opens
automatically while setup is incomplete (model, `llama-server` or PDFium
missing) and keeps the optional overrides. The full lookup order is in
[Search order](#search-order).

### Performance

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

### Limitations

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
- The model is licensed OpenRAIL-M; commercial use is not promised. See
  [NOTICE](NOTICE).
- The public repository contains no OCR test documents. `samples/SOURCES.md`
  documents only the provenance of local fixtures; the files contain personal
  data and are never published (the whole folder except `SOURCES.md` is
  gitignored).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- HOW IT WORKS -->
## How It Works

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
  killed with `taskkill /T /F` and a kill-on-close (`KILL_ON_JOB_CLOSE`) Windows
  job object (Unix: a process group plus `PR_SET_PDEATHSIG`) on
  Drop/cancel/crash, so the sidecar does not survive a `taskkill /F` of the CLI
  or closing the GUI. `/props` or `/v1/models` must confirm the alias or model
  path before the server is accepted, so a process that wins the free-port race
  is detected. The sidecar
  always gets `-np 1` or `-np 2`; two slots use `-c 20000`, and the vision token
  budget is passed as `LLAMA_ARG_IMAGE_MAX_TOKENS`.
- The sidecar is a separate, console-hidden process managed by the GUI
  (in-process inference is out of scope). On Windows, child processes
  (`llama-server`, `taskkill`, `llama-server --version`) are started with
  `CREATE_NO_WINDOW`, so the console-less GUI never flashes a window.
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

### Pinned components

| Component | Pinned version |
|---|---|
| llama.cpp | b12535 |
| PDFium (pdfium-binaries) | chromium/8165 (PDFium 160.0.8165.0) |
| Model | `mradermacher/chandra-ocr-2-GGUF` Q4_K_M 3.07 GB + mmproj Q8_0 367 MB |
| Rust | 1.82+ (tested with 1.99) |

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

### Privacy and network access

All recognition is local. The only network operations are explicit model
downloads (the GUI **DOWNLOAD MODEL** button or `models download`, both to
Hugging Face) and explicit runtime downloads (the GUI **DOWNLOAD ENGINE**
button or `runtime download`, both to GitHub), plus the packaging
`fetch-runtime` scripts. No telemetry. Output files can contain sensitive text;
protect them accordingly.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- ROADMAP -->
## Roadmap

- [x] 0.1.0 — offline CLI + GUI pipeline, MD/HTML/JSON output, block assets,
  Quality/Fast profiles, two pages at once, model preload.
- [x] CI test/build matrix on Windows, Ubuntu and macOS; release archives with
  the pinned runtime.
- [ ] Release workflow hardening: reproducible pinned downloads and artifact
  retention.
- [ ] Code signing (Authenticode) and macOS notarization.
- [ ] Installers (MSI/NSIS on Windows, `.dmg` on macOS).
- [ ] macOS universal binary.
- [ ] More interface languages and documentation.
- [ ] Published benchmarks on a wider hardware set.

Known problems and proposals are tracked in
[GitHub Issues](https://github.com/onive/ocr-tool/issues).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- CONTRIBUTING -->
## Contributing

Bug reports, fixes, documentation and ideas are all welcome. Any contribution is
**greatly appreciated**.

1. Fork the project.
2. Create your feature branch (`git checkout -b feature/AmazingFeature`).
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`).
4. Push to the branch (`git push origin feature/AmazingFeature`).
5. Open a pull request.

Please read [CONTRIBUTING.md](CONTRIBUTING.md) for prerequisites, platform
packages, code style, checks and PR rules. Report security issues privately as
described in [SECURITY.md](SECURITY.md). Release history is in
[CHANGELOG.md](CHANGELOG.md). The project is new, so there is no contributors
wall yet — every PR is listed in the repository history.

### Development checks

```bash
cargo fmt --all -- --check
cargo test --workspace   # 216 tests
cargo build --release
```

Packaging (the model is not included):

```powershell
.\scripts\package.ps1
```

```bash
./scripts/package.sh
```

`scripts/fetch-runtime.ps1|.sh` download the pinned llama.cpp and PDFium
archives into `runtime/<target>/` for packaging; GUI and CLI users do not need
them (**DOWNLOAD ENGINE** / `runtime download` install the same layout).
`scripts/package.ps1|.sh` copy the executables, `runtime/<target>`,
`README.md`/`README.ru.md`, `LICENSE`, `CHANGELOG.md` and `NOTICE` into `dist/`.
GitHub Actions runs the test/build matrix on Windows, Ubuntu and macOS and
produces the release archives; code signing/notarization is not configured, so
Windows SmartScreen may warn on first launch.

`scripts/acceptance.ps1` is the local acceptance test used during development.
It expects `.tmp/models`, `.tmp/llama`, `.tmp/llama-cpu` and `.tmp/pdfium` and
the sample files; it checks the recognized substrings, PDF pages, CPU fallback,
`doctor`, GUI startup, the PE subsystem of both executables
(`scripts/check-gui-subsystem.ps1`: GUI=2 Windows GUI, CLI=3 console), the GUI
file log, and the offline `runtime --help` / `runtime verify` commands.
`scripts/acceptance-runtime.ps1` performs the real ~69 MB runtime download E2E
(archive SHA-256, `llama-server --version`, `doctor`, CPU OCR).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- LICENSE -->
## License

The Rust code in this repository is **MIT** (see [LICENSE](LICENSE)). Bundled
fonts are under the SIL Open Font License (see `assets/fonts/OFL*.txt`). The
Chandra OCR 2 model weights are **OpenRAIL-M**, and llama.cpp/PDFium keep their
own licenses; downloaded components are not part of this repository's source
license. See [NOTICE](NOTICE) for details.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- CONTACT -->
## Contact

Use [GitHub Issues](https://github.com/onive/ocr-tool/issues) for bug reports and
feature requests. For other questions, open a discussion or reach the maintainer
through the GitHub profile [@OniVe](https://github.com/OniVe).

Project Link: [https://github.com/onive/ocr-tool](https://github.com/onive/ocr-tool)

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- ACKNOWLEDGMENTS -->
## Acknowledgments

- [Chandra OCR 2](https://github.com/datalab-to/chandra) — the OCR model and
  layout prompt.
- [llama.cpp](https://github.com/ggml-org/llama.cpp) — the pinned `llama-server`
  inference engine.
- [PDFium](https://pdfium.googlesource.com/pdfium/) and
  [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) — PDF
  rendering.
- [egui/eframe](https://github.com/emilk/egui) — the immediate-mode GUI.
- [Press Start 2P](https://fonts.google.com/specimen/Press+Start+2P) and
  [IBM Plex Mono](https://github.com/IBM/plex) — pixel and monospace fonts.
- [shields.io](https://shields.io) — README badges.
- [Best-README-Template](https://github.com/othneildrew/Best-README-Template) —
  the structure of this document.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- MARKDOWN LINKS & IMAGES -->
[ci-shield]: https://img.shields.io/github/actions/workflow/status/onive/ocr-tool/ci.yml?branch=master&style=for-the-badge
[ci-url]: https://github.com/onive/ocr-tool/actions/workflows/ci.yml
[release-shield]: https://img.shields.io/github/v/release/onive/ocr-tool?style=for-the-badge&label=release
[release-url]: https://github.com/onive/ocr-tool/releases
[license-shield]: https://img.shields.io/badge/license-MIT-blue?style=for-the-badge
[license-url]: LICENSE
[rust-shield]: https://img.shields.io/badge/rust-1.82%2B-orange?style=for-the-badge&logo=rust&logoColor=white
[rust-url]: https://www.rust-lang.org/
[platform-shield]: https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey?style=for-the-badge
[platform-url]: https://github.com/onive/ocr-tool/releases
[offline-shield]: https://img.shields.io/badge/offline-100%25%20local-brightgreen?style=for-the-badge
[offline-url]: #how-it-works
[rust-stack-shield]: https://img.shields.io/badge/Rust-1.82%2B-000000?style=for-the-badge&logo=rust&logoColor=white
[rust-stack-url]: https://www.rust-lang.org/
[egui-shield]: https://img.shields.io/badge/egui%2Feframe-0.31-FF5E00?style=for-the-badge
[egui-url]: https://github.com/emilk/egui
[llama-shield]: https://img.shields.io/badge/llama.cpp-b12535-2C2C2C?style=for-the-badge
[llama-url]: https://github.com/ggml-org/llama.cpp
[pdfium-shield]: https://img.shields.io/badge/PDFium-8165-6A5ACD?style=for-the-badge
[pdfium-url]: https://github.com/bblanchon/pdfium-binaries
[chandra-shield]: https://img.shields.io/badge/Chandra%20OCR-2-0A7E8C?style=for-the-badge
[chandra-url]: https://github.com/datalab-to/chandra
[hero-screenshot]: docs/screenshots/pixel-ui/09-ready-en.png
