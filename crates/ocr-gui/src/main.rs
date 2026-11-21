//! Desktop GUI for `ocr-tool`.
//!
//! Release binaries use the Windows GUI subsystem so a double-click opens only
//! the app window, without a black console. Debug builds keep the console for
//! logs; diagnostics are always appended to `gui.log` (see [`gui_log`]).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod demo;
mod gui_log;
mod prefs;
mod ui;

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ocr_app::download::{human_bytes, DownloadProgress, REPO_URL};
use ocr_app::runtime::{download_runtime, RuntimeComponent, RuntimeInstall, RuntimeProgress};
use ocr_app::setup::{
    model_status, pdfium_for_runtime, runtime_status, ModelStatus, PdfiumSource, RuntimeStatus,
};
use ocr_app::{download_models_cancellable, run_job, JobOptions, JobReport};
use ocr_core::{
    default_runtime_dir, resolve_model_dir, BlockExtraction, Device, LogLevel, OutputFormat,
    ProgressEvent, MODEL_ENV,
};
use ocr_engine::{DEFAULT_IMAGE_MAX_TOKENS, DEFAULT_MAX_TOKENS};
use ocr_pdf::DEFAULT_MAX_PIXELS;

fn main() -> eframe::Result<()> {
    let log_path = gui_log::init();
    gui_log::install_panic_hook();
    gui_log::write(format!(
        "ocr-tool-gui {} starting (pid {}, log {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        log_path.display()
    ));

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1180.0, 840.0])
        .with_min_inner_size([1000.0, 720.0])
        .with_title("ocr-tool — Распознавание документов");
    // The window/taskbar icon is separate from the icon embedded into the exe.
    match eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon-256.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(error) => gui_log::write(format!("warning: cannot load the window icon: {error}")),
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        "ocr-tool",
        options,
        Box::new(|cc| Ok(Box::new(OcrApp::new_with_context(cc)))),
    );
    match &result {
        Ok(()) => gui_log::write("ocr-tool-gui exited normally"),
        Err(error) => {
            gui_log::write(format!("fatal: cannot start the GUI: {error:#}"));
            show_startup_error(&format!("cannot start the GUI: {error:#}"), &log_path);
        }
    }
    result
}

/// A release GUI has no console, so a fatal error before the window exists is
/// reported twice: the file log keeps the full error/backtrace, and a dialog
/// names the log path.
fn show_startup_error(error: &str, log_path: &Path) {
    let description = format!(
        "Не удалось запустить окно.\n\n{error}\n\nПолный журнал: {}",
        log_path.display()
    );
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("ocr-tool — не удалось запустить")
        .set_description(description)
        .show();
}

struct LogLine {
    text: String,
    error: bool,
}

enum Msg {
    Event(ProgressEvent),
    Done(Result<JobReport, String>),
    Download(DownloadProgress),
    DownloadDone(Result<Vec<PathBuf>, String>),
    Runtime(RuntimeProgress),
    RuntimeDone(Result<RuntimeInstall, String>),
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

/// Same safety net for the runtime download thread.
struct RuntimeGuard {
    tx: Sender<Msg>,
    sent: bool,
}

impl RuntimeGuard {
    fn new(tx: Sender<Msg>) -> Self {
        Self { tx, sent: false }
    }

    fn finish(&mut self, result: Result<RuntimeInstall, String>) {
        let _ = self.tx.send(Msg::RuntimeDone(result));
        self.sent = true;
    }
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        if !self.sent {
            let _ = self.tx.send(Msg::RuntimeDone(Err(
                "runtime download thread panicked before reporting a result".to_string(),
            )));
        }
    }
}

/// What the user can do after an error. Kept separate from the message so the
/// error card can offer the right button instead of a generic "try again".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FailureKind {
    Model,
    Runtime,
    Pdfium,
    File,
    Output,
    Password,
    Pages,
    Timeout,
    #[default]
    Unknown,
}

/// What the bottom status bar should say, derived from the app state instead of
/// a stale sentence left over from the previous action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusPhase {
    Empty,
    Idle(usize),
    Running,
    /// A download failure/cancellation notice that must survive until the next
    /// action (the detailed cause is in the setup card and log).
    Notice,
    Result {
        files: usize,
        pages: usize,
        cancelled: bool,
    },
    Error,
}

struct OcrApp {
    inputs: Vec<PathBuf>,
    output_dir: String,
    format: OutputFormat,
    extract_blocks: BlockExtraction,
    device: Device,
    dpi: u32,
    pages: String,
    password: String,
    recursive: bool,
    model_dir: String,
    llama_dir: String,
    pdfium: String,
    runtime_dir: String,
    max_pixels: u64,
    max_tokens: u32,
    /// Fast/quality profile: 2048 or 4096 vision tokens per image.
    image_max_tokens: u32,
    /// PDF pages recognized at the same time (1 or 2).
    parallel_pages: u32,
    timeout_secs: u64,

    running: bool,
    cancelling: bool,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<Msg>>,
    progress: f32,
    job_progress: f32,
    status: String,
    logs: Vec<LogLine>,
    last_report: Option<JobReport>,
    last_error: Option<String>,
    last_error_kind: FailureKind,
    download_notice: bool,

    job_started: Option<Instant>,
    job_files_total: usize,
    job_file_index: usize,
    current_file: Option<String>,
    page: u32,
    page_total: u32,
    page_selected_index: u32,
    page_selected_total: u32,
    pages_done: u32,
    page_seconds_total: f64,
    /// Speed settings of the running job (settings can change between jobs).
    job_image_max_tokens: u32,
    job_parallel_pages: u32,

    model_status: ModelStatus,
    runtime: RuntimeStatus,
    setup_key: (String, String, String, String),

    downloading: bool,
    runtime_downloading: bool,
    download_cancel: Arc<AtomicBool>,
    runtime_cancel: Arc<AtomicBool>,
    download_text: String,
    runtime_text: String,
    download_last_percent: Option<u32>,
    runtime_last_percent: Option<u32>,
    download_file_index: usize,
    download_file_total: usize,
    download_started: Option<Instant>,
    runtime_started: Option<Instant>,
    runtime_component: Option<RuntimeComponent>,
    confirm_replace: bool,
    confirm_runtime_replace: bool,

