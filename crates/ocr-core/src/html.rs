use ammonia::Builder;

/// Remove chat-template artefacts and code fences, then isolate the HTML
/// fragment produced by the model. If the model answered with plain text (for
/// example a tiny crop or a CPU-truncated response), wrap it into a paragraph
/// instead of dropping it.
pub fn clean_model_html(raw: &str) -> String {
    let mut text = raw.trim().trim_start_matches('\u{feff}').trim();

    if text.starts_with("```") {
        // Drop the opening fence (```html, ```HTML, ...) and the closing fence.
        if let Some(newline) = text.find('\n') {
            text = &text[newline + 1..];
        }
        if let Some(end) = text.rfind("```") {
            text = &text[..end];
        }
        text = text.trim();
    }

    // Some chat templates leak special tokens into the content.
    let mut owned = text.to_string();
    for token in ["<|endoftext|>", "<|im_end|>", "<|im_start|>"] {
        owned = owned.replace(token, "");
    }
    let mut text = owned.trim();

    let lower = text.to_ascii_lowercase();
    let starts = [
        "<div",
        "<table",
        "<p",
        "<h1",
        "<h2",
        "<h3",
        "<h4",
        "<h5",
        "<ul",
        "<ol",
        "<pre",
        "<section",
        "<math",
        "<span",
        "<blockquote",
        "<figure",
    ];
    let start = starts.iter().filter_map(|needle| lower.find(needle)).min();
    let has_html_block = start.is_some();
    if let Some(start) = start {
        if start > 0 {
            text = text[start..].trim();
        }
        if let Some(end) = text.rfind('>') {
            text = &text[..=end];
        }
    }

    if text.is_empty() {
        return String::new();
    }

    if !has_html_block {
        return format!("<p>{}</p>", escape_html(text));
    }

    text.to_string()
}

/// Sanitize one model-produced HTML fragment. Layout attributes required by the
/// official prompt (`data-bbox`, `data-label`) are preserved.
pub fn sanitize_fragment(fragment: &str) -> String {
    let mut builder = Builder::default();
    builder
        .add_tags(&[
            "section",
            "math",
            "chem",
            "caption",
            "small",
            "big",
            "code",
            "pre",
            "u",
            "s",
            "sup",
            "sub",
            "del",
            "ins",
            "mark",
            "figure",
            "figcaption",
            "blockquote",
            "hr",
            "input",
            "tbody",
            "thead",
            "tfoot",
            "colgroup",
            "col",
            "dl",
            "dt",
            "dd",
        ])
        .add_generic_attributes(&[
            "class", "colspan", "rowspan", "align", "valign", "alt", "href", "type", "checked",
            "value", "border", "width", "height", "id", "title", "start",
        ])
        .add_generic_attribute_prefixes(&["data-"])
        .link_rel(Some("noopener noreferrer"))
        .rm_tags(&["script", "style", "iframe", "object", "embed", "form"]);
    builder.clean(fragment).to_string()
}

