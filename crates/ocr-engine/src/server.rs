use std::collections::HashSet;
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use ocr_core::{discover_model_paths, platform_tag, runtime_dir_names, Device, ModelPaths};

use crate::protocol::{build_request, parse_chat_response};

pub const LLAMA_ENV: &str = "OCR_LLAMA_SERVER";
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
pub const DEFAULT_CONTEXT_SIZE: u32 = 10_000;
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub model_dir: PathBuf,
    pub llama_dir: Option<PathBuf>,
    pub device: Device,
    pub context_size: u32,
    pub max_tokens: u32,
    pub startup_timeout: Duration,
    pub request_timeout: Duration,
}

impl EngineConfig {
    pub fn new(model_dir: impl Into<PathBuf>) -> Self {
        Self {
            model_dir: model_dir.into(),
            llama_dir: None,
            device: Device::Auto,
            context_size: DEFAULT_CONTEXT_SIZE,
            max_tokens: DEFAULT_MAX_TOKENS,
            startup_timeout: STARTUP_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    exe: PathBuf,
    device: Device,
    source: String,
}

struct Discovery {
    candidates: Vec<Candidate>,
    searched: Vec<String>,
}

/// A running `llama-server` sidecar. One instance is reused for all pages of a
/// job and is killed on Drop (process tree, not only the launcher process).
pub struct OcrServer {
    child: Mutex<Option<Child>>,
    port: u16,
    device_used: Device,
    exe: PathBuf,
    models: ModelPaths,
    log_path: Option<PathBuf>,
    request_timeout: Duration,
    killed: AtomicBool,
}

impl OcrServer {
    pub fn start(
        config: &EngineConfig,
        cancel: &AtomicBool,
        log: &mut dyn FnMut(&str),
    ) -> Result<Self> {
        let models = discover_model_paths(&config.model_dir)?;
        let discovery = discover_candidates(config);
        if discovery.candidates.is_empty() {
            bail!(
                "llama-server executable not found.\n\
                 Set {LLAMA_ENV} to llama-server.exe, pass --llama-dir, or install the \
                 runtime into runtime/{}/llama-vulkan (or llama-cpu).\nSearched: {}",
                platform_tag(),
                discovery.searched.join(", ")
            );
        }

        let mut failures: Vec<String> = Vec::new();
        for candidate in discovery.candidates {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled while starting llama-server");
            }
            log(&format!(
                "starting llama-server ({}, {}) from {}",
                candidate.device,
                candidate.source,
                candidate.exe.display()
            ));
            match Self::start_candidate(config, &models, &candidate, cancel, log) {
                Ok(server) => return Ok(server),
                Err(error) => {
                    let text = format!(
                        "{} ({}): {error:#}",
                        candidate.exe.display(),
                        candidate.device
                    );
                    log(&format!("llama-server candidate failed: {text}"));
                    failures.push(text);
                }
            }
        }

        Err(anyhow!(
            "failed to start llama-server.\n{}",
            failures.join("\n---\n")
        ))
    }

    fn start_candidate(
        config: &EngineConfig,
        models: &ModelPaths,
        candidate: &Candidate,
        cancel: &AtomicBool,
        log: &mut dyn FnMut(&str),
    ) -> Result<Self> {
        let port = free_port()?;
        let (stdout, stderr, log_path) = make_log_files(port);
        // The server runs with its own directory (`current_dir` below), so model
        // arguments must be absolute, otherwise relative --model-dir paths break.
        let model = models
            .model
            .canonicalize()
            .unwrap_or_else(|_| models.model.clone());
        let mmproj = models
            .mmproj
            .canonicalize()
            .unwrap_or_else(|_| models.mmproj.clone());
        let mut command = Command::new(&candidate.exe);
        command
            .arg("-m")
            .arg(&model)
            .arg("--mmproj")
            .arg(&mmproj)
            .arg("-c")
            .arg(config.context_size.to_string())
            .arg("-ngl")
            .arg(if candidate.device == Device::Cpu {
                "0"
            } else {
                "99"
            })
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--no-webui")
            .arg("--reasoning")
            .arg("off")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);

        if let Some(parent) = candidate.exe.parent() {
            command.current_dir(parent);
        }
        configure_child(&mut command);

        let mut child = command.spawn().with_context(|| {
            format!(
                "cannot spawn llama-server {} (missing DLLs or wrong architecture?)",
                candidate.exe.display()
            )
        })?;

        let health_url = format!("http://127.0.0.1:{port}/health");
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .build();
        let deadline = Instant::now() + config.startup_timeout;
        #[allow(unused_assignments)]
        let mut last_status: Option<String> = None;

        loop {
            if cancel.load(Ordering::Relaxed) {
                kill_child_tree(&mut child);
                bail!("cancelled while waiting for llama-server health check");
            }
            if let Some(status) = child
                .try_wait()
                .context("cannot query llama-server status")?
            {
                let tail = tail_of(log_path.as_deref());
                bail!(
                    "llama-server exited during startup with {status}{}",
                    tail.map(|t| format!(":\n{t}")).unwrap_or_default()
                );
            }
            match agent.get(&health_url).call() {
                Ok(response) => {
                    if response.status() == 200 {
                        log(&format!(
                            "llama-server is ready on 127.0.0.1:{port} ({})",
                            candidate.device
                        ));
                        return Ok(Self {
                            child: Mutex::new(Some(child)),
                            port,
                            device_used: candidate.device,
                            exe: candidate.exe.clone(),
                            models: models.clone(),
                            log_path,
                            request_timeout: config.request_timeout,
                            killed: AtomicBool::new(false),
                        });
                    }
                    last_status = Some(format!("HTTP {}", response.status()));
                }
                Err(ureq::Error::Status(code, _)) => {
                    last_status = Some(format!("HTTP {code}"));
                }
                Err(error) => {
                    last_status = Some(format!("{error}"));
                }
            }
            if Instant::now() >= deadline {
                kill_child_tree(&mut child);
                let tail = tail_of(log_path.as_deref());
                bail!(
                    "llama-server did not become healthy within {} s (last health: {}{})",
                    config.startup_timeout.as_secs(),
                    last_status.as_deref().unwrap_or("no response"),
                    tail.map(|t| format!(":\n{t}")).unwrap_or_default()
                );
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn device_used(&self) -> Device {
        self.device_used
    }

    pub fn exe_path(&self) -> &Path {
        &self.exe
    }

    pub fn models(&self) -> &ModelPaths {
        &self.models
    }

    pub fn log_path(&self) -> Option<&Path> {
        self.log_path.as_deref()
    }

    /// Send one image and return the model's raw HTML answer.
    pub fn recognize_jpeg(&self, jpeg: &[u8], max_tokens: u32) -> Result<String> {
        if self.killed.load(Ordering::Relaxed) {
            bail!("llama-server has been stopped");
        }
        let body = build_request("chandra", jpeg, max_tokens).to_string();
        let url = format!("http://127.0.0.1:{}/v1/chat/completions", self.port);
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(self.request_timeout)
            .build();

        let response = match agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(&body)
        {
            Ok(response) => response,
            Err(ureq::Error::Status(code, response)) => {
                let text = response.into_string().unwrap_or_default();
                let text = text.chars().take(800).collect::<String>();
                bail!("llama-server returned HTTP {code}: {text}");
            }
            Err(error) => bail!("request to llama-server failed: {error}"),
        };

        let text = response
            .into_string()
            .context("cannot read llama-server response body")?;
        let answer = parse_chat_response(&text)?;
        Ok(answer)
    }

    /// Kill the whole process tree. Safe to call multiple times.
    pub fn kill(&self) {
        self.killed.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                kill_child_tree(&mut child);
            }
        }
    }
}

impl Drop for OcrServer {
    fn drop(&mut self) {
        self.killed.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                kill_child_tree(&mut child);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CandidateInfo {
    pub exe: PathBuf,
    pub device: Device,
    pub source: String,
}

/// Inspect which llama-server binaries would be tried, without starting one.
/// Used by `ocr-tool doctor`.
pub fn inspect_candidates(config: &EngineConfig) -> (Vec<CandidateInfo>, Vec<String>) {
    let discovery = discover_candidates(config);
    (
        discovery
            .candidates
            .into_iter()
            .map(|c| CandidateInfo {
                exe: c.exe,
                device: c.device,
                source: c.source,
            })
            .collect(),
        discovery.searched,
    )
}

fn discover_candidates(config: &EngineConfig) -> Discovery {
    let mut candidates = Vec::new();
    let mut searched = Vec::new();
    let requested = config.device;
    let exe_name = server_exe_name();

    if let Some(env_value) = std::env::var_os(LLAMA_ENV).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(env_value);
        searched.push(format!("{LLAMA_ENV}={}", path.display()));
        let device = requested_or_inferred(requested, &path);
        if let Some(exe) = resolve_exe_path(&path, exe_name) {
            candidates.push(Candidate {
                exe,
                device,
                source: LLAMA_ENV.to_string(),
            });
        }
    }

    if let Some(dir) = config.llama_dir.as_deref() {
        searched.push(format!("--llama-dir={}", dir.display()));
        let device = requested_or_inferred(requested, dir);
        if let Some(exe) = resolve_exe_path(dir, exe_name) {
            candidates.push(Candidate {
                exe,
                device,
                source: "--llama-dir".to_string(),
            });
        } else if dir.is_dir() {
            for (sub, sub_device) in subdirs(device) {
                let subdir = dir.join(&sub);
                if let Some(exe) = server_in(&subdir, exe_name) {
                    candidates.push(Candidate {
                        exe,
                        device: sub_device,
                        source: format!("--llama-dir/{sub}"),
                    });
                }
            }
        }
    }

    let runtime_names = runtime_dir_names();
    let bases = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf)),
        std::env::current_dir().ok(),
    ];
    for base in bases.into_iter().flatten() {
        for tag in &runtime_names {
            let runtime = base.join("runtime").join(tag);
            for (sub, device) in subdirs(requested) {
                let dir = runtime.join(&sub);
                searched.push(dir.display().to_string());
                if let Some(exe) = server_in(&dir, exe_name) {
                    candidates.push(Candidate {
                        exe,
                        device,
                        source: format!("runtime/{tag}/{sub}"),
                    });
                }
            }
        }
        // Flat layout: runtime/<tag>/llama-server.exe (macOS Metal build).
        for tag in &runtime_names {
            let dir = base.join("runtime").join(tag);
            if let Some(exe) = server_in(&dir, exe_name) {
                candidates.push(Candidate {
                    exe,
                    device: if requested == Device::Cpu {
                        Device::Cpu
                    } else {
                        Device::Gpu
                    },
                    source: format!("runtime/{tag}"),
                });
            }
        }
    }

    // Last resort: PATH. Never treats an unrelated binary as a GPU build.
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(exe_name);
            if candidate.is_file() {
                candidates.push(Candidate {
                    exe: candidate,
                    device: requested_or_inferred(requested, &dir),
                    source: "PATH".to_string(),
                });
            }
        }
    }

    // Filter by explicitly requested device and remove duplicates while
    // preserving discovery order (GPU before CPU for `auto`).
    let mut seen = HashSet::new();
    let mut filtered = Vec::new();
    for candidate in candidates {
        if requested == Device::Cpu && candidate.device != Device::Cpu {
            continue;
        }
        if requested == Device::Gpu && candidate.device != Device::Gpu {
            continue;
        }
        let key = candidate
            .exe
            .canonicalize()
            .unwrap_or_else(|_| candidate.exe.clone())
            .to_string_lossy()
            .to_ascii_lowercase();
        if seen.insert(key) {
            filtered.push(candidate);
        }
    }

    // For auto, GPU candidates must be attempted before CPU candidates.
    filtered.sort_by_key(|c| match (requested, c.device) {
        (Device::Auto, Device::Gpu) => 0,
        (Device::Auto, Device::Cpu) => 1,
        (Device::Auto, Device::Auto) => 0,
        _ => 0,
    });

    Discovery {
        candidates: filtered,
        searched,
    }
}

