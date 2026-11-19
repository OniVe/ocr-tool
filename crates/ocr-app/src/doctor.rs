use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ocr_core::{discover_model_paths, model_dir_source, resolve_model_dir, MODEL_ENV};
use ocr_engine::{inspect_candidates, CandidateInfo, EngineConfig};
use serde::Serialize;

use crate::download::{human_bytes, ModelFile, MODEL_FILES};

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub path: String,
    pub detail: String,
    pub version: Option<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlamaCheck {
    pub exe: String,
    pub device: String,
    pub source: String,
    pub exists: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub tool_version: String,
    pub os: String,
    pub arch: String,
    pub exe: String,
    pub cwd: String,
    pub model_dir: String,
    pub model_dir_source: String,
    pub model: Check,
    pub mmproj: Check,
    pub pdfium: Check,
    pub llama_candidates: Vec<LlamaCheck>,
    pub llama_searched: Vec<String>,
    pub env: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

/// Collect a non-fatal component report. Missing components are represented in
/// the report instead of making `doctor` itself fail.
pub fn doctor(model_dir_arg: Option<&Path>, llama_dir: Option<&Path>) -> DoctorReport {
    let env_model = std::env::var_os(MODEL_ENV);
    let model_dir = resolve_model_dir(model_dir_arg, env_model.as_deref());
    let source = model_dir_source(model_dir_arg, env_model.as_deref());
    let mut warnings = Vec::new();

    let (model_check, mmproj_check) = match discover_model_paths(&model_dir) {
        Ok(paths) => (
            model_file_check("model", &paths.model, &MODEL_FILES[0]),
            model_file_check("mmproj", &paths.mmproj, &MODEL_FILES[1]),
        ),
        Err(error) => {
            warnings.push(format!("{error:#}"));
            (
                Check {
                    name: "model".to_string(),
                    ok: false,
                    path: model_dir.display().to_string(),
                    detail: error.to_string(),
                    version: None,
                    size_bytes: None,
                },
                Check {
                    name: "mmproj".to_string(),
                    ok: false,
                    path: model_dir.display().to_string(),
                    detail: "model directory not resolved".to_string(),
                    version: None,
                    size_bytes: None,
                },
            )
        }
    };
    for check in [&model_check, &mmproj_check] {
        if !check.ok && check.size_bytes.is_some() {
            warnings.push(format!(
                "{}: {} (run `ocr-tool models verify --deep` for a SHA-256 check)",
                check.name, check.detail
            ));
        }
    }

    let pdfium = match ocr_pdf::locate_pdfium() {
        Ok(path) => {
            let mut check = file_check("pdfium", &path);
            check.version = file_version(&path);
            check
        }
        Err(error) => {
            warnings.push(format!("{error:#}"));
            Check {
                name: "pdfium".to_string(),
                ok: false,
                path: String::new(),
                detail: error.to_string(),
                version: None,
                size_bytes: None,
            }
        }
    };

    let mut engine_config = EngineConfig::new(&model_dir);
    engine_config.llama_dir = llama_dir.map(Path::to_path_buf);
    let (candidates, searched, discovery_warnings) = inspect_candidates(&engine_config);
    warnings.extend(discovery_warnings);
    let gpu = candidates
        .iter()
        .filter(|c| c.device == ocr_core::Device::Gpu)
        .count();
    let cpu = candidates
        .iter()
        .filter(|c| c.device == ocr_core::Device::Cpu)
        .count();
    if candidates.is_empty() {
        warnings.push(
            "llama-server not found: set OCR_LLAMA_SERVER, --llama-dir, or install runtime/"
                .to_string(),
        );
    } else if gpu == 0 {
        warnings.push("no GPU llama-server build found; GPU OCR is unavailable".to_string());
    }
    if cpu == 0 {
        warnings.push("no CPU llama-server build found; CPU fallback is unavailable".to_string());
    }

    let env = [
        "OCR_LLAMA_SERVER",
        "OCR_MODEL_DIR",
        "OCR_PDFIUM",
        "HF_ENDPOINT",
    ]
    .iter()
    .map(|name| {
        (
            (*name).to_string(),
            std::env::var(name).unwrap_or_else(|_| "<unset>".to_string()),
        )
    })
    .collect();

    DoctorReport {
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        exe: std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        cwd: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        model_dir: model_dir.display().to_string(),
        model_dir_source: source.to_string(),
        model: model_check,
        mmproj: mmproj_check,
        pdfium,
        llama_candidates: candidates
            .into_iter()
            .map(
                |CandidateInfo {
                     exe,
                     device,
                     source,
                 }| {
                    let exists = exe.is_file();
                    let version = if exists {
                        command_version(&exe, Duration::from_secs(5))
                    } else {
                        None
                    };
                    LlamaCheck {
                        exists,
                        exe: exe.display().to_string(),
                        device: device.to_string(),
                        source,
                        version,
                    }
                },
            )
            .collect(),
        llama_searched: searched,
        env,
        warnings,
    }
}

fn file_check(name: &str, path: &Path) -> Check {
    let metadata = std::fs::metadata(path).ok();
    let exists = metadata.as_ref().map(|m| m.is_file()).unwrap_or(false);
    Check {
        name: name.to_string(),
        ok: exists,
        path: path.display().to_string(),
        detail: if exists {
            human_bytes(metadata.as_ref().map(|m| m.len()).unwrap_or(0))
        } else {
            "not found".to_string()
        },
        version: None,
        size_bytes: metadata.map(|m| m.len()),
    }
}

/// A model file check that also compares the size with the pinned download.
/// This catches a 1 KiB placeholder without hashing 3 GB on every run.
fn model_file_check(name: &str, path: &Path, spec: &ModelFile) -> Check {
    let mut check = file_check(name, path);
    if let Some(size) = check.size_bytes {
        if size != spec.size {
            check.ok = false;
            check.detail = format!(
                "{} (expected {}, pinned {})",
                human_bytes(size),
                human_bytes(spec.size),
                spec.name
            );
        }
    }
    check
}

/// Run `llama-server --version` with a hard timeout. The process is killed if
/// it does not exit in time; partial output is still parsed.
fn command_version(exe: &Path, timeout: Duration) -> Option<String> {
    if !exe.is_file() {
        return None;
    }
    let mut child = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut text);
        }
        if let Some(mut err) = stderr {
            let _ = err.read_to_string(&mut text);
        }
        text
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let text = reader.join().ok()?;
    extract_version_line(&text)
}

