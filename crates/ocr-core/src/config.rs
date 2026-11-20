use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// Environment variable that points at a `llama-server` executable.
pub const LLAMA_ENV: &str = "OCR_LLAMA_SERVER";
/// Environment variable that points at a directory containing the GGUF files.
pub const MODEL_ENV: &str = "OCR_MODEL_DIR";
/// Environment variable that points at `pdfium.dll` / `libpdfium.so` /
/// `libpdfium.dylib`, or at a directory containing it.
pub const PDFIUM_ENV: &str = "OCR_PDFIUM";

/// Directory (under the platform app-data directory) used by `models download`.
pub const DEFAULT_MODEL_SUBDIR: &str = "ocr-tool/models/chandra-ocr-2";
/// Canonical main model file name downloaded from Hugging Face.
pub const DEFAULT_MODEL_FILE: &str = "chandra-ocr-2.Q4_K_M.gguf";
/// Canonical multimodal projector file name.
pub const DEFAULT_MMPROJ_FILE: &str = "chandra-ocr-2.mmproj-Q8_0.gguf";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    #[default]
    Auto,
    Gpu,
    Cpu,
}

impl Device {
    pub fn as_str(self) -> &'static str {
        match self {
            Device::Auto => "auto",
            Device::Gpu => "gpu",
            Device::Cpu => "cpu",
        }
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Device {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Device::Auto),
            "gpu" | "vulkan" | "cuda" | "metal" => Ok(Device::Gpu),
            "cpu" => Ok(Device::Cpu),
            other => Err(format!(
                "invalid device '{other}' (expected auto, gpu or cpu)"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Md,
    Html,
    Json,
    #[default]
    Both,
    All,
}

impl OutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            OutputFormat::Md => "md",
            OutputFormat::Html => "html",
            OutputFormat::Json => "json",
            OutputFormat::Both => "both",
            OutputFormat::All => "all",
        }
    }

    pub fn writes_markdown(self) -> bool {
        matches!(
            self,
            OutputFormat::Md | OutputFormat::Both | OutputFormat::All
        )
    }

    pub fn writes_html(self) -> bool {
        matches!(
            self,
            OutputFormat::Html | OutputFormat::Both | OutputFormat::All
        )
    }

    pub fn writes_json(self) -> bool {
        matches!(self, OutputFormat::Json | OutputFormat::All)
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Ok(OutputFormat::Md),
            "html" => Ok(OutputFormat::Html),
            "json" => Ok(OutputFormat::Json),
            "both" => Ok(OutputFormat::Both),
            "all" => Ok(OutputFormat::All),
            other => Err(format!(
                "invalid format '{other}' (expected md, html, json, both or all)"
            )),
        }
    }
}

/// How much of each page is saved as crop/typed files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BlockExtraction {
    /// Do not write per-block files (JSON is still written when requested).
    #[default]
    None,
    /// Crop only `Image`/`Figure` blocks (the old `--extract-images`).
    Images,
    /// Crop every non-blank block and write typed files (md/html/csv/tex/txt).
    All,
}

impl BlockExtraction {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockExtraction::None => "none",
            BlockExtraction::Images => "images",
            BlockExtraction::All => "all",
        }
    }

    pub fn writes_files(self) -> bool {
        !matches!(self, BlockExtraction::None)
    }
}

impl fmt::Display for BlockExtraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BlockExtraction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "none" | "off" => Ok(BlockExtraction::None),
            "images" | "image" => Ok(BlockExtraction::Images),
            "all" | "blocks" => Ok(BlockExtraction::All),
            other => Err(format!(
                "invalid block extraction '{other}' (expected none, images or all)"
            )),
        }
    }
}

/// Model files actually used for one job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPaths {
    pub dir: PathBuf,
    pub model: PathBuf,
    pub mmproj: PathBuf,
}

/// Resolve the model directory using the documented precedence:
/// explicit `--model-dir`, then `OCR_MODEL_DIR`, then platform app-data.
///
/// `env_value` is passed explicitly to keep this function easy to test.
pub fn resolve_model_dir(cli_value: Option<&Path>, env_value: Option<&std::ffi::OsStr>) -> PathBuf {
    if let Some(path) = cli_value {
        return path.to_path_buf();
    }
    if let Some(path) = env_value {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    default_model_dir()
}

/// Human-readable description of where the model directory came from.
pub fn model_dir_source(
    cli_value: Option<&Path>,
    env_value: Option<&std::ffi::OsStr>,
) -> &'static str {
    if cli_value.is_some() {
        "--model-dir"
    } else if env_value.map(|v| !v.is_empty()).unwrap_or(false) {
        MODEL_ENV
    } else {
        "app-data default"
    }
}