fn subdirs(requested: Device) -> Vec<(String, Device)> {
    let mut out = Vec::new();
    if requested != Device::Cpu {
        for name in ["llama-vulkan", "llama-metal", "llama-cuda"] {
            out.push((name.to_string(), Device::Gpu));
        }
    }
    if requested != Device::Gpu {
        out.push(("llama-cpu".to_string(), Device::Cpu));
    }
    out
}

fn requested_or_inferred(requested: Device, path: &Path) -> Device {
    if requested != Device::Auto {
        return requested;
    }
    let text = path.to_string_lossy().to_ascii_lowercase();
    if text.contains("cpu") {
        Device::Cpu
    } else {
        Device::Gpu
    }
}

fn resolve_exe_path(path: &Path, exe_name: &str) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    if path.is_dir() {
        if let Some(exe) = server_in(path, exe_name) {
            return Some(exe);
        }
    }
    None
}

fn server_in(dir: &Path, exe_name: &str) -> Option<PathBuf> {
    let candidate = dir.join(exe_name);
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

fn server_exe_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

fn free_port() -> Result<u16> {
    let listener =
        TcpListener::bind("127.0.0.1:0").context("cannot reserve a local port for llama-server")?;
    Ok(listener.local_addr()?.port())
}

fn make_log_files(port: u16) -> (Stdio, Stdio, Option<PathBuf>) {
    let dir = std::env::temp_dir().join("ocr-tool");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("llama-server-{}-{port}.log", std::process::id()));
    match std::fs::File::create(&path) {
        Ok(file) => match file.try_clone() {
            Ok(clone) => (Stdio::from(file), Stdio::from(clone), Some(path)),
            Err(_) => (Stdio::null(), Stdio::null(), None),
        },
        Err(_) => (Stdio::null(), Stdio::null(), None),
    }
}

