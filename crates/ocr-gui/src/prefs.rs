//! Remembered GUI preferences.
//!
//! Only harmless choices are stored: the output folder, the output format, the
//! device and the DPI. Passwords, document names and document contents are
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuiPrefs {
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub dpi: Option<u32>,
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
            extract_blocks: Some("all".to_string()),
            extract_images: Some(false),
            fast: Some(true),
            image_max_tokens: Some(2048),
            parallel_pages: Some(2),
        };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path), prefs);
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
