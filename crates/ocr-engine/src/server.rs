use std::collections::HashSet;
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use ocr_core::{discover_model_paths, platform_tag, runtime_dir_names, Device, ModelPaths};

use crate::protocol::{build_request, parse_chat_response};

pub const LLAMA_ENV: &str = "OCR_LLAMA_SERVER";
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
pub const DEFAULT_CONTEXT_SIZE: u32 = 10_000;
pub const DEFAULT_MAX_TOKENS: u32 = 4096;
/// Older llama-server log files kept in `%TEMP%/ocr-tool`.
const KEEP_LOG_FILES: usize = 20;

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
    warnings: Vec<String>,
}

/// A running `llama-server` sidecar. One instance is reused for all pages of a
/// job and is killed on Drop (process tree, not only the launcher process).
///
/// On Windows the child is assigned to a job object with
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so it is killed by the OS even when the
/// parent is terminated with `taskkill /F` or crashes. On Linux the child asks
/// the kernel for `PR_SET_PDEATHSIG` and runs in its own process group.
pub struct OcrServer {
    child: Mutex<Option<Child>>,
    port: u16,
    device_used: Device,
    exe: PathBuf,
    models: ModelPaths,
    log_path: Option<PathBuf>,
    request_timeout: Duration,
    killed: AtomicBool,
    #[cfg(windows)]
    job_handle: std::sync::atomic::AtomicUsize,
}

impl OcrServer {
    pub fn start(
        config: &EngineConfig,
        cancel: &AtomicBool,
        log: &mut dyn FnMut(&str),
    ) -> Result<Self> {
        let models = discover_model_paths(&config.model_dir)?;
        let discovery = discover_candidates(config);
        for warning in &discovery.warnings {
            log(&format!("warning: {warning}"));
        }
        if discovery.candidates.is_empty() {
            let details = if discovery.warnings.is_empty() {
                String::new()
            } else {
                format!("\n{}", discovery.warnings.join("\n"))
            };
            bail!(
                "llama-server executable not found.{details}\n\
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

    /// Try one executable. A port grabbed by an unrelated process is detected by
    /// the unique `--alias`/`/props` identity check; in that case retry once on a
    /// fresh port before moving on to the next candidate.
    fn start_candidate(
        config: &EngineConfig,
        models: &ModelPaths,
        candidate: &Candidate,
        cancel: &AtomicBool,
        log: &mut dyn FnMut(&str),
    ) -> Result<Self> {
        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 1..=2 {
            let port = free_port()?;
            match Self::start_on_port(config, models, candidate, cancel, log, port) {
                Ok(server) => return Ok(server),
                Err(error) => {
                    if attempt < 2 && error.downcast_ref::<IdentityMismatch>().is_some() {
                        log(&format!(
                            "llama-server identity check failed on port {port}: {error}; \
                             retrying on a new port"
                        ));
                        last_error = Some(error);
                        continue;
                    }
                    return Err(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("llama-server did not start")))
    }

    fn start_on_port(
        config: &EngineConfig,
        models: &ModelPaths,
        candidate: &Candidate,
        cancel: &AtomicBool,
        log: &mut dyn FnMut(&str),
        port: u16,
    ) -> Result<Self> {
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
        // A unique alias lets us prove that the process answering on `port` is
        // ours and not another program that won the free-port race.
        let alias = format!("ocr-tool-{}-{}-{port}", std::process::id(), next_alias_id());
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
            .arg("--alias")
            .arg(&alias)
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

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                remove_empty_log(log_path.as_deref());
                return Err(error).with_context(|| {
                    format!(
                        "cannot spawn llama-server {} (missing DLLs or wrong architecture?)",
                        candidate.exe.display()
                    )
                });
            }
        };
        // Assign the child to a kill-on-close job object immediately after spawn.
        // Windows then kills it even if this process dies without running Drop.
        #[cfg(windows)]
        let job_handle = match win_job::create_kill_on_close(&child) {
            Some(handle) => handle,
            None => {
                log(
                    "warning: cannot create a Windows job object for llama-server; \
                     a hard kill of this process may leave the sidecar running",
                );
                0
            }
        };

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
                #[cfg(windows)]
                win_job::close(job_handle);
                remove_empty_log(log_path.as_deref());
                bail!("cancelled while waiting for llama-server health check");
            }
            if let Some(status) = child
                .try_wait()
                .context("cannot query llama-server status")?
            {
                let tail = tail_of(log_path.as_deref());
                #[cfg(windows)]
                win_job::close(job_handle);
                remove_empty_log(log_path.as_deref());
                bail!(
                    "llama-server exited during startup with {status}{}",
                    tail.map(|t| format!(":\n{t}")).unwrap_or_default()
                );
            }
            match agent.get(&health_url).call() {
                Ok(response) => {
                    if response.status() == 200 {
                        match verify_server_identity(&agent, port, &alias, &model) {
                            Ok(()) => {
                                log(&format!(
                                    "llama-server is ready on 127.0.0.1:{port} \
                                     ({}, identity verified)",
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
                                    #[cfg(windows)]
                                    job_handle: std::sync::atomic::AtomicUsize::new(job_handle),
                                });
                            }
                            Err(error) => {
                                kill_child_tree(&mut child);
                                #[cfg(windows)]
                                win_job::close(job_handle);
                                remove_empty_log(log_path.as_deref());
                                return Err(error);
                            }
                        }
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
                #[cfg(windows)]
                win_job::close(job_handle);
                let tail = tail_of(log_path.as_deref());
                remove_empty_log(log_path.as_deref());
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
        self.stop();
    }

    fn stop(&self) {
        self.killed.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                kill_child_tree(&mut child);
            }
        }
        #[cfg(windows)]
        {
            let handle = self.job_handle.swap(0, Ordering::SeqCst);
            if handle != 0 {
                // Closing the job object kills every process still attached to
                // it, including grandchildren that escaped taskkill.
                win_job::close(handle);
            }
        }
        remove_empty_log(self.log_path.as_deref());
    }
}

impl Drop for OcrServer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug, Clone)]
pub struct CandidateInfo {
    pub exe: PathBuf,
    pub device: Device,
    pub source: String,
}

/// Inspect which llama-server binaries would be tried, without starting one.
/// Used by `ocr-tool doctor`. Also returns non-fatal warnings (for example an
/// explicit `--llama-dir` that does not exist).
pub fn inspect_candidates(config: &EngineConfig) -> (Vec<CandidateInfo>, Vec<String>, Vec<String>) {
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
        discovery.warnings,
    )
}

