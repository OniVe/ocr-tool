//! Small, explicit harness for documentation screenshots.
//!
//! The GUI is normally driven by a person. Taking screenshots of several
//! states used to require fragile UI automation, so the app understands a few
//! opt-in environment variables. They are inert unless set and never add a
//! network call or change recognition:
//!
//! ```text
//! OCR_GUI_SHOT=path\shot.png        take one in-app screenshot and keep running
//! OCR_GUI_SHOT_DELAY_MS=2000        wait before the shot (default 2000)
//! OCR_GUI_DEMO_INPUTS=a.pdf;b.jpg   pre-fill the file list
//! OCR_GUI_DEMO_OUTPUT=path          pre-fill the output folder
//! OCR_GUI_DEMO_PAGES=1-3            pre-fill the page selection
//! OCR_GUI_DEMO_DEVICE=cpu|gpu|auto  pre-select the device
//! OCR_GUI_DEMO_RUN=1                press Recognize automatically
//! OCR_GUI_DEMO_STATE=done|running|error  simulate a result state (screenshots)
//! OCR_GUI_THEME=dark|light          force a theme for the shot
//! OCR_GUI_DEMO_HELP=1               open the short help window
//! OCR_GUI_DEMO_ADVANCED=1           open the advanced section
//! ```
//!
//! The screenshot is rendered by egui itself, so it shows exactly what the
//! window shows even when the desktop compositor cannot be captured.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ocr_core::Device;

pub struct Demo {
    pub inputs: Vec<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub pages: Option<String>,
    pub device: Option<Device>,
    pub theme: Option<egui::Theme>,
    pub show_help: bool,
    pub advanced: bool,
    pub run: bool,
    pub state: Option<String>,
    pub shot: Option<Shot>,
}

pub struct Shot {
    pub path: PathBuf,
    delay: Duration,
    requested: bool,
    saved: bool,
}

impl Demo {
    /// Read the opt-in variables. Any parse error means "not requested".
    pub fn from_env() -> Self {
        let inputs = std::env::var("OCR_GUI_DEMO_INPUTS")
            .ok()
            .map(|value| {
                value
                    .split(';')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        let output_dir = std::env::var_os("OCR_GUI_DEMO_OUTPUT")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty());
        let pages = std::env::var("OCR_GUI_DEMO_PAGES")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let device = std::env::var("OCR_GUI_DEMO_DEVICE")
            .ok()
            .and_then(|value| value.parse::<Device>().ok());
        let theme = match std::env::var("OCR_GUI_THEME").ok().as_deref() {
            Some("dark") => Some(egui::Theme::Dark),
            Some("light") => Some(egui::Theme::Light),
            _ => None,
        };
        let show_help = std::env::var("OCR_GUI_DEMO_HELP").as_deref() == Ok("1");
        let advanced = std::env::var("OCR_GUI_DEMO_ADVANCED").as_deref() == Ok("1");
        let run = std::env::var("OCR_GUI_DEMO_RUN").as_deref() == Ok("1");
        let state = std::env::var("OCR_GUI_DEMO_STATE")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| matches!(value.as_str(), "done" | "running" | "error"));
        let shot = std::env::var_os("OCR_GUI_SHOT")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .map(|path| Shot {
                path,
                delay: std::env::var("OCR_GUI_SHOT_DELAY_MS")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(Duration::from_millis)
                    .unwrap_or(Duration::from_millis(2000)),
                requested: false,
                saved: false,
            });
        Self {
            inputs,
            output_dir,
            pages,
            device,
            theme,
            show_help,
            advanced,
            run,
            state,
            shot,
        }
    }
}

/// Ask egui for a screenshot once the delay elapsed, then write the PNG when
/// the viewport answers. Keeps repainting while waiting so the shot is not
/// delayed by the idle event loop.
pub fn drive_screenshot(ctx: &egui::Context, shot: &mut Shot, started: Instant) {
    if !shot.requested && started.elapsed() >= shot.delay {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        shot.requested = true;
    }

    if !shot.saved {
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            save_png(&shot.path, &image);
            shot.saved = true;
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

fn save_png(path: &std::path::Path, image: &egui::ColorImage) {
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            super::gui_log::write(format!(
                "cannot create screenshot folder {}: {error}",
                parent.display()
            ));
            return;
        }
    }
    let [width, height] = image.size;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_array());
    }
    let Some(buffer) = image::RgbaImage::from_raw(width as u32, height as u32, rgba) else {
        super::gui_log::write("cannot build screenshot buffer");
        return;
    };
    // Write next to the target first: the screenshot harness waits for the
    // final file, so it must not observe a half-written PNG.
    let temporary = path.with_file_name(format!(
        "{}.part.png",
        path.file_stem()
            .map(|stem| stem.to_string_lossy())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_file(&temporary);
    match buffer.save_with_format(&temporary, image::ImageFormat::Png) {
        Ok(()) => match std::fs::rename(&temporary, path) {
            Ok(()) => super::gui_log::write(format!("screenshot saved: {}", path.display())),
            Err(error) => {
                let _ = std::fs::remove_file(&temporary);
                super::gui_log::write(format!(
                    "cannot move screenshot {}: {error}",
                    path.display()
                ));
            }
        },
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            super::gui_log::write(format!(
                "cannot save screenshot {}: {error}",
                path.display()
            ));
        }
    }
}
