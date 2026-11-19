use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use ocr_app::download::{human_bytes, verify_models, DownloadProgress, MODEL_FILES};
use ocr_app::{doctor, download_models, preflight, print_doctor, run_job, JobOptions};
use ocr_core::{resolve_model_dir, Device, LogLevel, OutputFormat, ProgressEvent, MODEL_ENV};

#[derive(Parser)]
#[command(
    name = "ocr-tool",
    version,
    about = "Local OCR for PDF and images (Chandra OCR 2 via llama.cpp)",
    long_about = "Recognize Russian/English documents locally. Outputs Markdown and/or HTML.\n\
                  Requires pdfium, a llama-server build and the Chandra GGUF model; see README."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Recognize images and PDFs
    Ocr(OcrArgs),
    /// Check model, pdfium, llama-server and GPU/CPU builds
    Doctor(DoctorArgs),
    /// Model management
    Models(ModelsArgs),
    /// Launch the desktop GUI
    Gui,
}

#[derive(Args)]
struct OcrArgs {
    /// Files and/or directories (png, jpg, tiff, webp, bmp, pdf)
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

    /// Output directory
    #[arg(short, long, default_value = "ocr-output")]
    output: PathBuf,

    /// Output format
    #[arg(long, default_value = "both", value_parser = parse_format)]
    format: OutputFormat,

    /// Device selection; auto tries GPU and falls back to CPU
    #[arg(long, default_value = "auto", value_parser = parse_device)]
    device: Device,

    /// PDF rendering DPI
    #[arg(long, default_value_t = 192)]
    dpi: u32,

    /// PDF page selection, 1-based (for example 1-5,7)
    #[arg(long)]
    pages: Option<String>,

    /// Password for encrypted PDFs (never written to logs or reports)
    #[arg(long)]
    password: Option<String>,

    /// Directory containing chandra-ocr-2.Q4_K_M.gguf and the mmproj
    #[arg(long)]
    model_dir: Option<PathBuf>,

    /// Directory containing llama-server (or the executable itself)
    #[arg(long)]
    llama_dir: Option<PathBuf>,

    /// Recurse into subdirectories
    #[arg(long)]
    recursive: bool,

    /// Maximum decoded pixels per page/image
    #[arg(long, default_value_t = 40_000_000)]
    max_pixels: u64,

    /// Maximum generated tokens per page
    #[arg(long, default_value_t = 4096)]
    max_tokens: u32,

    /// Per-page HTTP timeout in seconds
    #[arg(long, default_value_t = 600)]
    timeout: u64,

