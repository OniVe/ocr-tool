//! Pinned runtime downloader: llama.cpp builds plus PDFium.
//!
//! The GUI and `ocr-tool runtime download` install the same layout the release
//! package ships in `runtime/<tag>`:
//!
//! ```text
//! <runtime>/llama-vulkan/llama-server[.exe] + libraries
//! <runtime>/llama-cpu/...
//! <runtime>/pdfium/bin|lib/...
//! ```
//!
//! Every archive is pinned by size and SHA-256, downloaded to a `.part` file
//! (resumable), verified before extraction, and unpacked into a temporary
//! directory that is renamed into place. Archive paths are validated against
//! directory traversal and executable bits are preserved on Unix.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::download::sha256_file;

/// llama.cpp build pinned by the runtime downloads.
pub const LLAMA_BUILD: &str = "b12535";
/// PDFium build pinned by the runtime downloads.
pub const PDFIUM_BUILD: &str = "8165";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeComponent {
    LlamaVulkan,
    LlamaCpu,
    LlamaMetal,
    Pdfium,
}

impl RuntimeComponent {
    /// Stable identifier used by progress messages and CLI/GUI text.
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeComponent::LlamaVulkan => "llama-vulkan",
            RuntimeComponent::LlamaCpu => "llama-cpu",
            RuntimeComponent::LlamaMetal => "llama-metal",
            RuntimeComponent::Pdfium => "pdfium",
        }
    }

    pub fn dir_name(self) -> &'static str {
        self.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeAsset {
    pub component: RuntimeComponent,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub archive: ArchiveKind,
    /// Directory under the runtime root (`llama-vulkan`, `pdfium`, ...).
    pub dest: &'static str,
    /// Top-level directory inside the archive to strip (`llama-b12535`).
    pub strip_top: Option<&'static str>,
    /// macOS ships one universal Metal build; it is also copied to this
    /// directory so CPU fallback finds a `llama-cpu` candidate.
    pub also_copy_to: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct RuntimeProgress {
    pub component: RuntimeComponent,
    pub downloaded: u64,
    pub total: u64,
    pub done: bool,
    pub skipped: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeInstall {
    pub dir: PathBuf,
    pub llama_gpu: PathBuf,
    pub llama_cpu: PathBuf,
    pub pdfium: PathBuf,
    /// Bytes actually fetched during this run (0 for verify or a full skip).
    pub downloaded_bytes: u64,
}

/// Asset table for the current build target. Unsupported targets report an
/// actionable error instead of guessing a URL.
pub fn runtime_assets() -> Result<Vec<RuntimeAsset>> {
    runtime_assets_for(ocr_core::RUNTIME_TAG)
}

/// Asset table for an explicit platform tag. Kept public so the tables of all
/// supported platforms can be tested on any build host.
pub fn runtime_assets_for(tag: &str) -> Result<Vec<RuntimeAsset>> {
    let assets = match tag {
        "windows-x64" => vec![
            llama_asset(
                RuntimeComponent::LlamaVulkan,
                "https://github.com/ggml-org/llama.cpp/releases/download/b12535/llama-b12535-bin-win-vulkan-x64.zip",
                "2bb0ca4bc24f3d5442891ddf310cd6372c1591cfea1cbb08952f08a7f705c803",
                39_456_320,
                ArchiveKind::Zip,
            ),
            llama_asset(
                RuntimeComponent::LlamaCpu,
                "https://github.com/ggml-org/llama.cpp/releases/download/b12535/llama-b12535-bin-win-cpu-x64.zip",
                "595522a63267853c71460c0d5f021d44a0642852f28e65532ddaab65c584aab1",
                25_696_689,
                ArchiveKind::Zip,
            ),
            pdfium_asset(
                "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8165/pdfium-win-x64.tgz",
                "ca7338ae7779b8b1b82f4580305c4d762d214f8df1d54b47647fe3cc75989ded",
                4_043_420,
            ),
        ],
        "linux-x64" => vec![
            llama_asset(
                RuntimeComponent::LlamaVulkan,
                "https://github.com/ggml-org/llama.cpp/releases/download/b12535/llama-b12535-bin-ubuntu-vulkan-x64.tar.gz",
                "35da5b8e8dad996ed0d7c423188a72eaf71511231487d7b1e94e7319900d7cc2",
                37_487_740,
                ArchiveKind::TarGz,
            ),
            llama_asset(
                RuntimeComponent::LlamaCpu,
                "https://github.com/ggml-org/llama.cpp/releases/download/b12535/llama-b12535-bin-ubuntu-x64.tar.gz",
                "5cc6b603996adfe38108d3c76e37308736b034c959ca1c0353cd82af8db305d5",
                23_899_232,
                ArchiveKind::TarGz,
            ),
            pdfium_asset(
                "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8165/pdfium-linux-x64.tgz",
                "ba7865e16c0271daab75256790be06c26272190afe6e2a70eae0bb39bb99089c",
                3_951_977,
            ),
        ],
        "macos-arm64" => vec![
            llama_asset(
                RuntimeComponent::LlamaMetal,
                "https://github.com/ggml-org/llama.cpp/releases/download/b12535/llama-b12535-bin-macos-arm64.tar.gz",
                "3486ab83d0e561e1771b1eafc6719804d89fb74e81c47aab7c1a418c96e6b532",
                17_335_652,
                ArchiveKind::TarGz,
            ),
            pdfium_asset(
                "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8165/pdfium-mac-univ.tgz",
                "80d88c4bec1876e04448523b76d1a70ece00bbc33e15301419d4fb9d2f8cc895",
                7_394_247,
            ),
        ],
        "macos-x64" => vec![
            llama_asset(
                RuntimeComponent::LlamaMetal,
                "https://github.com/ggml-org/llama.cpp/releases/download/b12535/llama-b12535-bin-macos-x64.tar.gz",
                "f2a73ce33821adec95fb123d2ac8776ac0f70fab47c64bb1a84ccdcf4f96cd67",
                13_109_389,
                ArchiveKind::TarGz,
            ),
            pdfium_asset(
                "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8165/pdfium-mac-univ.tgz",
                "80d88c4bec1876e04448523b76d1a70ece00bbc33e15301419d4fb9d2f8cc895",
                7_394_247,
            ),
        ],
        other => bail!(
            "runtime downloads are not available for platform '{other}' \
             (supported: windows-x64, linux-x64, macos-arm64, macos-x64); \
             use the release package or scripts/fetch-runtime"
        ),
    };
    Ok(assets)
}

fn llama_asset(
    component: RuntimeComponent,
    url: &'static str,
    sha256: &'static str,
    size: u64,
    archive: ArchiveKind,
) -> RuntimeAsset {
    let (dest, also_copy_to) = match component {
        RuntimeComponent::LlamaMetal => ("llama-metal", Some("llama-cpu")),
        other => (other.dir_name(), None),
    };
    RuntimeAsset {
        component,
        url,
        sha256,
        size,
        archive,
        dest,
        strip_top: Some("llama-b12535"),
        also_copy_to,
    }
}

fn pdfium_asset(url: &'static str, sha256: &'static str, size: u64) -> RuntimeAsset {
    RuntimeAsset {
        component: RuntimeComponent::Pdfium,
        url,
        sha256,
        size,
        archive: ArchiveKind::TarGz,
        dest: "pdfium",
        strip_top: None,
        also_copy_to: None,
    }
}

/// Approximate decimal size of the whole runtime download for the current
/// platform; the GUI uses it in the `Download runtime (~N MB)` button.
pub fn runtime_download_mb() -> Option<u64> {
    runtime_assets()
        .ok()
        .map(|assets| (assets.iter().map(|asset| asset.size).sum::<u64>() + 500_000) / 1_000_000)
}

pub fn server_exe_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// Download and install every pinned runtime component into `dir`.
///
/// Existing valid components are skipped unless `force` is set. Archives are
/// cached in `<dir>/.downloads` while `.part` files make interrupted transfers
/// resumable; a cancelled download keeps the `.part` file.
pub fn download_runtime(
    dir: &Path,
    force: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(RuntimeProgress),
) -> Result<RuntimeInstall> {
    if cancel.load(Ordering::Relaxed) {
        bail!(
            "runtime download cancelled before it started; partial .part files are kept for resume"
        );
    }
    let assets = runtime_assets()?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("cannot create runtime directory {}", dir.display()))?;
    let downloads = dir.join(".downloads");
    std::fs::create_dir_all(&downloads)
        .with_context(|| format!("cannot create download cache {}", downloads.display()))?;

    let mut downloaded_bytes = 0u64;
    for asset in &assets {
        if cancel.load(Ordering::Relaxed) {
            bail!(
                "runtime download cancelled; installed components are kept and .part files resume"
            );
        }
        if !force && asset_fully_installed(dir, asset) {
            progress(RuntimeProgress {
                component: asset.component,
                downloaded: asset.size,
                total: asset.size,
                done: true,
                skipped: true,
            });
            continue;
        }
        // On macOS the same archive feeds llama-metal and llama-cpu. If the
        // Metal copy is still valid, only the CPU copy needs restoring.
        if !force && asset.also_copy_to.is_some() && asset_roots_ready(dir, asset) {
            let copy = dir.join(asset.also_copy_to.unwrap());
            let tmp = staging_dir(dir, &format!("{}-copy", asset.dest));
            copy_dir_all(&dir.join(asset.dest), &tmp)?;
            install_extracted(&tmp, &copy)?;
            progress(RuntimeProgress {
                component: asset.component,
                downloaded: asset.size,
                total: asset.size,
                done: true,
                skipped: true,
            });
            continue;
        }

        let archive = downloads.join(file_name_from_url(asset.url));
        if force {
            let _ = std::fs::remove_file(&archive);
            let _ = std::fs::remove_file(part_path(&archive));
        }
        let mut need_fetch = true;
        if archive.is_file() {
            let size = std::fs::metadata(&archive)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            if size == asset.size && sha256_file(&archive)?.eq_ignore_ascii_case(asset.sha256) {
                need_fetch = false;
            } else {
                let _ = std::fs::remove_file(&archive);
                let _ = std::fs::remove_file(part_path(&archive));
            }
        }
        if need_fetch {
            fetch_asset(asset, &archive, cancel, progress)?;
            downloaded_bytes += asset.size;
        }
        extract_and_install(asset, &archive, dir, cancel)?;
        // The archive is only a cache; keeping 70 MB next to the installed
        // runtime buys nothing now that the component is ready.
        let _ = std::fs::remove_file(&archive);
        progress(RuntimeProgress {
            component: asset.component,
            downloaded: asset.size,
            total: asset.size,
            done: true,
            skipped: false,
        });
    }

    let mut install = verify_runtime(dir)?;
    install.downloaded_bytes = downloaded_bytes;
    Ok(install)
}

/// Check an installed runtime without touching the network. Returns the paths
/// the engine and PDFium need, or an error naming the missing component.
pub fn verify_runtime(dir: &Path) -> Result<RuntimeInstall> {
    let assets = runtime_assets()?;
    let mut missing = Vec::new();
    for asset in &assets {
        if !asset_fully_installed(dir, asset) {
            missing.push(asset.component.as_str());
        }
    }
    if !missing.is_empty() {
        bail!(
            "runtime in {} is incomplete or missing: {}; run `ocr-tool runtime download --dir {}` \
             (or the GUI Download runtime button)",
            dir.display(),
            missing.join(", "),
            dir.display()
        );
    }

    let gpu = assets
        .iter()
        .find(|asset| {
            matches!(
                asset.component,
                RuntimeComponent::LlamaVulkan | RuntimeComponent::LlamaMetal
            )
        })
        .context("runtime table has no GPU llama-server component")?;
    let pdfium_asset = assets
        .iter()
        .find(|asset| asset.component == RuntimeComponent::Pdfium)
        .context("runtime table has no PDFium component")?;
    let pdfium = pdfium_library_in(&dir.join(pdfium_asset.dest)).with_context(|| {
        format!(
            "PDFium library not found under {}",
            dir.join(pdfium_asset.dest).display()
        )
    })?;

    Ok(RuntimeInstall {
        dir: dir.to_path_buf(),
        llama_gpu: dir.join(gpu.dest),
        llama_cpu: dir.join("llama-cpu"),
        pdfium,
        downloaded_bytes: 0,
    })
}

fn file_name_from_url(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or("runtime-archive")
}

fn server_exe_path(root: &Path, asset: &RuntimeAsset) -> PathBuf {
    root.join(asset.dest).join(server_exe_name())
}

fn asset_roots_ready(root: &Path, asset: &RuntimeAsset) -> bool {
    match asset.component {
        RuntimeComponent::Pdfium => pdfium_library_in(&root.join(asset.dest)).is_some(),
        _ => server_exe_path(root, asset).is_file(),
    }
}

fn asset_fully_installed(root: &Path, asset: &RuntimeAsset) -> bool {
    if !asset_roots_ready(root, asset) {
        return false;
    }
    match asset.also_copy_to {
        Some(copy) => root.join(copy).join(server_exe_name()).is_file(),
        None => true,
    }
}

fn pdfium_library_in(dir: &Path) -> Option<PathBuf> {
    for base in [dir.to_path_buf(), dir.join("bin"), dir.join("lib")] {
        for name in ocr_pdf::pdfium_library_names() {
            let candidate = base.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn part_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
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
    })
}

fn resume_offset(part_len: u64, expected: u64) -> u64 {
    if part_len > expected {
        0
    } else {
        part_len
    }
}

fn fetch_asset(
    asset: &RuntimeAsset,
    target: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(RuntimeProgress),
) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!(
            "runtime download cancelled before {} started; a partial .part file is kept for resume",
            asset.component.as_str()
        );
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let part = part_path(target);
    let part_len = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut existing = resume_offset(part_len, asset.size);
    if part_len > asset.size {
        let _ = std::fs::remove_file(&part);
    }

    if existing == asset.size {
        let sha = sha256_file(&part)?;
        if sha.eq_ignore_ascii_case(asset.sha256) {
            finish_part(&part, target)?;
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
    let mut request = agent.get(asset.url);
    if existing > 0 {
        request = request.set("Range", &format!("bytes={existing}-"));
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::Status(416, _)) if existing == asset.size => {
            let sha = sha256_file(&part)?;
            if sha.eq_ignore_ascii_case(asset.sha256) {
                finish_part(&part, target)?;
                return Ok(());
            }
            let _ = std::fs::remove_file(&part);
            bail!(
                "resumed {} is complete but has the wrong SHA-256; rerun the download",
                asset.component.as_str()
            );
        }
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            let body = body.chars().take(300).collect::<String>();
            bail!(
                "download of {} returned HTTP {code}: {body}",
                asset.component.as_str()
            );
        }
        Err(error) => bail!("download of {} failed: {error}", asset.component.as_str()),
    };

    let status = response.status();
    if status != 200 && status != 206 {
        bail!(
            "unexpected HTTP {status} while downloading {}",
            asset.component.as_str()
        );
    }

    let append = status == 206 && existing > 0;
    let base = if append { existing } else { 0 };
    let content_length = response
        .header("content-length")
        .and_then(|value| value.parse::<u64>().ok());
    let total = if status == 206 {
        content_length
            .map(|length| base + length)
            .or(Some(asset.size))
    } else {
        content_length.or(Some(asset.size))
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
    progress(RuntimeProgress {
        component: asset.component,
        downloaded,
        total: total.unwrap_or(asset.size),
        done: false,
        skipped: false,
    });
    loop {
        let read = reader.read(&mut buffer).with_context(|| {
            format!(
                "network error while downloading {}",
                asset.component.as_str()
            )
        })?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .with_context(|| format!("cannot write {}", part.display()))?;
        downloaded += read as u64;
        if last_report.elapsed() >= Duration::from_millis(500) {
            last_report = Instant::now();
            progress(RuntimeProgress {
                component: asset.component,
                downloaded,
                total: total.unwrap_or(asset.size),
                done: false,
                skipped: false,
            });
        }
        if cancel.load(Ordering::Relaxed) {
            file.flush().ok();
            drop(file);
            bail!(
                "download of {} cancelled after {} of {} bytes; the .part file is kept for resume",
                asset.component.as_str(),
                downloaded,
                asset.size
            );
        }
        if downloaded > asset.size {
            let _ = std::fs::remove_file(&part);
            bail!(
                "{} is larger than expected ({} > {} bytes); the remote file changed",
                asset.component.as_str(),
                downloaded,
                asset.size
            );
        }
    }
    file.flush().ok();
    drop(file);

    if downloaded != asset.size {
        bail!(
            "download of {} ended at {} of {} bytes after {:?}; rerun to resume",
            asset.component.as_str(),
            downloaded,
            asset.size,
            started.elapsed()
        );
    }

    let sha = sha256_file(&part)?;
    if !sha.eq_ignore_ascii_case(asset.sha256) {
        let _ = std::fs::remove_file(&part);
        bail!(
            "SHA-256 mismatch for {}: expected {}, got {}; the partial file was removed",
            asset.component.as_str(),
            asset.sha256,
            sha
        );
    }
    finish_part(&part, target)?;
    Ok(())
}

fn staging_dir(runtime: &Path, label: &str) -> PathBuf {
    runtime.join(format!(".install-{}-{}", label, std::process::id()))
}

fn extract_and_install(
    asset: &RuntimeAsset,
    archive: &Path,
    runtime: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    let tmp = staging_dir(runtime, asset.dest);
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)
        .with_context(|| format!("cannot create temporary directory {}", tmp.display()))?;
    let extracted = match asset.archive {
        ArchiveKind::Zip => extract_zip(archive, &tmp, asset.strip_top, cancel),
        ArchiveKind::TarGz => extract_tar_gz(archive, &tmp, asset.strip_top, cancel),
    };
    if let Err(error) = extracted {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(error).with_context(|| {
            format!(
                "cannot extract {} from {}",
                asset.component.as_str(),
                archive.display()
            )
        });
    }

    let installed = match asset.component {
        RuntimeComponent::Pdfium => pdfium_library_in(&tmp).is_some(),
        _ => tmp.join(server_exe_name()).is_file(),
    };
    if !installed {
        let _ = std::fs::remove_dir_all(&tmp);
        bail!(
            "{} archive {} does not contain {}",
            asset.component.as_str(),
            archive.display(),
            if asset.component == RuntimeComponent::Pdfium {
                "the PDFium library"
            } else {
                "llama-server"
            }
        );
    }

    let dest = runtime.join(asset.dest);
    install_extracted(&tmp, &dest)?;
    if let Some(copy) = asset.also_copy_to {
        let copy_tmp = staging_dir(runtime, &format!("{}-copy", asset.dest));
        let _ = std::fs::remove_dir_all(&copy_tmp);
        copy_dir_all(&dest, &copy_tmp)?;
        install_extracted(&copy_tmp, &runtime.join(copy))?;
    }
    Ok(())
}

