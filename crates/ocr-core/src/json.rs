//! JSON output compatible with Chandra's `_chunks.json`.
//!
//! The official package writes `<name>_chunks.json` as
//! `[{page_num, page_box, chunks: [{bbox, label, content}]}]`, where bboxes are
//! page pixels and `content` is the block's inner HTML. Our file adds a schema
//! header, stable block ids, a generator marker, plain text and relative paths
//! to the files created for a block.

use serde::{Deserialize, Serialize};

pub const CHANDRA_JSON_SCHEMA: &str = "chandra-json/v1";

/// Files written for one layout block. Missing formats are `None` (omitted
/// from JSON) because only real files are advertised.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChandraAssets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub png: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csv: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txt: Option<String>,
}

impl ChandraAssets {
    pub fn is_empty(&self) -> bool {
        self.png.is_none()
            && self.md.is_none()
            && self.html.is_none()
            && self.csv.is_none()
            && self.tex.is_none()
            && self.txt.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChandraChunk {
    /// Stable id, e.g. `p1-01`.
    pub block_id: String,
    /// Pixel bbox `[x0, y0, x1, y1]` in the page image.
    pub bbox: [u32; 4],
    pub label: String,
    /// Sanitized inner HTML of the layout block.
    pub content: String,
    /// Plain text extracted from `content` (includes `img alt`).
    pub text: String,
    /// Files created for this block, relative to the JSON file.
    #[serde(default)]
    pub assets: ChandraAssets,
    /// Backwards-compatible shortcut: the PNG of an `Image`/`Figure` block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChandraPage {
    pub page_num: u32,
    /// `[0, 0, width, height]` in pixels.
    pub page_box: [u32; 4],
    pub chunks: Vec<ChandraChunk>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChandraDocument {
    pub schema: String,
    pub source: String,
    pub generator: String,
    pub pages: Vec<ChandraPage>,
}

impl ChandraDocument {
    pub fn new(source: impl Into<String>, pages: Vec<ChandraPage>) -> Self {
        Self {
            schema: CHANDRA_JSON_SCHEMA.to_string(),
            source: source.into(),
            generator: "ocr-tool".to_string(),
            pages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_official_chunk_shape_with_assets() {
        let document = ChandraDocument::new(
            "contract_ru.pdf",
            vec![ChandraPage {
                page_num: 1,
                page_box: [0, 0, 2480, 3508],
                chunks: vec![
                    ChandraChunk {
                        block_id: "p1-01".to_string(),
                        bbox: [100, 200, 2400, 500],
                        label: "Text".to_string(),
                        content: "<p>Договор</p>".to_string(),
                        text: "Договор".to_string(),
                        assets: ChandraAssets {
                            png: Some("contract_ru_blocks/page-1-01-text.png".to_string()),
                            md: Some("contract_ru_blocks/page-1-01-text.md".to_string()),
                            ..ChandraAssets::default()
                        },
                        image: None,
                    },
                    ChandraChunk {
                        block_id: "p1-02".to_string(),
                        bbox: [1800, 2500, 2100, 2800],
                        label: "Image".to_string(),
                        content: "<img alt=\"печать\"/>".to_string(),
                        text: "печать".to_string(),
                        assets: ChandraAssets {
                            png: Some("contract_ru_blocks/page-1-02-image.png".to_string()),
                            txt: Some("contract_ru_blocks/page-1-02-image.txt".to_string()),
                            ..ChandraAssets::default()
                        },
                        image: Some("contract_ru_blocks/page-1-02-image.png".to_string()),
                    },
                ],
            }],
        );

        let value = serde_json::to_value(&document).unwrap();
        assert_eq!(value["schema"], CHANDRA_JSON_SCHEMA);
        assert_eq!(value["source"], "contract_ru.pdf");
        assert_eq!(value["generator"], "ocr-tool");
        assert_eq!(value["pages"][0]["page_num"], 1);
        assert_eq!(
            value["pages"][0]["page_box"],
            serde_json::json!([0, 0, 2480, 3508])
        );
        assert_eq!(
            value["pages"][0]["chunks"][0]["bbox"],
            serde_json::json!([100, 200, 2400, 500])
        );
        assert_eq!(value["pages"][0]["chunks"][0]["block_id"], "p1-01");
        assert_eq!(value["pages"][0]["chunks"][0]["label"], "Text");
        assert_eq!(value["pages"][0]["chunks"][0]["content"], "<p>Договор</p>");
        assert_eq!(value["pages"][0]["chunks"][0]["text"], "Договор");
        assert_eq!(
            value["pages"][0]["chunks"][0]["assets"]["png"],
            "contract_ru_blocks/page-1-01-text.png"
        );
        assert!(value["pages"][0]["chunks"][0]["assets"]
            .get("csv")
            .is_none());
        assert_eq!(
            value["pages"][0]["chunks"][1]["assets"]["txt"],
            "contract_ru_blocks/page-1-02-image.txt"
        );
    }

    #[test]
    fn assets_roundtrip_and_empty_is_detected() {
        let assets = ChandraAssets {
            csv: Some("a.csv".to_string()),
            tex: Some("a.tex".to_string()),
            ..ChandraAssets::default()
        };
        let json = serde_json::to_string(&assets).unwrap();
        assert!(json.contains("\"csv\":\"a.csv\""));
        assert!(!json.contains("png"));
        let back: ChandraAssets = serde_json::from_str(&json).unwrap();
        assert_eq!(back, assets);
        assert!(ChandraAssets::default().is_empty());
        assert!(!assets.is_empty());
    }
}
