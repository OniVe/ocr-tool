use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use ocr_core::html::escape_html;
use ocr_core::{
    clean_model_html, discover_model_paths, model_dir_source, now_rfc3339, page_to_markdown,
    parse_layout_blocks, parse_page_selection, render_html_document, render_markdown_document,
    resolve_model_dir, sanitize_label, stem_of, table_to_csv, validate_page_spec, BlockExtraction,
    ChandraAssets, ChandraChunk, ChandraDocument, ChandraPage, DocumentMeta, JobMeta, ModelPaths,
    OutputAllocator, OutputFormat, ProgressEvent, MODEL_ENV,
};
use ocr_engine::{inspect_candidates, EngineConfig, OcrServer};
use ocr_pdf::{PdfRenderer, RenderOptions, DEFAULT_DPI};
use serde::Serialize;
use walkdir::WalkDir;

use crate::device_name;
use crate::download::check_model_sizes;

#[derive(Debug, Clone)]
pub struct JobOptions {
    pub inputs: Vec<PathBuf>,
    pub output_dir: PathBuf,
    pub format: OutputFormat,
    pub device: ocr_core::Device,
    pub dpi: u32,
    pub pages: Option<String>,
    pub password: Option<String>,
    pub recursive: bool,
    pub model_dir: Option<PathBuf>,
    pub llama_dir: Option<PathBuf>,
    /// PDFium library file or a directory containing it. Overrides the ambient
    /// search order (`OCR_PDFIUM`, next to the executable, `runtime/<tag>`).
    pub pdfium: Option<PathBuf>,
    pub max_pixels: u64,
    pub max_tokens: u32,
    pub request_timeout_secs: u64,
    /// Write per-block crops and typed files (`none`/`images`/`all`).
    pub extract_blocks: BlockExtraction,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            output_dir: PathBuf::from("ocr-output"),
            format: OutputFormat::Both,
            device: ocr_core::Device::Auto,
            dpi: DEFAULT_DPI,
            pages: None,
            password: None,
            recursive: false,
            model_dir: None,
            llama_dir: None,
            pdfium: None,
            max_pixels: ocr_pdf::DEFAULT_MAX_PIXELS,
            max_tokens: ocr_engine::DEFAULT_MAX_TOKENS,
            request_timeout_secs: 600,
            extract_blocks: BlockExtraction::None,
        }
    }
}

/// One recognized page plus the exact JPEG the model saw.
///
/// The JPEG is kept until the document is written so image blocks can be
/// cropped with the same pixels the bboxes refer to.
struct PageOutput {
    page_number: Option<u32>,
    html: String,
    jpeg: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone)]
pub struct PreparedJob {
    pub files: Vec<PathBuf>,
    pub output_dir: PathBuf,
    pub model_dir: PathBuf,
    pub model_source: &'static str,
    pub models: ModelPaths,
    pub pdfium: Option<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobReport {
    pub output_dir: String,
    pub documents_ok: usize,
    pub documents_failed: usize,
    pub pages_ok: usize,
    pub pages_failed: usize,
    pub cancelled: bool,
    pub elapsed_ms: u64,
    pub device: String,
    pub model: String,
    pub llama_server: String,
    pub outputs: Vec<String>,
    pub documents: Vec<DocumentMeta>,
}

/// Expand inputs, validate configuration and locate external components.
/// Errors here are user/config errors (CLI exit code 2), not processing errors.
pub fn preflight(options: &JobOptions) -> Result<PreparedJob> {
    if options.inputs.is_empty() {
        bail!("no input files or directories given");
    }
    if options.dpi == 0 || options.dpi > 1200 {
        bail!("invalid --dpi {} (expected 1..=1200)", options.dpi);
    }
    if options.max_tokens == 0 {
        bail!("invalid --max-tokens 0");
    }
    if options.max_pixels == 0 {
        bail!("invalid --max-pixels 0");
    }
    if let Some(spec) = options.pages.as_deref() {
        // Catch `--pages 0` and other syntax errors before the job starts, so
        // the CLI reports a configuration error (exit code 2), not a runtime
        // failure (exit code 3).
        validate_page_spec(spec).context("invalid --pages")?;
    }

    let mut files = Vec::new();
    let mut unsupported = Vec::new();
    for input in &options.inputs {
        if input.is_file() {
            if is_supported(input) {
                files.push(input.clone());
            } else {
                unsupported.push(input.display().to_string());
            }
        } else if input.is_dir() {
            let walker = if options.recursive {
                WalkDir::new(input)
            } else {
                WalkDir::new(input).max_depth(1)
            };
            for entry in walker.follow_links(false) {
                let entry = entry.with_context(|| format!("cannot walk {}", input.display()))?;
                if entry.file_type().is_file() && is_supported(entry.path()) {
                    files.push(entry.path().to_path_buf());
                }
            }
        } else {
            bail!("input does not exist: {}", input.display());
        }
    }

    if !unsupported.is_empty() {
        bail!(
            "unsupported input file(s): {}\nSupported: png, jpg/jpeg, tif/tiff, webp, bmp, pdf",
            unsupported.join(", ")
        );
    }

    files.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    let mut seen = HashSet::new();
    files.retain(|path| {
        let key = path
            .canonicalize()
            .unwrap_or_else(|_| path.clone())
            .to_string_lossy()
            .to_ascii_lowercase();
        seen.insert(key)
    });

    if files.is_empty() {
        bail!("no supported files found in the given inputs");
    }

    std::fs::create_dir_all(&options.output_dir).with_context(|| {
        format!(
            "cannot create output directory {}",
            options.output_dir.display()
        )
    })?;

    let env_model_dir = std::env::var_os(MODEL_ENV);
    let model_dir = resolve_model_dir(options.model_dir.as_deref(), env_model_dir.as_deref());
    let models = discover_model_paths(&model_dir).with_context(|| {
        format!(
            "Model files not found in {}; use Download model or choose a folder",
            model_dir.display()
        )
    })?;
    // Fast size check: catches a truncated/placeholder model without hashing it.
    check_model_sizes(&models).with_context(|| {
        format!(
            "Model files in {} are incomplete; use Download model or choose a folder",
            model_dir.display()
        )
    })?;

    let needs_pdfium = files.iter().any(|p| is_pdf(p));
    let pdfium = if needs_pdfium {
        Some(locate_pdfium_for(options)?)
    } else {
        None
    };

    let mut engine_config = EngineConfig::new(&model_dir);
    engine_config.device = options.device;
    engine_config.llama_dir = options.llama_dir.clone();
    let (candidates, searched, warnings) = inspect_candidates(&engine_config);
    if candidates.is_empty() {
        bail!(
            "llama-server executable not found (device {}). Set OCR_LLAMA_SERVER, pass \
             --llama-dir, or install runtime/<platform>/llama-vulkan (and llama-cpu).\nSearched: {}",
            options.device,
            if searched.is_empty() {
                "none".to_string()
            } else {
                searched.join(", ")
            }
        );
    }

    Ok(PreparedJob {
        files,
        output_dir: options.output_dir.clone(),
        model_dir,
        model_source: model_dir_source(options.model_dir.as_deref(), env_model_dir.as_deref()),
        models,
        pdfium,
        warnings,
    })
}

/// Resolve PDFium from `JobOptions.pdfium` (the GUI runtime folder or CLI
/// `--pdfium`) or from the ambient search order, with an actionable error.
fn locate_pdfium_for(options: &JobOptions) -> Result<PathBuf> {
    let located = match options.pdfium.as_deref() {
        Some(path) => ocr_pdf::locate_pdfium_at(path),
        None => ocr_pdf::locate_pdfium(),
    };
    located.context("PDFium not found; choose the runtime folder or set OCR_PDFIUM")
}

/// Run one job: one llama-server for all pages, sequential page recognition.
pub fn run_job(
    options: &JobOptions,
    cancel: &AtomicBool,
    events: &mut dyn FnMut(ProgressEvent),
) -> Result<JobReport> {
    let started = Instant::now();
    let prepared = preflight(options)?;
    events(ProgressEvent::JobStarted {
        files: prepared.files.len(),
        output_dir: prepared.output_dir.display().to_string(),
    });
    for warning in &prepared.warnings {
        events(ProgressEvent::Log {
            level: ocr_core::LogLevel::Warning,
            message: warning.clone(),
        });
    }
    events(ProgressEvent::Log {
        level: ocr_core::LogLevel::Info,
        message: format!(
            "model: {} ({}), mmproj: {}",
            prepared.models.model.display(),
            prepared.model_source,
            prepared.models.mmproj.display()
        ),
    });
    if let Some(pdfium) = &prepared.pdfium {
        events(ProgressEvent::Log {
            level: ocr_core::LogLevel::Info,
            message: format!("pdfium: {}", pdfium.display()),
        });
    }

    if cancel.load(Ordering::Relaxed) {
        return Ok(empty_cancelled_report(&prepared, started));
    }

    let mut engine_config = EngineConfig::new(&prepared.model_dir);
    engine_config.device = options.device;
    engine_config.max_tokens = options.max_tokens;
    engine_config.request_timeout = Duration::from_secs(options.request_timeout_secs.max(1));
    engine_config.llama_dir = options.llama_dir.clone();

    let server = {
        let mut engine_log = |message: &str| {
            events(ProgressEvent::Log {
                level: ocr_core::LogLevel::Info,
                message: message.to_string(),
            });
        };
        OcrServer::start(&engine_config, cancel, &mut engine_log)?
    };

    events(ProgressEvent::Log {
        level: ocr_core::LogLevel::Info,
        message: format!(
            "engine: {} ({})",
            server.exe_path().display(),
            device_name(server.device_used())
        ),
    });
    if options.device == ocr_core::Device::Auto && server.device_used() == ocr_core::Device::Cpu {
        events(ProgressEvent::Log {
            level: ocr_core::LogLevel::Warning,
            message: "GPU llama-server unavailable; continuing on CPU (slow)".to_string(),
        });
    }

    let renderer = if prepared.files.iter().any(|p| is_pdf(p)) {
        let library = prepared.pdfium.as_deref().ok_or_else(|| {
            anyhow!("internal error: PDFium path was not resolved during preflight")
        })?;
        Some(PdfRenderer::new_at(library)?)
    } else {
        None
    };
    let mut allocator = OutputAllocator::new(&prepared.output_dir);
    let mut documents = Vec::new();
    let mut outputs = Vec::new();
    let mut cancelled = false;

    for (index, file) in prepared.files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let source = file.display().to_string();
        events(ProgressEvent::FileStarted {
            index: index + 1,
            total: prepared.files.len(),
            path: source.clone(),
        });

        match process_document(
            file,
            &prepared,
            options,
            &server,
            renderer.as_ref(),
            &mut allocator,
            cancel,
            events,
        ) {
            Ok(document) => {
                outputs.extend(document.outputs.clone());
                events(ProgressEvent::FileFinished {
                    path: source,
                    outputs: document.outputs.clone(),
                    pages_ok: document.pages_ok,
                    pages_failed: document.pages_failed,
                    elapsed: Duration::from_millis(document.elapsed_ms),
                });
                if document.errors.iter().any(|e| e == "cancelled") {
                    cancelled = true;
                }
                documents.push(document);
                if cancelled {
                    break;
                }
            }
            Err(error) => {
                events(ProgressEvent::FileFailed {
                    path: source,
                    error: format!("{error:#}"),
                });
                documents.push(DocumentMeta {
                    source: file.display().to_string(),
                    outputs: Vec::new(),
                    pages_total: 0,
                    pages_ok: 0,
                    pages_failed: 0,
                    errors: vec![format!("{error:#}")],
                    elapsed_ms: 0,
                    device: device_name(server.device_used()).to_string(),
                    model: prepared.models.model.display().to_string(),
                    mmproj: prepared.models.mmproj.display().to_string(),
                    llama_server: server.exe_path().display().to_string(),
                    dpi: None,
                    format: options.format.to_string(),
                    images: Vec::new(),
                    assets: Vec::new(),
                    generated_at: now_rfc3339(),
                });
            }
        }
    }

