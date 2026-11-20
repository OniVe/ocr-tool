//! Cheap, network-free setup checks shared by the GUI (and available to the CLI).
//!
//! These functions only touch the filesystem: [`model_status`] resolves the GGUF
//! pair and compares the two pinned sizes; [`runtime_status`] locates PDFium and
//! enumerates `llama-server` candidates. No OCR is started and no network is
//! used, so the GUI can refresh them while the user edits paths.

use std::path::{Path, PathBuf};

use ocr_core::{discover_model_paths, ModelPaths};
use ocr_engine::{inspect_candidates, CandidateInfo, EngineConfig};

use crate::download::MODEL_FILES;

/// Result of checking a model directory without hashing the weights.
#[derive(Debug, Clone)]
pub enum ModelStatus {
    Ready {
        dir: PathBuf,
        paths: ModelPaths,
        model_bytes: u64,
        mmproj_bytes: u64,
    },
    /// No usable GGUF pair: the directory is absent or one of the files is
    /// missing.
    Missing { dir: PathBuf, reason: String },
    /// Both files were discovered but at least one has the wrong size (a
    /// truncated download, a placeholder, or another model).
    Incomplete {
        dir: PathBuf,
        paths: ModelPaths,
        reason: String,
    },
}

impl ModelStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, ModelStatus::Ready { .. })
    }

    pub fn dir(&self) -> &Path {
        match self {
            ModelStatus::Ready { dir, .. }
            | ModelStatus::Missing { dir, .. }
            | ModelStatus::Incomplete { dir, .. } => dir,
        }
    }

    /// Short status-bar text. Deliberately contains no raw OS error: the first
    /// run must say what to do, not print `os error 3`.
    pub fn message(&self) -> String {
        match self {
            ModelStatus::Ready { dir, .. } => format!("Model ready in {}", dir.display()),
            ModelStatus::Missing { dir, .. } => format!(
                "Model not found in {}; use Download model or choose a folder",
                dir.display()
            ),
            ModelStatus::Incomplete { dir, .. } => format!(
                "Model files in {} are incomplete; download the model again or choose a folder",
                dir.display()
            ),
        }
    }

    /// Technical detail for the panel/log (may include the discovery error).
    pub fn detail(&self) -> Option<&str> {
        match self {
            ModelStatus::Ready { .. } => None,
            ModelStatus::Missing { reason, .. } | ModelStatus::Incomplete { reason, .. } => {
                Some(reason)
            }
        }
    }
}

/// Check a directory against the pinned download sizes.
pub fn model_status(dir: &Path) -> ModelStatus {
    check_model_status(dir, MODEL_FILES[0].size, MODEL_FILES[1].size)
}

/// Same as [`model_status`] but with injectable expected sizes, so tests can
/// exercise the ready/incomplete split without creating 3.4 GB of weights.
pub fn check_model_status(dir: &Path, model_size: u64, mmproj_size: u64) -> ModelStatus {
    if !dir.is_dir() {
        return ModelStatus::Missing {
            dir: dir.to_path_buf(),
            reason: format!("model directory {} does not exist", dir.display()),
        };
    }
    let paths = match discover_model_paths(dir) {
        Ok(paths) => paths,
        Err(error) => {
            return ModelStatus::Missing {
                dir: dir.to_path_buf(),
                reason: error.to_string(),
            }
        }
    };
    let actual_model = std::fs::metadata(&paths.model)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let actual_mmproj = std::fs::metadata(&paths.mmproj)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if actual_model != model_size {
        return ModelStatus::Incomplete {
            dir: dir.to_path_buf(),
            reason: format!(
                "{} is {} bytes, expected {}",
                paths.model.display(),
                actual_model,
                model_size
            ),
            paths,
        };
    }
    if actual_mmproj != mmproj_size {
        return ModelStatus::Incomplete {
            dir: dir.to_path_buf(),
            reason: format!(
                "{} is {} bytes, expected {}",
                paths.mmproj.display(),
                actual_mmproj,
                mmproj_size
            ),
            paths,
        };
    }
    ModelStatus::Ready {
        dir: dir.to_path_buf(),
        paths,
        model_bytes: actual_model,
        mmproj_bytes: actual_mmproj,
    }
}

