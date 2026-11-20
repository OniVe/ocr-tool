//! Best-effort HTML table → CSV conversion for `Table` layout blocks.
//!
//! Only the first top-level table is used. `colspan`/`rowspan` are expanded
//! into a rectangular grid with empty cells, and fields are RFC 4180 quoted.
//! Nested tables are flattened into their parent cell (best effort).

use crate::layout::{find_tag_end, html_to_text, TagInfo};

#[derive(Debug, Clone)]
struct RawCell {
    text: String,
    colspan: usize,
    rowspan: usize,
}

/// Convert the first table in `html` to CSV, or `None` when there is no table
/// or it has no cells.
pub fn table_to_csv(html: &str) -> Option<String> {
    let rows = collect_rows(html);
    if rows.iter().all(|row| row.is_empty()) {
        return None;
    }

    let mut grid: Vec<Vec<Option<String>>> = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        if grid.len() <= row_index {
            grid.push(Vec::new());
        }
        let mut column = 0usize;
        for cell in row {
            while column < grid[row_index].len() && grid[row_index][column].is_some() {
                column += 1;
            }
            for dr in 0..cell.rowspan {
                let target_row = row_index + dr;
                while grid.len() <= target_row {
                    grid.push(Vec::new());
                }
                for dc in 0..cell.colspan {
                    let target_column = column + dc;
                    if grid[target_row].len() <= target_column {
                        grid[target_row].resize(target_column + 1, None);
                    }
                    if grid[target_row][target_column].is_none() {
                        grid[target_row][target_column] = Some(if dr == 0 && dc == 0 {
                            cell.text.clone()
                        } else {
                            String::new()
                        });
                    }
                }
            }
            column += cell.colspan;
        }
    }

    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return None;
    }

    let mut csv = String::new();
    for row in grid {
        let mut fields = Vec::with_capacity(width);
        for column in 0..width {
            fields.push(escape_csv_field(
                row.get(column).and_then(Option::as_deref).unwrap_or(""),
            ));
        }
        csv.push_str(&fields.join(","));
        csv.push('\n');
    }
    Some(csv)
}

fn collect_rows(html: &str) -> Vec<Vec<RawCell>> {
    let mut rows = Vec::new();
    let mut row: Option<Vec<RawCell>> = None;
    let mut cell: Option<(usize, usize, usize)> = None; // (content_start, colspan, rowspan)
    let mut table_depth = 0usize;
    let mut started = false;
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

        match (tag.name.as_str(), tag.closing) {
            ("table", false) => {
                if table_depth == 0 {
                    started = true;
                }
                table_depth += 1;
            }
            ("table", true) => {
                table_depth = table_depth.saturating_sub(1);
                if started && table_depth == 0 {
                    break;
                }
            }
            ("tr", false) if table_depth == 1 => {
                if let Some(finished) = row.take() {
                    if !finished.is_empty() {
                        rows.push(finished);
                    }
                }
                row = Some(Vec::new());
            }
            ("tr", true) if table_depth == 1 => {
                if let Some(finished) = row.take() {
                    if !finished.is_empty() {
                        rows.push(finished);
                    }
                }
            }
            ("td" | "th", false) if table_depth == 1 => {
                if row.is_none() {
                    row = Some(Vec::new());
                }
                let colspan = attribute_usize(&tag, "colspan");
                let rowspan = attribute_usize(&tag, "rowspan");
                if tag.self_closing {
                    if let Some(current) = row.as_mut() {
                        current.push(RawCell {
                            text: String::new(),
                            colspan,
                            rowspan,
                        });
                    }
                } else {
                    cell = Some((tag_end, colspan, rowspan));
                }
            }
            ("td" | "th", true) if table_depth == 1 => {
                if let Some((content_start, colspan, rowspan)) = cell.take() {
                    let text = html_to_text(&html[content_start..tag_start]);
                    if let Some(current) = row.as_mut() {
                        current.push(RawCell {
                            text,
                            colspan,
                            rowspan,
                        });
                    }
                }
            }
            _ => {}
        }

        index = tag_end;
    }

    if let Some(mut finished) = row {
        if let Some((content_start, colspan, rowspan)) = cell.take() {
            let text = html_to_text(&html[content_start..]);
            finished.push(RawCell {
                text,
                colspan,
                rowspan,
            });
        }
        if !finished.is_empty() {
            rows.push(finished);
        }
    }
    rows
}

fn attribute_usize(tag: &TagInfo, name: &str) -> usize {
    tag.attributes
        .iter()
        .find(|(attribute, _)| attribute == name)
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 100)
}

fn escape_csv_field(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_table_becomes_csv_and_escapes_commas() {
        let html = r#"<table><tr><th>Товар</th><th>Сумма</th></tr>
            <tr><td>Болт, М8</td><td>100</td></tr></table>"#;
        assert_eq!(
            table_to_csv(html).unwrap(),
            "Товар,Сумма\n\"Болт, М8\",100\n"
        );
    }

    #[test]
    fn colspan_and_rowspan_fill_empty_cells() {
        let colspan = r#"<table><tr><td colspan="2">Итого</td></tr>
            <tr><td>A</td><td>B</td></tr></table>"#;
        assert_eq!(table_to_csv(colspan).unwrap(), "Итого,\nA,B\n");

        let rowspan = r#"<table><tr><td rowspan="2">A</td><td>B</td></tr>
            <tr><td>C</td></tr></table>"#;
        assert_eq!(table_to_csv(rowspan).unwrap(), "A,B\n,C\n");
    }

    #[test]
    fn quotes_are_doubled_and_newlines_quoted() {
        let html = r#"<table><tr><td>Он сказал "да"</td><td>a<br>b</td></tr></table>"#;
        assert_eq!(
            table_to_csv(html).unwrap(),
            "\"Он сказал \"\"да\"\"\",a b\n"
        );
        assert_eq!(escape_csv_field("a\nb"), "\"a\nb\"");
    }

    #[test]
    fn nested_table_is_flattened_into_the_cell() {
        let html =
            r#"<table><tr><td>outer<table><tr><td>inner</td></tr></table></td></tr></table>"#;
        let csv = table_to_csv(html).unwrap();
        assert!(csv.contains("outer"), "{csv}");
        assert!(csv.contains("inner"), "{csv}");
    }

    #[test]
    fn no_table_returns_none() {
        assert!(table_to_csv("<p>no table</p>").is_none());
        assert!(table_to_csv("<table></table>").is_none());
    }
}
