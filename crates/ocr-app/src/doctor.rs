use std::path::Path;

use ocr_core::{discover_model_paths, model_dir_source, resolve_model_dir, MODEL_ENV};
use ocr_engine::{inspect_candidates, CandidateInfo, EngineConfig};
use serde::Serialize;

use crate::download::human_bytes;

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub path: String,
    pub detail: String,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlamaCheck {
    pub exe: String,
    pub device: String,
    pub source: String,
    pub exists: bool,
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
            file_check("model", &paths.model),
            file_check("mmproj", &paths.mmproj),
        ),
        Err(error) => {
            warnings.push(format!("{error:#}"));
            (
                Check {
                    name: "model".to_string(),
                    ok: false,
                    path: model_dir.display().to_string(),
                    detail: error.to_string(),
                    size_bytes: None,
                },
                Check {
                    name: "mmproj".to_string(),
                    ok: false,
                    path: model_dir.display().to_string(),
                    detail: "model directory not resolved".to_string(),
                    size_bytes: None,
                },
            )
        }
    };

    let pdfium = match ocr_pdf::locate_pdfium() {
        Ok(path) => file_check("pdfium", &path),
        Err(error) => {
            warnings.push(format!("{error:#}"));
            Check {
                name: "pdfium".to_string(),
                ok: false,
                path: String::new(),
                detail: error.to_string(),
                size_bytes: None,
            }
        }
    };

    let mut engine_config = EngineConfig::new(&model_dir);
    engine_config.llama_dir = llama_dir.map(Path::to_path_buf);
    let (candidates, searched) = inspect_candidates(&engine_config);
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
                 }| LlamaCheck {
                    exists: exe.is_file(),
                    exe: exe.display().to_string(),
                    device: device.to_string(),
                    source,
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
        size_bytes: metadata.map(|m| m.len()),
    }
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
                "  [{}] {} ({})",
                candidate.device, candidate.exe, candidate.source
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
        "  [{mark}] {}: {}{}",
        check.name,
        check.path,
        if check.detail.is_empty() {
            String::new()
        } else {
            format!(" — {}", check.detail)
        }
    );
}
