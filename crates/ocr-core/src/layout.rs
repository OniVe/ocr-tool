//! Minimal, dependency-free parser for the layout HTML produced by Chandra.
//!
//! Chandra wraps every layout block in a top-level
//! `<div data-bbox="x0 y0 x1 y1" data-label="Text">…</div>` and normalizes the
//! bbox to 0–1000. This module finds those top-level blocks, converts their
//! bboxes to pixels, preserves their inner HTML (minus nested layout
//! attributes) and extracts plain text for the JSON output.

use crate::html::sanitize_fragment;

/// One layout block ready for the JSON writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutBlock {
    /// Pixel coordinates `[x0, y0, x1, y1]` inside the page.
    pub bbox: [u32; 4],
    pub label: String,
    /// Sanitized inner HTML of the block, without the wrapper `<div>`.
    pub content: String,
    /// Plain text of `content` (whitespace collapsed).
    pub text: String,
}

impl LayoutBlock {
    /// `Image`/`Figure` blocks are the only ones cropped by default; the same
    /// rule as the official Chandra `extract_images`.
    pub fn is_image(&self) -> bool {
        self.label.eq_ignore_ascii_case("image") || self.label.eq_ignore_ascii_case("figure")
    }
}

enum Frame {
    Block {
        content_start: usize,
        bbox: [f64; 4],
        label: String,
    },
    Nested,
}

/// Parse top-level layout blocks and scale their bboxes to `width × height`.
///
/// * Only divs carrying both a valid `data-bbox` and a `data-label` are layout
///   blocks. Layout divs nested inside another block are kept as content (their
///   `data-bbox`/`data-label` attributes are removed).
/// * `Blank-Page` blocks are skipped.
/// * When the answer has no layout divs (plain HTML/text or a model that
///   ignored the prompt), one `block` covering the whole page is returned.
pub fn parse_layout_blocks(html: &str, width: u32, height: u32) -> Vec<LayoutBlock> {
    let mut blocks = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut saw_valid_layout = false;
    let mut index = 0;

    while index < html.len() {
        let Some(relative) = html[index..].find('<') else {
            break;
        };
        let tag_start = index + relative;

        if html[tag_start..].starts_with("<!--") {
            index = html[tag_start + 4..]
                .find("-->")
                .map(|end| tag_start + 4 + end + 3)
                .unwrap_or(html.len());
            continue;
        }

        let Some(tag_end) = find_tag_end(html, tag_start) else {
            break;
        };
        let Some(tag) = TagInfo::parse(&html[tag_start..tag_end]) else {
            index = tag_end;
            continue;
        };

        if tag.name == "div" {
            if tag.closing {
                if let Some(Frame::Block {
                    content_start,
                    bbox,
                    label,
                }) = stack.pop()
                {
                    if !is_blank_label(&label) {
                        blocks.push(build_block(
                            &html[content_start..tag_start],
                            bbox,
                            label,
                            width,
                            height,
                        ));
                    }
                }
            } else {
                let inside_block = stack
                    .iter()
                    .any(|frame| matches!(frame, Frame::Block { .. }));
                match tag.layout() {
                    Some((bbox, label)) if !inside_block => {
                        saw_valid_layout = true;
                        if tag.self_closing {
                            if !is_blank_label(&label) {
                                blocks.push(build_block("", bbox, label, width, height));
                            }
                        } else {
                            stack.push(Frame::Block {
                                content_start: tag_end,
                                bbox,
                                label,
                            });
                        }
                    }
                    _ if tag.self_closing => {}
                    _ => stack.push(Frame::Nested),
                }
            }
        }
        index = tag_end;
    }

    // Malformed model HTML with unclosed divs: keep the captured content rather
    // than dropping the whole page.
    while let Some(frame) = stack.pop() {
        if let Frame::Block {
            content_start,
            bbox,
            label,
        } = frame
        {
            if !is_blank_label(&label) {
                blocks.push(build_block(
                    &html[content_start..],
                    bbox,
                    label,
                    width,
                    height,
                ));
            }
        }
    }

    if blocks.is_empty() && !saw_valid_layout {
        let content = sanitize_fragment(&strip_layout_attributes(html));
        let text = html_to_text(&content);
        return vec![LayoutBlock {
            bbox: [0, 0, width, height],
            label: "block".to_string(),
            content,
            text,
        }];
    }

    blocks
}

