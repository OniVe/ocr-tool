//! PDF rendering through PDFium.
//!
//! PDFium is loaded dynamically at runtime. The library is searched in the
//! environment (`OCR_PDFIUM`), beside the executable, and in the packaged
//! `runtime/<platform>/pdfium/{bin,lib}` layout. A missing library is reported
//! as an ordinary error with the list of searched paths.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, bail, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use pdfium_render::prelude::*;

pub const DEFAULT_DPI: u32 = 192;
pub const DEFAULT_MAX_PIXELS: u64 = 40_000_000;
pub const PDFIUM_ENV: &str = "OCR_PDFIUM";

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub dpi: u32,
    pub max_pixels: u64,
    pub password: Option<String>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            dpi: DEFAULT_DPI,
            max_pixels: DEFAULT_MAX_PIXELS,
            password: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderedPage {
    /// Zero-based page index.
    pub index: u32,
    pub width: u32,
    pub height: u32,
    pub jpeg: Vec<u8>,
}

#[derive(Debug)]
pub struct RenderOutcome {
    pub page_count: usize,
    pub pages: Vec<RenderedPage>,
    /// Page-level failures; the rest of the document is still rendered.
    pub errors: Vec<(u32, String)>,
    pub cancelled: bool,
}

pub struct PdfRenderer {
    pdfium: Pdfium,
    library: PathBuf,
}

impl PdfRenderer {
    /// Load PDFium from the first valid location.
    pub fn new() -> Result<Self> {
        Self::from_library(locate_pdfium()?)
    }

    /// Load PDFium from an explicit library file or a directory tree containing
    /// it. Used when the caller (the GUI runtime folder, CLI `--pdfium`) already
    /// knows where the library is.
    pub fn new_at(path: &Path) -> Result<Self> {
        Self::from_library(locate_pdfium_at(path)?)
    }

    fn from_library(library: PathBuf) -> Result<Self> {
        let pdfium = match Pdfium::bind_to_library(&library) {
            Ok(bindings) => Pdfium::new(bindings),
            // `pdfium-render` keeps the bindings in a process-wide `OnceCell`
            // (see `Pdfium::new` in the crate), so only the first renderer in
            // the process can load the library; every later one gets this
            // error even though the functions are already available. The error
            // is returned only when the bindings are initialized, and that is
            // exactly the state `Pdfium::default()` reuses without touching
            // the filesystem again, so the second and further recognition jobs
            // in the same process keep the library that the first job loaded.
            Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => Pdfium::default(),
            Err(error) => {
                return Err(anyhow!(
                    "failed to load PDFium from {}: {error}\n\
                     Hint: the library must match the architecture of this executable, \
                     and its dependencies (for example the MSVC runtime) must be available.",
                    library.display()
                ))
            }
        };
        Ok(Self { pdfium, library })
    }

    pub fn library_path(&self) -> &Path {
        &self.library
    }

    pub fn page_count(&self, path: &Path, password: Option<&str>) -> Result<usize> {
        let document = self
            .pdfium
            .load_pdf_from_file(path, password)
            .with_context(|| format!("cannot open PDF {}", path.display()))?;
        Ok(document.pages().len() as usize)
    }

    /// Render the selected (zero-based) pages as JPEG. A failing page is
    /// recorded in `errors` and does not abort the document.
    pub fn render_pages(
        &self,
        path: &Path,
        indices: &[usize],
        options: &RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<RenderOutcome> {
        let document = self
            .pdfium
            .load_pdf_from_file(path, options.password.as_deref())
            .with_context(|| format!("cannot open PDF {}", path.display()))?;
        let page_count = document.pages().len() as usize;
        let mut rendered = Vec::new();
        let mut errors = Vec::new();
        let mut cancelled = false;

        for &index in indices {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            if index >= page_count {
                errors.push((index as u32, format!("page {} is out of range", index + 1)));
                continue;
            }
            match render_one(&document, index as i32, options) {
                Ok(page) => rendered.push(page),
                Err(error) => errors.push((index as u32, format!("{error:#}"))),
            }
        }

        Ok(RenderOutcome {
            page_count,
            pages: rendered,
            errors,
            cancelled,
        })
    }
}

fn render_one(
    document: &PdfDocument<'_>,
    index: i32,
    options: &RenderOptions,
) -> Result<RenderedPage> {
    let page = document
        .pages()
        .get(index)
        .with_context(|| format!("cannot access page {}", index + 1))?;

    let width_pts = page.width().value.max(1.0) as f64;
    let height_pts = page.height().value.max(1.0) as f64;
    let scale = options.dpi as f64 / 72.0;

    let mut width = (width_pts * scale).round().max(1.0) as u64;
    let mut height = (height_pts * scale).round().max(1.0) as u64;
    let pixels = width.saturating_mul(height);
    if pixels > options.max_pixels {
        let factor = (options.max_pixels as f64 / pixels as f64).sqrt();
        width = ((width as f64 * factor).round() as u64).max(1);
        height = ((height as f64 * factor).round() as u64).max(1);
    }

    let config = PdfRenderConfig::new()
        .set_target_width(width as i32)
        .set_maximum_height(height as i32);
    let bitmap = page
        .render_with_config(&config)
        .with_context(|| format!("PDFium failed to render page {}", index + 1))?;
    let image = bitmap
        .as_image()
        .with_context(|| format!("cannot convert page {} bitmap", index + 1))?
        .to_rgb8();

    let mut jpeg = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg, 90);
    encoder
        .encode_image(&image)
        .with_context(|| format!("cannot encode page {} as JPEG", index + 1))?;