    let documents_ok = documents
        .iter()
        .filter(|d| d.pages_ok > 0 && d.errors.is_empty())
        .count();
    let documents_failed = documents.len() - documents_ok;
    let pages_ok: usize = documents.iter().map(|d| d.pages_ok).sum();
    let pages_failed: usize = documents.iter().map(|d| d.pages_failed).sum();

    let elapsed = started.elapsed();
    let job_meta = JobMeta {
        tool: "ocr-tool".to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: now_rfc3339(),
        output_dir: prepared.output_dir.display().to_string(),
        device_requested: options.device.to_string(),
        device_used: device_name(server.device_used()).to_string(),
        model_dir: prepared.model_dir.display().to_string(),
        model: prepared.models.model.display().to_string(),
        mmproj: prepared.models.mmproj.display().to_string(),
        llama_server: server.exe_path().display().to_string(),
        documents: documents.clone(),
    };
    if let Err(error) = write_job_meta(&prepared.output_dir, &job_meta) {
        events(ProgressEvent::Log {
            level: ocr_core::LogLevel::Warning,
            message: format!("cannot write _meta.json: {error:#}"),
        });
    }

    events(ProgressEvent::JobFinished {
        documents_ok,
        documents_failed,
        elapsed,
        cancelled,
    });

    Ok(JobReport {
        output_dir: prepared.output_dir.display().to_string(),
        documents_ok,
        documents_failed,
        pages_ok,
        pages_failed,
        cancelled,
        elapsed_ms: elapsed.as_millis() as u64,
        device: device_name(server.device_used()).to_string(),
        model: prepared.models.model.display().to_string(),
        llama_server: server.exe_path().display().to_string(),
        outputs,
        documents,
    })
}

