use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use ocr_app::download::{human_bytes, DownloadProgress, REPO_URL};
use ocr_app::setup::{
    model_status, pdfium_for_runtime, runtime_status, ModelStatus, PdfiumSource, RuntimeStatus,
};
use ocr_app::{download_models_cancellable, run_job, JobOptions, JobReport};
use ocr_core::{resolve_model_dir, Device, OutputFormat, ProgressEvent, MODEL_ENV};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 840.0])
            .with_min_inner_size([760.0, 620.0])
            .with_title("ocr-tool — Chandra OCR 2"),
        ..Default::default()
    };
    eframe::run_native(
        "ocr-tool",
        options,
        Box::new(|_cc| Ok(Box::new(OcrApp::new()))),
    )
}

const OK_COLOR: egui::Color32 = egui::Color32::from_rgb(80, 170, 100);
const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(210, 160, 60);

struct LogLine {
    text: String,
    error: bool,
}

enum Msg {
    Event(ProgressEvent),
    Done(Result<JobReport, String>),
    Download(DownloadProgress),
    DownloadDone(Result<Vec<PathBuf>, String>),
}

/// Sends `Msg::Done` when the worker disappears, including a panic unwind that
/// never reaches the normal send. Without it the UI would stay in `running`.
struct DoneGuard {
    tx: Sender<Msg>,
    sent: bool,
}

impl DoneGuard {
    fn new(tx: Sender<Msg>) -> Self {
        Self { tx, sent: false }
    }

    fn finish(&mut self, result: Result<JobReport, String>) {
        let _ = self.tx.send(Msg::Done(result));
        self.sent = true;
    }
}

impl Drop for DoneGuard {
    fn drop(&mut self) {
        if !self.sent {
            let _ = self.tx.send(Msg::Done(Err(
                "worker thread panicked before reporting a result".to_string(),
            )));
        }
    }
}

/// Same safety net for the download thread: a panic must leave `downloading`
/// instead of freezing the setup panel.
struct DownloadGuard {
    tx: Sender<Msg>,
    sent: bool,
}

impl DownloadGuard {
    fn new(tx: Sender<Msg>) -> Self {
        Self { tx, sent: false }
    }

    fn finish(&mut self, result: Result<Vec<PathBuf>, String>) {
        let _ = self.tx.send(Msg::DownloadDone(result));
        self.sent = true;
    }
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        if !self.sent {
            let _ = self.tx.send(Msg::DownloadDone(Err(
                "download thread panicked before reporting a result".to_string(),
            )));
        }
    }
}

struct OcrApp {
    inputs: Vec<PathBuf>,
    output_dir: String,
    format: OutputFormat,
    device: Device,
    dpi: u32,
    pages: String,
    password: String,
    recursive: bool,
    model_dir: String,
    llama_dir: String,
    pdfium: String,
    runtime_dir: String,

    running: bool,
    cancelling: bool,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<Msg>>,
    progress: f32,
    status: String,
    logs: Vec<LogLine>,
    last_report: Option<JobReport>,

    model_status: ModelStatus,
    runtime: RuntimeStatus,
    setup_key: (String, String, String, String),

    downloading: bool,
    download_cancel: Arc<AtomicBool>,
    download_text: String,
    download_last_percent: Option<u32>,
    confirm_replace: bool,
}

impl OcrApp {
    fn new() -> Self {
        let output = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("ocr-output");
        let mut app = Self {
            inputs: Vec::new(),
            output_dir: output.display().to_string(),
            format: OutputFormat::Both,
            device: Device::Auto,
            dpi: 192,
            pages: String::new(),
            password: String::new(),
            recursive: false,
            model_dir: String::new(),
            llama_dir: String::new(),
            pdfium: String::new(),
            runtime_dir: String::new(),
            running: false,
            cancelling: false,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            progress: 0.0,
            status: "Add files or a folder, then press Recognize.".to_string(),
            logs: Vec::new(),
            last_report: None,
            model_status: ModelStatus::Missing {
                dir: PathBuf::new(),
                reason: String::new(),
            },
            runtime: RuntimeStatus {
                pdfium: Err(String::new()),
                pdfium_source: PdfiumSource::Auto,
                llama: Vec::new(),
                llama_searched: Vec::new(),
                llama_warnings: Vec::new(),
            },
            setup_key: (String::new(), String::new(), String::new(), String::new()),
            downloading: false,
            download_cancel: Arc::new(AtomicBool::new(false)),
            download_text: String::new(),
            download_last_percent: None,
            confirm_replace: false,
        };
        app.refresh_setup();
        app
    }

    fn log(&mut self, text: impl Into<String>, error: bool) {
        self.logs.push(LogLine {
            text: text.into(),
            error,
        });
        if self.logs.len() > 2000 {
            self.logs.drain(..200);
        }
    }

    fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            if !self.inputs.iter().any(|p| p == &path) {
                self.inputs.push(path);
            }
        }
    }

    fn path_key(&self) -> (String, String, String, String) {
        (
            self.model_dir.trim().to_string(),
            self.llama_dir.trim().to_string(),
            self.pdfium.trim().to_string(),
            self.runtime_dir.trim().to_string(),
        )
    }

    /// Re-run the cheap setup checks: model files/sizes and the external
    /// runtime. Called at startup, after a path changes and after a download.
    fn refresh_setup(&mut self) {
        let model_dir = self.resolved_model_dir();
        self.model_status = model_status(&model_dir);
        let llama = non_empty_path(&self.llama_dir);
        let pdfium = non_empty_path(&self.pdfium);
        self.runtime = runtime_status(llama.as_deref(), pdfium.as_deref());
        self.setup_key = self.path_key();
    }

    fn maybe_refresh_setup(&mut self) {
        if self.path_key() != self.setup_key {
            self.refresh_setup();
        }
    }

    fn resolved_model_dir(&self) -> PathBuf {
        let explicit = non_empty_path(&self.model_dir);
        let env = std::env::var_os(MODEL_ENV);
        resolve_model_dir(explicit.as_deref(), env.as_deref())
    }

    fn setup_complete(&self) -> bool {
        // PDFium is part of the setup: a runtime with llama-server but no
        // PDFium still fails on every PDF, so keep the paths (and the PDFium
        // panel) visible instead of collapsing Advanced.
        self.model_status.is_ready() && self.runtime.is_ready()
    }

    fn start(&mut self) {
        if self.running || self.downloading || self.inputs.is_empty() {
            return;
        }
        let output_dir = PathBuf::from(self.output_dir.trim());
        if output_dir.as_os_str().is_empty() {
            self.status = "Choose an output directory first.".to_string();
            return;
        }

        self.maybe_refresh_setup();
        if !self.model_status.is_ready() {
            self.status = self.model_status.message();
            self.log(self.status.clone(), true);
            return;
        }
        if !self.runtime.llama_ready() {
            self.status = self.runtime.message();
            self.log(self.status.clone(), true);
            return;
        }
        if self.inputs.iter().any(|path| is_pdf_input(path)) && !self.runtime.pdfium_ready() {
            self.status =
                "PDFium not found; choose the runtime folder or set OCR_PDFIUM.".to_string();
            self.log(self.status.clone(), true);
            return;
        }

        let options = JobOptions {
            inputs: self.inputs.clone(),
            output_dir,
            format: self.format,
            device: self.device,
            dpi: self.dpi,
            pages: if self.pages.trim().is_empty() {
                None
            } else {
                Some(self.pages.trim().to_string())
            },
            password: if self.password.trim().is_empty() {
                None
            } else {
                Some(self.password.trim().to_string())
            },
            recursive: self.recursive,
            model_dir: non_empty_path(&self.model_dir),
            llama_dir: non_empty_path(&self.llama_dir),
            pdfium: non_empty_path(&self.pdfium),
            ..JobOptions::default()
        };

        self.running = true;
        self.cancelling = false;
        self.progress = 0.0;
        self.last_report = None;
        self.logs.clear();
        self.status = "Starting engine...".to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = cancel.clone();

        let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let mut guard = DoneGuard::new(tx.clone());
            let mut emit = |event: ProgressEvent| {
                let _ = tx.send(Msg::Event(event));
            };
            let result =
                run_job(&options, &cancel, &mut emit).map_err(|error| format!("{error:#}"));
            guard.finish(result);
        });
    }

    /// Download the pinned model files into the resolved model directory. The
    /// existing valid files (including renamed pairs such as `chandra-q4.gguf`)
    /// are recognised by the downloader and skipped.
    fn start_download(&mut self) {
        self.start_download_with_force(false);
    }

    /// Replace the model files: the downloader deletes the canonical files and
    /// their `.part` remainders, then fetches the pinned pair again. Used by
    /// the “Replace / Re-download” button after its explicit confirmation.
    fn start_download_force(&mut self) {
        self.start_download_with_force(true);
    }

    fn start_download_with_force(&mut self, force: bool) {
        if self.downloading || self.running {
            return;
        }
        self.maybe_refresh_setup();
        let dir = self.resolved_model_dir();
        if let Err(error) = std::fs::create_dir_all(&dir) {
            self.status = format!("Cannot create model directory {}", dir.display());
            self.log(
                format!("error: cannot create {}: {error}", dir.display()),
                true,
            );
            return;
        }
        self.downloading = true;
        self.confirm_replace = false;
        self.progress = 0.0;
        self.download_text = "Starting download...".to_string();
        self.download_last_percent = None;
        self.status = if force {
            format!("Replacing model files in {}...", dir.display())
        } else {
            format!("Downloading model to {}...", dir.display())
        };
        self.log(
            format!(
                "download{}: {REPO_URL} -> {}",
                if force { " (replace)" } else { "" },
                dir.display()
            ),
            false,
        );
        let cancel = Arc::new(AtomicBool::new(false));
        self.download_cancel = cancel.clone();

        let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let mut guard = DownloadGuard::new(tx.clone());
            let mut emit = |event: DownloadProgress| {
                let _ = tx.send(Msg::Download(event));
            };
            let result = download_models_cancellable(&dir, force, &cancel, &mut emit)
                .map_err(|error| format!("{error:#}"));
            guard.finish(result);
        });
    }

    fn apply_runtime_dir(&mut self, folder: PathBuf) {
        self.runtime_dir = folder.display().to_string();
        self.llama_dir = folder.display().to_string();
        self.pdfium = pdfium_for_runtime(&folder)
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.refresh_setup();
        let status = self.runtime.message();
        self.log(
            format!("runtime folder: {} — {status}", folder.display()),
            !self.runtime.is_ready(),
        );
        if !self.runtime.is_ready() {
            self.status = status;
        }
    }

    fn open_model_folder(&mut self) {
        let dir = self.resolved_model_dir();
        if let Err(error) = std::fs::create_dir_all(&dir) {
            self.log(
                format!("error: cannot create {}: {error}", dir.display()),
                true,
            );
            return;
        }
        if let Err(error) = open::that(&dir) {
            self.log(format!("cannot open {}: {error}", dir.display()), true);
        }
    }

    fn poll(&mut self) {
        loop {
            let message = match &self.rx {
                Some(rx) => match rx.try_recv() {
                    Ok(message) => message,
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // The worker vanished without a final message (panic
                        // before the guard existed, or a dropped sender): leave
                        // the busy state instead of spinning forever.
                        if self.running {
                            self.running = false;
                            self.cancelling = false;
                            self.status = "Worker stopped unexpectedly. See the log.".to_string();
                            self.log(
                                "error: worker thread exited without reporting a result"
                                    .to_string(),
                                true,
                            );
                        }
                        if self.downloading {
                            self.downloading = false;
                            self.status =
                                "Download worker stopped unexpectedly. See the log.".to_string();
                            self.log(
                                "error: download thread exited without reporting a result"
                                    .to_string(),
                                true,
                            );
                        }
                        self.rx = None;
                        break;
                    }
                },
                None => return,
            };
            match message {
                Msg::Event(event) => self.handle_event(event),
                Msg::Done(result) => {
                    self.running = false;
                    self.cancelling = false;
                    match result {
                        Ok(report) => {
                            self.status = if report.cancelled {
                                "Cancelled; partial output kept.".to_string()
                            } else if report.documents_failed > 0 || report.pages_failed > 0 {
                                format!(
                                    "Finished with errors: {} page(s) ok, {} failed.",
                                    report.pages_ok, report.pages_failed
                                )
                            } else {
                                format!(
                                    "Done: {} document(s), {} page(s) in {:.1}s on {}.",
                                    report.documents_ok,
                                    report.pages_ok,
                                    report.elapsed_ms as f64 / 1000.0,
                                    report.device
                                )
                            };
                            if report.documents_failed > 0 || report.pages_failed > 0 {
                                let status = self.status.clone();
                                self.log(status, true);
                            }
                            self.last_report = Some(report);
                            self.progress = 1.0;
                        }
                        Err(error) => {
                            // Keep the raw chain in the log only; the status bar
                            // names the cause and never shows `os error 3`.
                            self.status = friendly_failure(&error);
                            self.log(format!("error: {error}"), true);
                        }
                    }
                    self.rx = None;
                    return;
                }
                Msg::Download(progress) => self.handle_download_progress(progress),
                Msg::DownloadDone(result) => {
                    self.downloading = false;
                    match result {
                        Ok(files) => {
                            self.progress = 1.0;
                            for file in &files {
                                let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                                self.log(
                                    format!(
                                        "model file: {} ({})",
                                        file.display(),
                                        human_bytes(size)
                                    ),
                                    false,
                                );
                            }
                            self.refresh_setup();
                            self.status = if self.model_status.is_ready() {
                                format!("Model ready: {}", self.model_status.dir().display())
                            } else {
                                self.model_status.message()
                            };
                            let error = !self.model_status.is_ready();
                            self.log(self.status.clone(), error);
                        }
                        Err(error) => {
                            let cancelled = error.to_ascii_lowercase().contains("cancel");
                            self.status = download_failure_status(&error, cancelled);
                            self.log(format!("error: {error}"), true);
                            if download_needs_replace(&error) {
                                self.log(
                                    "use the Replace / Re-download button to replace the existing model files",
                                    true,
                                );
                            }
                            self.refresh_setup();
                        }
                    }
                    self.rx = None;
                    return;
                }
            }
        }
    }

    fn handle_download_progress(&mut self, progress: DownloadProgress) {
        let total = progress.total.unwrap_or(0);
        let fraction = if total > 0 {
            (progress.downloaded as f32 / total as f32).min(1.0)
        } else {
            0.0
        };
        self.progress = fraction;
        let percent = (fraction * 100.0).round() as u32;
        let suffix = if progress.skipped {
            " (already present)"
        } else {
            ""
        };
        self.download_text = if total > 0 {
            format!(
                "{}: {} / {} ({percent}%){suffix}",
                progress.file,
                human_bytes(progress.downloaded),
                human_bytes(total)
            )
        } else {
            format!(
                "{}: {}{suffix}",
                progress.file,
                human_bytes(progress.downloaded)
            )
        };
        self.status = if progress.done {
            format!(
                "{}: {}{suffix}",
                progress.file,
                if progress.skipped {
                    "already present"
                } else {
                    "downloaded"
                }
            )
        } else {
            format!("Downloading {}: {percent}%", progress.file)
        };
        let bucket = percent / 10;
        if progress.done
            || progress.skipped
            || self.download_last_percent.map(|last| last / 10) != Some(bucket)
        {
            self.download_last_percent = Some(percent);
            self.log(self.download_text.clone(), false);
        }
    }

    fn handle_event(&mut self, event: ProgressEvent) {
        match event {
            ProgressEvent::JobStarted { files, output_dir } => {
                self.status = format!("Starting: {files} file(s) -> {output_dir}");
                self.log(format!("job: {files} file(s) -> {output_dir}"), false);
            }
            ProgressEvent::FileStarted { index, total, path } => {
                self.status = format!("[{index}/{total}] {path}");
                self.progress = if total > 0 {
                    (index.saturating_sub(1)) as f32 / total as f32
                } else {
                    0.0
                };
                self.log(format!("[{index}/{total}] {path}"), false);
            }
            ProgressEvent::PageStarted {
                page,
                total_pages,
                selected_index,
                selected_total,
                ..
            } => {
                self.status = if selected_total != total_pages {
                    format!(
                        "Page {page}/{total_pages} (selected {selected_index}/{selected_total}): recognizing..."
                    )
                } else {
                    format!("Page {page}/{total_pages}: recognizing...")
                };
                let base = self.progress;
                let fraction = if total_pages > 0 {
                    (page.saturating_sub(1)) as f32 / total_pages as f32
                } else {
                    0.0
                };
                self.progress = (base + fraction / 10.0).min(0.99);
            }
            ProgressEvent::PageFinished {
                page,
                chars,
                elapsed,
                ..
            } => {
                self.log(
                    format!(
                        "  page {page}: {chars} chars in {:.1}s",
                        elapsed.as_secs_f64()
                    ),
                    false,
                );
                self.progress = (self.progress + 0.02).min(0.99);
            }
            ProgressEvent::PageFailed { page, error, .. } => {
                self.log(format!("  page {page}: FAILED: {error}"), true);
            }
            ProgressEvent::FileFinished {
                outputs,
                pages_ok,
                pages_failed,
                ..
            } => {
                self.log(
                    format!("  {pages_ok} page(s) ok, {pages_failed} failed"),
                    pages_failed > 0,
                );
                for output in outputs {
                    self.log(format!("  -> {output}"), false);
                }
            }
            ProgressEvent::FileFailed { error, .. } => {
                self.log(format!("  FAILED: {error}"), true);
            }
            ProgressEvent::Log { message, .. } => {
                self.log(message, false);
            }
            ProgressEvent::JobFinished {
                documents_ok,
                documents_failed,
                elapsed,
                cancelled,
            } => {
                self.status = format!(
                    "Finished in {:.1}s: {} ok, {} failed{}",
                    elapsed.as_secs_f64(),
                    documents_ok,
                    documents_failed,
                    if cancelled { ", cancelled" } else { "" }
                );
            }
        }
    }

    fn ui_setup(&mut self, ui: &mut egui::Ui) {
        let model_ready = self.model_status.is_ready();
        let model_dir = self.model_status.dir().to_path_buf();
        let model_detail = self.model_status.detail().map(str::to_string);
        let model_message = self.model_status.message();

        if !model_ready {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                let incomplete = matches!(self.model_status, ModelStatus::Incomplete { .. });
                ui.heading(if incomplete {
                    format!("Model files incomplete in {}", model_dir.display())
                } else {
                    format!("Model not found in {}", model_dir.display())
                });
                ui.label(
                    "The Chandra OCR 2 weights (~3.4 GB) are not bundled. Download them once \
                     or point the app at a folder that already contains them.",
                );
                if let Some(detail) = &model_detail {
                    ui.label(detail);
                }
                ui.horizontal(|ui| {
                    let can_start = !self.downloading && !self.running;
                    if ui
                        .add_enabled(
                            can_start,
                            egui::Button::new(
                                egui::RichText::new("Download model (~3.4 GB)").strong(),
                            ),
                        )
                        .clicked()
                    {
                        self.start_download();
                    }
                    if ui
                        .add_enabled(
                            !self.downloading,
                            egui::Button::new("Choose existing model folder..."),
                        )
                        .clicked()
                    {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.model_dir = folder.display().to_string();
                            self.refresh_setup();
                            self.status = self.model_status.message();
                        }
                    }
                    if ui.button("Open model folder").clicked() {
                        self.open_model_folder();
                    }
                    if self.downloading && ui.button("Cancel download").clicked() {
                        self.download_cancel.store(true, Ordering::SeqCst);
                        self.status = "Cancelling download...".to_string();
                    }
                });
                // A canonical file with the wrong size (or a different SHA-256)
                // cannot be fixed by a plain download: the downloader refuses
                // to overwrite it. Offer the replace path the GUI can actually
                // execute instead of only mentioning `--force`.
                if !self.downloading && !self.running {
                    if !self.confirm_replace {
                        if ui.button("Replace / Re-download…").clicked() {
                            self.confirm_replace = true;
                        }
                    } else {
                        ui.colored_label(
                            WARN_COLOR,
                            "Replace deletes the existing model files (including any .part \
                             remainder) and downloads the pinned pair again.",
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new("Yes, replace and re-download").strong(),
                                ))
                                .clicked()
                            {
                                self.start_download_force();
                            }
                            if ui.button("Cancel").clicked() {
                                self.confirm_replace = false;
                            }
                        });
                    }
                }
                if self.downloading {
                    ui.add(
                        egui::ProgressBar::new(self.progress)
                            .show_percentage()
                            .animate(true),
                    );
                    ui.label(&self.download_text);
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.colored_label(OK_COLOR, "Model ready");
                ui.label(model_dir.display().to_string());
            });
        }

        let runtime = self.runtime.clone();
        let mut runtime_trouble = false;
        if !runtime.llama_ready() {
            runtime_trouble = true;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.heading("Runtime not found");
                ui.label(
                    "llama-server was not found. In the release build the runtime folder sits \
                     next to ocr-tool-gui.exe; otherwise choose the folder that contains \
                     llama-vulkan / llama-cpu and pdfium (or set OCR_LLAMA_SERVER).",
                );
                if let Some(warning) = runtime.llama_warnings.first() {
                    ui.label(warning);
                }
                ui.horizontal(|ui| {
                    if ui.button("Choose runtime folder...").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.apply_runtime_dir(folder);
                        }
                    }
                });
            });
        }
        // PDFium is checked independently: a missing llama-server must not
        // hide a missing PDFium (both break the first PDF job).
        if !runtime.pdfium_ready() {
            runtime_trouble = true;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.heading("PDFium not found");
                ui.label(
                    "PDF rendering needs PDFium. Choose the runtime folder that contains \
                     pdfium/bin, or set OCR_PDFIUM to the library file.",
                );
                ui.horizontal(|ui| {
                    if ui.button("Choose runtime folder...").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.apply_runtime_dir(folder);
                        }
                    }
                });
            });
        }
        if !runtime_trouble {
            ui.horizontal(|ui| {
                ui.colored_label(OK_COLOR, "Runtime ready");
                ui.label(format!(
                    "{} llama-server candidate(s), pdfium {} (from {})",
                    runtime.llama.len(),
                    runtime
                        .pdfium
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default(),
                    runtime.pdfium_source.label()
                ));
            });
            if runtime.pdfium_source.is_env() {
                ui.colored_label(
                    WARN_COLOR,
                    "PDFium is taken from OCR_PDFIUM, not from the runtime folder; unset \
                     OCR_PDFIUM to use the bundled library.",
                );
            }
        }

        if model_ready && runtime.is_ready() {
            ui.weak(model_message);
        }
    }

    fn ui_files(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Add files…").clicked() {
                if let Some(files) = rfd::FileDialog::new()
                    .add_filter(
                        "Documents",
                        &["pdf", "png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp"],
                    )
                    .pick_files()
                {
                    self.add_paths(files);
                }
            }
            if ui.button("Add folder…").clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.add_paths([folder]);
                    self.recursive = true;
                }
            }
            if ui.button("Clear").clicked() {
                self.inputs.clear();
            }
            ui.label(format!("{} input(s)", self.inputs.len()));
        });

        egui::ScrollArea::vertical()
            .id_salt("inputs")
            .max_height(130.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let mut remove = None;
                for (index, path) in self.inputs.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("✕").clicked() {
                            remove = Some(index);
                        }
                        ui.label(path.display().to_string());
                    });
                }
                if let Some(index) = remove {
                    self.inputs.remove(index);
                }
            });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        egui::Grid::new("settings")
            .num_columns(4)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Output");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(420.0));
                    if ui.button("Choose…").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.output_dir = folder.display().to_string();
                        }
                    }
                });
                ui.end_row();

                ui.label("Format");
                egui::ComboBox::from_id_salt("format")
                    .selected_text(match self.format {
                        OutputFormat::Md => "Markdown",
                        OutputFormat::Html => "HTML",
                        OutputFormat::Both => "Markdown + HTML",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.format,
                            OutputFormat::Both,
                            "Markdown + HTML",
                        );
                        ui.selectable_value(&mut self.format, OutputFormat::Md, "Markdown");
                        ui.selectable_value(&mut self.format, OutputFormat::Html, "HTML");
                    });
                ui.label("Device");
                egui::ComboBox::from_id_salt("device")
                    .selected_text(match self.device {
                        Device::Auto => "Auto (GPU → CPU)",
                        Device::Gpu => "GPU",
                        Device::Cpu => "CPU (slow)",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.device, Device::Auto, "Auto (GPU → CPU)");
                        ui.selectable_value(&mut self.device, Device::Gpu, "GPU");
                        ui.selectable_value(&mut self.device, Device::Cpu, "CPU (slow)");
                    });
                ui.end_row();

                ui.label("DPI");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.dpi)
                            .range(72..=600)
                            .speed(8.0),
                    );
                    ui.label("Pages (PDF)");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.pages)
                            .hint_text("all, e.g. 1-5,7")
                            .desired_width(130.0),
                    );
                });
                ui.label("Options");
                ui.checkbox(&mut self.recursive, "Recurse into subfolders");
                ui.end_row();

                ui.label("PDF password");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                        .hint_text("optional; never logged")
                        .desired_width(240.0),
                );
                ui.end_row();
            });

        // Open automatically while setup is incomplete, so the paths are one
        // click away on the first run.
        let advanced_open = !self.setup_complete();
        egui::CollapsingHeader::new("Advanced paths")
            .default_open(advanced_open)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Model dir");
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.model_dir)
                                .hint_text("auto: OCR_MODEL_DIR or app-data")
                                .desired_width(430.0),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui.button("…").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.model_dir = folder.display().to_string();
                            changed = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("llama dir");
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.llama_dir)
                                .hint_text("auto: OCR_LLAMA_SERVER or runtime/")
                                .desired_width(430.0),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui.button("…").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.llama_dir = folder.display().to_string();
                            changed = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("PDFium");
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.pdfium)
                                .hint_text("auto: OCR_PDFIUM, next to the exe, runtime/")
                                .desired_width(430.0),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui.button("…").clicked() {
                        if let Some(file) = rfd::FileDialog::new()
                            .add_filter("PDFium library", &["dll", "so", "dylib"])
                            .pick_file()
                        {
                            self.pdfium = file.display().to_string();
                            changed = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Runtime dir");
                    let shown = if self.runtime_dir.trim().is_empty() {
                        "(auto: runtime/ next to the exe)".to_string()
                    } else {
                        self.runtime_dir.clone()
                    };
                    ui.label(shown);
                    if ui.button("Choose runtime…").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.apply_runtime_dir(folder);
                        }
                    }
                });
                ui.label(format!(
                    "Model files: {}",
                    self.model_status.dir().display()
                ));
                ui.label(format!(
                    "PDFium: {} (source: {})",
                    self.runtime
                        .pdfium
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|error| format!("not found ({error})")),
                    self.runtime.pdfium_source.label()
                ));
                ui.label(format!(
                    "llama-server: {} candidate(s); searched {} location(s)",
                    self.runtime.llama.len(),
                    self.runtime.llama_searched.len()
                ));
                ui.label(
                    "The model and llama-server are not bundled. Download model stores the \
                     weights in the app-data model folder (or Model dir). The release package \
                     ships runtime/ next to the exe.",
                );
            });

        if changed {
            self.refresh_setup();
        }
    }

    fn ui_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let recognize = ui.add_enabled(
                !self.running && !self.downloading && !self.inputs.is_empty(),
                egui::Button::new(egui::RichText::new("Recognize").strong())
                    .min_size(egui::vec2(150.0, 32.0)),
            );
            if recognize.clicked() {
                self.start();
            }
            if self.running {
                if ui
                    .add(egui::Button::new("Cancel").min_size(egui::vec2(100.0, 32.0)))
                    .clicked()
                {
                    self.cancel.store(true, Ordering::SeqCst);
                    self.cancelling = true;
                    self.status = "Cancelling...".to_string();
                }
                ui.spinner();
            }
            if self.downloading {
                ui.spinner();
                ui.label("Downloading model...");
            }
            if ui
                .add_enabled(
                    !self.running,
                    egui::Button::new("Open output folder").min_size(egui::vec2(160.0, 32.0)),
                )
                .clicked()
            {
                let path = self
                    .last_report
                    .as_ref()
                    .map(|report| PathBuf::from(&report.output_dir))
                    .unwrap_or_else(|| PathBuf::from(self.output_dir.trim()));
                let target = if path.exists() {
                    path
                } else {
                    path.parent().map(PathBuf::from).unwrap_or(path)
                };
                if let Err(error) = open::that(&target) {
                    self.log(format!("cannot open {}: {error}", target.display()), true);
                }
            }
            if self.cancelling {
                ui.label("Waiting for the current request to stop...");
            }
        });

        ui.add(
            egui::ProgressBar::new(self.progress)
                .show_percentage()
                .animate(self.running || self.downloading),
        );
        ui.label(&self.status);
    }

    fn ui_log(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Log");
            if ui.small_button("Copy").clicked() {
                let text = self
                    .logs
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.ctx().copy_text(text);
            }
            if ui.small_button("Clear").clicked() {
                self.logs.clear();
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("log")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.logs.is_empty() {
                    ui.weak("Events and errors appear here.");
                }
                for line in &self.logs {
                    let mut text = egui::RichText::new(&line.text).monospace();
                    if line.error {
                        text = text.color(egui::Color32::from_rgb(220, 90, 90));
                    }
                    ui.label(text);
                }
            });
    }
}