/// Platform app-data directory, e.g. `%LOCALAPPDATA%` on Windows.
pub fn app_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_model_dir() -> PathBuf {
    app_data_dir().join(DEFAULT_MODEL_SUBDIR)
}

/// Platform tag of a runtime with pinned downloads. Mirrors the packaging
/// layout (`runtime/<tag>`); build targets without pinned archives report
/// `unsupported`.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub const RUNTIME_TAG: &str = "windows-x64";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const RUNTIME_TAG: &str = "linux-x64";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const RUNTIME_TAG: &str = "macos-arm64";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub const RUNTIME_TAG: &str = "macos-x64";
#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
)))]
pub const RUNTIME_TAG: &str = "unsupported";

/// Where the GUI/CLI install the downloaded runtime by default:
/// `<app-data>/ocr-tool/runtime/<RUNTIME_TAG>`. This is deliberately separate
/// from the model directory; both are shown in the GUI.
pub fn default_runtime_dir() -> PathBuf {
    app_data_dir()
        .join("ocr-tool")
        .join("runtime")
        .join(RUNTIME_TAG)
}

/// Locate the two GGUF files in a directory.
///
/// The canonical names are preferred. For local testing the directory may hold
/// renamed files (for example `chandra-q4.gguf` + `mmproj.gguf`); in that case
/// the multimodal projector is any file containing `mmproj`, and the main model
/// is the largest remaining `.gguf` file.
pub fn discover_model_paths(dir: &Path) -> Result<ModelPaths> {
    let canonical_main = dir.join(DEFAULT_MODEL_FILE);
    let canonical_mmproj = dir.join(DEFAULT_MMPROJ_FILE);
    if canonical_main.is_file() && canonical_mmproj.is_file() {
        return Ok(ModelPaths {
            dir: dir.to_path_buf(),
            model: canonical_main,
            mmproj: canonical_mmproj,
        });
    }

    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read model directory {}", dir.display()))?;
    let mut main: Option<(u64, PathBuf)> = None;
    let mut mmproj: Option<PathBuf> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_gguf = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("gguf"))
            .unwrap_or(false);
        if !is_gguf {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if name.contains("mmproj") {
            if mmproj.is_none() {
                mmproj = Some(path);
            }
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if main.as_ref().map(|(s, _)| size > *s).unwrap_or(true) {
            main = Some((size, path));
        }
    }

    match (main, mmproj) {
        (Some((_, model)), Some(mmproj)) => Ok(ModelPaths {
            dir: dir.to_path_buf(),
            model,
            mmproj,
        }),
        (None, _) => Err(anyhow!(
            "no main .gguf model found in {} (expected {} or any non-mmproj .gguf file)",
            dir.display(),
            DEFAULT_MODEL_FILE
        )),
        (_, None) => Err(anyhow!(
            "no mmproj .gguf found in {} (expected {} or any file with 'mmproj' in its name)",
            dir.display(),
            DEFAULT_MMPROJ_FILE
        )),
    }
}

/// Runtime directory tag used by the packaging scripts: `windows-x64`,
/// `linux-x64`, `macos-arm64`. Rust reports `x86_64`/`aarch64`, so normalize it.
pub fn platform_tag() -> String {
    format!("{}-{}", std::env::consts::OS, normalized_arch())
}

pub fn normalized_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        other => other,
    }
}

