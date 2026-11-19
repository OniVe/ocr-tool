use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

pub const REPO_URL: &str = "https://huggingface.co/mradermacher/chandra-ocr-2-GGUF";

#[derive(Debug, Clone, Copy)]
pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

/// Pinned model files. SHA-256 values were verified against the local
/// downloads (llama.cpp b12535 + pdfium 8165 acceptance run, 2026-11-19).
pub const MODEL_FILES: [ModelFile; 2] = [
    ModelFile {
        name: ocr_core::DEFAULT_MODEL_FILE,
        url: "https://huggingface.co/mradermacher/chandra-ocr-2-GGUF/resolve/main/chandra-ocr-2.Q4_K_M.gguf?download=true",
        sha256: "29ec72bf3afa35ca13a017efa696c09ba406671f90bcedbcdcf8e884391d3efb",
        size: 3_066_385_440,
    },
    ModelFile {
        name: ocr_core::DEFAULT_MMPROJ_FILE,
        url: "https://huggingface.co/mradermacher/chandra-ocr-2-GGUF/resolve/main/chandra-ocr-2.mmproj-Q8_0.gguf?download=true",
        sha256: "709be8aeda77c5732e58e957367439eae14ebb73b4f4a78b558013d000948bc3",
        size: 366_894_368,
    },
];

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub file_index: usize,
    pub file_count: usize,
    pub file: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub done: bool,
    pub skipped: bool,
}

/// Quick, network-free check used by preflight and `doctor`: the resolved model
/// files must have exactly the pinned size. Hashing 3 GB on every run is not an
/// option; `verify_models(dir, true)` does the full SHA-256 check on demand.
pub fn check_model_sizes(models: &ocr_core::ModelPaths) -> Result<()> {
    for (path, spec) in [
        (&models.model, &MODEL_FILES[0]),
        (&models.mmproj, &MODEL_FILES[1]),
    ] {
        let size = std::fs::metadata(path)
            .with_context(|| format!("cannot read {}", path.display()))?
            .len();
        if size != spec.size {
            bail!(
                "{} has {} bytes, expected {} (pinned {}). Run `ocr-tool models download` \
                 or check the model directory with `ocr-tool models verify`.",
                path.display(),
                size,
                spec.size,
                spec.name
            );
        }
    }
    Ok(())
}

/// Download both GGUF files into `dir`, resuming `.part` files and verifying
/// SHA-256 before the final rename. Existing valid files are skipped.
///
/// Files already present under the names accepted by [`ocr_core::discover_model_paths`]
/// (for example `chandra-q4.gguf` + `mmproj.gguf`) are recognised instead of
/// starting a 3.4 GB download.
pub fn download_models(
    dir: &Path,
    force: bool,
    progress: &mut dyn FnMut(DownloadProgress),
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("cannot create model directory {}", dir.display()))?;
    let resolved = if force {
        None
    } else {
        ocr_core::discover_model_paths(dir).ok()
    };
    let mut result = Vec::new();
    for (index, spec) in MODEL_FILES.iter().enumerate() {
        let target = dir.join(spec.name);
        if force {
            let _ = std::fs::remove_file(&target);
            let _ = std::fs::remove_file(part_path(&target));
        }
        if !target.is_file() {
            if let Some(paths) = &resolved {
                let existing = if index == 0 {
                    &paths.model
                } else {
                    &paths.mmproj
                };
                if existing.is_file() {
                    let size = std::fs::metadata(existing).map(|m| m.len()).unwrap_or(0);
                    if size != spec.size {
                        bail!(
                            "found {} ({} bytes) but the pinned {} is {} bytes; \
                             pass --force to download the pinned file into {}",
                            existing.display(),
                            size,
                            spec.name,
                            spec.size,
                            dir.display()
                        );
                    }
                    let sha = sha256_file(existing)?;
                    if sha.eq_ignore_ascii_case(spec.sha256) {
                        progress(DownloadProgress {
                            file_index: index + 1,
                            file_count: MODEL_FILES.len(),
                            file: existing
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(spec.name)
                                .to_string(),
                            downloaded: size,
                            total: Some(size),
                            done: true,
                            skipped: true,
                        });
                        result.push(existing.clone());
                        continue;
                    }
                    bail!(
                        "found {} with the pinned size but a different SHA-256; \
                         pass --force to download {}",
                        existing.display(),
                        spec.name
                    );
                }
            }
        }
        if target.is_file() {
            let size = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
            if size == spec.size {
                let sha = sha256_file(&target)?;
                if sha.eq_ignore_ascii_case(spec.sha256) {
                    progress(DownloadProgress {
                        file_index: index + 1,
                        file_count: MODEL_FILES.len(),
                        file: spec.name.to_string(),
                        downloaded: size,
                        total: Some(size),
                        done: true,
                        skipped: true,
                    });
                    result.push(target);
                    continue;
                }
            }
            bail!(
                "{} exists but is incomplete or has the wrong SHA-256; rerun with --force to replace it",
                target.display()
            );
        }
        download_one(spec, &target, index, progress)?;
        result.push(target);
    }
    Ok(result)
}