    /// Print the final report as JSON on stdout (progress still goes to stderr)
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct DoctorArgs {
    /// Model directory to inspect (overrides OCR_MODEL_DIR)
    #[arg(long)]
    model_dir: Option<PathBuf>,

    /// llama-server directory to inspect
    #[arg(long)]
    llama_dir: Option<PathBuf>,

    /// Machine-readable output
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ModelsArgs {
    #[command(subcommand)]
    command: ModelsCommand,
}

#[derive(Subcommand)]
enum ModelsCommand {
    /// Download the pinned Q4_K_M model and mmproj from Hugging Face
    Download(DownloadArgs),
    /// Check the model directory; --deep also verifies SHA-256 (~3.4 GB read)
    Verify(VerifyArgs),
}

#[derive(Args)]
struct DownloadArgs {
    /// Destination directory (defaults to OCR_MODEL_DIR or app-data)
    #[arg(long)]
    dir: Option<PathBuf>,

    /// Replace existing files and restart partial downloads
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct VerifyArgs {
    /// Model directory (defaults to OCR_MODEL_DIR or app-data)
    #[arg(long)]
    dir: Option<PathBuf>,

    /// Hash the weights and compare with the pinned SHA-256
    #[arg(long)]
    deep: bool,
}

fn parse_device(value: &str) -> Result<Device, String> {
    value.parse()
}

fn parse_format(value: &str) -> Result<OutputFormat, String> {
    value.parse()
}

fn main() {
    let cli = Cli::parse();
    let code = match execute(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ocr-tool: error: {error:#}");
            3
        }
    };
    std::process::exit(code);
}

fn execute(cli: Cli) -> Result<i32> {
    match cli.command {
        Command::Ocr(args) => run_ocr(args),
        Command::Doctor(args) => run_doctor(args),
        Command::Models(args) => run_models(args),
        Command::Gui => run_gui(),
    }
}

fn run_ocr(args: OcrArgs) -> Result<i32> {
    let options = JobOptions {
        inputs: args.inputs,
        output_dir: args.output,
        format: args.format,
        device: args.device,
        dpi: args.dpi,
        pages: args.pages,
        password: args.password,
        recursive: args.recursive,
        model_dir: args.model_dir,
        llama_dir: args.llama_dir,
        max_pixels: args.max_pixels,
        max_tokens: args.max_tokens,
        request_timeout_secs: args.timeout,
    };

    // Preflight errors are configuration/user errors -> exit 2.
    if let Err(error) = preflight(&options) {
        eprintln!("ocr-tool: configuration error: {error:#}");
        return Ok(2);
    }

    let cancel = Arc::new(AtomicBool::new(false));
    install_ctrlc(cancel.clone());

    let mut printer = ProgressPrinter::new();
    let result = run_job(&options, &cancel, &mut |event| printer.handle(event));

    match result {
        Ok(report) => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!();
                println!(
                    "Done: {}/{} document(s), {} page(s) in {:.1}s on {}",
                    report.documents_ok,
                    report.documents_ok + report.documents_failed,
                    report.pages_ok,
                    report.elapsed_ms as f64 / 1000.0,
                    report.device
                );
                if report.documents_failed > 0 || report.pages_failed > 0 {
                    println!(
                        "Errors: {} document(s), {} page(s); see above and _meta.json",
                        report.documents_failed, report.pages_failed
                    );
                }
                if report.cancelled {
                    println!("Cancelled; partial outputs were kept.");
                }
                println!("Output: {}", report.output_dir);
                for output in &report.outputs {
                    println!("  {output}");
                }
                let succeeded =
                    !report.cancelled && report.documents_failed == 0 && report.pages_failed == 0;
                return Ok(if succeeded { 0 } else { 3 });
            }
            let succeeded =
                !report.cancelled && report.documents_failed == 0 && report.pages_failed == 0;
            Ok(if succeeded { 0 } else { 3 })
        }
        Err(error) => {
            eprintln!("ocr-tool: processing failed: {error:#}");
            Ok(3)
        }
    }
}

fn run_doctor(args: DoctorArgs) -> Result<i32> {
    let report = doctor(args.model_dir.as_deref(), args.llama_dir.as_deref());
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_doctor(&report);
    }
    Ok(0)
}

fn run_models(args: ModelsArgs) -> Result<i32> {
    match args.command {
        ModelsCommand::Download(download) => {
            let env_dir = std::env::var_os(MODEL_ENV);
            let dir = resolve_model_dir(download.dir.as_deref(), env_dir.as_deref());
            println!(
                "Downloading {} file(s) from {} to {}",
                MODEL_FILES.len(),
                ocr_app::download::REPO_URL,
                dir.display()
            );
            let mut printer = DownloadPrinter::default();
            match download_models(&dir, download.force, &mut |progress| {
                printer.handle(progress)
            }) {
                Ok(files) => {
                    printer.finish_line();
                    println!("Model ready:");
                    for file in files {
                        let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
                        println!("  {} ({})", file.display(), human_bytes(size));
                    }
                    Ok(0)
                }
                Err(error) => {
                    printer.finish_line();
                    eprintln!("ocr-tool: model download failed: {error:#}");
                    Ok(3)
                }
            }
        }
        ModelsCommand::Verify(verify) => {
            let env_dir = std::env::var_os(MODEL_ENV);
            let dir = resolve_model_dir(verify.dir.as_deref(), env_dir.as_deref());
            match verify_models(&dir, verify.deep) {
                Ok(()) => {
                    println!(
                        "Model OK: {} ({} checked)",
                        dir.display(),
                        if verify.deep {
                            "size + SHA-256"
                        } else {
                            "size"
                        }
                    );
                    Ok(0)
                }
                Err(error) => {
                    eprintln!("ocr-tool: model verification failed: {error:#}");
                    Ok(2)
                }
            }
        }
    }
}

fn run_gui() -> Result<i32> {
    let current = std::env::current_exe()?;
    let dir = current
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let name = if cfg!(windows) {
        "ocr-tool-gui.exe"
    } else {
        "ocr-tool-gui"
    };
    let gui = dir.join(name);
    if !gui.is_file() {
        eprintln!(
            "ocr-tool: GUI executable not found at {}; build it with `cargo build --release -p ocr-gui`",
            gui.display()
        );
        return Ok(2);
    }
    std::process::Command::new(&gui).spawn()?;
    println!("Launched {}", gui.display());
    Ok(0)
}

