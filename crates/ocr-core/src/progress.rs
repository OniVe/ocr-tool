use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Info => "info",
            LogLevel::Warning => "warning",
            LogLevel::Error => "error",
        }
    }
}

/// Events emitted by the job runner. The CLI renders them to stderr; the GUI
/// forwards them to its log/progress widgets.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    JobStarted {
        files: usize,
        output_dir: String,
    },
    FileStarted {
        index: usize,
        total: usize,
        path: String,
    },
    PageStarted {
        path: String,
        page: u32,
        total_pages: u32,
    },
    PageFinished {
        path: String,
        page: u32,
        chars: usize,
        elapsed: Duration,
    },
    PageFailed {
        path: String,
        page: u32,
        error: String,
    },
    FileFinished {
        path: String,
        outputs: Vec<String>,
        pages_ok: usize,
        pages_failed: usize,
        elapsed: Duration,
    },
    FileFailed {
        path: String,
        error: String,
    },
    Log {
        level: LogLevel,
        message: String,
    },
    JobFinished {
        documents_ok: usize,
        documents_failed: usize,
        elapsed: Duration,
        cancelled: bool,
    },
}