fn download_one(
    spec: &ModelFile,
    target: &Path,
    index: usize,
    progress: &mut dyn FnMut(DownloadProgress),
) -> Result<()> {
    let part = part_path(target);
    let part_len = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut existing = resume_offset(part_len, spec.size);
    if part_len > spec.size {
        let _ = std::fs::remove_file(&part);
    }

    if existing == spec.size {
        let sha = sha256_file(&part)?;
        if sha.eq_ignore_ascii_case(spec.sha256) {
            finish_part(&part, target)?;
            progress(DownloadProgress {
                file_index: index + 1,
                file_count: MODEL_FILES.len(),
                file: spec.name.to_string(),
                downloaded: spec.size,
                total: Some(spec.size),
                done: true,
                skipped: false,
            });
            return Ok(());
        }
        let _ = std::fs::remove_file(&part);
        existing = 0;
    }

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .timeout_write(Duration::from_secs(120))
        .build();
    let mut request = agent.get(spec.url);
    if existing > 0 {
        request = request.set("Range", &format!("bytes={existing}-"));
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::Status(416, _)) if existing == spec.size => {
            // Range already satisfied; verify the .part below.
            let sha = sha256_file(&part)?;
            if sha.eq_ignore_ascii_case(spec.sha256) {
                finish_part(&part, target)?;
                return Ok(());
            }
            let _ = std::fs::remove_file(&part);
            bail!(
                "resumed file {} is complete but has the wrong SHA-256; rerun the command to restart the download",
                part.display()
            );
        }
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            let body = body.chars().take(300).collect::<String>();
            bail!("download {REPO_URL} returned HTTP {code}: {body}");
        }
        Err(error) => bail!("download {REPO_URL} failed: {error}"),
    };

    let status = response.status();
    if status != 200 && status != 206 {
        bail!("unexpected HTTP {status} while downloading {}", spec.url);
    }

    // If the server ignored Range, restart from zero.
    let append = status == 206 && existing > 0;
    let base = if append { existing } else { 0 };
    let content_length = response
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok());
    let total = if status == 206 {
        content_length.map(|len| base + len).or(Some(spec.size))
    } else {
        content_length.or(Some(spec.size))
    };

    let mut file = if append {
        OpenOptions::new()
            .append(true)
            .open(&part)
            .with_context(|| format!("cannot append to {}", part.display()))?
    } else {
        File::create(&part).with_context(|| format!("cannot create {}", part.display()))?
    };

    let mut reader = response.into_reader();
    let mut downloaded = base;
    let mut buffer = vec![0u8; 1024 * 1024];
    let started = Instant::now();
    let mut last_report = Instant::now();
    progress(DownloadProgress {
        file_index: index + 1,
        file_count: MODEL_FILES.len(),
        file: spec.name.to_string(),
        downloaded,
        total,
        done: false,
        skipped: false,
    });
    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("network error while downloading {}", spec.name))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .with_context(|| format!("cannot write {}", part.display()))?;
        downloaded += read as u64;
        if last_report.elapsed() >= Duration::from_millis(500) {
            last_report = Instant::now();
            progress(DownloadProgress {
                file_index: index + 1,
                file_count: MODEL_FILES.len(),
                file: spec.name.to_string(),
                downloaded,
                total,
                done: false,
                skipped: false,
            });
        }
        if downloaded > spec.size {
            bail!(
                "{} is larger than expected ({} > {} bytes); the remote file changed",
                spec.name,
                downloaded,
                spec.size
            );
        }
    }
    file.flush().ok();
    drop(file);

    if downloaded != spec.size {
        bail!(
            "download of {} ended at {} of {} bytes after {:?}; rerun to resume",
            spec.name,
            downloaded,
            spec.size,
            started.elapsed()
        );
    }

    let sha = sha256_file(&part)?;
    if !sha.eq_ignore_ascii_case(spec.sha256) {
        let _ = std::fs::remove_file(&part);
        bail!(
            "SHA-256 mismatch for {}: expected {}, got {}; the partial file was removed",
            spec.name,
            spec.sha256,
            sha
        );
    }
    finish_part(&part, target)?;
    progress(DownloadProgress {
        file_index: index + 1,
        file_count: MODEL_FILES.len(),
        file: spec.name.to_string(),
        downloaded,
        total,
        done: true,
        skipped: false,
    });
    Ok(())
}