fn install_ctrlc(cancel: Arc<AtomicBool>) {
    let signals = Arc::new(AtomicUsize::new(0));
    if ctrlc::set_handler(move || {
        let count = signals.fetch_add(1, Ordering::SeqCst) + 1;
        if count == 1 {
            cancel.store(true, Ordering::SeqCst);
            eprintln!("\nocr-tool: cancelling (press Ctrl+C again to force quit)...");
        } else {
            std::process::exit(130);
        }
    })
    .is_err()
    {
        eprintln!("ocr-tool: warning: could not install Ctrl+C handler");
    }
}

struct ProgressPrinter;

impl ProgressPrinter {
    fn new() -> Self {
        Self
    }

    fn handle(&mut self, event: ProgressEvent) {
        match event {
            ProgressEvent::JobStarted { files, output_dir } => {
                eprintln!("ocr-tool: {files} file(s) -> {output_dir}");
            }
            ProgressEvent::FileStarted { index, total, path } => {
                eprintln!("[{index}/{total}] {path}");
            }
            ProgressEvent::PageStarted {
                page,
                total_pages,
                selected_index,
                selected_total,
                ..
            } => {
                eprint!(
                    "{}",
                    page_progress_label(page, total_pages, selected_index, selected_total)
                );
            }
            ProgressEvent::PageFinished {
                page,
                chars,
                elapsed,
                ..
            } => {
                eprintln!(
                    "\r  page {page}: {chars} chars in {:.1}s        ",
                    elapsed.as_secs_f64()
                );
            }
            ProgressEvent::PageFailed { page, error, .. } => {
                eprintln!("\r  page {page}: FAILED: {error}");
            }
            ProgressEvent::FileFinished {
                outputs,
                pages_ok,
                pages_failed,
                elapsed,
                ..
            } => {
                eprintln!(
                    "  {pages_ok} page(s) ok, {pages_failed} failed in {:.1}s",
                    elapsed.as_secs_f64()
                );
                for output in outputs {
                    eprintln!("  -> {output}");
                }
            }
            ProgressEvent::FileFailed { error, .. } => {
                eprintln!("  FAILED: {error}");
            }
            ProgressEvent::Log { level, message } => {
                let prefix = match level {
                    LogLevel::Info => "  ",
                    LogLevel::Warning => "  warning: ",
                    LogLevel::Error => "  error: ",
                };
                eprintln!("{prefix}{message}");
            }
            ProgressEvent::JobFinished {
                documents_ok,
                documents_failed,
                elapsed,
                cancelled,
            } => {
                eprintln!(
                    "ocr-tool: finished in {:.1}s ({} ok, {} failed{})",
                    elapsed.as_secs_f64(),
                    documents_ok,
                    documents_failed,
                    if cancelled { ", cancelled" } else { "" }
                );
            }
        }
    }
}

#[derive(Default)]
struct DownloadPrinter {
    last_percent: Option<u64>,
    line_open: bool,
}

impl DownloadPrinter {
    fn handle(&mut self, progress: DownloadProgress) {
        let percent = progress
            .total
            .map(|total| (progress.downloaded.saturating_mul(100) / total.max(1)).min(100));
        if progress.done {
            self.finish_line();
            eprintln!(
                "  {}: {} ({} / {} files)",
                progress.file,
                if progress.skipped {
                    "already present"
                } else {
                    "downloaded"
                },
                progress.file_index,
                progress.file_count
            );
            self.last_percent = None;
            return;
        }
        if percent == self.last_percent {
            return;
        }
        self.last_percent = percent;
        let pct = percent
            .map(|p| format!("{p:3}%"))
            .unwrap_or_else(|| "  ?%".to_string());
        eprint!(
            "\r  [{}] {} {pct}  {}",
            progress.file_index,
            progress.file,
            human_bytes(progress.downloaded)
        );
        self.line_open = true;
        use std::io::Write;
        let _ = std::io::stderr().flush();
    }

    fn finish_line(&mut self) {
        if self.line_open {
            eprintln!();
            self.line_open = false;
        }
    }
}

/// `--pages 9-99` on an 11-page document must print `page 9/11`, not `9/3`.
fn page_progress_label(page: u32, total: u32, selected_index: u32, selected_total: u32) -> String {
    if selected_total != total {
        format!(
            "  page {page}/{total} (selected {selected_index}/{selected_total}): recognizing..."
        )
    } else {
        format!("  page {page}/{total}: recognizing...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_progress_shows_document_page_and_selected_index() {
        assert_eq!(
            page_progress_label(9, 11, 1, 3),
            "  page 9/11 (selected 1/3): recognizing..."
        );
        assert_eq!(
            page_progress_label(1, 2, 1, 2),
            "  page 1/2: recognizing..."
        );
    }
}