fn empty_cancelled_report(prepared: &PreparedJob, started: Instant) -> JobReport {
    JobReport {
        output_dir: prepared.output_dir.display().to_string(),
        documents_ok: 0,
        documents_failed: 0,
        pages_ok: 0,
        pages_failed: 0,
        cancelled: true,
        elapsed_ms: started.elapsed().as_millis() as u64,
        device: "not started".to_string(),
        model: prepared.models.model.display().to_string(),
        llama_server: String::new(),
        outputs: Vec::new(),
        documents: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn process_document(
    file: &Path,
    prepared: &PreparedJob,
    options: &JobOptions,
    server: &OcrServer,
    renderer: Option<&PdfRenderer>,
    allocator: &mut OutputAllocator,
    cancel: &AtomicBool,
    events: &mut dyn FnMut(ProgressEvent),
) -> Result<DocumentMeta> {
    let started = Instant::now();
    let path_text = file.display().to_string();
    let mut pages: Vec<PageOutput> = Vec::new();
    let mut errors = Vec::new();
    let mut pages_ok = 0usize;
    let mut pages_failed = 0usize;
    let mut pages_selected = 1usize;
    let mut cancelled = false;

    if is_pdf(file) {
        let renderer =
            renderer.ok_or_else(|| anyhow!("internal error: PDF renderer not initialized"))?;
        let total_pages = renderer
            .page_count(file, options.password.as_deref())
            .with_context(|| format!("cannot read PDF page count for {path_text}"))?;
        let indices = parse_page_selection(options.pages.as_deref(), total_pages)
            .with_context(|| format!("invalid --pages for {path_text}"))?;
        if indices.is_empty() {
            bail!("page selection is empty for {path_text}");
        }
        pages_selected = indices.len();

        events(ProgressEvent::Log {
            level: ocr_core::LogLevel::Info,
            message: format!(
                "rendering {} page(s) from {path_text} at {} DPI",
                indices.len(),
                options.dpi
            ),
        });
        let render_options = render_options(options);
        let outcome = renderer
            .render_pages(file, &indices, &render_options, cancel)
            .with_context(|| format!("cannot render PDF {path_text}"))?;
        for (index, error) in &outcome.errors {
            pages_failed += 1;
            let message = format!("{error:#}");
            errors.push(format!("page {}: {message}", index + 1));
            events(ProgressEvent::PageFailed {
                path: path_text.clone(),
                page: index + 1,
                error: message,
            });
        }
        cancelled = outcome.cancelled;

        let total_selected = indices.len() as u32;
        for (selected_offset, page) in outcome.pages.into_iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            let page_number = page.index + 1;
            events(ProgressEvent::PageStarted {
                path: path_text.clone(),
                page: page_number,
                total_pages: total_pages as u32,
                selected_index: selected_offset as u32 + 1,
                selected_total: total_selected,
            });
            let page_started = Instant::now();
            match recognize_cancellable(server, &page.jpeg, options.max_tokens, cancel) {
                Ok(answer) => {
                    let clean = clean_model_html(&answer);
                    if clean.is_empty() {
                        pages_failed += 1;
                        let message = "model returned an empty page".to_string();
                        errors.push(format!("page {page_number}: {message}"));
                        events(ProgressEvent::PageFailed {
                            path: path_text.clone(),
                            page: page_number,
                            error: message,
                        });
                    } else {
                        pages_ok += 1;
                        events(ProgressEvent::PageFinished {
                            path: path_text.clone(),
                            page: page_number,
                            chars: clean.chars().count(),
                            elapsed: page_started.elapsed(),
                        });
                        pages.push(PageOutput {
                            page_number: Some(page_number),
                            html: clean,
                            jpeg: page.jpeg,
                            width: page.width,
                            height: page.height,
                        });
                    }
                }
                Err(error) => {
                    if cancel.load(Ordering::Relaxed) {
                        cancelled = true;
                        break;
                    }
                    pages_failed += 1;
                    let message = format!("{error:#}");
                    errors.push(format!("page {page_number}: {message}"));
                    events(ProgressEvent::PageFailed {
                        path: path_text.clone(),
                        page: page_number,
                        error: message,
                    });
                }
            }
        }
    } else {
        events(ProgressEvent::PageStarted {
            path: path_text.clone(),
            page: 1,
            total_pages: 1,
            selected_index: 1,
            selected_total: 1,
        });
        let page_started = Instant::now();
        let (jpeg, width, height) = prepare_image(file, options.max_pixels)
            .with_context(|| format!("cannot read image {path_text}"))?;
        match recognize_cancellable(server, &jpeg, options.max_tokens, cancel) {
            Ok(answer) => {
                let clean = clean_model_html(&answer);
                if clean.is_empty() {
                    pages_failed += 1;
                    errors.push("page 1: model returned an empty page".to_string());
                } else {
                    pages_ok += 1;
                    events(ProgressEvent::PageFinished {
                        path: path_text.clone(),
                        page: 1,
                        chars: clean.chars().count(),
                        elapsed: page_started.elapsed(),
                    });
                    pages.push(PageOutput {
                        page_number: None,
                        html: clean,
                        jpeg,
                        width,
                        height,
                    });
                }
            }
            Err(error) => {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                } else {
                    pages_failed += 1;
                    let message = format!("{error:#}");
                    errors.push(format!("page 1: {message}"));
                    events(ProgressEvent::PageFailed {
                        path: path_text.clone(),
                        page: 1,
                        error: message,
                    });
                }
            }
        }
    }

    if cancelled {
        errors.push("cancelled".to_string());
    }

    let mut produced_outputs = Vec::new();
    if pages_ok > 0 {
        let stem = allocator.allocate(&stem_of(file));
        let title = stem.clone();
        let legacy_pages: Vec<(Option<u32>, String)> = pages
            .iter()
            .map(|page| (page.page_number, page.html.clone()))
            .collect();

        let mut extracted_assets = Vec::new();
        let needs_output = options.format.writes_json() || options.extract_blocks.writes_files();
        let output = if needs_output {
            let source_name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(path_text.as_str());
            let output = build_document_output(
                source_name,
                &stem,
                &prepared.output_dir,
                &pages,
                options.extract_blocks,
            );
            extracted_assets = output.files.clone();
            for warning in &output.warnings {
                events(ProgressEvent::Log {
                    level: ocr_core::LogLevel::Warning,
                    message: format!("{path_text}: {warning}"),
                });
            }
            Some(output)
        } else {
            None
        };

        if options.format.writes_markdown() {
            let markdown = match &output {
                Some(output) if options.extract_blocks.writes_files() => {
                    render_markdown_with_blocks(&title, &output.pages, options.extract_blocks)
                }
                _ => render_markdown_document(&title, &legacy_pages),
            };
            let path = prepared.output_dir.join(format!("{stem}.md"));
            std::fs::write(&path, markdown.as_bytes())
                .with_context(|| format!("cannot write {}", path.display()))?;
            produced_outputs.push(path.display().to_string());
        }
        if options.format.writes_html() {
            let html = match &output {
                Some(output) if options.extract_blocks.writes_files() => {
                    render_html_with_blocks(&title, &output.pages, options.extract_blocks)
                }
                _ => render_html_document(&title, &legacy_pages),
            };
            let path = prepared.output_dir.join(format!("{stem}.html"));
            std::fs::write(&path, html.as_bytes())
                .with_context(|| format!("cannot write {}", path.display()))?;
            produced_outputs.push(path.display().to_string());
        }
        if options.format.writes_json() {
            if let Some(output) = &output {
                let path = prepared.output_dir.join(format!("{stem}.json"));
                write_json(&path, &output.json)
                    .with_context(|| format!("cannot write {}", path.display()))?;
                produced_outputs.push(path.display().to_string());
            }
        }

        let meta = DocumentMeta {
            source: path_text.clone(),
            outputs: produced_outputs.clone(),
            pages_total: pages_selected,
            pages_ok,
            pages_failed,
            errors: errors.clone(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            device: device_name(server.device_used()).to_string(),
            model: prepared.models.model.display().to_string(),
            mmproj: prepared.models.mmproj.display().to_string(),
            llama_server: server.exe_path().display().to_string(),
            dpi: if is_pdf(file) {
                Some(options.dpi)
            } else {
                None
            },
            format: options.format.to_string(),
            images: extracted_assets
                .iter()
                .filter(|path| path.to_ascii_lowercase().ends_with(".png"))
                .cloned()
                .collect(),
            assets: extracted_assets,
            generated_at: now_rfc3339(),
        };
        let meta_path = prepared.output_dir.join(format!("{stem}_meta.json"));
        write_json(&meta_path, &meta)
            .with_context(|| format!("cannot write {}", meta_path.display()))?;

        if pages_failed > 0 {
            events(ProgressEvent::Log {
                level: ocr_core::LogLevel::Warning,
                message: format!(
                    "{path_text}: {pages_ok} page(s) recognized, {pages_failed} failed"
                ),
            });
        }
        return Ok(meta);
    }

    // No page succeeded: still report every error, but write no empty outputs.
    Ok(DocumentMeta {
        source: path_text,
        outputs: Vec::new(),
        pages_total: pages_selected,
        pages_ok,
        pages_failed,
        errors,
        elapsed_ms: started.elapsed().as_millis() as u64,
        device: device_name(server.device_used()).to_string(),
        model: prepared.models.model.display().to_string(),
        mmproj: prepared.models.mmproj.display().to_string(),
        llama_server: server.exe_path().display().to_string(),
        dpi: if is_pdf(file) {
            Some(options.dpi)
        } else {
            None
        },
        format: options.format.to_string(),
        images: Vec::new(),
        assets: Vec::new(),
        generated_at: now_rfc3339(),
    })
}

/// The PDF password must reach PDFium through `RenderOptions`; it is not stored
/// anywhere else (no logs, no reports).
fn render_options(options: &JobOptions) -> RenderOptions {
    RenderOptions {
        dpi: options.dpi,
        max_pixels: options.max_pixels,
        password: options.password.clone(),
    }
}

/// Run one blocking request while polling `cancel`. On cancellation the server
/// process tree is killed, which immediately aborts the HTTP request.
fn recognize_cancellable(
    server: &OcrServer,
    jpeg: &[u8],
    max_tokens: u32,
    cancel: &AtomicBool,
) -> Result<String> {
    std::thread::scope(|scope| {
        let handle = scope.spawn(|| server.recognize_jpeg(jpeg, max_tokens));
        while !handle.is_finished() {
            if cancel.load(Ordering::Relaxed) {
                server.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        match handle.join() {
            Ok(result) => result,
            Err(_) => Err(anyhow!("llama-server request thread panicked")),
        }
    })
}

/// Images up to this multiple of `--max-pixels` are decoded and downscaled;
/// anything larger is rejected before the bitmap is allocated.
const DECODE_AREA_FACTOR: u64 = 2;

/// Minimum crop edge; smaller blocks are usually noise.
const MIN_IMAGE_SIZE: u32 = 8;
/// Hard cap on files written for one document to avoid runaway output.
const MAX_BLOCK_FILES: usize = 2000;

#[derive(Debug, Clone)]
struct OutputBlock {
    block_id: String,
    label: String,
    content: String,
    text: String,
    assets: ChandraAssets,
}

#[derive(Debug, Clone)]
struct OutputPage {
    page_number: Option<u32>,
    blocks: Vec<OutputBlock>,
}

struct DocumentOutput {
    json: ChandraDocument,
    pages: Vec<OutputPage>,
    files: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    Png,
    Md,
    Html,
    Csv,
    Tex,
    Txt,
}

fn is_image_label(label: &str) -> bool {
    label.eq_ignore_ascii_case("image") || label.eq_ignore_ascii_case("figure")
}

fn is_visual_label(label: &str) -> bool {
    is_image_label(label) || label.eq_ignore_ascii_case("diagram")
}

/// Typed files written for a label in `--extract-blocks all` mode. PNG is
/// listed for completeness; the actual crop is created separately.
fn asset_kinds(label: &str) -> &'static [AssetKind] {
    const TEXT: &[AssetKind] = &[AssetKind::Png, AssetKind::Md];
    const TABLE: &[AssetKind] = &[AssetKind::Png, AssetKind::Html, AssetKind::Csv];
    const EQUATION: &[AssetKind] = &[AssetKind::Png, AssetKind::Tex];
    const CODE: &[AssetKind] = &[AssetKind::Png, AssetKind::Txt];
    const FORM: &[AssetKind] = &[AssetKind::Png, AssetKind::Html];
    const IMAGE: &[AssetKind] = &[AssetKind::Png, AssetKind::Txt];
    const CHEMICAL: &[AssetKind] = &[AssetKind::Png, AssetKind::Html, AssetKind::Txt];

    if label.eq_ignore_ascii_case("blank-page") {
        return &[];
    }
    if label.eq_ignore_ascii_case("table") {
        return TABLE;
    }
    if label.eq_ignore_ascii_case("equation-block") {
        return EQUATION;
    }
    if label.eq_ignore_ascii_case("code-block") {
        return CODE;
    }
    if label.eq_ignore_ascii_case("form") {
        return FORM;
    }
    if label.eq_ignore_ascii_case("image")
        || label.eq_ignore_ascii_case("figure")
        || label.eq_ignore_ascii_case("diagram")
    {
        return IMAGE;
    }
    if label.eq_ignore_ascii_case("chemical-block") {
        return CHEMICAL;
    }
    // Text, Section-Header, Caption, Footnote, List-Group, Table-Of-Contents,
    // Bibliography, Page-Header, Page-Footer, Complex-Block and the plain
    // `block` fallback.
    TEXT
}

#[derive(Default)]
struct AssetState {
    files: Vec<String>,
    warnings: Vec<String>,
    limit_warned: bool,
}

fn can_write_asset(state: &mut AssetState, page: u32) -> bool {
    if state.files.len() < MAX_BLOCK_FILES {
        return true;
    }
    if !state.limit_warned {
        state.limit_warned = true;
        state.warnings.push(format!(
            "page {page}: stopped after {MAX_BLOCK_FILES} block files; remaining blocks stay in JSON"
        ));
    }
    false
}

fn write_text_asset(
    output_dir: &Path,
    stem: &str,
    file_name: &str,
    bytes: &[u8],
    state: &mut AssetState,
    page: u32,
) -> Option<String> {
    if !can_write_asset(state, page) {
        return None;
    }
    let relative = format!("{stem}_blocks/{file_name}");
    let path = output_dir.join(&relative);
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            state
                .warnings
                .push(format!("cannot create {}: {error}", parent.display()));
            return None;
        }
    }
    match std::fs::write(&path, bytes) {
        Ok(()) => {
            state.files.push(path.display().to_string());
            Some(relative)
        }
        Err(error) => {
            state
                .warnings
                .push(format!("cannot write {}: {error}", path.display()));
            None
        }
    }
}