/// Where the located PDFium library came from. The GUI shows it next to
/// “Runtime ready” so a stale `OCR_PDFIUM` is not mistaken for the bundled
/// runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfiumSource {
    /// An explicit path: the GUI runtime folder, the PDFium field, or CLI `--pdfium`.
    Explicit,
    /// The ambient search found it through the `OCR_PDFIUM` environment variable.
    Env,
    /// The ambient search found it next to the executable, in `runtime/`, or in
    /// the current directory.
    Auto,
}

impl PdfiumSource {
    pub fn is_env(self) -> bool {
        matches!(self, PdfiumSource::Env)
    }

    /// Human-readable label for the GUI/doctor.
    pub fn label(self) -> &'static str {
        match self {
            PdfiumSource::Explicit => "chosen runtime folder or path",
            PdfiumSource::Env => "OCR_PDFIUM environment variable",
            PdfiumSource::Auto => "auto-discovered next to the app",
        }
    }
}

/// Result of locating the external runtime (PDFium + llama-server candidates).
#[derive(Debug, Clone)]
pub struct RuntimeStatus {
    pub pdfium: Result<PathBuf, String>,
    pub pdfium_source: PdfiumSource,
    pub llama: Vec<CandidateInfo>,
    pub llama_searched: Vec<String>,
    pub llama_warnings: Vec<String>,
}

impl RuntimeStatus {
    pub fn llama_ready(&self) -> bool {
        !self.llama.is_empty()
    }

    pub fn pdfium_ready(&self) -> bool {
        self.pdfium.is_ok()
    }

    pub fn is_ready(&self) -> bool {
        self.llama_ready() && self.pdfium_ready()
    }

    /// Short status-bar text without raw OS errors.
    pub fn message(&self) -> String {
        if !self.llama_ready() {
            "Runtime not found: llama-server was not found; choose the runtime folder".to_string()
        } else if !self.pdfium_ready() {
            "PDFium not found; choose the runtime folder or set OCR_PDFIUM".to_string()
        } else if self.pdfium_source.is_env() {
            "Runtime ready; PDFium is taken from OCR_PDFIUM (not from the runtime folder)"
                .to_string()
        } else {
            "Runtime ready".to_string()
        }
    }
}

/// Inspect PDFium and `llama-server` without starting anything. An explicit
/// `pdfium` path (the GUI runtime folder or CLI `--pdfium`) wins over the
/// ambient search order; `llama_dir` is the runtime folder or the directory
/// that holds `llama-server`.
pub fn runtime_status(llama_dir: Option<&Path>, pdfium: Option<&Path>) -> RuntimeStatus {
    let pdfium_source = if pdfium.is_some() {
        PdfiumSource::Explicit
    } else if std::env::var_os(ocr_pdf::PDFIUM_ENV)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
    {
        PdfiumSource::Env
    } else {
        PdfiumSource::Auto
    };
    let located = match pdfium {
        Some(path) => ocr_pdf::locate_pdfium_at(path),
        None => ocr_pdf::locate_pdfium(),
    }
    .map_err(|error| format!("{error:#}"));

    let mut config = EngineConfig::new(".");
    config.llama_dir = llama_dir.map(Path::to_path_buf);
    let (llama, llama_searched, llama_warnings) = inspect_candidates(&config);

    RuntimeStatus {
        pdfium: located,
        pdfium_source,
        llama,
        llama_searched,
        llama_warnings,
    }
}

/// The library path used by the packaged layout: `<runtime>/pdfium/bin/<lib>`.
pub fn runtime_pdfium_candidate(runtime: &Path) -> PathBuf {
    runtime
        .join("pdfium")
        .join("bin")
        .join(ocr_pdf::pdfium_library_names()[0])
}