    show_help: bool,
    show_advanced: bool,
    show_password: bool,
    show_details: bool,
    scroll_log: bool,
    fonts_installed: bool,
    forced_theme: Option<egui::Theme>,
    shot: Option<demo::Shot>,
    started_at: Instant,
}

impl OcrApp {
    /// Install the embedded fonts before the first egui pass: `set_fonts` only
    /// becomes active at the start of the next pass, so doing it inside `draw`
    /// would panic on the first pixel label.
    fn new_with_context(cc: &eframe::CreationContext<'_>) -> Self {
        ui::install_fonts(&cc.egui_ctx);
        ui::apply_style(&cc.egui_ctx);
        let mut app = Self::new();
        app.fonts_installed = true;
        app
    }

    fn new() -> Self {
        let prefs = prefs::load();
        let demo = demo::Demo::from_env();
        let run_demo = demo.run;
        let demo_state = demo.state.clone();
        let output = demo
            .output_dir
            .clone()
            .or_else(|| prefs.output_dir().map(PathBuf::from))
            .unwrap_or_else(prefs::default_output_dir);
        let format = prefs
            .format
            .as_deref()
            .and_then(|value| OutputFormat::from_str(value).ok())
            .unwrap_or(OutputFormat::Both);
        let extract_blocks = prefs.extract_blocks();
        let device = demo
            .device
            .or_else(|| {
                prefs
                    .device
                    .as_deref()
                    .and_then(|value| Device::from_str(value).ok())
            })
            .unwrap_or(Device::Auto);
        let dpi = prefs.dpi.unwrap_or(192).clamp(72, 600);
        let image_max_tokens = prefs.image_max_tokens(DEFAULT_IMAGE_MAX_TOKENS);
        let parallel_pages = prefs.parallel_pages();

        let mut app = Self {
            inputs: demo.inputs,
            output_dir: output.display().to_string(),
            format,
            extract_blocks,
            device,
            dpi,
            pages: demo.pages.unwrap_or_default(),
            password: String::new(),
            recursive: false,
            model_dir: String::new(),
            llama_dir: String::new(),
            pdfium: String::new(),
            runtime_dir: String::new(),
            max_pixels: DEFAULT_MAX_PIXELS,
            max_tokens: DEFAULT_MAX_TOKENS,
            image_max_tokens,
            parallel_pages,
            timeout_secs: 600,

            running: false,
            cancelling: false,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            progress: 0.0,
            job_progress: 0.0,
            status: "Добавьте файлы — начнём с шага 1.".to_string(),
            logs: Vec::new(),
            last_report: None,
            last_error: None,
            last_error_kind: FailureKind::Unknown,
            download_notice: false,

            job_started: None,
            job_files_total: 0,
            job_file_index: 0,
            current_file: None,
            page: 0,
            page_total: 0,
            page_selected_index: 0,
            page_selected_total: 0,
            pages_done: 0,
            page_seconds_total: 0.0,
            job_image_max_tokens: DEFAULT_IMAGE_MAX_TOKENS,
            job_parallel_pages: 1,

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
            runtime_downloading: false,
            download_cancel: Arc::new(AtomicBool::new(false)),
            runtime_cancel: Arc::new(AtomicBool::new(false)),
            download_text: String::new(),
            runtime_text: String::new(),
            download_last_percent: None,
            runtime_last_percent: None,
            download_file_index: 0,
            download_file_total: 0,
            download_started: None,
            runtime_started: None,
            runtime_component: None,
            confirm_replace: false,
            confirm_runtime_replace: false,

            show_help: demo.show_help,
            show_advanced: demo.advanced,
            show_password: false,
            show_details: false,
            scroll_log: false,
            fonts_installed: false,
            forced_theme: demo.theme,
            shot: demo.shot,
            started_at: Instant::now(),
        };
        app.refresh_setup();
        app.apply_demo_state(demo_state.as_deref());
        // Screenshot harness only: pre-fill state and optionally press the main
        // button once. The flags are inert when the variables are absent.
        if run_demo && app.can_start() {
            app.start();
        }
        app
    }