/// Render the final standalone HTML document: UTF-8, `lang="ru"`, print-friendly
/// styles and one `<section data-page="N">` per PDF page.
pub fn render_html_document(title: &str, pages: &[(Option<u32>, String)]) -> String {
    let mut body = String::new();
    for (number, page) in pages {
        let clean = sanitize_fragment(page);
        match number {
            Some(number) => {
                body.push_str(&format!(
                    "<section class=\"ocr-page\" data-page=\"{number}\">\n{clean}\n</section>\n"
                ));
            }
            None => {
                body.push_str(&format!(
                    "<section class=\"ocr-page\">\n{clean}\n</section>\n"
                ));
            }
        }
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="ru">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="ocr-tool (Chandra OCR 2 via llama.cpp)">
<title>{title}</title>
<style>
:root {{ color-scheme: light dark; }}
body {{ max-width: 60rem; margin: 0 auto; padding: 1.5rem; font-family: system-ui, "Segoe UI", Roboto, sans-serif; line-height: 1.5; }}
.ocr-page {{ margin: 0 0 2.5rem 0; }}
.ocr-page + .ocr-page {{ border-top: 1px dashed #999; padding-top: 1.5rem; }}
table {{ border-collapse: collapse; margin: 0.75rem 0; max-width: 100%; }}
td, th {{ border: 1px solid #888; padding: 0.25rem 0.4rem; vertical-align: top; }}
img {{ max-width: 100%; }}
[data-label="Page-Header"], [data-label="Page-Footer"] {{ color: #666; font-size: 0.9em; }}
@media print {{ .ocr-page {{ break-after: page; }} body {{ max-width: none; }} }}
</style>
</head>
<body>
<main>
{body}</main>
</body>
</html>
"#,
        title = escape_html(title),
        body = body
    )
}

/// Convert one page fragment to Markdown. Keeps the model's tables as GFM
/// tables where possible.
pub fn page_to_markdown(page_html: &str) -> String {
    let clean = sanitize_fragment(&clean_model_html(page_html));
    if clean.is_empty() {
        return String::new();
    }
    match htmd::convert(&clean) {
        Ok(markdown) => normalize_table_separators(markdown.trim()),
        Err(_) => {
            // Extremely defensive fallback: strip tags, keep the text.
            let mut out = String::new();
            let mut in_tag = false;
            for ch in clean.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => {
                        in_tag = false;
                        out.push(' ');
                    }
                    _ if !in_tag => out.push(ch),
                    _ => {}
                }
            }
            out.trim().to_string()
        }
    }
}

/// htmd emits table separator cells such as `-`; GFM requires at least three
/// dashes. Normalize them while preserving left/center/right alignment.
fn normalize_table_separators(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            let cells: Vec<&str> = trimmed.trim_matches('|').split('|').collect();
            let is_separator = !cells.is_empty()
                && cells.iter().all(|cell| {
                    let cell = cell.trim();
                    !cell.is_empty() && cell.chars().all(|ch| matches!(ch, '-' | ':'))
                });
            if is_separator {
                let normalized: Vec<&str> = cells
                    .iter()
                    .map(|cell| {
                        let cell = cell.trim();
                        match (cell.starts_with(':'), cell.ends_with(':')) {
                            (true, true) => ":---:",
                            (true, false) => ":---",
                            (false, true) => "---:",
                            (false, false) => "---",
                        }
                    })
                    .collect();
                out.push_str("| ");
                out.push_str(&normalized.join(" | "));
                out.push_str(" |\n");
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// Render the final Markdown document. PDF pages are separated with
/// `<!-- page N -->` comments so the page structure survives conversion.
pub fn render_markdown_document(title: &str, pages: &[(Option<u32>, String)]) -> String {
    let mut out = format!("# {}\n\n", title.trim());
    let mut first = true;
    for (number, page) in pages {
        let markdown = page_to_markdown(page);
        if markdown.is_empty() {
            continue;
        }
        if !first {
            out.push_str("\n\n");
        }
        first = false;
        if let Some(number) = number {
            out.push_str(&format!("<!-- page {number} -->\n\n"));
        }
        out.push_str(&markdown);
        out.push('\n');
    }
    out
}

pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL_OUTPUT: &str = r#"<div data-bbox="1 2 3 4" data-label="Text"><p>Внимание! Счет № 99</p></div><div data-label="Table"><table border="1"><tr><td>Дедморозим</td><td>43,200.00</td></tr></table></div>"#;

    #[test]
    fn cleans_fences_and_chatter() {
        let raw = "Here is the OCR:\n```html\n<div><p>Привет</p></div>\n```\nDone.";
        let clean = clean_model_html(raw);
        assert_eq!(clean, "<div><p>Привет</p></div>");
    }

    #[test]
    fn plain_text_fallback_becomes_paragraph() {
        let clean = clean_model_html("Просто текст <без тегов");
        assert!(clean.starts_with("<p>"));
        assert!(clean.contains("Просто текст"));
    }

    #[test]
    fn sanitizer_keeps_layout_attributes_and_drops_scripts() {
        let dirty = r#"<div data-bbox="1 2 3 4" data-label="Text" onclick="x()"><p>Привет</p><script>alert(1)</script></div>"#;
        let clean = sanitize_fragment(dirty);
        assert!(clean.contains("data-bbox=\"1 2 3 4\""));
        assert!(clean.contains("data-label=\"Text\""));
        assert!(!clean.contains("script"));
        assert!(!clean.contains("onclick"));
        assert!(clean.contains("Привет"));
    }

    #[test]
    fn sanitizer_keeps_table_layout_and_drops_embedded_scripts() {
        let dirty = r#"<table border="1"><tr><td colspan="2" onclick="x()">A<script>alert(1)</script></td></tr></table><iframe src="https://evil"></iframe><style>p{}</style>"#;
        let clean = sanitize_fragment(dirty);
        assert!(clean.contains("<table"), "{clean}");
        assert!(clean.contains("colspan=\"2\""), "{clean}");
        assert!(!clean.contains("script"), "{clean}");
        assert!(!clean.contains("iframe"), "{clean}");
        assert!(!clean.contains("onclick"), "{clean}");
        assert!(!clean.contains("<style"), "{clean}");
    }

    #[test]
    fn html_document_is_utf8_russian_and_has_page_sections() {
        let html = render_html_document(
            "Счет <99>",
            &[
                (Some(1), MODEL_OUTPUT.to_string()),
                (Some(2), "<p>Второй абзац</p>".to_string()),
            ],
        );
        assert!(html.contains("<meta charset=\"utf-8\">"));
        assert!(html.contains("lang=\"ru\""));
        assert!(html.contains("<section class=\"ocr-page\" data-page=\"1\">"));
        assert!(html.contains("data-page=\"2\""));
        assert!(html.contains("Счет &lt;99&gt;"));
        assert!(html.contains("Внимание"));
        assert!(html.contains("43,200.00"));
    }

    #[test]
    fn markdown_contains_text_tables_and_page_markers() {
        let markdown = render_markdown_document(
            "Счет",
            &[
                (Some(1), MODEL_OUTPUT.to_string()),
                (Some(2), "<p>Второй абзац</p>".to_string()),
            ],
        );
        assert!(markdown.contains("Внимание"));
        assert!(markdown.contains("Счет"));
        assert!(markdown.contains("Дедморозим"));
        assert!(markdown.contains("43,200.00"));
        assert!(markdown.contains("<!-- page 1 -->"));
        assert!(markdown.contains("<!-- page 2 -->"));
        assert!(markdown.contains("Второй абзац"));
    }

    #[test]
    fn markdown_table_separators_are_valid_gfm() {
        let html = r#"<div data-label="Table"><table border="1">
            <thead><tr><th>№</th><th>Товар</th><th>Сумма</th></tr></thead>
            <tbody><tr><td>1</td><td>Запчасть</td><td>43,200.00</td></tr></tbody>
        </table></div>"#;
        let markdown = page_to_markdown(html);
        assert!(
            markdown.contains("| --- | --- | --- |"),
            "expected normalized GFM separators, got: {markdown}"
        );
    }

    #[test]
    fn markdown_keeps_russian_utf8() {
        let md = page_to_markdown("<div><p>Договор возмездного оказания услуг</p></div>");
        assert!(md.contains("Договор возмездного оказания услуг"));
    }
}
