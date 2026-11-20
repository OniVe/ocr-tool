//! Shared building blocks for `ocr-tool`.
//!
//! This crate intentionally has no dependency on the OCR engine, PDF renderer or
//! any UI toolkit so it can be unit-tested quickly and reused everywhere.

pub mod config;
pub mod html;
pub mod meta;
pub mod naming;
pub mod pages;
pub mod progress;
pub mod prompt;

pub use config::{
    app_data_dir, default_model_dir, default_runtime_dir, discover_model_paths, model_dir_source,
    platform_tag, resolve_model_dir, runtime_dir_names, Device, ModelPaths, OutputFormat,
    DEFAULT_MMPROJ_FILE, DEFAULT_MODEL_FILE, DEFAULT_MODEL_SUBDIR, LLAMA_ENV, MODEL_ENV,
    PDFIUM_ENV, RUNTIME_TAG,
};
pub use html::{
    clean_model_html, page_to_markdown, render_html_document, render_markdown_document,
    sanitize_fragment,
};
pub use meta::{now_rfc3339, unix_timestamp_ms, DocumentMeta, JobMeta};
pub use naming::{stem_of, OutputAllocator};
pub use pages::{parse_page_selection, validate_page_spec};
pub use progress::{LogLevel, ProgressEvent};
pub use prompt::OCR_LAYOUT_PROMPT;
