//! The official Chandra OCR `ocr_layout` prompt, copied verbatim from
//! <https://github.com/datalab-to/chandra/blob/master/chandra/prompts.py>
//! so the model receives exactly the instructions it was trained on.

pub const ALLOWED_TAGS: &[&str] = &[
    "math", "br", "i", "b", "u", "del", "sup", "sub", "table", "tr", "td", "p", "th", "div", "pre",
    "h1", "h2", "h3", "h4", "h5", "ul", "ol", "li", "input", "a", "span", "img", "hr", "tbody",
    "small", "caption", "strong", "thead", "big", "code", "chem",
];

pub const ALLOWED_ATTRIBUTES: &[&str] = &[
    "class",
    "colspan",
    "rowspan",
    "display",
    "checked",
    "type",
    "border",
    "value",
    "style",
    "href",
    "alt",
    "align",
    "data-bbox",
    "data-label",
];

pub const OCR_LAYOUT_PROMPT: &str = r#"OCR this image to HTML, arranged as layout blocks.  Each layout block should be a div with the data-bbox attribute representing the bounding box of the block in x0 y0 x1 y1 format.  Bboxes are normalized 0-1000. The data-label attribute is the label for the block.

Use the following labels:
- Caption
- Footnote
- Equation-Block
- List-Group
- Page-Header
- Page-Footer
- Image
- Section-Header
- Table
- Text
- Complex-Block
- Code-Block
- Form
- Table-Of-Contents
- Figure
- Chemical-Block
- Diagram
- Bibliography
- Blank-Page

Only use these tags ["math", "br", "i", "b", "u", "del", "sup", "sub", "table", "tr", "td", "p", "th", "div", "pre", "h1", "h2", "h3", "h4", "h5", "ul", "ol", "li", "input", "a", "span", "img", "hr", "tbody", "small", "caption", "strong", "thead", "big", "code", "chem"], and these attributes ["class", "colspan", "rowspan", "display", "checked", "type", "border", "value", "style", "href", "alt", "align", "data-bbox", "data-label"].

Guidelines:
* Inline math: Surround math with <math>...</math> tags. Math expressions should be rendered in KaTeX-compatible LaTeX. Use display for block math.
* Tables: Use colspan and rowspan attributes to match table structure.
* Formatting: Maintain consistent formatting with the image, including spacing, indentation, subscripts/superscripts, and special characters.
* Images: Include a description of any images in the alt attribute of an <img> tag. Do not fill out the src property. Describe in detail inside the div tag. Also convert charts to high fidelity data, and convert diagrams to mermaid.
* Forms: Mark checkboxes and radio buttons properly.
* Text: join lines together properly into paragraphs using <p>...</p> tags.  Use <br> tags for line breaks within paragraphs, but only when absolutely necessary to maintain meaning.
* Chemistry: Use <chem>...</chem> tags for chemical formulas with reactive SMILES.
* Lists: Preserve indents and proper list markers.
* Use the simplest possible HTML structure that accurately represents the content of the block.
* Make sure the text is accurate and easy for a human to read and interpret.  Reading order should be correct and natural."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_is_the_official_layout_prompt() {
        assert!(OCR_LAYOUT_PROMPT.starts_with("OCR this image to HTML"));
        assert!(OCR_LAYOUT_PROMPT.contains("data-bbox"));
        assert!(OCR_LAYOUT_PROMPT.contains("Page-Footer"));
        assert!(OCR_LAYOUT_PROMPT.contains("Only use these tags"));
        assert!(OCR_LAYOUT_PROMPT.contains("<chem>"));
        assert!(!OCR_LAYOUT_PROMPT.contains("{PROMPT_ENDING}"));
        assert!(!OCR_LAYOUT_PROMPT.contains("{{"));
    }

    #[test]
    fn prompt_lists_match_tag_constants() {
        for tag in ALLOWED_TAGS {
            assert!(
                OCR_LAYOUT_PROMPT.contains(&format!("\"{tag}\"")),
                "prompt is missing allowed tag {tag}"
            );
        }
        for attr in ALLOWED_ATTRIBUTES {
            assert!(
                OCR_LAYOUT_PROMPT.contains(&format!("\"{attr}\"")),
                "prompt is missing allowed attribute {attr}"
            );
        }
    }
}