fn discover_candidates(config: &EngineConfig) -> Discovery {
    let mut candidates = Vec::new();
    let mut searched = Vec::new();
    let mut warnings = Vec::new();
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
        } else {
            warnings.push(format!(
                "{LLAMA_ENV}={} is set but contains no {exe_name}; it was ignored",
                path.display()
            ));
        }
    }

    if let Some(dir) = config.llama_dir.as_deref() {
        searched.push(format!("--llama-dir={}", dir.display()));
        if !dir.exists() {
            warnings.push(format!(
                "--llama-dir {} does not exist; falling back to the discovered runtime",
                dir.display()
            ));
        }
        let device = requested_or_inferred(requested, dir);
        if let Some(exe) = resolve_exe_path(dir, exe_name) {
            candidates.push(Candidate {
                exe,
                device,
                source: "--llama-dir".to_string(),
            });
        } else if dir.is_dir() {
            // A parent directory holds both `llama-vulkan` and `llama-cpu`.
            // Enumerate every subdirectory for the requested device; for `auto`
            // this must include the CPU fallback (subdirs(Auto) is GPU first).
            let before = candidates.len();
            for (sub, sub_device) in subdirs(requested) {
                let subdir = dir.join(&sub);
                if let Some(exe) = server_in(&subdir, exe_name) {
                    candidates.push(Candidate {
                        exe,
                        device: sub_device,
                        source: format!("--llama-dir/{sub}"),
                    });
                }
            }
            if candidates.len() == before {
                warnings.push(format!(
                    "--llama-dir {} contains no {exe_name} (also checked llama-vulkan/llama-cpu subdirectories); falling back to the discovered runtime",
                    dir.display()
                ));
            }
        } else {
            warnings.push(format!(
                "--llama-dir {} is neither a file nor a directory; falling back to the discovered runtime",
                dir.display()
            ));
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
        warnings,
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

/// Infer the device of an explicitly supplied path. Only exact directory names
/// are trusted: a substring such as "cpu" in `my-cpu-stuff` must not silently
/// change the device, and an arbitrary directory is first tried as a GPU build.
fn requested_or_inferred(requested: Device, path: &Path) -> Device {
    if requested != Device::Auto {
        return requested;
    }
    for name in [path.file_name(), path.parent().and_then(Path::file_name)] {
        let Some(name) = name.and_then(|n| n.to_str()) else {
            continue;
        };
        match name.to_ascii_lowercase().as_str() {
            "llama-cpu" => return Device::Cpu,
            "llama-vulkan" | "llama-metal" | "llama-cuda" => return Device::Gpu,
            _ => {}
        }
    }
    Device::Gpu
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
    let result = match std::fs::File::create(&path) {
        Ok(file) => match file.try_clone() {
            Ok(clone) => (Stdio::from(file), Stdio::from(clone), Some(path)),
            Err(_) => (Stdio::null(), Stdio::null(), None),
        },
        Err(_) => (Stdio::null(), Stdio::null(), None),
    };
    // Covers leftover empty files from earlier crashes and caps the directory.
    prune_log_files(&dir);
    result
}

/// Remove empty leftovers and keep only the newest `KEEP_LOG_FILES` sidecar
/// logs (they are diagnostics of past runs, not user data).
fn prune_log_files(dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("llama-server-") || !name.ends_with(".log") {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.len() == 0 {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        logs.push((modified, entry.path()));
    }
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in logs.into_iter().skip(KEEP_LOG_FILES) {
        let _ = std::fs::remove_file(&path);
    }
}

fn remove_empty_log(path: Option<&Path>) {
    let Some(path) = path else { return };
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.is_file() && metadata.len() == 0 {
            let _ = std::fs::remove_file(path);
        }
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

/// The GUI runs without a console, so the inference sidecar is a separate
/// hidden process: `CREATE_NO_WINDOW` keeps its console from ever appearing.
/// Fully in-process inference is out of scope for this architecture.
#[cfg(windows)]
fn configure_child(command: &mut Command) {
    ocr_core::hide_console(command);
}

#[cfg(unix)]
fn configure_child(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    #[cfg(target_os = "linux")]
    unsafe {
        command.pre_exec(|| {
            // The kernel kills the sidecar if this process disappears (crash,
            // SIGKILL, second Ctrl+C) without running Drop.
            let _ = libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
}

#[cfg(not(any(windows, unix)))]
fn configure_child(_command: &mut Command) {}

/// Unique alias suffix for the sidecar identity check.
fn next_alias_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// The process on our port answered, but it is not the server we spawned.
/// Retried once on a fresh port before the whole candidate is given up.
#[derive(Debug)]
struct IdentityMismatch(String);

impl std::fmt::Display for IdentityMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IdentityMismatch {}

/// Verify that the process listening on `port` is the sidecar we started: the
/// unique alias or the model path must match. This closes the `free_port`
/// TOCTOU window where another program grabs the port before llama-server.
fn verify_server_identity(agent: &ureq::Agent, port: u16, alias: &str, model: &Path) -> Result<()> {
    let props = agent
        .get(&format!("http://127.0.0.1:{port}/props"))
        .call()
        .ok()
        .and_then(|response| response.into_json::<Value>().ok());
    let models = agent
        .get(&format!("http://127.0.0.1:{port}/v1/models"))
        .call()
        .ok()
        .and_then(|response| response.into_json::<Value>().ok());
    if identity_matches(alias, model, props.as_ref(), models.as_ref()) {
        return Ok(());
    }
    Err(anyhow::Error::new(IdentityMismatch(format!(
        "server on 127.0.0.1:{port} is not the llama-server we started \
         (alias {alias}, model {})",
        model.display()
    ))))
}

fn identity_matches(
    alias: &str,
    model: &Path,
    props: Option<&Value>,
    models: Option<&Value>,
) -> bool {
    if let Some(props) = props {
        if props
            .get("model_alias")
            .and_then(Value::as_str)
            .map(|value| value == alias)
            .unwrap_or(false)
        {
            return true;
        }
        if props
            .get("model_path")
            .and_then(Value::as_str)
            .map(|value| same_path(value, model))
            .unwrap_or(false)
        {
            return true;
        }
    }
    if let Some(models) = models {
        if let Some(data) = models.get("data").and_then(Value::as_array) {
            if data
                .iter()
                .filter_map(|entry| entry.get("id").and_then(Value::as_str))
                .any(|id| id == alias)
            {
                return true;
            }
        }
    }
    false
}

fn same_path(reported: &str, expected: &Path) -> bool {
    let reported = Path::new(reported);
    let reported = reported
        .canonicalize()
        .unwrap_or_else(|_| reported.to_path_buf());
    let expected = expected
        .canonicalize()
        .unwrap_or_else(|_| expected.to_path_buf());
    #[cfg(windows)]
    {
        reported
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        reported == expected
    }
}

#[cfg(windows)]
mod win_job {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    type Handle = *mut c_void;

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimitInformation {
        basic: BasicLimitInformation,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: u32, info: *const c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    /// Create a kill-on-close job object and attach `child` to it. The OS kills
    /// the sidecar when the last handle closes, including an abrupt parent
    /// termination that never runs `Drop`.
    pub fn create_kill_on_close(child: &Child) -> Option<usize> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info = ExtendedLimitInformation::default();
            info.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                &info as *const _ as *const c_void,
                std::mem::size_of::<ExtendedLimitInformation>() as u32,
            );
            if ok == 0 {
                let _ = CloseHandle(job);
                return None;
            }
            let process = child.as_raw_handle() as Handle;
            let ok = AssignProcessToJobObject(job, process);
            if ok == 0 {
                // For example, the current process is already in a job that
                // forbids nesting; fall back to taskkill on Drop.
                let _ = CloseHandle(job);
                return None;
            }
            Some(job as usize)
        }
    }

    pub fn close(handle: usize) {
        if handle != 0 {
            unsafe {
                let _ = CloseHandle(handle as Handle);
            }
        }
    }
}

fn kill_child_tree(child: &mut Child) {
    let pid = child.id();
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        ocr_core::hide_console(&mut command);
        let _ = command.status();
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
    fn llama_dir_parent_enumerates_cpu_for_auto() {
        let tmp = tempfile::tempdir().unwrap();
        for sub in ["llama-vulkan", "llama-cpu"] {
            let dir = tmp.path().join(sub);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(server_exe_name()), b"fake").unwrap();
        }
        let mut config = EngineConfig::new("unused-model-dir");
        config.llama_dir = Some(tmp.path().to_path_buf());
        let discovery = discover_candidates(&config);
        let ordered: Vec<(String, Device)> = discovery
            .candidates
            .iter()
            .map(|c| (c.source.clone(), c.device))
            .collect();
        assert_eq!(
            ordered,
            vec![
                ("--llama-dir/llama-vulkan".to_string(), Device::Gpu),
                ("--llama-dir/llama-cpu".to_string(), Device::Cpu),
            ]
        );
    }

    #[test]
    fn missing_explicit_llama_dir_is_reported() {
        let mut config = EngineConfig::new("unused-model-dir");
        config.llama_dir = Some(PathBuf::from("definitely-missing-llama-dir"));
        let discovery = discover_candidates(&config);
        assert!(
            discovery
                .warnings
                .iter()
                .any(|warning| warning.contains("--llama-dir")),
            "warnings: {:?}",
            discovery.warnings
        );
    }

    #[test]
    fn device_inference_uses_exact_directory_names() {
        assert_eq!(
            requested_or_inferred(Device::Auto, Path::new("some/cpu-tools/llama-server.exe")),
            Device::Gpu,
            "a substring 'cpu' must not select the CPU build"
        );
        assert_eq!(
            requested_or_inferred(Device::Auto, Path::new("some/llama-cpu/llama-server.exe")),
            Device::Cpu
        );
    }

    #[test]
    fn identity_requires_alias_or_model_path() {
        use serde_json::json;
        let model = Path::new("C:/models/pinned.gguf");
        let alias = "ocr-tool-1-1-1234";
        assert!(identity_matches(
            alias,
            model,
            Some(&json!({"model_alias": alias})),
            None
        ));
        assert!(identity_matches(
            alias,
            model,
            Some(&json!({"model_path": "C:/models/pinned.gguf"})),
            None
        ));
        assert!(identity_matches(
            alias,
            model,
            None,
            Some(&json!({"data": [{"id": alias}]}))
        ));
        assert!(!identity_matches(
            alias,
            model,
            Some(&json!({"model_alias": "someone-else", "model_path": "C:/other.gguf"})),
            Some(&json!({"data": [{"id": "someone-else"}]}))
        ));
    }

    #[test]
    fn cancelled_start_does_not_spawn() {
        let model_dir = tempfile::tempdir().unwrap();
        std::fs::write(model_dir.path().join("chandra-q4.gguf"), b"m").unwrap();
        std::fs::write(model_dir.path().join("mmproj.gguf"), b"p").unwrap();
        let llama_dir = tempfile::tempdir().unwrap();
        std::fs::write(llama_dir.path().join(server_exe_name()), b"fake").unwrap();
        let mut config = EngineConfig::new(model_dir.path());
        config.llama_dir = Some(llama_dir.path().to_path_buf());
        let error = OcrServer::start(&config, &AtomicBool::new(true), &mut |_| {})
            .err()
            .expect("cancelled start must fail");
        assert!(error.to_string().contains("cancelled"), "{error:#}");
    }

    #[test]
    fn empty_sidecar_log_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("llama-server-1-2.log");
        std::fs::write(&log, b"").unwrap();
        remove_empty_log(Some(&log));
        assert!(!log.exists());
        std::fs::write(&log, b"diagnostics").unwrap();
        remove_empty_log(Some(&log));
        assert!(log.exists(), "non-empty logs are kept for diagnostics");
    }

    #[test]
    fn old_sidecar_logs_are_pruned() {
        let tmp = tempfile::tempdir().unwrap();
        for index in 0..KEEP_LOG_FILES + 5 {
            std::fs::write(tmp.path().join(format!("llama-server-{index}.log")), b"x").unwrap();
        }
        prune_log_files(tmp.path());
        let left = std::fs::read_dir(tmp.path()).unwrap().count();
        assert_eq!(left, KEEP_LOG_FILES);
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