/// Rename a fully extracted directory into place. An existing runtime
/// component is moved aside first and only removed after the new one is in
/// place, so a failed rename can restore the previous working copy.
fn install_extracted(tmp: &Path, dest: &Path) -> Result<()> {
    let parent = dest
        .parent()
        .with_context(|| format!("{} has no parent directory", dest.display()))?;
    let name = dest
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("runtime");
    let backup = parent.join(format!(".old-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&backup);

    let had_dest = dest.exists();
    if had_dest {
        std::fs::rename(dest, &backup).with_context(|| {
            format!(
                "cannot move the existing {} aside before replacing it",
                dest.display()
            )
        })?;
    }
    match std::fs::rename(tmp, dest) {
        Ok(()) => {
            if had_dest {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(error) => {
            if had_dest {
                let _ = std::fs::rename(&backup, dest);
            }
            Err(error).with_context(|| format!("cannot install {}", dest.display()))
        }
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("cannot create directory {}", dst.display()))?;
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("cannot read directory {}", src.display()))?
    {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .with_context(|| format!("cannot copy {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Reject absolute paths, Windows prefixes and `..` components. Returns `None`
/// for the archive root entry itself.
fn safe_relative_path(raw: &Path, strip_top: Option<&str>) -> Result<Option<PathBuf>> {
    let mut out = PathBuf::new();
    let mut first = true;
    for component in raw.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.is_empty() {
                    continue;
                }
                if first && strip_top == Some(part.as_ref()) {
                    first = false;
                    continue;
                }
                first = false;
                out.push(part.as_ref());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("archive entry {:?} has an unsafe path", raw.display());
            }
        }
    }
    if out.as_os_str().is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

/// Validate a symlink target: it must stay inside the extraction root. The
/// target is resolved lexically relative to the entry's parent directory.
#[cfg_attr(not(unix), allow(dead_code))]
fn validate_link_target(entry: &Path, target: &Path) -> Result<()> {
    if target.is_absolute() || target.has_root() {
        bail!("symlink {} points outside the archive", entry.display());
    }
    let mut depth = entry
        .parent()
        .map(|parent| parent.components().count())
        .unwrap_or(0) as i64;
    for component in target.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    bail!(
                        "symlink {} -> {} escapes the runtime directory",
                        entry.display(),
                        target.display()
                    );
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("symlink {} has an absolute target", entry.display())
            }
        }
    }
    Ok(())
}

fn write_file(
    out: &Path,
    mut reader: impl Read,
    mode: Option<u32>,
    cancel: &AtomicBool,
) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("runtime extraction cancelled; the partial runtime was not installed");
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create directory {}", parent.display()))?;
    }
    let mut file = File::create(out).with_context(|| format!("cannot create {}", out.display()))?;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("cannot read archive entry for {}", out.display()))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .with_context(|| format!("cannot write {}", out.display()))?;
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(out);
            bail!("runtime extraction cancelled; the partial runtime was not installed");
        }
    }
    file.flush().ok();
    drop(file);
    set_executable(out, mode)?;
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path, mode: Option<u32>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        if mode & 0o111 != 0 {
            let mut permissions = std::fs::metadata(path)
                .with_context(|| format!("cannot read permissions of {}", path.display()))?
                .permissions();
            permissions.set_mode((mode & 0o777) | 0o600);
            std::fs::set_permissions(path, permissions)
                .with_context(|| format!("cannot set executable bit on {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _mode: Option<u32>) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn create_symlink(entry: &Path, target: &Path, link: &Path) -> Result<()> {
    validate_link_target(entry, target)?;
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::os::unix::fs::symlink(target, link)
        .with_context(|| format!("cannot create symlink {}", link.display()))
}

#[cfg(not(unix))]
fn create_symlink(entry: &Path, target: &Path, _link: &Path) -> Result<()> {
    // Windows archives do not use symlinks; refuse rather than silently
    // dropping a file that might be needed.
    bail!(
        "archive contains a symlink {} -> {}; symlinks are not supported on this platform",
        entry.display(),
        target.display()
    )
}

fn extract_zip(
    archive: &Path,
    dest: &Path,
    strip_top: Option<&str>,
    cancel: &AtomicBool,
) -> Result<()> {
    let file = File::open(archive)
        .with_context(|| format!("cannot open archive {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("cannot read zip archive {}", archive.display()))?;
    for index in 0..zip.len() {
        if cancel.load(Ordering::Relaxed) {
            bail!("runtime extraction cancelled; the partial runtime was not installed");
        }
        let mut entry = zip
            .by_index(index)
            .with_context(|| format!("cannot read zip entry #{index}"))?;
        let raw = Path::new(entry.name());
        let Some(relative) = safe_relative_path(raw, strip_top)? else {
            continue;
        };
        let out = dest.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if entry.is_symlink() {
            let mut target = String::new();
            entry.read_to_string(&mut target)?;
            create_symlink(&relative, Path::new(&target), &out)?;
            continue;
        }
        let mode = entry.unix_mode();
        write_file(&out, &mut entry, mode, cancel)?;
    }
    Ok(())
}

fn extract_tar_gz(
    archive: &Path,
    dest: &Path,
    strip_top: Option<&str>,
    cancel: &AtomicBool,
) -> Result<()> {
    let file = File::open(archive)
        .with_context(|| format!("cannot open archive {}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    for entry in tar
        .entries()
        .with_context(|| format!("cannot read tar archive {}", archive.display()))?
    {
        if cancel.load(Ordering::Relaxed) {
            bail!("runtime extraction cancelled; the partial runtime was not installed");
        }
        let mut entry = entry?;
        let raw = entry.path()?.to_path_buf();
        let Some(relative) = safe_relative_path(&raw, strip_top)? else {
            continue;
        };
        let out = dest.join(&relative);
        let entry_type = entry.header().entry_type();
        let mode = entry.header().mode().ok();
        match entry_type {
            tar::EntryType::Directory => {
                std::fs::create_dir_all(&out)?;
            }
            tar::EntryType::Symlink => {
                let target = entry
                    .link_name()?
                    .with_context(|| format!("symlink {} has no target", raw.display()))?
                    .into_owned();
                create_symlink(&relative, &target, &out)?;
            }
            tar::EntryType::Link => {
                let target = entry
                    .link_name()?
                    .with_context(|| format!("hard link {} has no target", raw.display()))?
                    .into_owned();
                let target_rel = safe_relative_path(&target, strip_top)?
                    .with_context(|| format!("hard link {} has an unsafe target", raw.display()))?;
                let target_path = dest.join(target_rel);
                if !target_path.is_file() {
                    bail!(
                        "hard link {} points to {}, which is not in the archive",
                        raw.display(),
                        target.display()
                    );
                }
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::hard_link(&target_path, &out)
                    .with_context(|| format!("cannot create hard link {}", out.display()))?;
            }
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                write_file(&out, &mut entry, mode, cancel)?;
            }
            // Sparse files, devices, FIFOs and PAX/GNU metadata entries are not
            // part of the pinned runtime archives.
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Arc;

    #[test]
    fn asset_table_is_pinned_for_every_supported_platform() {
        let windows = runtime_assets_for("windows-x64").unwrap();
        assert_eq!(windows.len(), 3, "vulkan + cpu + pdfium");
        assert_eq!(windows[0].component, RuntimeComponent::LlamaVulkan);
        assert_eq!(windows[1].component, RuntimeComponent::LlamaCpu);
        assert_eq!(windows[2].component, RuntimeComponent::Pdfium);
        assert_eq!(
            windows[0].url,
            "https://github.com/ggml-org/llama.cpp/releases/download/b12535/llama-b12535-bin-win-vulkan-x64.zip"
        );
        assert_eq!(windows[0].size, 39_456_320);
        assert_eq!(
            windows[0].sha256,
            "2bb0ca4bc24f3d5442891ddf310cd6372c1591cfea1cbb08952f08a7f705c803"
        );
        assert_eq!(windows[1].size, 25_696_689);
        assert_eq!(windows[2].size, 4_043_420);
        assert!(windows
            .iter()
            .all(|asset| asset.sha256.len() == 64
                && asset.sha256.chars().all(|c| c.is_ascii_hexdigit())));
        assert!(windows
            .iter()
            .all(|asset| asset.strip_top == Some("llama-b12535")
                || asset.component == RuntimeComponent::Pdfium));

        let linux = runtime_assets_for("linux-x64").unwrap();
        assert_eq!(linux.len(), 3);
        assert!(linux[0]
            .url
            .ends_with("llama-b12535-bin-ubuntu-vulkan-x64.tar.gz"));
        assert_eq!(linux[0].archive, ArchiveKind::TarGz);
        assert_eq!(linux[0].size, 37_487_740);
        assert_eq!(linux[1].size, 23_899_232);
        assert_eq!(linux[2].size, 3_951_977);

        for tag in ["macos-arm64", "macos-x64"] {
            let mac = runtime_assets_for(tag).unwrap();
            assert_eq!(mac.len(), 2, "one universal Metal build + PDFium");
            assert_eq!(mac[0].component, RuntimeComponent::LlamaMetal);
            assert_eq!(mac[0].dest, "llama-metal");
            assert_eq!(mac[0].also_copy_to, Some("llama-cpu"));
            assert_eq!(mac[1].component, RuntimeComponent::Pdfium);
            assert_eq!(mac[1].size, 7_394_247);
        }
        assert!(runtime_assets_for("solaris-sparc").is_err());
    }

    #[test]
    fn current_platform_has_an_asset_table_or_a_clear_error() {
        match runtime_assets() {
            Ok(assets) => {
                assert!(!assets.is_empty());
                assert_eq!(assets, runtime_assets_for(ocr_core::RUNTIME_TAG).unwrap());
            }
            Err(error) => {
                assert_eq!(ocr_core::RUNTIME_TAG, "unsupported");
                assert!(error.to_string().contains("not available"));
            }
        }
    }

    #[test]
    fn runtime_size_hint_is_platform_specific() {
        if cfg!(windows) {
            assert_eq!(runtime_download_mb(), Some(69));
        }
    }

    #[test]
    fn traversal_paths_are_rejected() {
        for raw in ["../evil", "a/../../evil", "/etc/passwd", "/evil"] {
            let error = safe_relative_path(Path::new(raw), None).unwrap_err();
            assert!(error.to_string().contains("unsafe path"), "{raw}: {error}");
        }

        // Windows drive/prefix notation is only absolute on Windows. On Unix
        // the same bytes are an ordinary (still root-contained) file name, so
        // extracting it cannot escape the runtime directory.
        let windows_style = safe_relative_path(Path::new("C:\\evil"), None);
        if cfg!(windows) {
            let error = windows_style.unwrap_err();
            assert!(
                error.to_string().contains("unsafe path"),
                "C:\\evil: {error}"
            );
        } else if cfg!(unix) {
            assert_eq!(
                windows_style.unwrap().unwrap(),
                PathBuf::from("C:\\evil"),
                "on Unix C:\\evil is a normal relative name, not an absolute path"
            );
        }

        let absolute = std::env::temp_dir().join("evil");
        assert!(safe_relative_path(&absolute, None).is_err());

        assert_eq!(
            safe_relative_path(
                Path::new("llama-b12535/llama-server.exe"),
                Some("llama-b12535")
            )
            .unwrap()
            .unwrap(),
            PathBuf::from("llama-server.exe")
        );
        assert_eq!(
            safe_relative_path(Path::new("./pdfium/bin/pdfium.dll"), None)
                .unwrap()
                .unwrap(),
            PathBuf::from("pdfium/bin/pdfium.dll")
        );
        assert!(safe_relative_path(Path::new("/"), None).is_err());
        assert!(safe_relative_path(Path::new("."), None).unwrap().is_none());
    }

    #[test]
    fn symlink_targets_must_stay_inside_the_runtime() {
        assert!(validate_link_target(Path::new("lib/libx.so"), Path::new("liby.so")).is_ok());
        assert!(
            validate_link_target(Path::new("lib/libx.so"), Path::new("../liby.so")).is_ok(),
            "one level up from lib/ is still inside the runtime"
        );
        assert!(validate_link_target(Path::new("libx.so"), Path::new("../liby.so")).is_err());
        assert!(validate_link_target(Path::new("libx.so"), Path::new("/etc/passwd")).is_err());
    }

    fn make_zip(entries: &[(&str, &[u8], Option<u32>)]) -> Vec<u8> {
        use zip::write::SimpleFileOptions;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data, mode) in entries {
            let mut options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            if let Some(mode) = mode {
                options = options.unix_permissions(*mode);
            }
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn make_tar_gz(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        let encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (name, data, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(*mode);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_path(name).unwrap();
            header.set_cksum();
            builder.append(&header, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn zip_with_top_directory_is_stripped_and_installed() {
        let bytes = make_zip(&[
            ("llama-b12535/llama-server.exe", b"server", Some(0o755)),
            ("llama-b12535/ggml.dll", b"lib", Some(0o644)),
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("llama.zip");
        std::fs::write(&archive, bytes).unwrap();
        let dest = tmp.path().join("out");
        extract_zip(
            &archive,
            &dest,
            Some("llama-b12535"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(dest.join("llama-server.exe")).unwrap(),
            b"server"
        );
        assert_eq!(std::fs::read(dest.join("ggml.dll")).unwrap(), b"lib");
        assert!(
            !dest.join("llama-b12535").exists(),
            "the top-level directory must be stripped"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dest.join("llama-server.exe"))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0, "executable bit must be preserved");
        }
    }

    #[test]
    fn tar_with_top_directory_is_stripped_and_installed() {
        let bytes = make_tar_gz(&[
            ("llama-b12535/llama-server", b"server", 0o755),
            ("llama-b12535/lib/libllama.so", b"lib", 0o644),
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("llama.tar.gz");
        std::fs::write(&archive, bytes).unwrap();
        let dest = tmp.path().join("out");
        extract_tar_gz(
            &archive,
            &dest,
            Some("llama-b12535"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(std::fs::read(dest.join("llama-server")).unwrap(), b"server");
        assert_eq!(std::fs::read(dest.join("lib/libllama.so")).unwrap(), b"lib");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dest.join("llama-server"))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0, "executable bit must be preserved");
        }
    }

    #[test]
    fn zip_traversal_is_rejected_and_writes_nothing_outside() {
        // The zip writer normalises `..`, so patch a benign name in place. The
        // name lengths match, keeping the local and central headers valid.
        let mut bytes = make_zip(&[("xx/evil.txt", b"evil", Some(0o644))]);
        let benign = b"xx/evil.txt";
        let malicious = b"../evil.txt";
        assert_eq!(benign.len(), malicious.len());
        let mut patched = 0;
        let mut index = 0;
        while index + benign.len() <= bytes.len() {
            if &bytes[index..index + benign.len()] == benign {
                bytes[index..index + benign.len()].copy_from_slice(malicious);
                patched += 1;
                index += benign.len();
            } else {
                index += 1;
            }
        }
        assert_eq!(patched, 2, "local header + central directory");
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("evil.zip");
        std::fs::write(&archive, bytes).unwrap();
        let dest = tmp.path().join("out");
        let error = extract_zip(&archive, &dest, None, &AtomicBool::new(false)).unwrap_err();
        assert!(error.to_string().contains("unsafe path"), "{error}");
        assert!(!tmp.path().join("evil.txt").exists());
    }

    #[test]
    fn tar_traversal_is_rejected_and_writes_nothing_outside() {
        use flate2::write::GzEncoder;
        let encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        // The tar helper refuses `..` via set_path; write the raw header name.
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        let evil = b"../evil.txt";
        header.as_old_mut().name[..evil.len()].copy_from_slice(evil);
        header.set_cksum();
        builder.append(&header, &b"evil"[..]).unwrap();
        let bytes = builder.into_inner().unwrap().finish().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("evil.tar.gz");
        std::fs::write(&archive, bytes).unwrap();
        let dest = tmp.path().join("out");
        let error = extract_tar_gz(&archive, &dest, None, &AtomicBool::new(false)).unwrap_err();
        assert!(error.to_string().contains("unsafe path"), "{error}");
        assert!(!tmp.path().join("evil.txt").exists());
    }

    #[test]
    fn verify_runtime_names_missing_components() {
        let tmp = tempfile::tempdir().unwrap();
        let error = verify_runtime(tmp.path()).unwrap_err().to_string();
        assert!(error.contains("incomplete or missing"), "{error}");
        assert!(error.contains("llama-vulkan") || error.contains("llama-metal"));
        assert!(error.contains("pdfium"));
    }

    fn write_fake_runtime(root: &Path) {
        let assets = runtime_assets().unwrap();
        for asset in &assets {
            match asset.component {
                RuntimeComponent::Pdfium => {
                    let dir = root.join(asset.dest);
                    let name = ocr_pdf::pdfium_library_names()[0];
                    std::fs::create_dir_all(dir.join("bin")).unwrap();
                    std::fs::write(dir.join("bin").join(name), b"fake").unwrap();
                }
                _ => {
                    let dir = root.join(asset.dest);
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(dir.join(server_exe_name()), b"fake").unwrap();
                    if let Some(copy) = asset.also_copy_to {
                        let copy = root.join(copy);
                        std::fs::create_dir_all(&copy).unwrap();
                        std::fs::write(copy.join(server_exe_name()), b"fake").unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn verify_runtime_accepts_the_packaged_layout() {
        let tmp = tempfile::tempdir().unwrap();
        write_fake_runtime(tmp.path());
        let install = verify_runtime(tmp.path()).unwrap();
        assert_eq!(install.dir.as_path(), tmp.path());
        assert!(install.llama_cpu.ends_with("llama-cpu"));
        assert!(install.llama_gpu.ends_with(if cfg!(target_os = "macos") {
            "llama-metal"
        } else {
            "llama-vulkan"
        }));
        assert_eq!(
            install.pdfium,
            tmp.path()
                .join("pdfium/bin")
                .join(ocr_pdf::pdfium_library_names()[0])
        );
    }

    #[test]
    fn installed_runtime_is_skipped_without_network() {
        let tmp = tempfile::tempdir().unwrap();
        write_fake_runtime(tmp.path());
        let mut events = Vec::new();
        let install = download_runtime(tmp.path(), false, &AtomicBool::new(false), &mut |event| {
            events.push(event)
        })
        .unwrap();
        assert_eq!(install.downloaded_bytes, 0);
        assert_eq!(events.len(), runtime_assets().unwrap().len());
        assert!(
            events.iter().all(|event| event.done && event.skipped),
            "every installed component reports skipped: {events:?}"
        );
    }

    #[test]
    fn install_extracted_replaces_an_existing_component() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("runtime").join("llama-cpu");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("old"), b"old").unwrap();
        let staging = tmp.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("new"), b"new").unwrap();
        install_extracted(&staging, &dest).unwrap();
        assert!(!dest.join("old").exists());
        assert_eq!(std::fs::read(dest.join("new")).unwrap(), b"new");
        assert!(!staging.exists());
        let leftovers: Vec<_> = std::fs::read_dir(dest.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".old-"))
            .collect();
        assert!(leftovers.is_empty(), "backup directories must be removed");
    }

    #[test]
    fn resume_offset_restarts_only_for_oversized_parts() {
        assert_eq!(resume_offset(0, 100), 0);
        assert_eq!(resume_offset(50, 100), 50);
        assert_eq!(resume_offset(100, 100), 100);
        assert_eq!(resume_offset(101, 100), 0);
    }

    fn spawn_range_server(
        content: Arc<Vec<u8>>,
        ranges: Arc<std::sync::Mutex<Vec<Option<u64>>>>,
        requests: usize,
    ) -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
        use std::io::{BufRead, BufReader, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..requests {
                let Ok((stream, _)) = listener.accept() else {
                    break;
                };
                let _ = stream.set_nodelay(true);
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let mut start = None;
                loop {
                    let mut header = String::new();
                    match reader.read_line(&mut header) {
                        Ok(0) => break,
                        Ok(_) if header == "\r\n" || header == "\n" => break,
                        Ok(_) => {
                            if let Some(value) = header.to_ascii_lowercase().strip_prefix("range:")
                            {
                                if let Some(bytes) = value.trim().strip_prefix("bytes=") {
                                    start = bytes
                                        .split('-')
                                        .next()
                                        .and_then(|value| value.parse::<u64>().ok());
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                ranges.lock().unwrap().push(start);
                let offset = start.unwrap_or(0).min(content.len() as u64) as usize;
                let body = &content[offset..];
                let mut writer = stream;
                let head = if offset > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\
                         Content-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        body.len(),
                        offset,
                        content.len() as u64 - 1,
                        content.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                };
                if writer.write_all(head.as_bytes()).is_err() {
                    continue;
                }
                let _ = writer.flush();
                for chunk in body.chunks(64 * 1024) {
                    if writer.write_all(chunk).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        });
        (addr, handle)
    }

    fn test_asset(url: &'static str, sha256: &'static str, size: u64) -> RuntimeAsset {
        RuntimeAsset {
            component: RuntimeComponent::LlamaCpu,
            url,
            sha256,
            size,
            archive: ArchiveKind::Zip,
            dest: "llama-cpu",
            strip_top: Some("llama-b12535"),
            also_copy_to: None,
        }
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    #[test]
    fn pre_cancelled_download_keeps_part_without_network() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("asset.zip");
        let part = part_path(&target);
        std::fs::write(&part, b"partial").unwrap();
        let asset = test_asset(
            "http://127.0.0.1:1/asset.zip",
            Box::leak("0".repeat(64).into_boxed_str()),
            100,
        );
        let cancel = AtomicBool::new(true);
        let mut events = Vec::new();
        let error =
            fetch_asset(&asset, &target, &cancel, &mut |event| events.push(event)).unwrap_err();
        assert!(error.to_string().contains("cancelled"), "{error}");
        assert_eq!(std::fs::read(&part).unwrap(), b"partial");
        assert!(!target.exists());
        assert!(events.is_empty(), "no request before the first check");
    }

    #[test]
    fn partial_download_resumes_from_the_part_offset() {
        let total = 512 * 1024usize;
        let content = Arc::new(
            (0..total)
                .map(|index| (index.wrapping_mul(17) % 251) as u8)
                .collect::<Vec<u8>>(),
        );
        let sha = sha256_hex(&content);
        let ranges = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (addr, server) = spawn_range_server(content.clone(), ranges.clone(), 1);
        let url: &'static str = Box::leak(format!("http://{addr}/asset.zip").into_boxed_str());
        let asset = test_asset(url, Box::leak(sha.into_boxed_str()), total as u64);

        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("asset.zip");
        let part = part_path(&target);
        let already = total / 3;
        std::fs::write(&part, &content[..already]).unwrap();

        fetch_asset(&asset, &target, &AtomicBool::new(false), &mut |_| {}).unwrap();
        server.join().unwrap();
        assert_eq!(ranges.lock().unwrap()[0], Some(already as u64));
        assert_eq!(std::fs::read(&target).unwrap(), &content[..]);
        assert!(!part.exists());
    }

    #[test]
    fn sha_mismatch_removes_the_partial_file() {
        let content = Arc::new(vec![7u8; 64 * 1024]);
        let ranges = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (addr, server) = spawn_range_server(content.clone(), ranges, 1);
        let url: &'static str = Box::leak(format!("http://{addr}/asset.zip").into_boxed_str());
        let asset = test_asset(
            url,
            Box::leak("0".repeat(64).into_boxed_str()),
            content.len() as u64,
        );

        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("asset.zip");
        let error = fetch_asset(&asset, &target, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        server.join().unwrap();
        assert!(error.to_string().contains("SHA-256 mismatch"), "{error}");
        assert!(!target.exists());
        assert!(
            !part_path(&target).exists(),
            "a bad archive is not resumable"
        );
    }

    #[test]
    fn cached_archive_is_extracted_without_network() {
        let server = server_exe_name();
        let entry = format!("llama-b12535/{server}");
        let bytes = make_zip(&[(&entry, b"server", None)]);
        let sha = sha256_hex(&bytes);
        let asset = test_asset(
            "http://127.0.0.1:1/unreachable.zip",
            Box::leak(sha.into_boxed_str()),
            bytes.len() as u64,
        );
        let tmp = tempfile::tempdir().unwrap();
        let downloads = tmp.path().join(".downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::write(downloads.join("unreachable.zip"), &bytes).unwrap();

        let archive = downloads.join("unreachable.zip");
        assert_eq!(
            std::fs::metadata(&archive).unwrap().len(),
            asset.size,
            "the cache path is validated by size before extraction"
        );
        extract_and_install(&asset, &archive, tmp.path(), &AtomicBool::new(false)).unwrap();
        assert!(tmp.path().join("llama-cpu").join(server).is_file());
    }
}
