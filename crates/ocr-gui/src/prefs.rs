//! Remembered GUI preferences.
//!
//! Only harmless choices are stored: the output folder, the output format, the
//! device, the DPI, the tool folders (model, runtime, llama-server, PDFium) and
//! the advanced limits. Passwords, document names and document contents are
//! never written here. The file lives next to the GUI log in the platform
//! app-data folder (`%LOCALAPPDATA%\ocr-tool\gui-prefs.json` on Windows) and is
//! deliberately best-effort: an unreadable or malformed file falls back to the
//! defaults instead of blocking the first run.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use ocr_core::BlockExtraction;
use ocr_engine::{FAST_IMAGE_MAX_TOKENS, MAX_PARALLEL_PAGES};
use serde::{Deserialize, Serialize};

/// Last choices the user made on the main screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuiPrefs {
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub dpi: Option<u32>,
    /// Interface language code (`en`/`ru`/`zh`); missing means automatic
    /// detection from the OS locale.
    #[serde(default)]
    pub lang: Option<String>,
    /// `none`/`images`/`all`; the newer field.
    #[serde(default)]
    pub extract_blocks: Option<String>,
    /// Legacy boolean kept so older prefs keep working.
    #[serde(default)]
    pub extract_images: Option<bool>,
    /// Fast speed profile (2048 image tokens); migrated to/from
    /// `image_max_tokens`.
    #[serde(default)]
    pub fast: Option<bool>,
    /// Explicit image-token cap (advanced); wins over `fast`.
    #[serde(default)]
    pub image_max_tokens: Option<u32>,
    /// PDF pages in flight: 1 or 2.
    #[serde(default)]
    pub parallel_pages: Option<u32>,
    /// Keep llama-server loaded between jobs. Missing in an older prefs file
    /// means `true` (the preferred first-run behaviour).
    #[serde(default = "default_preload_model")]
    pub preload_model: bool,
    /// Explicit model folder (advanced); empty means auto-discovery.
    #[serde(default)]
    pub model_dir: String,
    /// Runtime folder shared by llama-server and PDFium lookup.
    #[serde(default)]
    pub runtime_dir: String,
    /// Explicit llama-server folder or executable (advanced).
    #[serde(default)]
    pub llama_dir: String,
    /// Explicit PDFium library path (advanced).
    #[serde(default)]
    pub pdfium: String,
    /// Page selection typed by the user, e.g. `1-5,7`.
    #[serde(default)]
    pub pages: String,
    /// Walk subfolders of added folders.
    #[serde(default)]
    pub recursive: bool,
    /// Advanced pixel cap; zero means "not stored yet" (use the default).
    #[serde(default)]
    pub max_pixels: u64,
    /// Advanced token cap; zero means "not stored yet" (use the default).
    #[serde(default)]
    pub max_tokens: u32,
    /// Advanced request timeout; zero means "not stored yet" (use the default).
    #[serde(default)]
    pub timeout_secs: u64,
}

fn default_preload_model() -> bool {
    true
}

impl Default for GuiPrefs {
    fn default() -> Self {
        Self {
            output_dir: None,
            format: None,
            device: None,
            dpi: None,
            lang: None,
            extract_blocks: None,
            extract_images: None,
            fast: None,
            image_max_tokens: None,
            parallel_pages: None,
            preload_model: true,
            model_dir: String::new(),
            runtime_dir: String::new(),
            llama_dir: String::new(),
            pdfium: String::new(),
            pages: String::new(),
            recursive: false,
            max_pixels: 0,
            max_tokens: 0,
            timeout_secs: 0,
        }
    }
}

