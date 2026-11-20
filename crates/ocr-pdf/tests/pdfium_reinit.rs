//! Regression: PDFium bindings are process-wide in `pdfium-render`, so the
//! *second* renderer created in the same process must reuse them instead of
//! failing with `PdfiumLibraryBindingsAlreadyInitialized`.
//!
//! The GUI runs one recognition job at a time but keeps the process alive, so
//! "Recognize more" used to construct a second `PdfRenderer` and the whole job
//! failed before rendering a single page.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use ocr_pdf::{locate_pdfium, PdfRenderer, RenderOptions};

/// PDFium is an optional runtime dependency: when it cannot be located the test
/// is skipped with an explicit message (run with `OCR_PDFIUM=<lib>` to force it).
fn pdfium_library() -> Option<PathBuf> {
    match locate_pdfium() {
        Ok(library) => Some(library),
        Err(error) => {
            eprintln!("SKIP pdfium_reinit: PDFium is not available ({error:#})");
            None
        }
    }
}

/// One-page PDF assembled with explicit cross-reference offsets so PDFium does
/// not have to reconstruct a broken xref table.
fn minimal_pdf() -> Vec<u8> {
    let stream = "1 0 0 RG 10 10 180 180 re S\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
         /Resources << >> /Contents 4 0 R >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{}endstream",
            stream.len(),
            stream
        ),
    ];

    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
    pdf.push_str("0000000000 65535 f \n");
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

#[test]
fn two_renderers_in_one_process_both_render() {
    let Some(library) = pdfium_library() else {
        return;
    };

    let dir = tempfile::tempdir().expect("temp dir");
    let pdf = dir.path().join("one-page.pdf");
    std::fs::write(&pdf, minimal_pdf()).expect("write fixture");

    let cancel = AtomicBool::new(false);
    let options = RenderOptions {
        dpi: 72,
        max_pixels: 4_000_000,
        password: None,
    };

    // First job: loads the library and initializes the global bindings.
    let first = PdfRenderer::new_at(&library).expect("first PDFium renderer");
    assert_eq!(first.library_path(), library.as_path());
    let outcome = first
        .render_pages(&pdf, &[0], &options, &cancel)
        .expect("first render");
    assert_eq!(outcome.page_count, 1);
    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(outcome.pages.len(), 1);
    assert!(!outcome.pages[0].jpeg.is_empty());
    // 200x200 pt MediaBox at 72 dpi: a real page was rasterized.
    assert_eq!(
        (outcome.pages[0].width, outcome.pages[0].height),
        (200, 200)
    );

    // Second job in the same process ("Recognize more"): must reuse the already
    // loaded bindings instead of failing with
    // `PdfiumLibraryBindingsAlreadyInitialized`.
    let second = PdfRenderer::new_at(&library)
        .expect("second PDFium renderer must reuse the loaded bindings");
    assert_eq!(second.library_path(), library.as_path());
    let outcome = second
        .render_pages(&pdf, &[0], &options, &cancel)
        .expect("second render");
    assert_eq!(outcome.page_count, 1);
    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(outcome.pages.len(), 1);
    assert!(!outcome.pages[0].jpeg.is_empty());
    assert_eq!(
        (outcome.pages[0].width, outcome.pages[0].height),
        (200, 200)
    );
    assert_eq!(outcome.pages[0].index, 0);
}