fn part_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

/// A `.part` file longer than the pinned file is corrupt; restart from zero.
/// A shorter one is a valid resume point.
fn resume_offset(part_len: u64, expected: u64) -> u64 {
    if part_len > expected {
        0
    } else {
        part_len
    }
}

fn finish_part(part: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        std::fs::remove_file(target)
            .with_context(|| format!("cannot replace {}", target.display()))?;
    }
    std::fs::rename(part, target).with_context(|| {
        format!(
            "cannot move completed download {} to {}",
            part.display(),
            target.display()
        )
    })?;
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Validate an already downloaded model directory without touching the network.
/// `deep` additionally hashes the 3.4 GB of weights; the shallow mode is what
/// preflight and `doctor` use on every run.
pub fn verify_models(dir: &Path, deep: bool) -> Result<()> {
    let models = ocr_core::discover_model_paths(dir)?;
    for (path, spec) in [
        (&models.model, &MODEL_FILES[0]),
        (&models.mmproj, &MODEL_FILES[1]),
    ] {
        if !path.is_file() {
            bail!("missing {}", path.display());
        }
        let size = std::fs::metadata(path)?.len();
        if size != spec.size {
            bail!(
                "{} has {} bytes, expected {}",
                path.display(),
                size,
                spec.size
            );
        }
        if deep {
            let sha = sha256_file(path)?;
            if !sha.eq_ignore_ascii_case(spec.sha256) {
                bail!(
                    "{} SHA-256 mismatch: expected {}, got {}",
                    path.display(),
                    spec.sha256,
                    sha
                );
            }
        }
    }
    Ok(())
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn model_specs_are_pinned() {
        assert_eq!(MODEL_FILES[0].sha256.len(), 64);
        assert_eq!(MODEL_FILES[1].sha256.len(), 64);
        assert!(MODEL_FILES[0].url.starts_with("https://huggingface.co/"));
        assert!(MODEL_FILES[0].size > MODEL_FILES[1].size);
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn verify_models_reports_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let err = verify_models(tmp.path(), false).unwrap_err().to_string();
        assert!(err.contains("gguf") || err.contains("model"), "{err}");
    }

    #[test]
    fn verify_models_size_check_rejects_a_tiny_model() {
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
        let err = verify_models(tmp.path(), false).unwrap_err().to_string();
        assert!(err.contains("expected"), "{err}");
        assert!(err.contains("bytes"), "{err}");
    }

    #[test]
    fn check_model_sizes_flags_a_truncated_file() {
        let tmp = tempfile::tempdir().unwrap();
        let model = tmp.path().join("chandra-q4.gguf");
        let mmproj = tmp.path().join("mmproj.gguf");
        std::fs::write(&model, vec![0u8; 32]).unwrap();
        std::fs::write(&mmproj, vec![0u8; 16]).unwrap();
        let paths = ocr_core::ModelPaths {
            dir: tmp.path().to_path_buf(),
            model,
            mmproj,
        };
        assert!(check_model_sizes(&paths).is_err());
    }

    #[test]
    fn download_models_recognises_renamed_files_and_never_starts_network() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("chandra-q4.gguf"), vec![0u8; 64]).unwrap();
        std::fs::write(tmp.path().join("mmproj.gguf"), vec![0u8; 16]).unwrap();
        let mut events = Vec::new();
        let error = download_models(tmp.path(), false, &mut |event| events.push(event))
            .expect_err("a 64-byte file must not be downloaded over");
        let text = format!("{error:#}");
        assert!(text.contains("pass --force"), "{text}");
        assert!(text.contains("chandra-q4.gguf"), "{text}");
        assert!(events.is_empty(), "progress must not start a download");
    }

    #[test]
    fn resume_offset_restarts_only_for_oversized_parts() {
        assert_eq!(resume_offset(0, 100), 0);
        assert_eq!(resume_offset(50, 100), 50);
        assert_eq!(resume_offset(100, 100), 100);
        assert_eq!(resume_offset(101, 100), 0);
    }

    #[test]
    fn finish_part_replaces_the_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("model.gguf");
        let part = part_path(&target);
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&part, b"new").unwrap();
        finish_part(&part, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!part.exists());
    }
}
