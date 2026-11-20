//! Shared building blocks for `ocr-tool`.
//!
//! This crate intentionally has no dependency on the OCR engine, PDF renderer or
//! any UI toolkit so it can be unit-tested quickly and reused everywhere.

pub mod config;
pub mod html;
pub mod json;
pub mod layout;
pub mod meta;
pub mod naming;
pub mod pages;
pub mod process;
pub mod progress;
pub mod prompt;
pub mod table;

pub use config::{
    app_data_dir, default_model_dir, default_runtime_dir, discover_model_paths, model_dir_source,
    platform_tag, resolve_model_dir, runtime_dir_names, BlockExtraction, Device, ModelPaths,
    OutputFormat, DEFAULT_MMPROJ_FILE, DEFAULT_MODEL_FILE, DEFAULT_MODEL_SUBDIR, LLAMA_ENV,
    MODEL_ENV, PDFIUM_ENV, RUNTIME_TAG,
};
pub use html::{
    clean_model_html, page_to_markdown, render_html_document, render_markdown_document,
    sanitize_fragment,
};
pub use json::{ChandraAssets, ChandraChunk, ChandraDocument, ChandraPage, CHANDRA_JSON_SCHEMA};
pub use layout::{html_to_text, parse_bbox_attr, parse_layout_blocks, scale_bbox, LayoutBlock};
pub use meta::{now_rfc3339, unix_timestamp_ms, DocumentMeta, JobMeta};
pub use naming::{sanitize_label, stem_of, OutputAllocator};
pub use pages::{parse_page_selection, validate_page_spec};
pub use process::hide_console;
pub use progress::{LogLevel, ProgressEvent};
pub use prompt::OCR_LAYOUT_PROMPT;
pub use table::table_to_csv;
