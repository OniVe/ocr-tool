//! File log for the GUI.
//!
//! The release Windows GUI is linked with `windows_subsystem = "windows"` and
//! has no attached console, so stdout/stderr cannot carry diagnostics after a
//! double-click. Normal messages stay in the in-window log; startup, preflight
//! and download errors, and panics must also survive a crash before the window
//! exists, so they are appended to a timestamped file.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// `%LOCALAPPDATA%\ocr-tool\logs\gui.log` on Windows
/// (`<app-data>/ocr-tool/logs/gui.log` on Linux/macOS).
pub fn default_log_path() -> PathBuf {
    #[cfg(test)]
    {
        // Keep `cargo test` away from the user's real diagnostics log.
        std::env::temp_dir().join(format!("ocr-tool-gui-test-{}.log", std::process::id()))
    }
    #[cfg(not(test))]
    {
        ocr_core::app_data_dir()
            .join("ocr-tool")
            .join("logs")
            .join("gui.log")
    }
}

/// Path used by this process. `OCR_GUI_LOG` overrides the platform default;
/// the acceptance smoke test uses it to keep the log inside its output folder.
pub fn log_path() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::var_os("OCR_GUI_LOG")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(default_log_path)
    })
}

struct Logger {
    file: File,
}

impl Logger {
    fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file })
    }

    /// Append one timestamped record; a multi-line block (backtrace) gets a
    /// timestamp on every line so it stays readable in a shared log.
    fn line(&mut self, message: &str) -> io::Result<()> {
        let stamp = ocr_core::now_rfc3339();
        if message.is_empty() {
            writeln!(self.file, "{stamp}")?;
        } else {
            for line in message.lines() {
                writeln!(self.file, "{stamp} {line}")?;
            }
        }
        self.file.flush()
    }
}

fn global() -> &'static Mutex<Option<Logger>> {
    static LOGGER: OnceLock<Mutex<Option<Logger>>> = OnceLock::new();
    LOGGER.get_or_init(|| Mutex::new(None))
}

/// Open the log and return its path. A failure to open is not fatal: the GUI
/// still runs and keeps the in-window log.
pub fn init() -> PathBuf {
    let path = log_path().to_path_buf();
    let mut slot = global().lock().unwrap_or_else(|error| error.into_inner());
    if slot.is_none() {
        match Logger::open(&path) {
            Ok(logger) => *slot = Some(logger),
            Err(error) => {
                #[cfg(debug_assertions)]
                eprintln!("ocr-tool-gui: cannot open log {}: {error}", path.display());
                let _ = error;
            }
        }
    }
    path
}

/// Append a line (or a multi-line block) to the file, opening it lazily if
/// [`init`] was not called. In debug builds it is echoed to the console too.
pub fn write(message: impl AsRef<str>) {
    let message = message.as_ref();
    let mut slot = global().lock().unwrap_or_else(|error| error.into_inner());
    if slot.is_none() {
        if let Ok(logger) = Logger::open(log_path()) {
            *slot = Some(logger);
        }
    }
    if let Some(logger) = slot.as_mut() {
        let _ = logger.line(message);
    }
    #[cfg(debug_assertions)]
    eprintln!("{}", message.trim_end());
}

/// Record panics with their thread, location and a full backtrace. Installed
/// before `eframe` starts, so a panic during startup still leaves a file.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("<unnamed>");
        let location = info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = if let Some(text) = info.payload().downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "non-string panic payload".to_string()
        };
        write(format!("PANIC thread '{thread}' at {location}: {payload}"));
        write(format!(
            "backtrace:\n{}",
            std::backtrace::Backtrace::force_capture()
        ));
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logger_appends_timestamped_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("gui.log");
        {
            let mut logger = Logger::open(&path).unwrap();
            logger.line("start").unwrap();
            logger.line("error: download failed").unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].starts_with("20"), "{text}");
        assert!(lines[0].contains(" start"), "{text}");
        assert!(lines[1].contains("error: download failed"), "{text}");

        let mut logger = Logger::open(&path).unwrap();
        logger.line("again").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3, "append, not truncate: {text}");
    }

    #[test]
    fn panic_hook_writes_location_and_backtrace() {
        let path = default_log_path();
        let _ = std::fs::remove_file(&path);
        let opened = init();
        assert_eq!(opened, path);
        install_panic_hook();
        let _ = std::panic::catch_unwind(|| panic!("gui panic test marker"));
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(text.contains("PANIC thread"), "{text}");
        assert!(text.contains("gui panic test marker"), "{text}");
        assert!(text.contains("backtrace:"), "{text}");
        assert!(
            text.lines()
                .any(|line| line.starts_with("20") && line.contains("PANIC")),
            "panic line must be timestamped: {text}"
        );
    }
}