/// Convert a normalized 0–1000 bbox to page pixels. Values are clamped to the
/// page; `x1`/`y1` may equal the page width/height.
pub fn scale_bbox(bbox: [f64; 4], width: u32, height: u32) -> [u32; 4] {
    let scale = |value: f64, size: u32| -> u32 {
        let normalized = value.clamp(0.0, 1000.0);
        (normalized / 1000.0 * size as f64).round() as u32
    };
    [
        scale(bbox[0], width),
        scale(bbox[1], height),
        scale(bbox[2], width),
        scale(bbox[3], height),
    ]
}

/// Parse the `data-bbox` attribute: four finite numbers separated by spaces
/// and/or commas.
pub fn parse_bbox_attr(value: &str) -> Option<[f64; 4]> {
    let mut parts = Vec::new();
    for part in value.split(|ch: char| ch.is_whitespace() || ch == ',') {
        if part.is_empty() {
            continue;
        }
        let number = part.parse::<f64>().ok()?;
        if !number.is_finite() {
            return None;
        }
        parts.push(number);
    }
    match parts.as_slice() {
        [x0, y0, x1, y1] => Some([*x0, *y0, *x1, *y1]),
        _ => None,
    }
}

/// Remove `data-bbox` and `data-label` from nested tags while keeping the rest
/// of the markup intact.
pub fn strip_layout_attributes(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut index = 0;
    while index < fragment.len() {
        let Some(relative) = fragment[index..].find('<') else {
            out.push_str(&fragment[index..]);
            break;
        };
        let tag_start = index + relative;
        out.push_str(&fragment[index..tag_start]);

        if fragment[tag_start..].starts_with("<!--") {
            let end = fragment[tag_start + 4..]
                .find("-->")
                .map(|offset| tag_start + 4 + offset + 3)
                .unwrap_or(fragment.len());
            out.push_str(&fragment[tag_start..end]);
            index = end;
            continue;
        }

        let Some(tag_end) = find_tag_end(fragment, tag_start) else {
            out.push_str(&fragment[tag_start..]);
            break;
        };
        let tag = &fragment[tag_start..tag_end];
        out.push_str(&strip_one_tag(tag));
        index = tag_end;
    }
    out
}

fn strip_one_tag(tag: &str) -> String {
    let Some(parsed) = TagInfo::parse(tag) else {
        return tag.to_string();
    };
    if parsed.closing {
        return tag.to_string();
    }
    let Some((name, attributes)) = raw_attributes(tag) else {
        return tag.to_string();
    };
    if !attributes
        .iter()
        .any(|attribute| attribute.name == "data-bbox" || attribute.name == "data-label")
    {
        return tag.to_string();
    }

    let mut out = String::with_capacity(tag.len());
    out.push('<');
    out.push_str(name);
    for attribute in attributes {
        if attribute.name != "data-bbox" && attribute.name != "data-label" {
            out.push(' ');
            out.push_str(&tag[attribute.start..attribute.end]);
        }
    }
    if parsed.self_closing {
        out.push_str(" /");
    }
    out.push('>');
    out
}

