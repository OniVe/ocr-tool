use anyhow::{bail, Result};

/// Parse a 1-based page selection such as `1-5,7,10-12` into a sorted, unique
/// list of zero-based page indices clamped to `total`.
///
/// An empty specification means "all pages".
pub fn parse_page_selection(spec: Option<&str>, total: usize) -> Result<Vec<usize>> {
    let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok((0..total).collect());
    };

    let mut selected = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            bail!("empty page range element in '{spec}'");
        }
        let (start, end) = match part.split_once('-') {
            Some((a, b)) => {
                let a = a.trim().parse::<usize>()?;
                let b = b.trim().parse::<usize>()?;
                (a, b)
            }
            None => {
                let page = part.parse::<usize>()?;
                (page, page)
            }
        };
        if start == 0 || end == 0 {
            bail!("page numbers are 1-based; 0 is not allowed in '{part}'");
        }
        if start > end {
            bail!("invalid page range '{part}': start is greater than end");
        }
        if start > total {
            bail!("page {start} is out of range (document has {total} pages)");
        }
        let end = end.min(total);
        for page in start..=end {
            selected.push(page - 1);
        }
    }
    selected.sort_unstable();
    selected.dedup();
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_pages_when_empty() {
        assert_eq!(parse_page_selection(None, 3).unwrap(), vec![0, 1, 2]);
        assert_eq!(parse_page_selection(Some("  "), 2).unwrap(), vec![0, 1]);
    }

    #[test]
    fn single_ranges_are_zero_based() {
        assert_eq!(
            parse_page_selection(Some("1-2,4"), 5).unwrap(),
            vec![0, 1, 3]
        );
    }

    #[test]
    fn duplicate_and_overlapping_ranges_are_deduplicated() {
        assert_eq!(
            parse_page_selection(Some("3,1-3,2"), 4).unwrap(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn end_is_clamped_to_document_length() {
        assert_eq!(parse_page_selection(Some("2-99"), 3).unwrap(), vec![1, 2]);
    }

    #[test]
    fn invalid_ranges_are_errors() {
        assert!(parse_page_selection(Some("0"), 3).is_err());
        assert!(parse_page_selection(Some("3-1"), 3).is_err());
        assert!(parse_page_selection(Some("4"), 3).is_err());
        assert!(parse_page_selection(Some("1,,2"), 3).is_err());
        assert!(parse_page_selection(Some("a"), 3).is_err());
    }
}