impl eframe::App for OcrApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.maybe_refresh_setup();

        // Accept files dropped onto the window.
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_paths(dropped);
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.heading("ocr-tool");
            ui.label("Local OCR for PDF and images · Chandra OCR 2 · llama.cpp");
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_setup(ui);
            ui.add_space(6.0);
            ui.heading("Inputs");
            self.ui_files(ui);
            ui.add_space(6.0);
            ui.heading("Settings");
            self.ui_settings(ui);
            ui.add_space(8.0);
            self.ui_actions(ui);
            if let Some(report) = &self.last_report {
                ui.add_space(4.0);
                ui.label(format!(
                    "{} page(s) written to {}",
                    report.pages_ok, report.output_dir
                ));
            }
            self.ui_log(ui);
        });

        if self.running || self.downloading {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

impl Drop for OcrApp {
    /// Closing the window must stop the sidecar before the process exits. The
    /// Windows job object would kill it anyway; this also covers a normal exit
    /// where the worker is waiting on a long HTTP request. A running model
    /// download is cancelled and its `.part` file is left for resume.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.download_cancel.store(true, Ordering::SeqCst);
        if let Some(rx) = self.rx.take() {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                match rx.recv_timeout(left) {
                    Ok(Msg::Done(_)) | Ok(Msg::DownloadDone(_)) => break,
                    Ok(_) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                }
            }
        }
    }
}