    Ok(RenderedPage {
        index: index as u32,
        width: image.width(),
        height: image.height(),
        jpeg,
    })
}

/// Names used by PDFium on each platform.
pub fn pdfium_library_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["pdfium.dll"]
    } else if cfg!(target_os = "macos") {
        &["libpdfium.dylib"]
    } else {
        &["libpdfium.so"]
    }
}

/// Locate PDFium using the environment, the executable directory and the
/// current directory. `OCR_PDFIUM` may point either at the library file itself
/// or at a directory containing it; an explicitly set but invalid path is an
/// error rather than a silent fallback.
pub fn locate_pdfium() -> Result<PathBuf> {
    let env_value = std::env::var_os(PDFIUM_ENV);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let cwd = std::env::current_dir().ok();
    search_pdfium(env_value.as_deref(), exe_dir.as_deref(), cwd.as_deref())
}

/// Locate PDFium at an explicit path: either the library file itself or a
/// directory tree containing it (the same layout search as the environment
/// variable, including `bin/` and `lib/` subdirectories). Unlike
/// [`locate_pdfium`] this never falls back to the executable directory, so the
/// GUI can tell the user that the chosen runtime folder is wrong.
pub fn locate_pdfium_at(path: &Path) -> Result<PathBuf> {
    find_library(path, 3).ok_or_else(|| {
        anyhow!(
            "no PDFium library ({}) found at {}",
            pdfium_library_names().join(", "),
            path.display()
        )
    })
}

fn search_pdfium(
    env_value: Option<&OsStr>,
    exe_dir: Option<&Path>,
    cwd: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(value) = env_value.filter(|v| !v.is_empty()) {
        let path = PathBuf::from(value);
        if let Some(found) = find_library(&path, 3) {
            return Ok(found);
        }
        bail!(
            "{PDFIUM_ENV} is set to {}, but no PDFium library ({}) was found there",
            path.display(),
            pdfium_library_names().join(", ")
        );
    }

    let mut roots: Vec<(PathBuf, u8)> = Vec::new();
    for base in [exe_dir, cwd].into_iter().flatten() {
        // Only check the executable/current directory itself, never recurse
        // through an arbitrary working tree.
        roots.push((base.to_path_buf(), 0));
        for name in ocr_core::config::runtime_dir_names() {
            let runtime = base.join("runtime").join(name);
            roots.push((runtime.clone(), 2));
            roots.push((runtime.join("pdfium"), 2));
        }
    }

    let mut searched = Vec::new();
    for (root, depth) in roots {
        searched.push(root.display().to_string());
        if let Some(found) = find_library(&root, depth) {
            return Ok(found);
        }
    }

    Err(anyhow!(
        "PDFium library not found. Install pdfium ({}) or set {PDFIUM_ENV} to the \
         library file or its directory.\nSearched: {}",
        pdfium_library_names().join(" / "),
        searched.join(", ")
    ))
}

fn find_library(path: &Path, depth: u8) -> Option<PathBuf> {
    if path.is_file() {
        let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
        if pdfium_library_names()
            .iter()
            .any(|n| name.eq_ignore_ascii_case(n))
        {
            return Some(path.to_path_buf());
        }
        return None;
    }
    if !path.is_dir() {
        return None;
    }

    for sub in ["", "bin", "lib"] {
        for name in pdfium_library_names() {
            let candidate = if sub.is_empty() {
                path.join(name)
            } else {
                path.join(sub).join(name)
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(path).ok()?;
    for entry in entries.flatten() {
        let child = entry.path();
        if child.is_dir() {
            if let Some(found) = find_library(&child, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_missing_pdfium_is_a_clean_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = search_pdfium(Some(tmp.path().as_os_str()), None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("OCR_PDFIUM"));
        assert!(err.contains("no PDFium library"));
    }

    #[test]
    fn explicit_file_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let name = pdfium_library_names()[0];
        let file = tmp.path().join(name);
        std::fs::write(&file, b"not a real library").unwrap();
        assert_eq!(
            search_pdfium(Some(file.as_os_str()), None, None).unwrap(),
            file
        );
    }

    #[test]
    fn explicit_directory_and_nested_bin_are_found() {
        let tmp = tempfile::tempdir().unwrap();
        let name = pdfium_library_names()[0];
        let direct = tmp.path().join(name);
        std::fs::write(&direct, b"x").unwrap();
        assert_eq!(locate_pdfium_at(tmp.path()).unwrap(), direct);

        let nested = tmp.path().join("runtime").join("pdfium").join("bin");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join(name);
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(locate_pdfium_at(&tmp.path().join("runtime")).unwrap(), file);
    }

    #[test]
    fn locate_pdfium_at_missing_path_is_a_clean_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = format!(
            "{:#}",
            locate_pdfium_at(&tmp.path().join("nope")).unwrap_err()
        );
        assert!(err.contains("no PDFium library"), "{err}");
        assert!(err.contains("nope"), "{err}");
    }

    #[test]
    fn packaged_layout_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let tag = ocr_core::platform_tag();
        let dir = tmp
            .path()
            .join("runtime")
            .join(tag)
            .join("pdfium")
            .join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(pdfium_library_names()[0]);
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(search_pdfium(None, Some(tmp.path()), None).unwrap(), file);
    }
}
