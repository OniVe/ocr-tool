use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub source: String,
    pub outputs: Vec<String>,
    pub pages_total: usize,
    pub pages_ok: usize,
    pub pages_failed: usize,
    pub errors: Vec<String>,
    pub elapsed_ms: u64,
    pub device: String,
    pub model: String,
    pub mmproj: String,
    pub llama_server: String,
    pub dpi: Option<u32>,
    pub format: String,
    /// Extracted PNG crops (absolute paths), when block extraction was enabled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    /// All extracted block files (PNGs and typed md/html/csv/tex/txt).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<String>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMeta {
    pub tool: String,
    pub tool_version: String,
    pub generated_at: String,
    pub output_dir: String,
    pub device_requested: String,
    pub device_used: String,
    pub model_dir: String,
    pub model: String,
    pub mmproj: String,
    pub llama_server: String,
    pub documents: Vec<DocumentMeta>,
}

pub fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// UTC timestamp in RFC 3339 form without pulling in a date/time crate.
pub fn now_rfc3339() -> String {
    format_rfc3339(unix_timestamp_ms())
}

fn format_rfc3339(ms: u128) -> String {
    let total_seconds = (ms / 1000) as i64;
    let millis = (ms % 1000) as u32;
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Howard Hinnant's `civil_from_days` algorithm (days since 1970-01-01).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_formatted_correctly() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn known_timestamp() {
        // 2026-11-19T12:34:56.789Z
        let ms = 1_795_091_696_789u128;
        assert_eq!(format_rfc3339(ms), "2026-11-19T12:34:56.789Z");
    }

    #[test]
    fn now_looks_like_rfc3339() {
        let now = now_rfc3339();
        assert_eq!(now.len(), 24);
        assert_eq!(&now[4..5], "-");
        assert_eq!(&now[10..11], "T");
        assert!(now.ends_with('Z'));
    }
}