fn tail_of(path: Option<&Path>) -> Option<String> {
    let path = path?;
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(8192);
    if start > 0 {
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(start)).ok()?;
    }
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).ok()?;
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(windows)]
fn configure_child(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(unix)]
fn configure_child(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(any(windows, unix)))]
fn configure_child(_command: &mut Command) {}

fn kill_child_tree(child: &mut Child) {
    let pid = child.id();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // The child was spawned as its own process-group leader.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_infer_cpu_from_directory() {
        assert_eq!(
            requested_or_inferred(Device::Auto, Path::new("runtime/win/llama-cpu")),
            Device::Cpu
        );
        assert_eq!(
            requested_or_inferred(Device::Auto, Path::new("runtime/win/llama-vulkan")),
            Device::Gpu
        );
        assert_eq!(
            requested_or_inferred(Device::Cpu, Path::new("whatever")),
            Device::Cpu
        );
    }

    #[test]
    fn auto_prefers_gpu_then_cpu() {
        assert_eq!(
            subdirs(Device::Auto)
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["llama-vulkan", "llama-metal", "llama-cuda", "llama-cpu"]
        );
    }

    #[test]
    fn no_candidates_is_error_with_hint() {
        let mut config = EngineConfig::new("definitely-missing-model-dir");
        config.llama_dir = Some(PathBuf::from("definitely-missing-llama-dir"));
        // Model discovery fails first, which is also a clean error.
        let error = match OcrServer::start(&config, &AtomicBool::new(false), &mut |_| {}) {
            Ok(_) => panic!("expected model discovery to fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("model"));
    }
}