fn write_png_asset(
    output_dir: &Path,
    stem: &str,
    file_name: &str,
    image: &image::RgbImage,
    state: &mut AssetState,
    page: u32,
) -> Option<String> {
    if !can_write_asset(state, page) {
        return None;
    }
    let relative = format!("{stem}_blocks/{file_name}");
    let path = output_dir.join(&relative);
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            state
                .warnings
                .push(format!("cannot create {}: {error}", parent.display()));
            return None;
        }
    }
    match image.save(&path) {
        Ok(()) => {
            state.files.push(path.display().to_string());
            Some(relative)
        }
        Err(error) => {
            state
                .warnings
                .push(format!("cannot write {}: {error}", path.display()));
            None
        }
    }
}

/// Build JSON pages and, when requested, crop/typed files for every block.
/// Layout parsing and file creation are best-effort: a failed crop or write is
/// a warning, never a failed document.
fn build_document_output(
    source_name: &str,
    stem: &str,
    output_dir: &Path,
    pages: &[PageOutput],
    mode: BlockExtraction,
) -> DocumentOutput {
    let mut state = AssetState::default();
    let mut json_pages = Vec::with_capacity(pages.len());
    let mut output_pages = Vec::with_capacity(pages.len());

    for page in pages {
        let page_num = page.page_number.unwrap_or(1);
        let blocks = parse_layout_blocks(&page.html, page.width, page.height);
        let needs_crop = mode == BlockExtraction::All
            || (mode == BlockExtraction::Images
                && blocks.iter().any(|block| is_image_label(&block.label)));
        let decoded = if needs_crop {
            match image::load_from_memory(&page.jpeg) {
                Ok(decoded) => Some(decoded),
                Err(error) => {
                    state.warnings.push(format!(
                        "page {page_num}: cannot decode the page JPEG for crops: {error}"
                    ));
                    None
                }
            }
        } else {
            None
        };

        let mut output_blocks = Vec::with_capacity(blocks.len());
        let mut chunks = Vec::with_capacity(blocks.len());
        for (index, block) in blocks.into_iter().enumerate() {
            let block_id = format!("p{page_num}-{:02}", index + 1);
            let base = format!(
                "page-{page_num}-{:02}-{}",
                index + 1,
                sanitize_label(&block.label)
            );
            let mut assets = ChandraAssets::default();

            let wants_png = mode == BlockExtraction::All
                || (mode == BlockExtraction::Images && is_image_label(&block.label));
            if wants_png {
                if let Some(decoded) = decoded.as_ref() {
                    match crop_bbox(decoded, block.bbox) {
                        Some(crop) => {
                            assets.png = write_png_asset(
                                output_dir,
                                stem,
                                &format!("{base}.png"),
                                &crop,
                                &mut state,
                                page_num,
                            );
                        }
                        None => state.warnings.push(format!(
                            "page {page_num}: skipped {label} block with invalid or tiny bbox {bbox:?}",
                            label = block.label,
                            bbox = block.bbox
                        )),
                    }
                }
            }

            if mode == BlockExtraction::All {
                for kind in asset_kinds(&block.label) {
                    match kind {
                        AssetKind::Png => {}
                        AssetKind::Md => {
                            let markdown = page_to_markdown(&block.content);
                            if !markdown.trim().is_empty() {
                                assets.md = write_text_asset(
                                    output_dir,
                                    stem,
                                    &format!("{base}.md"),
                                    markdown.as_bytes(),
                                    &mut state,
                                    page_num,
                                );
                            }
                        }
                        AssetKind::Html => {
                            if !block.content.trim().is_empty() {
                                assets.html = write_text_asset(
                                    output_dir,
                                    stem,
                                    &format!("{base}.html"),
                                    block.content.as_bytes(),
                                    &mut state,
                                    page_num,
                                );
                            }
                        }
                        AssetKind::Csv => {
                            if let Some(csv) = table_to_csv(&block.content) {
                                assets.csv = write_text_asset(
                                    output_dir,
                                    stem,
                                    &format!("{base}.csv"),
                                    csv.as_bytes(),
                                    &mut state,
                                    page_num,
                                );
                            }
                        }
                        AssetKind::Tex => {
                            let tex = extract_math_text(&block.content)
                                .unwrap_or_else(|| block.text.trim().to_string());
                            if !tex.is_empty() {
                                assets.tex = write_text_asset(
                                    output_dir,
                                    stem,
                                    &format!("{base}.tex"),
                                    tex.as_bytes(),
                                    &mut state,
                                    page_num,
                                );
                            }
                        }
                        AssetKind::Txt => {
                            let text = block.text.trim();
                            if !text.is_empty() {
                                assets.txt = write_text_asset(
                                    output_dir,
                                    stem,
                                    &format!("{base}.txt"),
                                    text.as_bytes(),
                                    &mut state,
                                    page_num,
                                );
                            }
                        }
                    }
                }
            }

            let image = if is_image_label(&block.label) {
                assets.png.clone()
            } else {
                None
            };
            chunks.push(ChandraChunk {
                block_id: block_id.clone(),
                bbox: block.bbox,
                label: block.label.clone(),
                content: block.content.clone(),
                text: block.text.clone(),
                assets: assets.clone(),
                image,
            });
            output_blocks.push(OutputBlock {
                block_id,
                label: block.label,
                content: block.content,
                text: block.text,
                assets,
            });
        }

        json_pages.push(ChandraPage {
            page_num,
            page_box: [0, 0, page.width, page.height],
            chunks,
        });
        output_pages.push(OutputPage {
            page_number: page.page_number,
            blocks: output_blocks,
        });
    }

    DocumentOutput {
        json: ChandraDocument::new(source_name, json_pages),
        pages: output_pages,
        files: state.files,
        warnings: state.warnings,
    }
}