/// Derive the PDFium library from a chosen runtime folder. Returns `None` when
/// neither the packaged layout nor a nested search finds it; the caller can
/// then fall back to the ambient search (`OCR_PDFIUM`, next to the exe).
pub fn pdfium_for_runtime(runtime: &Path) -> Option<PathBuf> {
    let candidate = runtime_pdfium_candidate(runtime);
    if candidate.is_file() {
        return Some(candidate);
    }
    ocr_pdf::locate_pdfium_at(runtime).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_sized(path: &Path, bytes: usize) {
        std::fs::write(path, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn missing_directory_is_reported_without_raw_os_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("absent");
        let status = model_status(&dir);
        assert!(!status.is_ready());
        let message = status.message();
        assert!(message.contains("Model not found in"), "{message}");
        assert!(message.contains("Download model"), "{message}");
        assert!(!message.contains("os error"), "{message}");
    }

    #[test]
    fn empty_directory_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let status = model_status(tmp.path());
        assert!(matches!(status, ModelStatus::Missing { .. }));
    }

    #[test]
    fn tiny_canonical_files_are_incomplete() {
        let tmp = tempfile::tempdir().unwrap();
        write_sized(&tmp.path().join(ocr_core::DEFAULT_MODEL_FILE), 1024);
        write_sized(&tmp.path().join(ocr_core::DEFAULT_MMPROJ_FILE), 512);
        let status = model_status(tmp.path());
        assert!(matches!(status, ModelStatus::Incomplete { .. }));
        assert!(
            status.message().contains("incomplete"),
            "{}",
            status.message()
        );
    }

    #[test]
    fn renamed_files_with_expected_sizes_are_ready() {
        let tmp = tempfile::tempdir().unwrap();
        write_sized(&tmp.path().join("chandra-q4.gguf"), 64);
        write_sized(&tmp.path().join("mmproj.gguf"), 16);
        let status = check_model_status(tmp.path(), 64, 16);
        match status {
            ModelStatus::Ready {
                paths,
                model_bytes,
                mmproj_bytes,
                ..
            } => {
                assert!(paths.model.ends_with("chandra-q4.gguf"));
                assert!(paths.mmproj.ends_with("mmproj.gguf"));
                assert_eq!(model_bytes, 64);
                assert_eq!(mmproj_bytes, 16);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[cfg(windows)]
    const SERVER_EXE: &str = "llama-server.exe";
    #[cfg(not(windows))]
    const SERVER_EXE: &str = "llama-server";

    #[test]
    fn runtime_folder_derives_pdfium_and_llama_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = tmp.path().join("runtime");
        let pdfium_dir = runtime.join("pdfium").join("bin");
        std::fs::create_dir_all(&pdfium_dir).unwrap();
        let pdfium = runtime_pdfium_candidate(&runtime);
        assert_eq!(pdfium, pdfium_dir.join(ocr_pdf::pdfium_library_names()[0]));
        std::fs::write(&pdfium, b"fake").unwrap();

        let llama_dir = runtime.join("llama-cpu");
        std::fs::create_dir_all(&llama_dir).unwrap();
        std::fs::write(llama_dir.join(SERVER_EXE), b"fake").unwrap();

        let status = runtime_status(Some(&runtime), Some(&runtime));
        assert_eq!(status.pdfium.as_ref().unwrap(), &pdfium);
        assert!(
            status.llama.iter().any(|candidate| candidate
                .exe
                .ends_with(Path::new("llama-cpu").join(SERVER_EXE))),
            "candidates: {:?}",
            status.llama
        );
        assert_eq!(
            pdfium_for_runtime(&runtime).as_deref(),
            Some(pdfium.as_path())
        );
        assert_eq!(status.pdfium_source, PdfiumSource::Explicit);
        assert!(status.is_ready());
    }

    #[test]
    fn runtime_ready_requires_pdfium_even_when_llama_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = tmp.path().join("runtime");
        let llama_dir = runtime.join("llama-cpu");
        std::fs::create_dir_all(&llama_dir).unwrap();
        std::fs::write(llama_dir.join(SERVER_EXE), b"fake").unwrap();

        let status = runtime_status(Some(&runtime), Some(&runtime.join("absent")));
        assert!(status.llama_ready(), "llama candidates: {:?}", status.llama);
        assert!(!status.pdfium_ready());
        assert!(
            !status.is_ready(),
            "a found llama-server must not make the runtime ready without PDFium"
        );
    }

    #[test]
    fn runtime_message_mentions_the_runtime_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let status = runtime_status(
            Some(&tmp.path().join("nope")),
            Some(&tmp.path().join("nope")),
        );
        assert!(!status.is_ready());
        let message = status.message();
        assert!(
            message.contains("Runtime not found") || message.contains("PDFium"),
            "{message}"
        );
        assert!(message.contains("choose the runtime folder"), "{message}");
    }
}