impl GuiPrefs {
    pub fn output_dir(&self) -> Option<&str> {
        self.output_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// Resolve the block extraction mode, migrating the old boolean pref.
    pub fn extract_blocks(&self) -> BlockExtraction {
        self.extract_blocks
            .as_deref()
            .and_then(|value| BlockExtraction::from_str(value).ok())
            .or_else(|| {
                self.extract_images
                    .filter(|enabled| *enabled)
                    .map(|_| BlockExtraction::Images)
            })
            .unwrap_or(BlockExtraction::None)
    }

    /// Resolve the image-token budget. An explicit value wins; otherwise the
    /// `fast` flag maps to 2048 and the quality default is 4096. This also
    /// migrates a prefs file that only knows one of the two fields.
    pub fn image_max_tokens(&self, quality_default: u32) -> u32 {
        match self.image_max_tokens {
            Some(value) if (1..=4096).contains(&value) => value,
            _ if self.fast.unwrap_or(false) => FAST_IMAGE_MAX_TOKENS,
            _ => quality_default,
        }
    }

    /// PDF pages in flight, clamped to the supported 1..=2 range.
    pub fn parallel_pages(&self) -> u32 {
        self.parallel_pages
            .unwrap_or(1)
            .clamp(1, MAX_PARALLEL_PAGES)
    }

    /// Advanced pixel cap; zero is the missing-field value from an older file.
    pub fn max_pixels_or(&self, default: u64) -> u64 {
        if self.max_pixels == 0 {
            default
        } else {
            self.max_pixels
        }
    }

    /// Advanced token cap; zero is the missing-field value from an older file.
    pub fn max_tokens_or(&self, default: u32) -> u32 {
        if self.max_tokens == 0 {
            default
        } else {
            self.max_tokens
        }
    }

    /// Advanced request timeout; zero is the missing-field value from an older
    /// file.
    pub fn timeout_secs_or(&self, default: u64) -> u64 {
        if self.timeout_secs == 0 {
            default
        } else {
            self.timeout_secs
        }
    }
}

/// Default preferences file. `OCR_GUI_PREFS` overrides it for tests and for the
/// screenshot harness; the real app always uses the app-data path.
pub fn default_path() -> PathBuf {
    #[cfg(test)]
    {
        std::env::temp_dir()
            .join("ocr-tool-gui-test-prefs")
            .join("gui-prefs.json")
    }
    #[cfg(not(test))]
    {
        std::env::var_os("OCR_GUI_PREFS")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| {
                ocr_core::app_data_dir()
                    .join("ocr-tool")
                    .join("gui-prefs.json")
            })
    }
}

/// Load the preferences, tolerating a missing or broken file.
pub fn load() -> GuiPrefs {
    load_from(&default_path())
}

pub fn load_from(path: &Path) -> GuiPrefs {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => GuiPrefs::default(),
    }
}

/// Save the preferences, creating the folder if needed. Failure is reported to
/// the GUI log but never interrupts recognition.
pub fn save(prefs: &GuiPrefs) {
    if let Err(error) = save_to(&default_path(), prefs) {
        super::gui_log::write(format!("cannot save GUI preferences: {error}"));
    }
}

pub fn save_to(path: &Path, prefs: &GuiPrefs) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(prefs)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
    std::fs::write(path, text)
}