/// Text inside the first/all `<math>` elements, used for `.tex` output.
fn extract_math_text(content: &str) -> Option<String> {
    let lower = content.to_ascii_lowercase();
    let mut index = 0;
    let mut parts = Vec::new();
    while let Some(relative) = lower[index..].find("<math") {
        let start = index + relative;
        let Some(open_end) = content[start..].find('>') else {
            break;
        };
        let inner_start = start + open_end + 1;
        let Some(close_relative) = lower[inner_start..].find("</math") else {
            break;
        };
        parts.push(ocr_core::html_to_text(
            &content[inner_start..inner_start + close_relative],
        ));
        index = inner_start + close_relative + 6;
    }
    let text = parts.join(" ");
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn block_should_embed(mode: BlockExtraction, label: &str) -> bool {
    match mode {
        BlockExtraction::None => false,
        BlockExtraction::Images => is_image_label(label),
        BlockExtraction::All => true,
    }
}

/// Render the final HTML with `<section class="ocr-block">` wrappers and asset
/// links. The page is still passed through the normal ammonia sanitizer.
fn render_html_with_blocks(title: &str, pages: &[OutputPage], mode: BlockExtraction) -> String {
    let page_html: Vec<(Option<u32>, String)> = pages
        .iter()
        .map(|page| {
            let mut html = String::new();
            for block in &page.blocks {
                html.push_str(&render_block_html(block, mode));
            }
            (page.page_number, html)
        })
        .collect();
    render_html_document(title, &page_html)
}

fn render_block_html(block: &OutputBlock, mode: BlockExtraction) -> String {
    if !block_should_embed(mode, &block.label) {
        return block.content.clone();
    }

    let mut out = format!(
        "<section class=\"ocr-block\" id=\"block-{id}\" data-block-id=\"{id}\" data-label=\"{label}\">",
        id = escape_html(&block.block_id),
        label = escape_html(&block.label)
    );
    let visual = is_visual_label(&block.label);
    let has_image = visual && block.assets.png.is_some();
    if has_image {
        let png = block.assets.png.as_deref().unwrap_or_default();
        let alt = block_alt(block);
        out.push_str(&format!(
            "<figure><img src=\"{}\" alt=\"{}\"/>",
            asset_url(png),
            escape_html(&alt)
        ));
    } else {
        out.push_str(&block.content);
    }

    let links = asset_links_html(&block.assets, if has_image { Some("png") } else { None });
    if !links.is_empty() {
        if has_image {
            out.push_str("<figcaption class=\"ocr-assets\">");
        } else {
            out.push_str("<div class=\"ocr-assets\">");
        }
        out.push_str(&links.join(" "));
        if has_image {
            out.push_str("</figcaption>");
        } else {
            out.push_str("</div>");
        }
    }
    if has_image {
        out.push_str("</figure>");
    }
    out.push_str("</section>");
    out
}

/// Render Markdown with `<!-- block: … -->` markers and compact asset links.
fn render_markdown_with_blocks(title: &str, pages: &[OutputPage], mode: BlockExtraction) -> String {
    let mut out = format!("# {}\n\n", title.trim());
    for page in pages {
        if let Some(number) = page.page_number {
            out.push_str(&format!("<!-- page {number} -->\n\n"));
        }
        for block in &page.blocks {
            let embed = block_should_embed(mode, &block.label);
            if embed {
                out.push_str(&format!(
                    "<!-- block: {} {} -->\n\n",
                    block.block_id, block.label
                ));
            }
            let visual = is_visual_label(&block.label);
            let has_image = visual && embed && block.assets.png.is_some();
            if has_image {
                let png = block.assets.png.as_deref().unwrap_or_default();
                out.push_str(&format!(
                    "![{}]({})\n",
                    markdown_alt(&block_alt(block)),
                    asset_url(png)
                ));
            } else {
                let markdown = page_to_markdown(&block.content);
                if !markdown.is_empty() {
                    out.push_str(&markdown);
                    out.push('\n');
                }
            }
            if embed {
                let links =
                    asset_links_markdown(&block.assets, if has_image { Some("png") } else { None });
                if !links.is_empty() {
                    out.push_str(&links.join(" · "));
                    out.push('\n');
                }
            }
            out.push('\n');
        }
        out.push('\n');
    }
    let mut trimmed = out.trim_end().to_string();
    trimmed.push('\n');
    trimmed
}

fn block_alt(block: &OutputBlock) -> String {
    let text = block.text.trim();
    if text.is_empty() {
        block.label.clone()
    } else {
        text.to_string()
    }
}

fn markdown_alt(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '[' | ']' | '\\') || ch.is_control() {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    let compact = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        "изображение".to_string()
    } else {
        compact
    }
}

fn asset_label(extension: &str) -> &'static str {
    match extension {
        "png" => "PNG",
        "md" => "MD",
        "html" => "HTML",
        "csv" => "CSV",
        "tex" => "TeX",
        "txt" => "TXT",
        _ => "FILE",
    }
}

fn asset_links_html(assets: &ChandraAssets, skip: Option<&str>) -> Vec<String> {
    let mut links = Vec::new();
    for (extension, path) in [
        ("png", &assets.png),
        ("md", &assets.md),
        ("html", &assets.html),
        ("csv", &assets.csv),
        ("tex", &assets.tex),
        ("txt", &assets.txt),
    ] {
        if skip == Some(extension) {
            continue;
        }
        if let Some(path) = path {
            links.push(format!(
                "<a class=\"ocr-asset\" href=\"{}\">{}</a>",
                asset_url(path),
                asset_label(extension)
            ));
        }
    }
    links
}

fn asset_links_markdown(assets: &ChandraAssets, skip: Option<&str>) -> Vec<String> {
    let mut links = Vec::new();
    for (extension, path) in [
        ("png", &assets.png),
        ("md", &assets.md),
        ("html", &assets.html),
        ("csv", &assets.csv),
        ("tex", &assets.tex),
        ("txt", &assets.txt),
    ] {
        if skip == Some(extension) {
            continue;
        }
        if let Some(path) = path {
            links.push(format!("[{}]({})", asset_label(extension), asset_url(path)));
        }
    }
    links
}