    /// Screenshot-only visual states. These do not start a job thread, open a
    /// socket or touch a document; they only populate the fields the result
    /// window reads.
    fn apply_demo_state(&mut self, state: Option<&str>) {
        match state {
            Some("done") => {
                let output = self.output_dir.clone();
                let files = self.inputs.len().max(1);
                self.last_report = Some(JobReport {
                    output_dir: output.clone(),
                    documents_ok: files,
                    documents_failed: 0,
                    pages_ok: 12,
                    pages_failed: 0,
                    cancelled: false,
                    elapsed_ms: 185_000,
                    device: "gpu".to_string(),
                    model: "local".to_string(),
                    llama_server: "llama-server".to_string(),
                    outputs: vec![
                        format!("{output}/invoice.md"),
                        format!("{output}/invoice.html"),
                        format!("{output}/invoice.json"),
                    ],
                    documents: Vec::new(),
                });
                self.status = format!("Готово: {} файлов, 12 страниц.", files);
            }
            Some("running") => {
                self.running = true;
                self.job_started = Some(Instant::now());
                self.job_files_total = self.inputs.len().max(1);
                self.job_file_index = 1;
                self.current_file = self
                    .inputs
                    .first()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned());
                self.page = 2;
                self.page_total = 5;
                self.page_selected_index = 2;
                self.page_selected_total = 5;
                self.pages_done = 1;
                self.page_seconds_total = 18.0;
                self.job_progress = 0.4;
                self.status = "Страница 2 из 5: распознаём…".to_string();
            }
            Some("error") => {
                self.last_error = Some(
                    "Не удалось прочитать файл. Он не изменён. Выберите другой файл.".to_string(),
                );
                self.last_error_kind = FailureKind::File;
                self.status = self.last_error.clone().unwrap_or_default();
            }
            _ => {}
        }
    }

    fn log(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        // The window log is not available before a crash; mirror everything
        // (especially preflight/download errors) into the file log.
        gui_log::write(&text);
        self.logs.push(LogLine { text, error });
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
        self.last_error = None;
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
        let llama = self.resolved_llama_dir();
        let pdfium = self.resolved_pdfium();
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

    /// Folder the next Download runtime goes to: the edited field or the
    /// platform app-data folder. Never a hidden path.
    fn target_runtime_dir(&self) -> PathBuf {
        non_empty_path(&self.runtime_dir).unwrap_or_else(default_runtime_dir)
    }

    /// The runtime folder participates in discovery even before the explicit
    /// llama dir / PDFium fields are filled (the user may have typed it).
    fn resolved_llama_dir(&self) -> Option<PathBuf> {
        non_empty_path(&self.llama_dir).or_else(|| non_empty_path(&self.runtime_dir))
    }

    fn resolved_pdfium(&self) -> Option<PathBuf> {
        non_empty_path(&self.pdfium).or_else(|| non_empty_path(&self.runtime_dir))
    }

    fn setup_complete(&self) -> bool {
        // PDFium is part of the setup: a runtime with llama-server but no
        // PDFium still fails on every PDF, so keep the paths (and the PDFium
        // panel) visible instead of collapsing Advanced.
        self.model_status.is_ready() && self.runtime.is_ready()
    }

    /// A setup error card disappears as soon as the missing piece is found, so
    /// the user is not left with a stale "not found" next to a ready chip.
    fn clear_setup_error_if_fixed(&mut self) {
        let fixed = match self.last_error_kind {
            FailureKind::Model => self.model_status.is_ready(),
            FailureKind::Runtime | FailureKind::Pdfium => self.runtime.is_ready(),
            _ => false,
        };
        if fixed {
            self.last_error = None;
            self.last_error_kind = FailureKind::Unknown;
            self.download_notice = false;
        }
    }

    /// Why the main button cannot be pressed right now. `None` means ready.
    fn start_blocker(&self) -> Option<&'static str> {
        if self.running {
            Some("Распознавание уже идёт.")
        } else if self.downloading || self.runtime_downloading {
            Some("Дождитесь окончания загрузки.")
        } else if self.inputs.is_empty() {
            Some("Сначала добавьте файлы.")
        } else if !self.setup_complete() {
            Some("Сначала подготовьте инструменты в карточке выше.")
        } else if self.output_dir.trim().is_empty() {
            Some("Выберите папку для сохранения.")
        } else {
            None
        }
    }

    fn can_start(&self) -> bool {
        self.start_blocker().is_none()
    }

    /// Derive the status-bar phase from high-level state. The stored `status`
    /// is only used as the detail for running/error/notice phases; an idle list
    /// always gets the actionable file hint instead of a leftover sentence.
    fn status_phase(&self) -> StatusPhase {
        if self.running || self.downloading || self.runtime_downloading {
            StatusPhase::Running
        } else if self.last_error.is_some() {
            StatusPhase::Error
        } else if self.download_notice {
            StatusPhase::Notice
        } else if let Some(report) = &self.last_report {
            StatusPhase::Result {
                files: report.documents_ok,
                pages: report.pages_ok,
                cancelled: report.cancelled,
            }
        } else if self.inputs.is_empty() {
            StatusPhase::Empty
        } else {
            StatusPhase::Idle(self.inputs.len())
        }
    }

    pub(crate) fn is_cpu_mode(&self) -> bool {
        match self.device {
            Device::Cpu => true,
            Device::Gpu => false,
            Device::Auto => !self
                .runtime
                .llama
                .iter()
                .any(|candidate| candidate.device == Device::Gpu),
        }
    }

    fn start(&mut self) {
        if self.running || self.downloading || self.runtime_downloading || self.inputs.is_empty() {
            return;
        }
        self.maybe_refresh_setup();
        if let Some(blocker) = self.start_blocker() {
            self.status = blocker.to_string();
            return;
        }
        let output_dir = PathBuf::from(self.output_dir.trim());
        if output_dir.as_os_str().is_empty() {
            self.status = "Выберите папку для сохранения.".to_string();
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
            max_pixels: self.max_pixels,
            max_tokens: self.max_tokens,
            image_max_tokens: self.image_max_tokens,
            parallel_pages: self.parallel_pages,
            request_timeout_secs: self.timeout_secs,
            extract_blocks: self.extract_blocks,
        };

        self.running = true;
        self.cancelling = false;
        self.progress = 0.0;
        self.job_progress = 0.0;
        self.last_report = None;
        self.last_error = None;
        self.last_error_kind = FailureKind::Unknown;
        self.download_notice = false;
        self.logs.clear();
        self.status = "Готовим документы…".to_string();
        self.job_started = Some(Instant::now());
        self.job_files_total = self.inputs.len();
        self.job_file_index = 0;
        self.current_file = None;
        self.page = 0;
        self.page_total = 0;
        self.page_selected_index = 0;
        self.page_selected_total = 0;
        self.pages_done = 0;
        self.page_seconds_total = 0.0;
        self.job_image_max_tokens = self.image_max_tokens;
        self.job_parallel_pages = self.parallel_pages.clamp(1, 2);
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
        if self.downloading || self.runtime_downloading || self.running {
            return;
        }
        self.maybe_refresh_setup();
        let dir = self.resolved_model_dir();
        if let Err(error) = std::fs::create_dir_all(&dir) {
            self.status = "Не могу создать папку для модели. Проверьте права доступа.".to_string();
            self.log(
                format!("error: cannot create {}: {error}", dir.display()),
                true,
            );
            return;
        }
        self.downloading = true;
        self.confirm_replace = false;
        self.progress = 0.0;
        self.download_text = "Готовим загрузку…".to_string();
        self.download_last_percent = None;
        self.download_file_index = 0;
        self.download_file_total = 0;
        self.download_started = None;
        self.last_error = None;
        self.download_notice = false;
        self.status = "Скачиваем модель…".to_string();
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
        self.clear_setup_error_if_fixed();
        if self.runtime.is_ready() {
            self.download_notice = false;
        }
        let status = runtime_message_ru(&self.runtime);
        self.log(
            format!("runtime folder: {} — {status}", folder.display()),
            !self.runtime.is_ready(),
        );
        self.status = status;
    }

    /// Called when the Runtime folder text field changes. The folder drives
    /// both llama-server discovery and the PDFium lookup until the user
    /// overrides them with the advanced fields.
    fn apply_runtime_dir_text(&mut self) {
        let trimmed = self.runtime_dir.trim().to_string();
        if trimmed.is_empty() {
            self.llama_dir.clear();
            self.pdfium.clear();
        } else {
            let folder = PathBuf::from(&trimmed);
            self.llama_dir = trimmed;
            self.pdfium = pdfium_for_runtime(&folder)
                .map(|path| path.display().to_string())
                .unwrap_or_default();
        }
        self.refresh_setup();
        self.clear_setup_error_if_fixed();
        if self.runtime.is_ready() {
            self.download_notice = false;
        }
    }

    /// Download llama-vulkan + llama-cpu + PDFium into the runtime folder.
    /// The model download and the runtime download are independent and never
    /// run at the same time.
    fn start_runtime_download(&mut self, force: bool) {
        if self.downloading || self.runtime_downloading || self.running {
            return;
        }
        self.maybe_refresh_setup();
        let dir = self.target_runtime_dir();
        if let Err(error) = std::fs::create_dir_all(&dir) {
            self.status = "Не могу создать папку для движка. Проверьте права доступа.".to_string();
            self.log(
                format!("error: cannot create {}: {error}", dir.display()),
                true,
            );
            return;
        }
        self.runtime_downloading = true;
        self.confirm_runtime_replace = false;
        self.progress = 0.0;
        self.runtime_text = "Готовим загрузку…".to_string();
        self.runtime_last_percent = None;
        self.runtime_started = None;
        self.runtime_component = None;
        self.last_error = None;
        self.download_notice = false;
        self.status = "Скачиваем движок…".to_string();
        self.log(
            format!(
                "runtime download{}: llama.cpp b12535 + pdfium 8165 -> {}",
                if force { " (replace)" } else { "" },
                dir.display()
            ),
            false,
        );
        let cancel = Arc::new(AtomicBool::new(false));
        self.runtime_cancel = cancel.clone();

        let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let mut guard = RuntimeGuard::new(tx.clone());
            let mut emit = |progress: RuntimeProgress| {
                let _ = tx.send(Msg::Runtime(progress));
            };
            let result = download_runtime(&dir, force, &cancel, &mut emit)
                .map_err(|error| format!("{error:#}"));
            guard.finish(result);
        });
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
                            self.last_error = Some(
                                "Работа неожиданно остановилась. Документы не изменены. \
                                 Подробности — в журнале."
                                    .to_string(),
                            );
                            self.last_error_kind = FailureKind::Unknown;
                            self.status =
                                "Работа неожиданно остановилась. Подробности — в журнале."
                                    .to_string();
                            self.log(
                                "error: worker thread exited without reporting a result"
                                    .to_string(),
                                true,
                            );
                        }
                        if self.downloading {
                            self.downloading = false;
                            self.status =
                                "Загрузка модели остановилась. Проверьте интернет и повторите."
                                    .to_string();
                            self.log(
                                "error: download thread exited without reporting a result"
                                    .to_string(),
                                true,
                            );
                        }
                        if self.runtime_downloading {
                            self.runtime_downloading = false;
                            self.status =
                                "Загрузка движка остановилась. Проверьте интернет и повторите."
                                    .to_string();
                            self.log(
                                "error: runtime download thread exited without reporting a result"
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
                    self.job_progress = 1.0;
                    match result {
                        Ok(report) => {
                            let partial = report.documents_failed > 0 || report.pages_failed > 0;
                            self.status = if report.cancelled {
                                "Остановлено. Уже готовые страницы сохранены.".to_string()
                            } else if partial {
                                format!(
                                    "Готово с замечаниями: {} сохранено, {} не удалось. \
                                     Подробности — в журнале.",
                                    format_pages(report.pages_ok),
                                    format_pages(report.pages_failed)
                                )
                            } else {
                                format!(
                                    "Готово: {} {}, {} {}.",
                                    report.documents_ok,
                                    ui::plural_ru(report.documents_ok, "файл", "файла", "файлов"),
                                    report.pages_ok,
                                    ui::plural_ru(
                                        report.pages_ok,
                                        "страница",
                                        "страницы",
                                        "страниц"
                                    ),
                                )
                            };
                            if partial {
                                let status = self.status.clone();
                                self.log(status, true);
                            }
                            self.last_error = None;
                            self.last_error_kind = FailureKind::Unknown;
                            self.download_notice = false;
                            self.last_report = Some(report);
                        }
                        Err(error) => {
                            // Keep the raw chain in the log only; the status bar
                            // names the cause and never shows `os error 3`.
                            self.last_error_kind = classify_failure(&error);
                            self.last_error = Some(friendly_failure(&error));
                            self.status = self.last_error.clone().unwrap_or_default();
                            self.log(format!("error: {error}"), true);
                        }
                    }
                    self.rx = None;
                    return;
                }
                Msg::Download(progress) => self.handle_download_progress(progress),
                Msg::DownloadDone(result) => {
                    self.downloading = false;
                    self.download_started = None;
                    match result {
                        Ok(files) => {
                            self.progress = 1.0;
                            self.download_notice = false;
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
                                "Модель готова. Можно продолжать.".to_string()
                            } else {
                                "Файлы модели неполные. Нажмите «Заменить…», чтобы скачать заново."
                                    .to_string()
                            };
                            let error = !self.model_status.is_ready();
                            self.log(self.status.clone(), error);
                        }
                        Err(error) => {
                            let cancelled = error.to_ascii_lowercase().contains("cancel");
                            self.status = download_failure_status(&error, cancelled);
                            self.download_text = self.status.clone();
                            self.download_notice = true;
                            self.log(format!("error: {error}"), true);
                            if download_needs_replace(&error) {
                                self.log(
                                    "use the Replace / Re-download button to replace the existing model files",
                                    true,
                                );
                                self.confirm_replace = true;
                            }
                            self.refresh_setup();
                        }
                    }
                    self.rx = None;
                    return;
                }
                Msg::Runtime(progress) => self.handle_runtime_progress(progress),
                Msg::RuntimeDone(result) => {
                    self.runtime_downloading = false;
                    self.runtime_started = None;
                    self.runtime_component = None;
                    match result {
                        Ok(install) => {
                            self.progress = 1.0;
                            self.download_notice = false;
                            let target = install.dir.clone();
                            self.apply_runtime_dir(target.clone());
                            self.log(
                                format!("runtime gpu: {}", install.llama_gpu.display()),
                                false,
                            );
                            self.log(
                                format!("runtime cpu: {}", install.llama_cpu.display()),
                                false,
                            );
                            self.log(
                                format!("runtime pdfium: {}", install.pdfium.display()),
                                false,
                            );
                            self.status = if self.runtime.is_ready() {
                                "Движок и просмотр PDF готовы. Можно распознавать.".to_string()
                            } else {
                                runtime_message_ru(&self.runtime)
                            };
                            let error = !self.runtime.is_ready();
                            self.log(self.status.clone(), error);
                        }
                        Err(error) => {
                            let cancelled = error.to_ascii_lowercase().contains("cancel");
                            self.status = if cancelled {
                                "Загрузка движка остановлена. Часть файлов сохранена — при \
                                 следующей попытке продолжим."
                                    .to_string()
                            } else {
                                "Не удалось скачать движок. Проверьте подключение к интернету \
                                 и повторите."
                                    .to_string()
                            };
                            self.runtime_text = self.status.clone();
                            self.download_notice = true;
                            self.log(format!("error: {error}"), true);
                            self.refresh_setup();
                        }
                    }
                    self.rx = None;
                    return;
                }
            }
        }
    }

    fn handle_runtime_progress(&mut self, progress: RuntimeProgress) {
        if self.runtime_component != Some(progress.component) {
            self.runtime_component = Some(progress.component);
            self.runtime_started = Some(Instant::now());
        }
        let total = progress.total.max(1);
        let fraction = (progress.downloaded as f32 / total as f32).min(1.0);
        self.progress = fraction;
        let percent = (fraction * 100.0).round() as u32;
        let label = runtime_component_label(progress.component);
        let suffix = if progress.skipped {
            " • уже на месте"
        } else {
            ""
        };
        self.runtime_text = format!(
            "{label} • {}{suffix}",
            ui::progress_text(progress.downloaded, progress.total, self.runtime_started)
        );
        self.status = if progress.done {
            format!("{label}: готово.")
        } else {
            "Скачиваем движок…".to_string()
        };
        let bucket = percent / 10;
        if progress.done
            || progress.skipped
            || self.runtime_last_percent.map(|last| last / 10) != Some(bucket)
        {
            self.runtime_last_percent = Some(percent);
            self.log(self.runtime_text.clone(), false);
        }
    }

    fn handle_download_progress(&mut self, progress: DownloadProgress) {
        if progress.file_index != self.download_file_index {
            self.download_file_index = progress.file_index;
            self.download_file_total = progress.file_count;
            // A new shard starts a new ETA window; otherwise the previous
            // 3 GB would distort the estimate for the small projector file.
            self.download_started = Some(Instant::now());
        }
        let total = progress.total.unwrap_or(0);
        let fraction = if total > 0 {
            (progress.downloaded as f32 / total as f32).min(1.0)
        } else {
            0.0
        };
        self.progress = fraction;
        let percent = (fraction * 100.0).round() as u32;
        let mut text = ui::progress_text(progress.downloaded, total, self.download_started);
        if self.download_file_total > 1 {
            text = format!(
                "Файл {} из {} • {text}",
                progress.file_index, self.download_file_total
            );
        }
        if progress.skipped {
            text.push_str(" • уже на месте");
        }
        self.download_text = text;
        self.status = if progress.done {
            "Модель: файл готов.".to_string()
        } else {
            "Скачиваем модель…".to_string()
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
                self.job_files_total = files;
                self.job_file_index = 0;
                self.job_progress = 0.0;
                self.status = "Готовим документы…".to_string();
                self.log(format!("job: {files} file(s) -> {output_dir}"), false);
            }
            ProgressEvent::FileStarted { index, total, path } => {
                self.job_file_index = index;
                self.job_files_total = total;
                self.current_file = Some(file_name_of(&path));
                self.page = 0;
                self.page_total = 0;
                self.page_selected_index = 0;
                self.page_selected_total = 0;
                self.job_progress = if total > 0 {
                    ((index.saturating_sub(1)) as f32 / total as f32).min(0.99)
                } else {
                    0.0
                };
                self.status = format!(
                    "Файл {index} из {total}: {}",
                    self.current_file.as_deref().unwrap_or(&path)
                );
                self.log(format!("[{index}/{total}] {path}"), false);
            }
            ProgressEvent::PageStarted {
                page,
                total_pages,
                selected_index,
                selected_total,
                ..
            } => {
                self.page = page;
                self.page_total = total_pages;
                self.page_selected_index = selected_index;
                self.page_selected_total = selected_total;
                self.status = if selected_total > 0 && selected_total != total_pages {
                    format!(
                        "Страница {page} из {total_pages} (выбрано {selected_index} из \
                         {selected_total}): распознаём…"
                    )
                } else {
                    format!("Страница {page} из {total_pages}: распознаём…")
                };
                let files = self.job_files_total.max(1) as f32;
                let file_position = self.job_file_index.saturating_sub(1) as f32;
                let within = if selected_total > 0 {
                    selected_index.saturating_sub(1) as f32 / selected_total as f32
                } else {
                    0.0
                };
                self.job_progress = ((file_position + within) / files).clamp(0.0, 0.99);
            }
            ProgressEvent::PageFinished {
                page,
                chars,
                elapsed,
                ..
            } => {
                self.pages_done = self.pages_done.saturating_add(1);
                self.page_seconds_total += elapsed.as_secs_f64();
                self.log(
                    format!(
                        "  page {page}: {chars} chars in {:.1}s",
                        elapsed.as_secs_f64()
                    ),
                    false,
                );
                let files = self.job_files_total.max(1) as f32;
                let file_position = self.job_file_index.saturating_sub(1) as f32;
                let within = if self.page_selected_total > 0 {
                    self.page_selected_index as f32 / self.page_selected_total as f32
                } else {
                    0.0
                };
                self.job_progress = ((file_position + within) / files).clamp(0.0, 0.99);
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
                let files = self.job_files_total.max(1) as f32;
                self.job_progress = (self.job_file_index as f32 / files).clamp(0.0, 0.99);
                if self.job_files_total > 1 {
                    self.status = format!(
                        "Файл {} из {} готов.",
                        self.job_file_index, self.job_files_total
                    );
                }
            }
            ProgressEvent::FileFailed { error, .. } => {
                self.log(format!("  FAILED: {error}"), true);
                self.status = "Один файл не удался, остальные продолжаются.".to_string();
            }
            ProgressEvent::Log { level, message } => {
                let error = matches!(level, LogLevel::Error);
                self.log(message, error);
            }
            ProgressEvent::JobFinished {
                documents_ok,
                documents_failed,
                elapsed,
                cancelled,
            } => {
                self.status = format!(
                    "Завершено за {:.1} с: {} {}, {} не удалось{}",
                    elapsed.as_secs_f64(),
                    documents_ok,
                    ui::plural_ru(documents_ok, "файл", "файла", "файлов"),
                    documents_failed,
                    if cancelled {
                        ", остановлено"
                    } else {
                        ""
                    }
                );
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (open, recognize, escape) = ctx.input_mut(|input| {
            (
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::O,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::Enter,
                )),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if open {
            if self.running || self.downloading || self.runtime_downloading {
                self.status =
                    "Дождитесь окончания текущего действия, затем добавьте файлы.".to_string();
            } else {
                self.pick_files();
            }
        }
        if recognize {
            if self.can_start() {
                self.start();
            } else if let Some(blocker) = self.start_blocker() {
                self.status = blocker.to_string();
            }
        }
        if escape {
            if self.running {
                self.cancel_job();
            } else if self.show_help {
                self.show_help = false;
            }
        }
    }

    fn update_shots(&mut self, ctx: &egui::Context) {
        let started = self.started_at;
        if let Some(shot) = self.shot.as_mut() {
            demo::drive_screenshot(ctx, shot, started);
        }
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
            let has_folder = dropped.iter().any(|path| path.is_dir());
            self.add_paths(dropped);
            if has_folder {
                self.recursive = true;
            }
            self.status = format!(
                "Добавлено в список: {}. Выберите папку и нажмите «Распознать».",
                self.inputs.len()
            );
        }

        self.handle_shortcuts(ctx);
        if let Some(theme) = self.forced_theme {
            ctx.set_theme(theme);
        }
        ui::draw(self, ctx);
        self.update_shots(ctx);

        if self.running || self.downloading || self.runtime_downloading || self.shot.is_some() {
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
        self.runtime_cancel.store(true, Ordering::SeqCst);
        if let Some(rx) = self.rx.take() {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                match rx.recv_timeout(left) {
                    Ok(Msg::Done(_)) | Ok(Msg::DownloadDone(_)) | Ok(Msg::RuntimeDone(_)) => break,
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

#[cfg(test)]
fn is_pdf_input(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn format_pages(pages: usize) -> String {
    format!(
        "{pages} {}",
        ui::plural_ru(pages, "страница", "страницы", "страниц")
    )
}

fn runtime_component_label(component: RuntimeComponent) -> &'static str {
    match component {
        RuntimeComponent::LlamaVulkan => "движок для видеокарты",
        RuntimeComponent::LlamaCpu => "движок для процессора",
        RuntimeComponent::LlamaMetal => "движок для Apple",
        RuntimeComponent::Pdfium => "просмотр PDF",
    }
}

fn runtime_message_ru(runtime: &RuntimeStatus) -> String {
    if !runtime.llama_ready() {
        "Не хватает движка распознавания. Скачайте его или укажите папку с движком.".to_string()
    } else if !runtime.pdfium_ready() {
        "Движок есть, но не хватает просмотрщика PDF. Скачайте движок или укажите папку."
            .to_string()
    } else {
        "Движок и просмотр PDF готовы.".to_string()
    }
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
        "Загрузка остановлена. Следующая попытка продолжится с места остановки.".to_string()
    } else if download_needs_replace(error) {
        "Файлы модели повреждены или отличаются. Нажмите «Заменить…», чтобы скачать заново."
            .to_string()
    } else {
        "Не удалось скачать модель. Проверьте подключение к интернету и повторите.".to_string()
    }
}

/// Map a full error chain to a user-facing cause and the action that fixes it.
fn classify_failure(error: &str) -> FailureKind {
    let lower = error.to_ascii_lowercase();
    if lower.contains("model files not found")
        || (lower.contains("model") && lower.contains("incomplete"))
        || (lower.contains("model directory") && lower.contains("cannot find"))
    {
        FailureKind::Model
    } else if lower.contains("pdfium") {
        FailureKind::Pdfium
    } else if lower.contains("llama-server") || lower.contains("runtime not found") {
        FailureKind::Runtime
    } else if lower.contains("cannot create output")
        || lower.contains("output directory")
        || lower.contains("permission denied")
        || lower.contains("access is denied")
        || lower.contains("os error 5")
        || (lower.contains("denied") && lower.contains("write"))
    {
        FailureKind::Output
    } else if lower.contains("invalid --pages") || lower.contains("invalid page") {
        FailureKind::Pages
    } else if lower.contains("password") || lower.contains("encrypted") {
        FailureKind::Password
    } else if lower.contains("input does not exist")
        || lower.contains("unsupported input")
        || lower.contains("no supported files")
        || lower.contains("cannot walk")
        || lower.contains("cannot read")
        || lower.contains("no input files")
    {
        FailureKind::File
    } else if lower.contains("timed out") || lower.contains("timeout") {
        FailureKind::Timeout
    } else {
        FailureKind::Unknown
    }
}

/// Short Russian sentence for the main status. The raw chain (for example
/// `os error 3`) goes to the log only; the main status must stay actionable on
/// the first run.
fn friendly_failure(error: &str) -> String {
    match classify_failure(error) {
        FailureKind::Model => {
            "Не хватает модели распознавания. Скачайте её (около 3,4 ГБ) или укажите \
             папку с моделью."
                .to_string()
        }
        FailureKind::Runtime => {
            "Не хватает движка распознавания. Скачайте его (около 69 МБ) или укажите \
             папку с движком."
                .to_string()
        }
        FailureKind::Pdfium => {
            "Для PDF нужен просмотрщик PDF. Скачайте движок или укажите папку, где он \
             уже есть."
                .to_string()
        }
        FailureKind::File => {
            "Не удалось прочитать файл. Он не изменён. Выберите другой файл.".to_string()
        }
        FailureKind::Output => "Не могу писать в эту папку. Выберите другую папку.".to_string(),
        FailureKind::Password => {
            "PDF защищён паролем. Введите пароль в разделе «PDF с паролем?».".to_string()
        }
        FailureKind::Pages => {
            "Не получилось разобрать номера страниц. Укажите их так: 1-5,7 — или \
             оставьте поле пустым."
                .to_string()
        }
        FailureKind::Timeout => {
            "Распознавание заняло слишком много времени. Попробуйте ещё раз или \
             уменьшите число страниц."
                .to_string()
        }
        FailureKind::Unknown => {
            "Не получилось завершить распознавание. Документы не изменены. Попробуйте \
             ещё раз."
                .to_string()
        }
    }
}

/// Bottom-bar sentence for the derived phase. Pure so the declensions and the
/// empty/idle/result wording can be unit-tested without a window.
fn status_text(phase: StatusPhase, detail: &str) -> String {
    match phase {
        StatusPhase::Empty => "Добавьте файлы — начнём с шага 1.".to_string(),
        StatusPhase::Idle(files) => {
            let noun = ui::plural_ru(files, "файл", "файла", "файлов");
            let verb = if noun == "файл" {
                "готов"
            } else {
                "готовы"
            };
            format!("{files} {noun} {verb} — проверьте папку и нажмите «Распознать».")
        }
        StatusPhase::Running | StatusPhase::Notice | StatusPhase::Error => {
            let detail = detail.trim();
            if detail.is_empty() {
                "Подождите…".to_string()
            } else {
                detail.to_string()
            }
        }
        StatusPhase::Result {
            cancelled: true, ..
        } => "Остановлено. Уже готовые страницы сохранены.".to_string(),
        StatusPhase::Result {
            files,
            pages,
            cancelled: false,
        } => format!(
            "Готово: {} {}, {} {}.",
            files,
            ui::plural_ru(files, "файл", "файла", "файлов"),
            pages,
            ui::plural_ru(pages, "страница", "страницы", "страниц"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocr_app::runtime_pdfium_candidate;

    #[test]
    fn status_text_handles_empty_idle_result_and_error_states() {
        assert_eq!(
            status_text(StatusPhase::Empty, ""),
            "Добавьте файлы — начнём с шага 1."
        );
        assert_eq!(
            status_text(StatusPhase::Idle(1), ""),
            "1 файл готов — проверьте папку и нажмите «Распознать»."
        );
        assert_eq!(
            status_text(StatusPhase::Idle(2), ""),
            "2 файла готовы — проверьте папку и нажмите «Распознать»."
        );
        assert_eq!(
            status_text(StatusPhase::Idle(5), ""),
            "5 файлов готовы — проверьте папку и нажмите «Распознать»."
        );
        assert_eq!(
            status_text(StatusPhase::Idle(11), ""),
            "11 файлов готовы — проверьте папку и нажмите «Распознать»."
        );
        assert_eq!(
            status_text(StatusPhase::Idle(21), ""),
            "21 файл готов — проверьте папку и нажмите «Распознать»."
        );

        assert_eq!(
            status_text(StatusPhase::Running, "Страница 2 из 5: распознаём…"),
            "Страница 2 из 5: распознаём…"
        );
        assert_eq!(
            status_text(StatusPhase::Notice, "Загрузка остановлена."),
            "Загрузка остановлена."
        );
        assert_eq!(
            status_text(StatusPhase::Error, "Не удалось прочитать файл."),
            "Не удалось прочитать файл."
        );

        assert_eq!(
            status_text(
                StatusPhase::Result {
                    files: 1,
                    pages: 1,
                    cancelled: false
                },
                "старый текст"
            ),
            "Готово: 1 файл, 1 страница."
        );
        assert_eq!(
            status_text(
                StatusPhase::Result {
                    files: 2,
                    pages: 5,
                    cancelled: false
                },
                "старый текст"
            ),
            "Готово: 2 файла, 5 страниц."
        );
        assert_eq!(
            status_text(
                StatusPhase::Result {
                    files: 1,
                    pages: 3,
                    cancelled: true
                },
                "старый текст"
            ),
            "Остановлено. Уже готовые страницы сохранены."
        );
    }

    #[test]
    fn status_phase_follows_busy_error_notice_and_idle_states() {
        let mut app = OcrApp::new();
        app.inputs.push(PathBuf::from("scan.pdf"));
        assert_eq!(app.status_phase(), StatusPhase::Idle(1));

        app.running = true;
        assert_eq!(app.status_phase(), StatusPhase::Running);

        app.running = false;
        app.last_error = Some("Не удалось прочитать файл.".to_string());
        assert_eq!(app.status_phase(), StatusPhase::Error);

        app.last_error = None;
        app.download_notice = true;
        assert_eq!(app.status_phase(), StatusPhase::Notice);

        app.download_notice = false;
        app.inputs.clear();
        assert_eq!(app.status_phase(), StatusPhase::Empty);
    }

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
            Ok(Msg::Event(_))
            | Ok(Msg::Download(_))
            | Ok(Msg::DownloadDone(_))
            | Ok(Msg::Runtime(_))
            | Ok(Msg::RuntimeDone(_)) => {
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
        assert!(status.contains("модели"), "{status}");
        assert_eq!(classify_failure(raw), FailureKind::Model);

        let runtime = friendly_failure(
            "llama-server executable not found (device auto).\nSearched: C:\\runtime",
        );
        assert!(runtime.contains("движка"), "{runtime}");
        assert_eq!(
            classify_failure("llama-server executable not found"),
            FailureKind::Runtime
        );

        let pdfium = friendly_failure(
            "PDFium not found; choose the runtime folder or set OCR_PDFIUM: no PDFium library",
        );
        assert!(pdfium.contains("просмотрщик"), "{pdfium}");
        assert_eq!(classify_failure("PDFium not found"), FailureKind::Pdfium);

        assert_eq!(
            friendly_failure("something unexpected\nsecond line"),
            "Не получилось завершить распознавание. Документы не изменены. Попробуйте ещё раз."
        );
    }

    #[test]
    fn common_failures_map_to_calm_russian_messages() {
        let output = friendly_failure(
            "cannot create output directory D:\\readonly: Access is denied. (os error 5)",
        );
        assert!(output.contains("папку"), "{output}");
        assert!(!output.contains("os error"), "{output}");
        assert_eq!(
            classify_failure("cannot create output directory D:\\readonly: Access is denied."),
            FailureKind::Output
        );

        let file = friendly_failure("input does not exist: C:\\gone\\scan.pdf");
        assert!(file.contains("файл"), "{file}");
        assert_eq!(
            classify_failure("input does not exist: C:\\gone\\scan.pdf"),
            FailureKind::File
        );

        let pages = friendly_failure("invalid --pages: 0");
        assert!(pages.contains("страниц"), "{pages}");

        let password = friendly_failure("PDF is encrypted; a password is required");
        assert!(password.contains("паролем"), "{password}");

        let timeout = friendly_failure("request timed out after 600 s");
        assert!(timeout.contains("времени"), "{timeout}");
    }

    #[test]
    fn runtime_guard_reports_a_panicking_worker() {
        let (tx, rx) = std::sync::mpsc::channel();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = RuntimeGuard::new(tx.clone());
            panic!("boom");
        }));
        assert!(result.is_err());
        match rx.try_recv() {
            Ok(Msg::RuntimeDone(Err(message))) => {
                assert!(message.contains("panicked"), "{message}")
            }
            _ => panic!("no RuntimeDone message"),
        }
    }

    #[test]
    fn runtime_folder_text_editing_fills_paths_and_download_target() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = tmp.path().join("runtime-folder");
        let pdfium = runtime_pdfium_candidate(&runtime);
        std::fs::create_dir_all(pdfium.parent().unwrap()).unwrap();
        std::fs::write(&pdfium, b"fake").unwrap();
        let server = ocr_app::runtime::server_exe_name();
        for sub in ["llama-vulkan", "llama-cpu"] {
            let dir = runtime.join(sub);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(server), b"fake").unwrap();
        }

        let mut app = OcrApp::new();
        app.runtime_dir = runtime.display().to_string();
        app.apply_runtime_dir_text();
        assert!(
            app.runtime.is_ready(),
            "typed runtime folder must drive discovery: {:?}",
            app.runtime
        );
        assert_eq!(app.target_runtime_dir(), runtime);
        assert_eq!(app.llama_dir, runtime.display().to_string());
        assert_eq!(app.pdfium, pdfium.display().to_string());

        app.runtime_dir.clear();
        app.apply_runtime_dir_text();
        assert_eq!(
            app.target_runtime_dir(),
            ocr_core::default_runtime_dir(),
            "an empty field downloads into the platform app-data runtime folder"
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
        assert!(status.contains("Заменить"), "{status}");
        assert!(!status.contains("--force"), "{status}");
        assert!(download_failure_status("download cancelled", true).contains("остановлена"));
        assert_eq!(
            download_failure_status("network down", false),
            "Не удалось скачать модель. Проверьте подключение к интернету и повторите."
        );
    }

    #[test]
    fn pdf_inputs_are_detected_for_the_pdfium_guard() {
        assert!(is_pdf_input(Path::new("a.PDF")));
        assert!(!is_pdf_input(Path::new("a.png")));
    }

    #[test]
    fn recognize_is_disabled_until_setup_files_and_folder_are_ready() {
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
        let pdfium = runtime_pdfium_candidate(&runtime);
        std::fs::create_dir_all(pdfium.parent().unwrap()).unwrap();
        std::fs::write(&pdfium, b"fake").unwrap();

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
        app.runtime = runtime_status(Some(&runtime), Some(&pdfium));
        assert!(app.setup_complete());
        assert!(!app.can_start(), "no files yet");
        assert_eq!(app.start_blocker(), Some("Сначала добавьте файлы."));

        app.inputs.push(tmp.path().join("scan.pdf"));
        assert!(app.can_start());
        assert_eq!(app.start_blocker(), None);

        app.output_dir.clear();
        assert!(!app.can_start(), "empty output folder disables Recognize");
        assert_eq!(app.start_blocker(), Some("Выберите папку для сохранения."));
    }

    #[test]
    fn keyboard_shortcuts_are_recognized() {
        let ctx = egui::Context::default();
        let press = |key: egui::Key, modifiers: egui::Modifiers| egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        };

        ctx.begin_pass(press(egui::Key::O, egui::Modifiers::CTRL));
        let open = ctx.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::O,
            ))
        });
        let _ = ctx.end_pass();
        assert!(open, "Ctrl+O must be consumed as Add files");

        ctx.begin_pass(press(egui::Key::Enter, egui::Modifiers::CTRL));
        let recognize = ctx.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::Enter,
            ))
        });
        let _ = ctx.end_pass();
        assert!(recognize, "Ctrl+Enter must be consumed as Recognize");

        ctx.begin_pass(press(egui::Key::Escape, egui::Modifiers::NONE));
        let cancel =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let _ = ctx.end_pass();
        assert!(cancel, "Esc must be consumed as Cancel");
    }

    #[test]
    fn leftover_english_backend_messages_are_not_shown_raw() {
        for error in [
            "Model files not found in C:\\x (os error 3)",
            "llama-server executable not found (device auto)",
            "PDFium not found: no PDFium library",
            "unexpected failure chain",
        ] {
            let message = friendly_failure(error);
            assert!(!message.contains("os error"), "{message}");
            assert!(
                message.chars().any(|ch| ('А'..='я').contains(&ch)),
                "message must be Russian: {message}"
            );
        }
    }
}
