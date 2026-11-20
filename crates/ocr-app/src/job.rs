use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use ocr_core::{
    clean_model_html, discover_model_paths, model_dir_source, now_rfc3339, parse_page_selection,
    render_html_document, render_markdown_document, resolve_model_dir, stem_of, validate_page_spec,
    DocumentMeta, JobMeta, ModelPaths, OutputAllocator, OutputFormat, ProgressEvent, MODEL_ENV,
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
        }
    }
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
    let mut pages: Vec<(Option<u32>, String)> = Vec::new();
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
                        pages.push((Some(page_number), clean));
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
        let jpeg = prepare_image(file, options.max_pixels)
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
                    pages.push((None, clean));
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
        if options.format.writes_markdown() {
            let markdown = render_markdown_document(&title, &pages);
            let path = prepared.output_dir.join(format!("{stem}.md"));
            std::fs::write(&path, markdown.as_bytes())
                .with_context(|| format!("cannot write {}", path.display()))?;
            produced_outputs.push(path.display().to_string());
        }
        if options.format.writes_html() {
            let html = render_html_document(&title, &pages);
            let path = prepared.output_dir.join(format!("{stem}.html"));
            std::fs::write(&path, html.as_bytes())
                .with_context(|| format!("cannot write {}", path.display()))?;
            produced_outputs.push(path.display().to_string());
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

fn prepare_image(path: &Path, max_pixels: u64) -> Result<Vec<u8>> {
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
    Ok(jpeg)
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
        let jpeg = prepare_image(&path, 300_000).unwrap();
        assert_eq!(&jpeg[..2], &[0xff, 0xd8]);
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert!(decoded.width() as u64 * decoded.height() as u64 <= 300_000);
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