/// Percent-encode a relative file path for HTML/Markdown links, keeping `/`
/// and the RFC 3986 unreserved characters.
fn asset_url(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Crop a page-pixel bbox, clamping it to the image and rejecting degenerate
/// or tiny areas. The returned image is RGB (white background for transparency).
fn crop_bbox(decoded: &image::DynamicImage, bbox: [u32; 4]) -> Option<image::RgbImage> {
    let width = decoded.width();
    let height = decoded.height();
    let x0 = bbox[0].min(width);
    let y0 = bbox[1].min(height);
    let x1 = bbox[2].min(width);
    let y1 = bbox[3].min(height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let crop_width = x1 - x0;
    let crop_height = y1 - y0;
    if crop_width < MIN_IMAGE_SIZE || crop_height < MIN_IMAGE_SIZE {
        return None;
    }
    Some(
        image::DynamicImage::ImageRgba8(
            image::imageops::crop_imm(decoded, x0, y0, crop_width, crop_height).to_image(),
        )
        .to_rgb8(),
    )
}

fn prepare_image(path: &Path, max_pixels: u64) -> Result<(Vec<u8>, u32, u32)> {
    // Read only the header first: a 6000x6000 PNG must not allocate its full
    // RGBA bitmap just to be shrunk afterwards.
    let (source_width, source_height) = image::ImageReader::open(path)
        .with_context(|| format!("cannot open {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("cannot detect format of {}", path.display()))?
        .into_dimensions()
        .with_context(|| format!("cannot read image dimensions of {}", path.display()))?;
    let source_pixels = source_width as u64 * source_height as u64;
    let decode_limit = max_pixels.saturating_mul(DECODE_AREA_FACTOR);
    if source_pixels > decode_limit {
        bail!(
            "{} is {source_width}x{source_height} ({source_pixels} pixels), more than the \
             {decode_limit} pixels allowed for --max-pixels {max_pixels}; downscale it \
             first or raise --max-pixels",
            path.display()
        );
    }

    let mut limits = image::Limits::default();
    // 2x the allowed area at 8 bytes/pixel (16-bit RGBA) bounds the decoder.
    limits.max_alloc = Some(decode_limit.saturating_mul(8).max(1));
    limits.max_image_width = Some(decode_limit.min(u32::MAX as u64) as u32);
    limits.max_image_height = Some(decode_limit.min(u32::MAX as u64) as u32);
    let mut reader = image::ImageReader::open(path)
        .with_context(|| format!("cannot open {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("cannot detect format of {}", path.display()))?;
    reader.limits(limits);
    let decoded = reader
        .decode()
        .with_context(|| format!("cannot decode {}", path.display()))?;

    let mut image = image::DynamicImage::ImageRgb8(flatten_on_white(decoded));
    let pixels = image.width() as u64 * image.height() as u64;
    if pixels > max_pixels {
        let factor = (max_pixels as f64 / pixels as f64).sqrt();
        let width = ((image.width() as f64 * factor).round() as u32).max(1);
        let height = ((image.height() as f64 * factor).round() as u32).max(1);
        image = image::DynamicImage::ImageRgb8(
            image
                .resize(width, height, image::imageops::FilterType::Triangle)
                .to_rgb8(),
        );
    }
    let final_pixels = image.width() as u64 * image.height() as u64;
    if final_pixels > max_pixels {
        bail!(
            "internal error: {} stayed at {final_pixels} pixels after downscaling to \
             --max-pixels {max_pixels}",
            path.display()
        );
    }

    let mut jpeg = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg, 92);
    encoder
        .encode_image(&image)
        .with_context(|| format!("cannot encode {} as JPEG", path.display()))?;
    Ok((jpeg, image.width(), image.height()))
}

fn flatten_on_white(image: image::DynamicImage) -> image::RgbImage {
    let rgba = image.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = pixel[3] as u16;
        let blend =
            |channel: u8| -> u8 { ((channel as u16 * alpha + 255 * (255 - alpha)) / 255) as u8 };
        rgb.put_pixel(
            x,
            y,
            image::Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]),
        );
    }
    rgb
}

pub fn is_supported(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "tif" | "tiff" | "webp" | "bmp" | "pdf"
    )
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

/// Merge this job's documents into `_meta.json` under a cooperative file lock,
/// so two jobs writing to the same output directory keep both sets of
/// documents instead of the last writer winning.
fn write_job_meta(dir: &Path, meta: &JobMeta) -> Result<()> {
    let _lock = MetaLock::acquire(dir)?;
    let path = dir.join("_meta.json");
    let mut merged = meta.clone();
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(existing) = serde_json::from_slice::<JobMeta>(&bytes) {
            merged.documents = merge_documents(existing.documents, meta.documents.clone());
        }
    }
    let temp = dir.join(format!(".ocr-meta-{}.tmp", std::process::id()));
    std::fs::write(&temp, serde_json::to_vec_pretty(&merged)?)
        .with_context(|| format!("cannot write {}", temp.display()))?;
    if path.exists() {
        // std::fs::rename does not replace an existing file on Windows.
        let _ = std::fs::remove_file(&path);
    }
    std::fs::rename(&temp, &path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

fn merge_documents(mut existing: Vec<DocumentMeta>, new: Vec<DocumentMeta>) -> Vec<DocumentMeta> {
    for document in new {
        if let Some(slot) = existing
            .iter_mut()
            .find(|entry| entry.source == document.source)
        {
            *slot = document;
        } else {
            existing.push(document);
        }
    }
    existing
}

/// Cross-platform cooperative lock via an exclusive-create file. Concurrent
/// ocr-tool jobs serialise their `_meta.json` update; a lock older than 60 s is
/// treated as left behind by a crashed process.
struct MetaLock {
    path: PathBuf,
    _file: Option<std::fs::File>,
}

impl MetaLock {
    fn acquire(dir: &Path) -> Result<Self> {
        let path = dir.join(".ocr-meta.lock");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        _file: Some(file),
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if let Ok(modified) = metadata.modified() {
                            if modified
                                .elapsed()
                                .map(|age| age > Duration::from_secs(60))
                                .unwrap_or(false)
                            {
                                let _ = std::fs::remove_file(&path);
                                continue;
                            }
                        }
                    }
                    if Instant::now() >= deadline {
                        bail!(
                            "timed out waiting for the _meta.json lock {}",
                            path.display()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => {
                    return Err(error).with_context(|| format!("cannot create {}", path.display()))
                }
            }
        }
    }
}

impl Drop for MetaLock {
    fn drop(&mut self) {
        self._file.take();
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions() {
        for name in [
            "a.png", "a.JPG", "a.jpeg", "a.tif", "a.tiff", "a.webp", "a.bmp", "a.PDF",
        ] {
            assert!(is_supported(Path::new(name)), "{name}");
        }
        for name in ["a.txt", "a.docx", "a.gif", "noext"] {
            assert!(!is_supported(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn preflight_reports_missing_input() {
        let options = JobOptions {
            inputs: vec![PathBuf::from("definitely-missing-input.pdf")],
            ..JobOptions::default()
        };
        assert!(preflight(&options).is_err());
    }

    #[test]
    fn image_is_converted_to_jpeg_and_downscaled() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.png");
        let image = image::RgbImage::from_pixel(1000, 500, image::Rgb([255, 0, 0]));
        image.save(&path).unwrap();
        let (jpeg, width, height) = prepare_image(&path, 300_000).unwrap();
        assert_eq!(&jpeg[..2], &[0xff, 0xd8]);
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert!(decoded.width() as u64 * decoded.height() as u64 <= 300_000);
        assert_eq!((decoded.width(), decoded.height()), (width, height));
    }

    #[test]
    fn crops_image_blocks_and_builds_chunks_json() {
        let tmp = tempfile::tempdir().unwrap();
        let mut image = image::RgbImage::from_pixel(100, 100, image::Rgb([240, 240, 240]));
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            if (10..50).contains(&x) && (10..50).contains(&y) {
                *pixel = image::Rgb([40, 80, 200]);
            }
        }
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 92)
            .encode_image(&image)
            .unwrap();

        let html = r#"<div data-bbox="100 100 500 500" data-label="Image"><img alt="печать"/></div>
            <div data-bbox="0 0 1000 1000" data-label="Text"><p>Счёт № 1</p></div>"#;
        let page = PageOutput {
            page_number: Some(1),
            html: html.to_string(),
            jpeg,
            width: 100,
            height: 100,
        };
        let output = build_document_output(
            "scan.jpg",
            "scan",
            tmp.path(),
            &[page],
            BlockExtraction::All,
        );
        assert!(output.warnings.is_empty(), "{:?}", output.warnings);
        assert_eq!(output.json.pages[0].page_box, [0, 0, 100, 100]);
        assert_eq!(output.json.pages[0].chunks[0].block_id, "p1-01");
        assert_eq!(output.json.pages[0].chunks[0].bbox, [10, 10, 50, 50]);
        assert_eq!(output.json.pages[0].chunks[0].label, "Image");
        assert_eq!(
            output.json.pages[0].chunks[0].image.as_deref(),
            Some("scan_blocks/page-1-01-image.png")
        );
        assert_eq!(
            output.json.pages[0].chunks[0].assets.png.as_deref(),
            Some("scan_blocks/page-1-01-image.png")
        );
        assert_eq!(
            output.json.pages[0].chunks[0].assets.txt.as_deref(),
            Some("scan_blocks/page-1-01-image.txt")
        );
        assert_eq!(output.json.pages[0].chunks[1].text, "Счёт № 1");
        assert_eq!(
            output.json.pages[0].chunks[1].assets.md.as_deref(),
            Some("scan_blocks/page-1-02-text.md")
        );
        assert!(output.json.pages[0].chunks[1].image.is_none());

        let crop = image::open(tmp.path().join("scan_blocks/page-1-01-image.png")).unwrap();
        assert_eq!((crop.width(), crop.height()), (40, 40));
        assert!(tmp.path().join("scan_blocks/page-1-02-text.png").is_file());
        assert_eq!(output.files.len(), 4, "{:?}", output.files);
    }

    #[test]
    fn invalid_or_tiny_image_bbox_is_skipped_but_block_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let image = image::RgbImage::from_pixel(100, 100, image::Rgb([255, 255, 255]));
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 92)
            .encode_image(&image)
            .unwrap();
        let html = r#"<div data-bbox="500 500 100 100" data-label="Figure"></div>
            <div data-bbox="0 0 10 10" data-label="Image"></div>"#;
        let page = PageOutput {
            page_number: None,
            html: html.to_string(),
            jpeg,
            width: 100,
            height: 100,
        };
        let output = build_document_output(
            "scan.jpg",
            "scan",
            tmp.path(),
            &[page],
            BlockExtraction::All,
        );
        assert!(output.files.is_empty(), "{:?}", output.files);
        assert_eq!(output.warnings.len(), 2, "{:?}", output.warnings);
        assert_eq!(output.json.pages[0].chunks.len(), 2);
        assert!(output.json.pages[0]
            .chunks
            .iter()
            .all(|chunk| chunk.assets.png.is_none()));
        assert_eq!(output.json.pages[0].page_num, 1);
    }

    fn synthetic_jpeg(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([250, 250, 250]));
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 92)
            .encode_image(&image)
            .unwrap();
        jpeg
    }

    fn all_label_fixtures() -> Vec<(&'static str, &'static str)> {
        vec![
            ("Caption", "<p>Подпись</p>"),
            ("Footnote", "<p>Сноска</p>"),
            ("Equation-Block", r#"<p>Формула <math>x^2 + y^2</math></p>"#),
            ("List-Group", "<ul><li>Первый</li></ul>"),
            ("Page-Header", "<p>Шапка</p>"),
            ("Page-Footer", "<p>Низ</p>"),
            ("Image", r#"<img alt="печать"/>"#),
            ("Section-Header", "<h2>Раздел</h2>"),
            (
                "Table",
                "<table><tr><th>Товар</th><th>Сумма</th></tr>\
                 <tr><td>Болт, М8</td><td>100</td></tr></table>",
            ),
            ("Text", "<p>Текст</p>"),
            ("Complex-Block", "<p>Сложный блок</p>"),
            ("Code-Block", "<pre><code>let x = 1;</code></pre>"),
            (
                "Form",
                r#"<p><input type="checkbox" checked="checked"/> Согласен</p>"#,
            ),
            ("Table-Of-Contents", "<p>Оглавление</p>"),
            ("Figure", r#"<img alt="логотип"/>"#),
            ("Chemical-Block", "<p><chem>CCO</chem></p>"),
            ("Diagram", r#"<img alt="схема"/>"#),
            ("Bibliography", "<p>Источники</p>"),
            ("Blank-Page", ""),
        ]
    }

    fn page_with_all_labels() -> PageOutput {
        let mut html = String::new();
        for (index, (label, content)) in all_label_fixtures().into_iter().enumerate() {
            let y0 = index * 52;
            html.push_str(&format!(
                r#"<div data-bbox="0 {y0} 1000 {y1}" data-label="{label}">{content}</div>"#,
                y1 = y0 + 40
            ));
        }
        PageOutput {
            page_number: Some(1),
            html,
            jpeg: synthetic_jpeg(1000, 1000),
            width: 1000,
            height: 1000,
        }
    }

    fn expected_extensions(label: &str) -> Vec<&'static str> {
        asset_kinds(label)
            .iter()
            .map(|kind| match kind {
                AssetKind::Png => "png",
                AssetKind::Md => "md",
                AssetKind::Html => "html",
                AssetKind::Csv => "csv",
                AssetKind::Tex => "tex",
                AssetKind::Txt => "txt",
            })
            .collect()
    }

    #[test]
    fn all_19_labels_map_to_expected_typed_files() {
        assert_eq!(all_label_fixtures().len(), 19);
        let mappings: [(&str, &[&str]); 19] = [
            ("Caption", &["png", "md"]),
            ("Footnote", &["png", "md"]),
            ("Equation-Block", &["png", "tex"]),
            ("List-Group", &["png", "md"]),
            ("Page-Header", &["png", "md"]),
            ("Page-Footer", &["png", "md"]),
            ("Image", &["png", "txt"]),
            ("Section-Header", &["png", "md"]),
            ("Table", &["png", "html", "csv"]),
            ("Text", &["png", "md"]),
            ("Complex-Block", &["png", "md"]),
            ("Code-Block", &["png", "txt"]),
            ("Form", &["png", "html"]),
            ("Table-Of-Contents", &["png", "md"]),
            ("Figure", &["png", "txt"]),
            ("Chemical-Block", &["png", "html", "txt"]),
            ("Diagram", &["png", "txt"]),
            ("Bibliography", &["png", "md"]),
            ("Blank-Page", &[]),
        ];
        for (label, expected) in mappings {
            assert_eq!(expected_extensions(label), expected, "{label}");
        }
        assert_eq!(expected_extensions("block"), vec!["png", "md"]);
    }

    #[test]
    fn all_mode_creates_png_and_typed_files_for_every_block() {
        let tmp = tempfile::tempdir().unwrap();
        let output = build_document_output(
            "scan.jpg",
            "scan",
            tmp.path(),
            &[page_with_all_labels()],
            BlockExtraction::All,
        );
        assert!(output.warnings.is_empty(), "{:?}", output.warnings);
        assert_eq!(output.json.pages[0].chunks.len(), 18);
        assert!(output.json.pages[0]
            .chunks
            .iter()
            .all(|chunk| !chunk.label.eq_ignore_ascii_case("blank-page")));

        let mut total_files = 0;
        for chunk in &output.json.pages[0].chunks {
            assert!(chunk.block_id.starts_with("p1-"), "{}", chunk.block_id);
            assert_eq!(chunk.block_id.len(), 5, "{}", chunk.block_id);
            let expected = expected_extensions(&chunk.label);
            let actual = asset_values(&chunk.assets)
                .into_iter()
                .map(|(extension, _)| extension)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{}", chunk.label);
            for (extension, path) in asset_values(&chunk.assets) {
                assert!(
                    path.starts_with("scan_blocks/page-1-"),
                    "{path} for {extension}"
                );
                assert!(
                    path.ends_with(&format!(".{extension}")),
                    "{path} for {extension}"
                );
                assert!(tmp.path().join(path).is_file(), "missing {path}");
                total_files += 1;
            }
        }
        assert_eq!(total_files, 38, "{:?}", output.files);
        assert_eq!(output.files.len(), 38);

        let by_label = |label: &str| {
            output.json.pages[0]
                .chunks
                .iter()
                .find(|chunk| chunk.label == label)
                .unwrap()
        };
        let table = by_label("Table");
        let table_path = tmp.path().join(table.assets.csv.as_deref().unwrap());
        let csv = std::fs::read_to_string(table_path).unwrap();
        assert_eq!(csv, "Товар,Сумма\n\"Болт, М8\",100\n");
        let table_html =
            std::fs::read_to_string(tmp.path().join(table.assets.html.as_deref().unwrap()))
                .unwrap();
        assert!(table_html.contains("<table"), "{table_html}");

        let equation = by_label("Equation-Block");
        let tex = std::fs::read_to_string(tmp.path().join(equation.assets.tex.as_deref().unwrap()))
            .unwrap();
        assert!(tex.contains("x^2 + y^2"), "{tex}");

        let code = by_label("Code-Block");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(code.assets.txt.as_deref().unwrap())).unwrap(),
            "let x = 1;"
        );

        let form = by_label("Form");
        let form_html =
            std::fs::read_to_string(tmp.path().join(form.assets.html.as_deref().unwrap())).unwrap();
        assert!(form_html.contains("checkbox"), "{form_html}");

        let image = by_label("Image");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(image.assets.txt.as_deref().unwrap())).unwrap(),
            "печать"
        );
        assert!(image.image.is_some());

        let chemical = by_label("Chemical-Block");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(chemical.assets.txt.as_deref().unwrap()))
                .unwrap(),
            "CCO"
        );
        assert!(
            std::fs::read_to_string(tmp.path().join(chemical.assets.html.as_deref().unwrap()))
                .unwrap()
                .contains("chem")
        );

        let section = by_label("Section-Header");
        assert!(section
            .assets
            .md
            .as_deref()
            .unwrap()
            .ends_with("page-1-08-section-header.md"));
    }

    fn asset_values(assets: &ChandraAssets) -> Vec<(&'static str, &str)> {
        let mut values = Vec::new();
        if let Some(path) = assets.png.as_deref() {
            values.push(("png", path));
        }
        if let Some(path) = assets.md.as_deref() {
            values.push(("md", path));
        }
        if let Some(path) = assets.html.as_deref() {
            values.push(("html", path));
        }
        if let Some(path) = assets.csv.as_deref() {
            values.push(("csv", path));
        }
        if let Some(path) = assets.tex.as_deref() {
            values.push(("tex", path));
        }
        if let Some(path) = assets.txt.as_deref() {
            values.push(("txt", path));
        }
        values
    }

    #[test]
    fn none_images_and_all_modes_differ() {
        let tmp = tempfile::tempdir().unwrap();
        let html = r#"<div data-bbox="0 0 500 500" data-label="Image"><img alt="печать"/></div>
            <div data-bbox="500 0 1000 500" data-label="Text"><p>Текст</p></div>"#;
        let page = || PageOutput {
            page_number: Some(1),
            html: html.to_string(),
            jpeg: synthetic_jpeg(100, 100),
            width: 100,
            height: 100,
        };

        let none = build_document_output(
            "scan.jpg",
            "scan",
            tmp.path(),
            &[page()],
            BlockExtraction::None,
        );
        assert!(none.files.is_empty());
        assert!(none.json.pages[0]
            .chunks
            .iter()
            .all(|chunk| chunk.assets.is_empty()));

        let images = build_document_output(
            "scan.jpg",
            "scan",
            tmp.path(),
            &[page()],
            BlockExtraction::Images,
        );
        assert_eq!(images.files.len(), 1);
        let image = &images.json.pages[0].chunks[0];
        assert!(image.assets.png.is_some());
        assert!(image.assets.txt.is_none());
        assert!(images.json.pages[0].chunks[1].assets.is_empty());

        let all = build_document_output(
            "scan.jpg",
            "scan",
            tmp.path(),
            &[page()],
            BlockExtraction::All,
        );
        assert!(all.files.len() > images.files.len());
        assert!(all.json.pages[0].chunks[1].assets.md.is_some());
        assert!(all.json.pages[0].chunks[1].assets.png.is_some());
    }

    #[test]
    fn html_and_markdown_embed_assets_and_block_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let page = page_with_all_labels();
        let output = build_document_output(
            "scan.jpg",
            "scan",
            tmp.path(),
            &[page],
            BlockExtraction::All,
        );

        let html = render_html_with_blocks("scan", &output.pages, BlockExtraction::All);
        assert!(
            html.contains(
                "class=\"ocr-block\" id=\"block-p1-01\" data-block-id=\"p1-01\" \
                 data-label=\"Caption\""
            ),
            "{html}"
        );
        assert!(html.contains("class=\"ocr-asset\""), "{html}");
        assert!(
            html.contains("href=\"scan_blocks/page-1-01-caption.md\""),
            "{html}"
        );
        assert!(
            html.contains("<img src=\"scan_blocks/page-1-07-image.png\""),
            "{html}"
        );
        assert!(html.contains("alt=\"печать\""), "{html}");
        assert!(
            html.contains("href=\"scan_blocks/page-1-09-table.csv\""),
            "{html}"
        );
        assert!(
            html.contains("href=\"scan_blocks/page-1-03-equation-block.tex\""),
            "{html}"
        );
        assert!(!html.contains("data-label=\"Blank-Page\""), "{html}");

        let markdown = render_markdown_with_blocks("scan", &output.pages, BlockExtraction::All);
        assert!(
            markdown.contains("<!-- block: p1-01 Caption -->"),
            "{markdown}"
        );
        assert!(
            markdown.contains("[MD](scan_blocks/page-1-01-caption.md)"),
            "{markdown}"
        );
        assert!(
            markdown.contains("[PNG](scan_blocks/page-1-01-caption.png)"),
            "{markdown}"
        );
        assert!(
            markdown.contains("![печать](scan_blocks/page-1-07-image.png)"),
            "{markdown}"
        );
        assert!(
            markdown.contains("[CSV](scan_blocks/page-1-09-table.csv)"),
            "{markdown}"
        );
        assert!(
            markdown.contains("[TeX](scan_blocks/page-1-03-equation-block.tex)"),
            "{markdown}"
        );
    }

    #[test]
    fn images_mode_embeds_only_image_figures() {
        let tmp = tempfile::tempdir().unwrap();
        let html = r#"<div data-bbox="0 0 500 500" data-label="Image"><img alt="печать"/></div>
            <div data-bbox="500 0 1000 500" data-label="Text"><p>Текст</p></div>"#;
        let page = PageOutput {
            page_number: Some(1),
            html: html.to_string(),
            jpeg: synthetic_jpeg(100, 100),
            width: 100,
            height: 100,
        };
        let output = build_document_output(
            "scan.jpg",
            "scan",
            tmp.path(),
            &[page],
            BlockExtraction::Images,
        );
        let markdown = render_markdown_with_blocks("scan", &output.pages, BlockExtraction::Images);
        assert!(
            markdown.contains("<!-- block: p1-01 Image -->"),
            "{markdown}"
        );
        assert!(
            markdown.contains("![печать](scan_blocks/page-1-01-image.png)"),
            "{markdown}"
        );
        assert!(
            !markdown.contains("<!-- block: p1-02 Text -->"),
            "{markdown}"
        );
        assert!(
            !markdown.contains("scan_blocks/page-1-02-text"),
            "{markdown}"
        );
        assert!(markdown.contains("Текст"), "{markdown}");

        let html = render_html_with_blocks("scan", &output.pages, BlockExtraction::Images);
        assert!(html.contains("id=\"block-p1-01\""), "{html}");
        assert!(!html.contains("id=\"block-p1-02\""), "{html}");
    }

    #[test]
    fn none_mode_leaves_markdown_and_html_unwrapped() {
        let tmp = tempfile::tempdir().unwrap();
        let output = build_document_output(
            "scan.jpg",
            "scan",
            tmp.path(),
            &[page_with_all_labels()],
            BlockExtraction::None,
        );
        let markdown = render_markdown_with_blocks("scan", &output.pages, BlockExtraction::None);
        assert!(!markdown.contains("<!-- block:"), "{markdown}");
        assert!(!markdown.contains("ocr-asset"), "{markdown}");
        assert!(!markdown.contains("scan_blocks"), "{markdown}");
        let html = render_html_with_blocks("scan", &output.pages, BlockExtraction::None);
        assert!(!html.contains("ocr-block"), "{html}");
    }

    #[test]
    fn math_extraction_and_asset_urls_are_safe() {
        assert_eq!(
            extract_math_text(r#"<p><math>x_1</math> и <math>y^2</math></p>"#).as_deref(),
            Some("x_1 y^2")
        );
        assert!(extract_math_text("<p>no math</p>").is_none());
        assert_eq!(
            asset_url("scan_blocks/Счет № 1 (копия)_blocks/a b.png"),
            "scan_blocks/%D0%A1%D1%87%D0%B5%D1%82%20%E2%84%96%201%20%28%D0%BA%D0%BE%D0%BF%D0%B8%D1%8F%29_blocks/a%20b.png"
        );
    }

    #[test]
    fn block_file_limit_stops_with_one_warning() {
        let mut state = AssetState::default();
        state.files = vec!["x".to_string(); MAX_BLOCK_FILES];
        assert!(!can_write_asset(&mut state, 1));
        assert!(!can_write_asset(&mut state, 1));
        assert_eq!(state.warnings.len(), 1);
        assert!(state.warnings[0].contains("2000"));
    }

    #[test]
    fn huge_image_is_rejected_before_decoding() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("huge.png");
        let image = image::RgbImage::from_pixel(2000, 2000, image::Rgb([0, 0, 0]));
        image.save(&path).unwrap();
        let error = prepare_image(&path, 100_000).unwrap_err().to_string();
        assert!(error.contains("--max-pixels"), "{error}");
        assert!(error.contains("downscale"), "{error}");
    }

    #[test]
    fn preflight_rejects_pages_zero_as_a_configuration_error() {
        let options = JobOptions {
            inputs: vec![PathBuf::from("anything.png")],
            pages: Some("0".to_string()),
            ..JobOptions::default()
        };
        let error = preflight(&options).unwrap_err();
        let text = format!("{error:#}");
        assert!(text.contains("--pages"), "{text}");
        assert!(text.contains("0"), "{text}");
    }

    #[test]
    fn explicit_pdfium_is_forwarded_and_missing_pdfium_is_actionable() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp
            .path()
            .join("runtime")
            .join("pdfium")
            .join("bin")
            .join(ocr_pdf::pdfium_library_names()[0]);
        std::fs::create_dir_all(library.parent().unwrap()).unwrap();
        std::fs::write(&library, b"fake").unwrap();

        let by_dir = JobOptions {
            pdfium: Some(tmp.path().join("runtime")),
            ..JobOptions::default()
        };
        assert_eq!(locate_pdfium_for(&by_dir).unwrap(), library);

        let by_file = JobOptions {
            pdfium: Some(library.clone()),
            ..JobOptions::default()
        };
        assert_eq!(locate_pdfium_for(&by_file).unwrap(), library);

        let missing = JobOptions {
            pdfium: Some(tmp.path().join("nope")),
            ..JobOptions::default()
        };
        let text = format!("{:#}", locate_pdfium_for(&missing).unwrap_err());
        assert!(text.contains("PDFium not found"), "{text}");
        assert!(text.contains("OCR_PDFIUM"), "{text}");
        assert!(text.contains("choose the runtime folder"), "{text}");
    }

    #[test]
    fn pdf_password_is_forwarded_to_render_options() {
        let options = JobOptions {
            password: Some("s3cret".to_string()),
            dpi: 144,
            max_pixels: 1234,
            ..JobOptions::default()
        };
        let render = render_options(&options);
        assert_eq!(render.password.as_deref(), Some("s3cret"));
        assert_eq!(render.dpi, 144);
        assert_eq!(render.max_pixels, 1234);
    }

    fn sample_document(source: &str) -> DocumentMeta {
        DocumentMeta {
            source: source.to_string(),
            outputs: Vec::new(),
            pages_total: 1,
            pages_ok: 1,
            pages_failed: 0,
            errors: Vec::new(),
            elapsed_ms: 1,
            device: "cpu".to_string(),
            model: "m".to_string(),
            mmproj: "p".to_string(),
            llama_server: "s".to_string(),
            dpi: None,
            format: "md".to_string(),
            images: Vec::new(),
            assets: Vec::new(),
            generated_at: now_rfc3339(),
        }
    }

    fn sample_job(document: DocumentMeta) -> JobMeta {
        JobMeta {
            tool: "ocr-tool".to_string(),
            tool_version: "0.0.0".to_string(),
            generated_at: now_rfc3339(),
            output_dir: "out".to_string(),
            device_requested: "auto".to_string(),
            device_used: "cpu".to_string(),
            model_dir: "models".to_string(),
            model: "m".to_string(),
            mmproj: "p".to_string(),
            llama_server: "s".to_string(),
            documents: vec![document],
        }
    }

    #[test]
    fn parallel_jobs_merge_documents_in_meta_json() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["a.pdf", "b.pdf"]
            .into_iter()
            .map(|source| {
                let dir = dir.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    write_job_meta(&dir, &sample_job(sample_document(source))).unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let meta: JobMeta =
            serde_json::from_slice(&std::fs::read(dir.join("_meta.json")).unwrap()).unwrap();
        let sources: Vec<&str> = meta.documents.iter().map(|d| d.source.as_str()).collect();
        assert!(sources.contains(&"a.pdf"), "{sources:?}");
        assert!(sources.contains(&"b.pdf"), "{sources:?}");
        assert!(!dir.join(".ocr-meta.lock").exists());
    }
}