fn non_empty_path(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

fn is_pdf_input(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

/// A model download that stopped because the existing canonical file has the
/// wrong size or SHA-256 requires the GUI Replace path, not only `--force`
/// (which is a CLI flag the GUI user cannot type).
fn download_needs_replace(error: &str) -> bool {
    error.contains("--force")
}

/// Status-bar text for a failed model download.
fn download_failure_status(error: &str, cancelled: bool) -> String {
    if cancelled {
        "Download cancelled; the .part file is kept and the next attempt resumes".to_string()
    } else if download_needs_replace(error) {
        "Model files are incomplete or different; use Replace / Re-download to fetch the pinned files again."
            .to_string()
    } else {
        "Model download failed. See the log.".to_string()
    }
}

/// Map a full error chain to a short status-bar sentence. The raw chain (for
/// example `os error 3`) goes to the log only; the main status must stay
/// actionable on the first run.
fn friendly_failure(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    if lower.contains("model files not found")
        || (lower.contains("model") && lower.contains("incomplete"))
    {
        "Model files not found or incomplete. Use Download model or choose a folder.".to_string()
    } else if lower.contains("pdfium not found") {
        "PDFium not found; choose the runtime folder or set OCR_PDFIUM.".to_string()
    } else if lower.contains("llama-server") && lower.contains("not found") {
        "Runtime not found: llama-server was not found; choose the runtime folder.".to_string()
    } else {
        "Failed. See the log for details.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocr_app::runtime_pdfium_candidate;

    #[test]
    fn done_guard_reports_a_panicking_worker() {
        let (tx, rx) = std::sync::mpsc::channel();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = DoneGuard::new(tx.clone());
            panic!("boom");
        }));
        assert!(result.is_err());
        match rx.try_recv() {
            Ok(Msg::Done(Err(message))) => assert!(message.contains("panicked"), "{message}"),
            Ok(Msg::Done(Ok(_))) => panic!("unexpected success"),
            Ok(Msg::Event(_)) | Ok(Msg::Download(_)) | Ok(Msg::DownloadDone(_)) => {
                panic!("unexpected message")
            }
            Err(error) => panic!("no Done message: {error}"),
        }
    }

    #[test]
    fn done_guard_does_not_duplicate_done() {
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut guard = DoneGuard::new(tx);
            guard.finish(Err("real error".to_string()));
        }
        assert!(matches!(rx.try_recv(), Ok(Msg::Done(Err(_)))));
        assert!(rx.try_recv().is_err(), "exactly one Done message");
    }

    #[test]
    fn download_guard_reports_a_panicking_worker() {
        let (tx, rx) = std::sync::mpsc::channel();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = DownloadGuard::new(tx.clone());
            panic!("boom");
        }));
        assert!(result.is_err());
        match rx.try_recv() {
            Ok(Msg::DownloadDone(Err(message))) => {
                assert!(message.contains("panicked"), "{message}")
            }
            _ => panic!("no DownloadDone message"),
        }
    }

    #[test]
    fn raw_os_errors_never_reach_the_status_bar() {
        let raw = "Model files not found in C:\\Users\\x\\AppData\\Local\\ocr-tool\\models\\chandra-ocr-2; use Download model or choose a folder: cannot read model directory C:\\Users\\x\\AppData\\Local\\ocr-tool\\models\\chandra-ocr-2: The system cannot find the path specified. (os error 3)";
        let status = friendly_failure(raw);
        assert!(!status.contains("os error"), "{status}");
        assert!(status.contains("Model files not found"), "{status}");

        let runtime = friendly_failure(
            "llama-server executable not found (device auto).\nSearched: C:\\runtime",
        );
        assert!(runtime.contains("Runtime not found"), "{runtime}");

        let pdfium = friendly_failure(
            "PDFium not found; choose the runtime folder or set OCR_PDFIUM: no PDFium library",
        );
        assert!(pdfium.contains("PDFium not found"), "{pdfium}");

        assert_eq!(
            friendly_failure("something unexpected\nsecond line"),
            "Failed. See the log for details."
        );
    }

    #[test]
    fn runtime_folder_selection_fills_llama_and_pdfium() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = tmp.path().join("windows-x64");
        let pdfium = runtime_pdfium_candidate(&runtime);
        std::fs::create_dir_all(pdfium.parent().unwrap()).unwrap();
        std::fs::write(&pdfium, b"fake").unwrap();
        let llama = runtime.join("llama-cpu");
        std::fs::create_dir_all(&llama).unwrap();
        let server = if cfg!(windows) {
            "llama-server.exe"
        } else {
            "llama-server"
        };
        std::fs::write(llama.join(server), b"fake").unwrap();

        let mut app = OcrApp::new();
        app.apply_runtime_dir(runtime.clone());
        assert_eq!(app.llama_dir, runtime.display().to_string());
        assert_eq!(app.pdfium, pdfium.display().to_string());
        assert!(app.runtime.pdfium_ready());
        assert!(
            app.runtime
                .llama
                .iter()
                .any(|candidate| candidate.exe.ends_with(Path::new("llama-cpu").join(server))),
            "candidates: {:?}",
            app.runtime.llama
        );
    }

    #[test]
    fn setup_complete_requires_pdfium_not_only_llama() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = tmp.path().join("runtime");
        let llama = runtime.join("llama-cpu");
        std::fs::create_dir_all(&llama).unwrap();
        let server = if cfg!(windows) {
            "llama-server.exe"
        } else {
            "llama-server"
        };
        std::fs::write(llama.join(server), b"fake").unwrap();

        let mut app = OcrApp::new();
        app.model_status = ModelStatus::Ready {
            dir: tmp.path().to_path_buf(),
            paths: ocr_core::ModelPaths {
                dir: tmp.path().to_path_buf(),
                model: tmp.path().join("model.gguf"),
                mmproj: tmp.path().join("mmproj.gguf"),
            },
            model_bytes: 0,
            mmproj_bytes: 0,
        };

        app.runtime = runtime_status(Some(&runtime), Some(&runtime.join("absent-pdfium")));
        assert!(app.runtime.llama_ready());
        assert!(
            !app.setup_complete(),
            "llama-server alone must not mark the setup complete without PDFium"
        );

        let pdfium = runtime_pdfium_candidate(&runtime);
        std::fs::create_dir_all(pdfium.parent().unwrap()).unwrap();
        std::fs::write(&pdfium, b"fake").unwrap();
        app.runtime = runtime_status(Some(&runtime), Some(&pdfium));
        assert!(app.runtime.pdfium_ready());
        assert_eq!(app.runtime.pdfium_source, ocr_app::PdfiumSource::Explicit);
        assert!(app.setup_complete());
    }

    #[test]
    fn force_required_download_error_points_at_the_replace_button() {
        let raw = "found C:\\models\\chandra-q4.gguf (1024 bytes) but the pinned \
                   chandra-ocr-2.Q4_K_M.gguf is 3066385440 bytes; pass --force to download \
                   the pinned file into C:\\models";
        assert!(download_needs_replace(raw));
        let status = download_failure_status(raw, false);
        assert!(status.contains("Replace / Re-download"), "{status}");
        assert!(!status.contains("--force"), "{status}");
        assert!(download_failure_status("download cancelled", true).contains("cancelled"));
        assert_eq!(
            download_failure_status("network down", false),
            "Model download failed. See the log."
        );
    }

    #[test]
    fn pdf_inputs_are_detected_for_the_pdfium_guard() {
        assert!(is_pdf_input(Path::new("a.PDF")));
        assert!(!is_pdf_input(Path::new("a.png")));
    }
}