/// Runtime directory names to search, in order: normalized tag, raw
/// `os-arch` tag (legacy), then the bare OS name.
pub fn runtime_dir_names() -> Vec<String> {
    let mut names = vec![platform_tag()];
    let raw = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    if !names.contains(&raw) {
        names.push(raw);
    }
    names.push(std::env::consts::OS.to_string());
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn device_parsing() {
        assert_eq!("auto".parse::<Device>().unwrap(), Device::Auto);
        assert_eq!("GPU".parse::<Device>().unwrap(), Device::Gpu);
        assert_eq!("vulkan".parse::<Device>().unwrap(), Device::Gpu);
        assert_eq!("cpu".parse::<Device>().unwrap(), Device::Cpu);
        assert!("tpu".parse::<Device>().is_err());
    }

    #[test]
    fn format_parsing_and_flags() {
        let both: OutputFormat = "both".parse().unwrap();
        assert!(both.writes_markdown() && both.writes_html() && !both.writes_json());
        let md: OutputFormat = "md".parse().unwrap();
        assert!(md.writes_markdown() && !md.writes_html() && !md.writes_json());
        let json: OutputFormat = "json".parse().unwrap();
        assert!(!json.writes_markdown() && !json.writes_html() && json.writes_json());
        let all: OutputFormat = "all".parse().unwrap();
        assert!(all.writes_markdown() && all.writes_html() && all.writes_json());
        assert_eq!(all.to_string(), "all");
        assert_eq!(json.to_string(), "json");
        assert_eq!(both.to_string(), "both");
        assert!("pdf".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn block_extraction_parsing_and_flags() {
        assert_eq!(
            "none".parse::<BlockExtraction>().unwrap(),
            BlockExtraction::None
        );
        assert_eq!(
            "images".parse::<BlockExtraction>().unwrap(),
            BlockExtraction::Images
        );
        assert_eq!(
            "all".parse::<BlockExtraction>().unwrap(),
            BlockExtraction::All
        );
        assert!(!BlockExtraction::None.writes_files());
        assert!(BlockExtraction::Images.writes_files());
        assert_eq!(BlockExtraction::All.to_string(), "all");
        assert!("some".parse::<BlockExtraction>().is_err());
    }

    #[test]
    fn precedence_cli_over_env_over_default() {
        let cli = PathBuf::from("cli-models");
        let env = OsStr::new("env-models");
        assert_eq!(resolve_model_dir(Some(&cli), Some(env)), cli);
        assert_eq!(
            resolve_model_dir(None, Some(env)),
            PathBuf::from("env-models")
        );
        assert_eq!(resolve_model_dir(None, None), default_model_dir());
        assert_eq!(
            resolve_model_dir(None, Some(OsStr::new(""))),
            default_model_dir()
        );
    }

    #[test]
    fn runtime_tag_matches_packaging_names() {
        let tag = platform_tag();
        assert!(
            !tag.contains("x86_64"),
            "packaging scripts use x64, got {tag}"
        );
        assert!(!tag.contains("aarch64"));
        assert!(runtime_dir_names().contains(&tag));
        if cfg!(windows) {
            assert_eq!(tag, "windows-x64");
        }
        if RUNTIME_TAG != "unsupported" {
            assert_eq!(RUNTIME_TAG, tag, "the pinned tag must match packaging");
        }
    }

    #[test]
    fn default_runtime_dir_is_app_data_runtime_tag() {
        let dir = default_runtime_dir();
        assert!(dir.ends_with(PathBuf::from("ocr-tool").join("runtime").join(RUNTIME_TAG)));
    }

    #[test]
    fn discovers_renamed_local_model_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("chandra-q4.gguf"), vec![0u8; 64]).unwrap();
        std::fs::write(tmp.path().join("mmproj.gguf"), vec![0u8; 16]).unwrap();
        let found = discover_model_paths(tmp.path()).unwrap();
        assert!(found.model.ends_with("chandra-q4.gguf"));
        assert!(found.mmproj.ends_with("mmproj.gguf"));
    }

    #[test]
    fn canonical_names_win() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(DEFAULT_MODEL_FILE), vec![0u8; 8]).unwrap();
        std::fs::write(tmp.path().join(DEFAULT_MMPROJ_FILE), vec![0u8; 8]).unwrap();
        std::fs::write(tmp.path().join("unrelated.gguf"), vec![0u8; 100]).unwrap();
        let found = discover_model_paths(tmp.path()).unwrap();
        assert_eq!(found.model, tmp.path().join(DEFAULT_MODEL_FILE));
        assert_eq!(found.mmproj, tmp.path().join(DEFAULT_MMPROJ_FILE));
    }

    #[test]
    fn missing_model_gives_actionable_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = discover_model_paths(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("chandra-ocr-2.Q4_K_M.gguf"));
    }
}