/// Where results go on a fresh install: `Документы\ocr-tool` when the platform
/// has a Documents folder, otherwise `./ocr-output` like the CLI default.
pub fn default_output_dir() -> PathBuf {
    if let Some(documents) = dirs::document_dir() {
        return documents.join("ocr-tool");
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("ocr-output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("gui-prefs.json");
        let prefs = GuiPrefs {
            output_dir: Some("D:\\сканы\\результат".to_string()),
            format: Some("all".to_string()),
            device: Some("cpu".to_string()),
            dpi: Some(240),
            lang: Some("ru".to_string()),
            extract_blocks: Some("all".to_string()),
            extract_images: Some(false),
            fast: Some(true),
            image_max_tokens: Some(2048),
            parallel_pages: Some(2),
            preload_model: false,
            model_dir: "E:\\модели\\Qwen".to_string(),
            runtime_dir: "D:\\runtime".to_string(),
            llama_dir: "D:\\runtime\\llama".to_string(),
            pdfium: "D:\\runtime\\pdfium.dll".to_string(),
            pages: "1-5,7".to_string(),
            recursive: true,
            max_pixels: 200_000_000,
            max_tokens: 8192,
            timeout_secs: 900,
        };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path), prefs);
    }

    #[test]
    fn old_preferences_without_new_fields_load_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gui-prefs.json");
        std::fs::write(
            &path,
            r#"{
                "output_dir": "C:\\выход",
                "format": "all",
                "device": "cpu",
                "dpi": 240,
                "extract_blocks": "all",
                "fast": true,
                "parallel_pages": 2
            }"#,
        )
        .unwrap();
        let prefs = load_from(&path);
        assert_eq!(prefs.output_dir.as_deref(), Some("C:\\выход"));
        assert_eq!(prefs.format.as_deref(), Some("all"));
        assert!(prefs.model_dir.is_empty());
        assert!(prefs.runtime_dir.is_empty());
        assert!(prefs.llama_dir.is_empty());
        assert!(prefs.pdfium.is_empty());
        assert!(prefs.pages.is_empty());
        assert!(!prefs.recursive);
        assert_eq!(prefs.max_pixels, 0);
        assert_eq!(prefs.max_tokens, 0);
        assert_eq!(prefs.timeout_secs, 0);
        assert_eq!(
            prefs.lang, None,
            "an older prefs file without the field means automatic detection"
        );
        assert!(
            prefs.preload_model,
            "an older prefs file without the field must preload the model"
        );
        assert_eq!(prefs.max_pixels_or(400_000_000), 400_000_000);
        assert_eq!(prefs.max_tokens_or(4096), 4096);
        assert_eq!(prefs.timeout_secs_or(600), 600);
    }

    #[test]
    fn language_preference_roundtrips_and_is_optional() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gui-prefs.json");
        for code in ["en", "ru", "zh"] {
            let prefs = GuiPrefs {
                lang: Some(code.to_string()),
                ..GuiPrefs::default()
            };
            save_to(&path, &prefs).unwrap();
            assert_eq!(load_from(&path).lang.as_deref(), Some(code));
        }

        std::fs::write(&path, r#"{"format":"all"}"#).unwrap();
        assert_eq!(
            load_from(&path).lang,
            None,
            "a prefs file without the field must keep automatic detection"
        );
    }

    #[test]
    fn saved_tool_paths_and_limits_are_returned() {
        let prefs = GuiPrefs {
            model_dir: "E:\\модели".to_string(),
            runtime_dir: "D:\\движок".to_string(),
            llama_dir: "D:\\движок\\llama-server.exe".to_string(),
            pdfium: "D:\\движок\\pdfium.dll".to_string(),
            pages: "2,4-6".to_string(),
            recursive: true,
            max_pixels: 123_000_000,
            max_tokens: 3072,
            timeout_secs: 1200,
            ..GuiPrefs::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gui-prefs.json");
        save_to(&path, &prefs).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.model_dir, "E:\\модели");
        assert_eq!(loaded.runtime_dir, "D:\\движок");
        assert_eq!(loaded.llama_dir, "D:\\движок\\llama-server.exe");
        assert_eq!(loaded.pdfium, "D:\\движок\\pdfium.dll");
        assert_eq!(loaded.pages, "2,4-6");
        assert!(loaded.recursive);
        assert_eq!(loaded.max_pixels_or(400_000_000), 123_000_000);
        assert_eq!(loaded.max_tokens_or(4096), 3072);
        assert_eq!(loaded.timeout_secs_or(600), 1200);
    }

    #[test]
    fn broken_preferences_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gui-prefs.json");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load_from(&path), GuiPrefs::default());
        assert_eq!(
            load_from(&dir.path().join("absent.json")),
            GuiPrefs::default()
        );
    }

    #[test]
    fn blank_output_directory_is_ignored() {
        let prefs = GuiPrefs {
            output_dir: Some("   ".to_string()),
            ..GuiPrefs::default()
        };
        assert_eq!(prefs.output_dir(), None);
    }

    #[test]
    fn extract_mode_prefers_new_field_and_migrates_the_old_boolean() {
        assert_eq!(GuiPrefs::default().extract_blocks(), BlockExtraction::None);
        let old = GuiPrefs {
            extract_images: Some(true),
            ..GuiPrefs::default()
        };
        assert_eq!(old.extract_blocks(), BlockExtraction::Images);
        let new = GuiPrefs {
            extract_blocks: Some("all".to_string()),
            extract_images: Some(true),
            ..GuiPrefs::default()
        };
        assert_eq!(new.extract_blocks(), BlockExtraction::All);
    }

    #[test]
    fn preload_defaults_to_on_and_roundtrips_the_off_choice() {
        assert!(GuiPrefs::default().preload_model);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gui-prefs.json");
        let off = GuiPrefs {
            preload_model: false,
            ..GuiPrefs::default()
        };
        save_to(&path, &off).unwrap();
        let loaded = load_from(&path);
        assert!(!loaded.preload_model);
        assert_eq!(loaded, off);

        std::fs::write(&path, r#"{"preload_model": true}"#).unwrap();
        assert!(load_from(&path).preload_model);
    }

    #[test]
    fn speed_prefs_default_to_quality_and_one_page() {
        let prefs = GuiPrefs::default();
        assert_eq!(prefs.image_max_tokens(4096), 4096);
        assert_eq!(prefs.parallel_pages(), 1);
    }

    #[test]
    fn speed_prefs_migrate_fast_and_clamp_parallel_pages() {
        let old = GuiPrefs {
            fast: Some(true),
            ..GuiPrefs::default()
        };
        assert_eq!(old.image_max_tokens(4096), 2048);

        let explicit = GuiPrefs {
            fast: Some(true),
            image_max_tokens: Some(3072),
            ..GuiPrefs::default()
        };
        assert_eq!(
            explicit.image_max_tokens(4096),
            3072,
            "an explicit advanced value wins over the fast flag"
        );

        let clamped = GuiPrefs {
            parallel_pages: Some(9),
            ..GuiPrefs::default()
        };
        assert_eq!(clamped.parallel_pages(), 2);
        let zero = GuiPrefs {
            parallel_pages: Some(0),
            ..GuiPrefs::default()
        };
        assert_eq!(zero.parallel_pages(), 1);
    }

    #[test]
    fn default_output_directory_is_absolute_and_non_empty() {
        let path = default_output_dir();
        assert!(path.is_absolute(), "{}", path.display());
        assert!(path.file_name().is_some(), "{}", path.display());
    }
}