/// Extract plain text from an HTML fragment, decoding the common entities and
/// collapsing whitespace.
pub fn html_to_text(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut index = 0;
    while index < fragment.len() {
        if fragment.as_bytes()[index] == b'<' {
            if fragment[index..].starts_with("<!--") {
                index = fragment[index + 4..]
                    .find("-->")
                    .map(|offset| index + 4 + offset + 3)
                    .unwrap_or(fragment.len());
                out.push(' ');
                continue;
            }
            match find_tag_end(fragment, index) {
                Some(end) => {
                    let tag = &fragment[index..end];
                    if let Some(parsed) = TagInfo::parse(tag) {
                        if parsed.name == "img" {
                            if let Some((_, alt)) =
                                parsed.attributes.iter().find(|(name, _)| name == "alt")
                            {
                                out.push_str(&decode_entities(alt));
                            }
                        }
                    }
                    out.push(' ');
                    index = end;
                }
                None => {
                    out.push_str(&decode_entities(&fragment[index..]));
                    break;
                }
            }
        } else {
            let next = fragment[index..]
                .find('<')
                .map(|offset| index + offset)
                .unwrap_or(fragment.len());
            out.push_str(&decode_entities(&fragment[index..next]));
            index = next;
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn build_block(
    inner_html: &str,
    bbox: [f64; 4],
    label: String,
    width: u32,
    height: u32,
) -> LayoutBlock {
    let content = sanitize_fragment(&strip_layout_attributes(inner_html));
    let text = html_to_text(&content);
    LayoutBlock {
        bbox: scale_bbox(bbox, width, height),
        label,
        content,
        text,
    }
}

fn is_blank_label(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("blank-page")
}

pub(crate) struct TagInfo {
    pub(crate) name: String,
    pub(crate) closing: bool,
    pub(crate) self_closing: bool,
    pub(crate) attributes: Vec<(String, String)>,
}

impl TagInfo {
    pub(crate) fn parse(tag: &str) -> Option<Self> {
        if !tag.starts_with('<') {
            return None;
        }
        let mut rest = tag[1..].trim_start();
        if rest.starts_with('!') || rest.starts_with('?') {
            return None;
        }
        let closing = rest.starts_with('/');
        if closing {
            rest = rest[1..].trim_start();
        }
        let name_end = rest
            .find(|ch: char| ch.is_whitespace() || ch == '>' || ch == '/')
            .unwrap_or(rest.len());
        if name_end == 0 {
            return None;
        }
        let name = rest[..name_end].to_ascii_lowercase();
        let mut body = &rest[name_end..];
        if let Some(stripped) = body.strip_suffix('>') {
            body = stripped;
        }
        let self_closing = body.trim_end().ends_with('/');
        Some(Self {
            name,
            closing,
            self_closing,
            attributes: parse_attributes(body),
        })
    }

    fn layout(&self) -> Option<([f64; 4], String)> {
        let bbox = self
            .attributes
            .iter()
            .find(|(name, _)| name == "data-bbox")
            .and_then(|(_, value)| parse_bbox_attr(value))?;
        let label = self
            .attributes
            .iter()
            .find(|(name, _)| name == "data-label")
            .map(|(_, value)| decode_entities(value))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())?;
        Some((bbox, label))
    }
}

struct RawAttribute {
    name: String,
    start: usize,
    end: usize,
}

/// Attribute spans relative to the whole tag, for lossless reconstruction.
fn raw_attributes(tag: &str) -> Option<(&str, Vec<RawAttribute>)> {
    let bytes = tag.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }
    let mut index = 1;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b'/' {
        return None;
    }
    let name_start = index;
    while index < bytes.len()
        && !bytes[index].is_ascii_whitespace()
        && bytes[index] != b'>'
        && bytes[index] != b'/'
    {
        index += 1;
    }
    let name_end = index;
    let mut attributes = Vec::new();
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'>' {
            break;
        }
        if bytes[index] == b'/' {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'>'
        {
            index += 1;
        }
        if index == start {
            break;
        }
        let name = tag[start..index].to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if index < bytes.len() && (bytes[index] == b'"' || bytes[index] == b'\'') {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                if index < bytes.len() {
                    index += 1;
                }
            } else {
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                {
                    index += 1;
                }
            }
        }
        attributes.push(RawAttribute {
            name,
            start,
            end: index,
        });
    }
    Some((&tag[name_start..name_end], attributes))
}

fn parse_attributes(body: &str) -> Vec<(String, String)> {
    let bytes = body.as_bytes();
    let mut attributes = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'>' || bytes[index] == b'/' {
            break;
        }
        let name_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'>'
        {
            index += 1;
        }
        if index == name_start {
            break;
        }
        let name = body[name_start..index].to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let mut value = String::new();
        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if index < bytes.len() && (bytes[index] == b'"' || bytes[index] == b'\'') {
                let quote = bytes[index];
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                value = body[value_start..index].to_string();
                if index < bytes.len() {
                    index += 1;
                }
            } else {
                let value_start = index;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                {
                    index += 1;
                }
                value = body[value_start..index].to_string();
            }
        }
        attributes.push((name, value));
    }
    attributes
}

pub(crate) fn find_tag_end(fragment: &str, start: usize) -> Option<usize> {
    let bytes = fragment.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = start + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(active) => {
                if byte == active {
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'>' => return Some(index + 1),
                _ => {}
            },
        }
        index += 1;
    }
    None
}