fn extract_version_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && line.to_ascii_lowercase().contains("version"))
        .map(|line| line.chars().take(200).collect())
}

#[cfg(windows)]
fn file_version(path: &Path) -> Option<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct FixedFileInfo {
        signature: u32,
        struc_version: u32,
        file_version_ms: u32,
        file_version_ls: u32,
        product_version_ms: u32,
        product_version_ls: u32,
        file_flags_mask: u32,
        file_flags: u32,
        file_os: u32,
        file_type: u32,
        file_subtype: u32,
        file_date_ms: u32,
        file_date_ls: u32,
    }

    #[link(name = "version")]
    extern "system" {
        fn GetFileVersionInfoSizeW(name: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoW(name: *const u16, handle: u32, len: u32, data: *mut u8) -> i32;
        fn VerQueryValueW(
            block: *const u8,
            sub_block: *const u16,
            buffer: *mut *mut u8,
            len: *mut u32,
        ) -> i32;
    }

    let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), handle, size, data.as_mut_ptr()) == 0 {
            return None;
        }
        let mut buffer: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        let root: Vec<u16> = "\\\0".encode_utf16().collect();
        if VerQueryValueW(data.as_ptr(), root.as_ptr(), &mut buffer, &mut len) == 0 {
            return None;
        }
        if buffer.is_null() || (len as usize) < std::mem::size_of::<FixedFileInfo>() {
            return None;
        }
        let info = &*(buffer as *const FixedFileInfo);
        if info.signature != 0xFEEF_04BD {
            return None;
        }
        Some(format!(
            "{}.{}.{}.{}",
            info.file_version_ms >> 16,
            info.file_version_ms & 0xFFFF,
            info.file_version_ls >> 16,
            info.file_version_ls & 0xFFFF
        ))
    }
}

#[cfg(not(windows))]
fn file_version(_path: &Path) -> Option<String> {
    None
}

pub fn print_doctor(report: &DoctorReport) {
    println!("ocr-tool {} — component check", report.tool_version);
    println!("platform: {} / {}", report.os, report.arch);
    println!("exe: {}", report.exe);
    println!("cwd: {}", report.cwd);
    println!();
    println!(
        "model dir: {} ({})",
        report.model_dir, report.model_dir_source
    );
    print_check(&report.model);
    print_check(&report.mmproj);
    print_check(&report.pdfium);
    println!();
    if report.llama_candidates.is_empty() {
        println!("llama-server: NOT FOUND");
    } else {
        println!("llama-server candidates:");
        for candidate in &report.llama_candidates {
            println!(
                "  [{}] {} ({}){}",
                candidate.device,
                candidate.exe,
                candidate.source,
                candidate
                    .version
                    .as_deref()
                    .map(|version| format!(" — {version}"))
                    .unwrap_or_default()
            );
        }
    }
    if !report.env.is_empty() {
        println!();
        println!("environment:");
        for (name, value) in &report.env {
            println!("  {name}={value}");
        }
    }
    if !report.warnings.is_empty() {
        println!();
        println!("warnings:");
        for warning in &report.warnings {
            println!("  - {warning}");
        }
    }
}

fn print_check(check: &Check) {
    let mark = if check.ok { "OK  " } else { "MISS" };
    println!(
        "  [{mark}] {}: {}{}{}",
        check.name,
        check.path,
        if check.detail.is_empty() {
            String::new()
        } else {
            format!(" — {}", check.detail)
        },
        check
            .version
            .as_deref()
            .map(|version| format!(" [version {version}]"))
            .unwrap_or_default()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_is_extracted_from_llama_output() {
        let text =
            "load time: 10 ms\nversion: 1.0.0-dev (build 12535, commit abc)\nbuilt with Clang";
        assert_eq!(
            extract_version_line(text).as_deref(),
            Some("version: 1.0.0-dev (build 12535, commit abc)")
        );
        assert!(extract_version_line("no banner here").is_none());
    }

    #[test]
    fn doctor_flags_a_tiny_canonical_model() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(ocr_core::DEFAULT_MODEL_FILE),
            vec![0u8; 1024],
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(ocr_core::DEFAULT_MMPROJ_FILE),
            vec![0u8; 1024],
        )
        .unwrap();
        let report = doctor(
            Some(tmp.path()),
            Some(Path::new("definitely-missing-llama-dir")),
        );
        assert!(!report.model.ok, "1 KiB model must not be [OK]");
        assert!(!report.mmproj.ok, "1 KiB mmproj must not be [OK]");
        assert!(
            report.model.detail.contains("expected"),
            "{}",
            report.model.detail
        );
    }
}
