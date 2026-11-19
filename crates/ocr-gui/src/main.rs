use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use ocr_app::{run_job, JobOptions, JobReport};
use ocr_core::{Device, OutputFormat, ProgressEvent};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 760.0])
            .with_min_inner_size([760.0, 560.0])
            .with_title("ocr-tool — Chandra OCR 2"),
        ..Default::default()
    };
    eframe::run_native(
        "ocr-tool",
        options,
        Box::new(|_cc| Ok(Box::new(OcrApp::new()))),
    )
}

struct LogLine {
    text: String,
    error: bool,
}

enum Msg {
    Event(ProgressEvent),
    Done(Result<JobReport, String>),
}

struct OcrApp {
    inputs: Vec<PathBuf>,
    output_dir: String,
    format: OutputFormat,
    device: Device,
    dpi: u32,
    pages: String,
    recursive: bool,
    model_dir: String,
    llama_dir: String,

    running: bool,
    cancelling: bool,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<Msg>>,
    progress: f32,
    status: String,
    logs: Vec<LogLine>,
    last_report: Option<JobReport>,
}

impl OcrApp {
    fn new() -> Self {
        let output = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("ocr-output");
        Self {
            inputs: Vec::new(),
            output_dir: output.display().to_string(),
            format: OutputFormat::Both,
            device: Device::Auto,
            dpi: 192,
            pages: String::new(),
            recursive: false,
            model_dir: String::new(),
            llama_dir: String::new(),
            running: false,
            cancelling: false,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            progress: 0.0,
            status: "Add files or a folder, then press Recognize.".to_string(),
            logs: Vec::new(),
            last_report: None,
        }
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

    fn start(&mut self) {
        if self.running || self.inputs.is_empty() {
            return;
        }
        let output_dir = PathBuf::from(self.output_dir.trim());
        if output_dir.as_os_str().is_empty() {
            self.status = "Choose an output directory first.".to_string();
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
            recursive: self.recursive,
            model_dir: non_empty_path(&self.model_dir),
            llama_dir: non_empty_path(&self.llama_dir),
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
            let mut emit = |event: ProgressEvent| {
                let _ = tx.send(Msg::Event(event));
            };
            let result =
                run_job(&options, &cancel, &mut emit).map_err(|error| format!("{error:#}"));
            let _ = tx.send(Msg::Done(result));
        });
    }

    fn poll(&mut self) {
        let messages: Vec<Msg> = match &self.rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        for message in messages {
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
                            self.status = "Failed. See the log.".to_string();
                            self.log(format!("error: {error}"), true);
                        }
                    }
                    self.rx = None;
                }
            }
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
                page, total_pages, ..
            } => {
                self.status = format!("Page {page}/{total_pages}: recognizing...");
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
            });

        ui.collapsing("Advanced paths", |ui| {
            ui.horizontal(|ui| {
                ui.label("Model dir");
                ui.add(
                    egui::TextEdit::singleline(&mut self.model_dir)
                        .hint_text("auto: OCR_MODEL_DIR or app-data")
                        .desired_width(430.0),
                );
                if ui.button("…").clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.model_dir = folder.display().to_string();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("llama dir");
                ui.add(
                    egui::TextEdit::singleline(&mut self.llama_dir)
                        .hint_text("auto: OCR_LLAMA_SERVER or runtime/")
                        .desired_width(430.0),
                );
                if ui.button("…").clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.llama_dir = folder.display().to_string();
                    }
                }
            });
            ui.label("The model and llama-server are not bundled. Use `ocr-tool models download` and scripts/fetch-runtime.");
        });
    }

    fn ui_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let recognize = ui.add_enabled(
                !self.running && !self.inputs.is_empty(),
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
                .animate(self.running),
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

        if self.running {
            ctx.request_repaint_after(Duration::from_millis(100));
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