/// Decode the entities the model actually emits in labels/text.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(relative) = text[index..].find('&') {
        let start = index + relative;
        out.push_str(&text[index..start]);
        let Some(semicolon) = text[start..].find(';').map(|offset| start + offset) else {
            out.push_str(&text[start..]);
            return out;
        };
        if semicolon - start > 12 {
            out.push('&');
            index = start + 1;
            continue;
        }
        let entity = &text[start + 1..semicolon];
        let decoded = if let Some(hex) = entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
        {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(decimal) = entity.strip_prefix('#') {
            decimal.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            match entity.to_ascii_lowercase().as_str() {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => None,
            }
        };
        match decoded {
            Some(ch) => out.push(ch),
            None => out.push_str(&text[start..=semicolon]),
        }
        index = semicolon + 1;
    }
    out.push_str(&text[index..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: (u32, u32) = (1000, 2000);

    #[test]
    fn parses_top_level_blocks_and_scales_bboxes() {
        let html = r#"<div data-bbox="0 0 500 100" data-label="Text"><p>Привет</p></div>
            <div data-bbox="500 100 1000 400" data-label="Table"><table><tr><td>42</td></tr></table></div>"#;
        let blocks = parse_layout_blocks(html, PAGE.0, PAGE.1);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].bbox, [0, 0, 500, 200]);
        assert_eq!(blocks[0].label, "Text");
        assert!(blocks[0].content.contains("<p>Привет</p>"));
        assert_eq!(blocks[0].text, "Привет");
        assert_eq!(blocks[1].bbox, [500, 200, 1000, 800]);
        assert_eq!(blocks[1].text, "42");
    }

    #[test]
    fn nested_layout_divs_are_content_not_separate_blocks() {
        let html = r#"<div data-bbox="0 0 1000 1000" data-label="Complex-Block"><div data-bbox="100 100 200 200" data-label="Text">Внутри</div></div>"#;
        let blocks = parse_layout_blocks(html, 1000, 1000);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].label, "Complex-Block");
        assert!(blocks[0].content.contains("Внутри"));
        assert!(
            !blocks[0].content.contains("data-bbox"),
            "nested layout attributes must be removed: {}",
            blocks[0].content
        );
        assert!(!blocks[0].content.contains("data-label"));
    }

    #[test]
    fn missing_bbox_falls_back_to_whole_page_block() {
        let html = "<p>Просто HTML без layout-разметки</p>";
        let blocks = parse_layout_blocks(html, 640, 480);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].label, "block");
        assert_eq!(blocks[0].bbox, [0, 0, 640, 480]);
        assert!(blocks[0].text.contains("Просто HTML"));
    }

    #[test]
    fn blank_page_is_skipped() {
        let blocks = parse_layout_blocks(
            r#"<div data-bbox="0 0 1000 1000" data-label="Blank-Page"></div>"#,
            800,
            600,
        );
        assert!(blocks.is_empty());
    }

    #[test]
    fn unclosed_layout_block_keeps_its_content() {
        let blocks = parse_layout_blocks(
            r#"<div data-bbox="0 0 500 500" data-label="Text"><p>Без закрытия"#,
            100,
            100,
        );
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].text.contains("Без закрытия"));
    }

    #[test]
    fn bbox_scaling_clamps_and_rounds() {
        assert_eq!(
            scale_bbox([0.0, 0.0, 1000.0, 1000.0], 2480, 3508),
            [0, 0, 2480, 3508]
        );
        assert_eq!(
            scale_bbox([-5.0, 100.0, 500.0, 1200.0], 100, 100),
            [0, 10, 50, 100]
        );
        assert_eq!(parse_bbox_attr("1 2 3 4"), Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(parse_bbox_attr("1,2,3,4"), Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(parse_bbox_attr("1 2 3"), None);
        assert_eq!(parse_bbox_attr("a b c d"), None);
    }

    #[test]
    fn labels_are_kept_verbatim_and_image_labels_detected() {
        let html = r#"<div data-bbox="1 2 3 4" data-label="Figure"><img alt="печать"/></div>
            <div data-bbox="1 2 3 4" data-label="Diagram"></div>"#;
        let blocks = parse_layout_blocks(html, 100, 100);
        assert_eq!(blocks[0].label, "Figure");
        assert!(blocks[0].is_image());
        assert_eq!(blocks[0].text, "печать");
        assert!(!blocks[1].is_image());
    }
}
