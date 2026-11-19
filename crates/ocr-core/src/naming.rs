use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Allocates collision-free output stems inside one directory.
///
/// `invoice.md` -> `invoice-2.md` when `invoice.md` (or `invoice.html`, or
/// `invoice_meta.json`) already exists, or when another input in the same job
/// already used the stem. Comparison is case-insensitive because Windows and
/// macOS filesystems usually are.
pub struct OutputAllocator {
    dir: PathBuf,
    used: HashSet<String>,
}

impl OutputAllocator {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            used: HashSet::new(),
        }
    }

    pub fn allocate(&mut self, requested: &str) -> String {
        let base = sanitize_stem(requested);
        let mut candidate = base.clone();
        let mut counter = 2u32;
        while self.is_taken(&candidate) {
            candidate = format!("{base}-{counter}");
            counter += 1;
        }
        self.used.insert(candidate.to_lowercase());
        candidate
    }

    fn is_taken(&self, stem: &str) -> bool {
        if self.used.contains(&stem.to_lowercase()) {
            return true;
        }
        for suffix in [".md", ".html", "_meta.json"] {
            if self.dir.join(format!("{stem}{suffix}")).exists() {
                return true;
            }
        }
        false
    }
}

/// Make a source file stem safe as a file name on all supported platforms.
pub fn sanitize_stem(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    for ch in stem.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let out = out.trim().trim_end_matches('.').trim();
    if out.is_empty() {
        "output".to_string()
    } else if is_windows_reserved(out) {
        // `CON.md`, `NUL.html` and friends cannot be created on Windows. The
        // underscore must go into the device-name part: `CON.txt_` is still
        // treated as the CON device.
        match out.split_once('.') {
            Some((base, rest)) => format!("{}_.{}", base.trim_end(), rest),
            None => format!("{out}_"),
        }
    } else {
        out.to_string()
    }
}

/// Windows treats these names (including any extension, e.g. `CON.txt`) as
/// devices, so they can never be used as a file stem.
fn is_windows_reserved(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or(stem).trim_end();
    let upper = base.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = upper.as_bytes();
    bytes.len() == 4
        && (upper.starts_with("COM") || upper.starts_with("LPT"))
        && bytes[3].is_ascii_digit()
        && bytes[3] != b'0'
}

pub fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(sanitize_stem)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "output".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_stem_is_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let mut alloc = OutputAllocator::new(tmp.path());
        assert_eq!(alloc.allocate("invoice"), "invoice");
    }

    #[test]
    fn job_level_collision_gets_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let mut alloc = OutputAllocator::new(tmp.path());
        assert_eq!(alloc.allocate("invoice"), "invoice");
        assert_eq!(alloc.allocate("invoice"), "invoice-2");
        assert_eq!(alloc.allocate("invoice"), "invoice-3");
    }

    #[test]
    fn existing_file_collision_gets_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("act.html"), b"x").unwrap();
        let mut alloc = OutputAllocator::new(tmp.path());
        assert_eq!(alloc.allocate("act"), "act-2");
    }

    #[test]
    fn collision_is_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let mut alloc = OutputAllocator::new(tmp.path());
        assert_eq!(alloc.allocate("Invoice"), "Invoice");
        assert_eq!(alloc.allocate("invoice"), "invoice-2");
    }

    #[test]
    fn unsafe_characters_are_replaced() {
        assert_eq!(sanitize_stem("a:b/c*d?"), "a_b_c_d_");
        assert_eq!(sanitize_stem("  ...  "), "output");
        assert_eq!(sanitize_stem("Счет № 99"), "Счет № 99");
    }

    #[test]
    fn windows_reserved_device_names_get_a_suffix() {
        for name in ["CON", "con", "Prn", "AUX", "NUL", "COM1", "lpt9"] {
            let stem = sanitize_stem(name);
            assert!(stem.ends_with('_'), "{name} -> {stem}");
        }
        assert_eq!(sanitize_stem("CON"), "CON_");
        assert_eq!(sanitize_stem("CON.txt"), "CON_.txt");
        assert_eq!(sanitize_stem("COM0"), "COM0");
        assert_eq!(sanitize_stem("console"), "console");
        assert_eq!(sanitize_stem("COM10"), "COM10");
    }
}
