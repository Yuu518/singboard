use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

use crate::params::{is_reparse_point, read_service_params, resolve_service_error_log_path};

static SERVICE_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const STARTUP_VERIFY_SECONDS: u64 = 2;
const STARTUP_READY_TIMEOUT_SECONDS: u64 = 25;
const STARTUP_RETRY_DELAYS: [u64; 3] = [3, 5, 8];
const STARTUP_MAX_ATTEMPTS: u32 = 4;
const STDERR_TAIL_BYTES: usize = 64 * 1024;
const GRACEFUL_STOP_SECONDS: u64 = 15;
const CACHE_LOCK_WAIT_SECONDS: u64 = 8;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

fn attach_kill_on_close_job(child: &Child) -> Result<JobHandle, String> {
    use windows_sys::Win32::System::JobObjects::*;

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(format!(
                "CreateJobObjectW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let handle = JobHandle(job);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            handle.0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            return Err(format!(
                "SetInformationJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if AssignProcessToJobObject(handle.0, child.as_raw_handle()) == 0 {
            return Err(format!(
                "AssignProcessToJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(handle)
    }
}

struct StderrState {
    tail: VecDeque<u8>,
    eof: bool,
}

#[derive(Clone)]
struct StderrBuffer {
    inner: Arc<(Mutex<StderrState>, Condvar)>,
}

impl StderrBuffer {
    fn new() -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(StderrState {
                    tail: VecDeque::with_capacity(STDERR_TAIL_BYTES),
                    eof: false,
                }),
                Condvar::new(),
            )),
        }
    }

    fn append(&self, chunk: &[u8]) {
        let (lock, _) = &*self.inner;
        if let Ok(mut state) = lock.lock() {
            let excess = (state.tail.len() + chunk.len()).saturating_sub(STDERR_TAIL_BYTES);
            state.tail.drain(..excess);
            state.tail.extend(chunk);
        }
    }

    fn finish(&self) {
        let (lock, cvar) = &*self.inner;
        if let Ok(mut state) = lock.lock() {
            state.eof = true;
            cvar.notify_all();
        }
    }

    fn snapshot(&self) -> String {
        let (lock, _) = &*self.inner;
        let Ok(mut state) = lock.lock() else {
            return String::new();
        };
        String::from_utf8_lossy(state.tail.make_contiguous())
            .trim()
            .to_string()
    }

    fn wait_and_snapshot(&self, timeout: Duration) -> Option<String> {
        let (lock, cvar) = &*self.inner;
        let Ok(mut state) = lock.lock() else {
            return None;
        };
        if !state.eof {
            let deadline = Instant::now() + timeout;
            while !state.eof {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    break;
                };
                let Ok((guard, result)) = cvar.wait_timeout(state, remaining) else {
                    return None;
                };
                state = guard;
                if result.timed_out() {
                    break;
                }
            }
        }
        let text = String::from_utf8_lossy(state.tail.make_contiguous())
            .trim()
            .to_string();
        (!text.is_empty()).then_some(text)
    }

    fn has_started_marker(&self) -> bool {
        self.snapshot().contains("sing-box started")
    }
}

struct CoreProcess {
    child: Child,
    stderr: StderrBuffer,
    _job: JobHandle,
}

impl CoreProcess {
    fn spawn(command: &mut Command) -> Result<Self, String> {
        let mut child = command
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn sing-box: {e}"))?;
        let _job = match attach_kill_on_close_job(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let stderr_pipe = child
            .stderr
            .take()
            .ok_or("Failed to capture sing-box stderr")?;
        let stderr = StderrBuffer::new();
        let writer = stderr.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("singbox-stderr".into())
            .spawn(move || {
                drain_stderr(stderr_pipe, &writer);
            })
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Failed to capture sing-box stderr: {error}"));
        }
        Ok(Self {
            child,
            stderr,
            _job,
        })
    }
}

fn drain_stderr(mut reader: impl std::io::Read, buffer_out: &StderrBuffer) {
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => buffer_out.append(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    buffer_out.finish();
}

#[cfg(test)]
fn read_stderr_tail(reader: impl std::io::Read) -> String {
    let buf = StderrBuffer::new();
    drain_stderr(reader, &buf);
    buf.snapshot()
}

fn get_service_name() -> &'static str {
    SERVICE_NAME.get().map(|s| s.as_str()).unwrap_or("sing-box")
}

pub fn run_service(service_name: &str) -> Result<(), String> {
    SERVICE_NAME.set(service_name.to_string()).ok();
    service_dispatcher::start(get_service_name(), ffi_service_main)
        .map_err(|e| format!("Failed to start service dispatcher: {:?}", e))
}

windows_service::define_windows_service!(ffi_service_main, service_main);

fn service_main(arguments: Vec<OsString>) {
    if let Err(e) = run_service_inner(arguments) {
        eprintln!("Service error: {}", e);
    }
}

fn is_fatal_config_error(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    [
        "decode config",
        "parse config",
        "invalid json",
        "unknown field",
        "syntax error",
        "duplicate ",
        "no such file",
        "cannot find the file",
        "cannot find the path",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn make_pending_status(checkpoint: u32, wait_hint_secs: u64) -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint,
        wait_hint: Duration::from_secs(wait_hint_secs),
        process_id: None,
    }
}

fn make_stopped_status() -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(1),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    }
}

fn read_stderr_output(child: &CoreProcess) -> Option<String> {
    child.stderr.wait_and_snapshot(Duration::from_secs(2))
}

#[derive(Debug, Default, PartialEq, Eq)]
struct StartupProbes {
    clash_api: Option<SocketAddr>,
    cache_db: Option<PathBuf>,
}

fn strip_jsonc(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut cleaned = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            cleaned.push(b);
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_string = true;
            cleaned.push(b);
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        cleaned.push(b);
        i += 1;
    }

    let mut out = Vec::with_capacity(cleaned.len());
    let mut j = 0;
    in_string = false;
    escaped = false;
    while j < cleaned.len() {
        let b = cleaned[j];
        if in_string {
            out.push(b);
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            j += 1;
            continue;
        }
        if b == b'"' {
            in_string = true;
            out.push(b);
            j += 1;
            continue;
        }
        if b == b',' {
            let mut k = j + 1;
            while k < cleaned.len() && cleaned[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < cleaned.len() && matches!(cleaned[k], b'}' | b']') {
                j += 1;
                continue;
            }
        }
        out.push(b);
        j += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_loopback_controller(raw: &str) -> Option<SocketAddr> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(port_str) = trimmed.strip_prefix(':') {
        let port = port_str.parse::<u16>().ok()?;
        return Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }
    if let Some(port_str) = trimmed
        .strip_prefix("localhost:")
        .or_else(|| trimmed.strip_prefix("LOCALHOST:"))
    {
        let port = port_str.parse::<u16>().ok()?;
        return Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }
    let addr = trimmed.parse::<SocketAddr>().ok()?;
    match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), addr.port()))
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), addr.port()))
        }
        ip if ip.is_loopback() => Some(addr),
        _ => None,
    }
}

fn parse_startup_probes(config_text: &str, work_dir: &Path) -> StartupProbes {
    let stripped = strip_jsonc(config_text);
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&stripped) else {
        return StartupProbes::default();
    };
    let experimental = &root["experimental"];
    let clash_api = experimental["clash_api"]["external_controller"]
        .as_str()
        .and_then(parse_loopback_controller);
    let cache_db = if experimental["cache_file"]["enabled"].as_bool() == Some(true) {
        let raw = experimental["cache_file"]["path"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("cache.db");
        let path = PathBuf::from(raw);
        Some(if path.is_absolute() {
            path
        } else {
            work_dir.join(path)
        })
    } else {
        None
    };
    StartupProbes {
        clash_api,
        cache_db,
    }
}

fn load_startup_probes(config_path: &str, work_dir: &Path) -> StartupProbes {
    let Ok(text) = fs::read_to_string(config_path) else {
        return StartupProbes::default();
    };
    parse_startup_probes(&text, work_dir)
}

fn probe_tcp_ready(addr: SocketAddr) -> bool {
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
}

fn file_is_unlocked(path: &Path) -> bool {
    match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
    {
        Ok(_) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Err(err) => !matches!(err.raw_os_error(), Some(32) | Some(33)),
    }
}

fn wait_for_file_unlocked(
    path: &Path,
    timeout: Duration,
    rx: &mpsc::Receiver<()>,
    mut on_tick: impl FnMut(),
) -> Result<bool, ()> {
    if file_is_unlocked(path) {
        return Ok(true);
    }
    let deadline = Instant::now() + timeout;
    let mut last_tick = Instant::now();
    while Instant::now() < deadline {
        if rx.try_recv().is_ok() {
            return Err(());
        }
        if file_is_unlocked(path) {
            return Ok(true);
        }
        if last_tick.elapsed() >= Duration::from_secs(1) {
            on_tick();
            last_tick = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(file_is_unlocked(path))
}

#[derive(Debug, PartialEq, Eq)]
enum ReadyOutcome {
    Ready,
    Exited,
    TimedOut(String),
    Stopped,
    Error(String),
}

fn wait_for_core_ready(
    child: &mut CoreProcess,
    clash_api: Option<SocketAddr>,
    port_busy_before_spawn: bool,
    verify_min: Duration,
    timeout: Duration,
    rx: &mpsc::Receiver<()>,
    mut on_tick: impl FnMut(),
) -> ReadyOutcome {
    let started_at = Instant::now();
    let deadline = started_at + timeout;
    let mut last_tick = Instant::now();
    let mut ready_seen_at: Option<Instant> = None;

    loop {
        if rx.try_recv().is_ok() {
            return ReadyOutcome::Stopped;
        }
        match child.child.try_wait() {
            Ok(Some(_)) => return ReadyOutcome::Exited,
            Ok(None) => {}
            Err(e) => return ReadyOutcome::Error(format!("检查 sing-box 进程状态失败: {}", e)),
        }

        let started_marker = child.stderr.has_started_marker();
        let ready_now = match clash_api {
            Some(addr) => {
                if started_marker {
                    probe_tcp_ready(addr) || started_at.elapsed() >= verify_min
                } else if !port_busy_before_spawn {
                    probe_tcp_ready(addr)
                } else {
                    false
                }
            }
            None => started_marker || started_at.elapsed() >= verify_min,
        };

        if ready_now {
            let first = *ready_seen_at.get_or_insert_with(Instant::now);
            if first.elapsed() >= Duration::from_millis(300) {
                return ReadyOutcome::Ready;
            }
        } else {
            ready_seen_at = None;
        }

        if Instant::now() >= deadline {
            let live_tail = child.stderr.snapshot();
            let detail = if live_tail.is_empty() {
                format!(
                    "sing-box 启动超过 {} 秒仍未就绪，进程可能阻塞在虚拟网卡、缓存锁或启动阶段",
                    timeout.as_secs()
                )
            } else {
                format!(
                    "sing-box 启动超过 {} 秒仍未就绪，已捕获最后输出:\n{}",
                    timeout.as_secs(),
                    live_tail
                )
            };
            return ReadyOutcome::TimedOut(detail);
        }

        if last_tick.elapsed() >= Duration::from_secs(1) {
            on_tick();
            last_tick = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn path_has_reparse_ancestor(path: &Path) -> bool {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        current.push(component);
        if matches!(component, std::path::Component::Normal(_)) && is_reparse_point(&current) {
            return true;
        }
    }
    false
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn remove_error_log(svc_name: &str) {
    let log_path = resolve_service_error_log_path(svc_name);
    if path_has_reparse_ancestor(&log_path) || is_reparse_point(&log_path) {
        return;
    }
    let _ = fs::remove_file(&log_path);
}

fn save_error_log(svc_name: &str, message: &str) {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::*;

    let log_path = resolve_service_error_log_path(svc_name);
    let Some(parent) = log_path.parent() else {
        return;
    };
    if path_has_reparse_ancestor(parent) || is_reparse_point(&log_path) {
        return;
    }
    let wide = wide_path(&log_path);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return;
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut info: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            FileAttributeTagInfo,
            &mut info as *mut _ as _,
            std::mem::size_of_val(&info) as u32,
        )
    };
    if ok == 0 || info.FileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return;
    }
    let mut file = fs::File::from(handle);
    let _ = file.set_len(0);
    let _ = file.write_all(message.as_bytes());
}

fn send_ctrl_break(pid: u32) -> bool {
    use windows_sys::Win32::System::Console::{
        AttachConsole, CTRL_BREAK_EVENT, FreeConsole, GenerateConsoleCtrlEvent,
        SetConsoleCtrlHandler,
    };

    unsafe {
        let _ = FreeConsole();
        if AttachConsole(pid) == 0 {
            return false;
        }
        let _ = SetConsoleCtrlHandler(None, 1);
        let sent = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) != 0;
        std::thread::sleep(Duration::from_millis(50));
        let _ = FreeConsole();
        let _ = SetConsoleCtrlHandler(None, 0);
        sent
    }
}

fn stop_core_gracefully(
    child: &mut CoreProcess,
    status_handle: &service_control_handler::ServiceStatusHandle,
) {
    if matches!(child.child.try_wait(), Ok(Some(_))) {
        return;
    }
    let pid = child.child.id();
    let mut checkpoint = 1u32;
    if send_ctrl_break(pid) {
        let deadline = Instant::now() + Duration::from_secs(GRACEFUL_STOP_SECONDS);
        let mut last_tick = Instant::now();
        while Instant::now() < deadline {
            if matches!(child.child.try_wait(), Ok(Some(_))) {
                return;
            }
            if last_tick.elapsed() >= Duration::from_secs(1) {
                checkpoint = checkpoint.saturating_add(1);
                let _ = status_handle.set_service_status(ServiceStatus {
                    service_type: SERVICE_TYPE,
                    current_state: ServiceState::StopPending,
                    controls_accepted: ServiceControlAccept::empty(),
                    exit_code: ServiceExitCode::Win32(0),
                    checkpoint,
                    wait_hint: Duration::from_secs(10),
                    process_id: None,
                });
                last_tick = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }
    let _ = child.child.kill();
    let _ = child.child.wait();
}

fn run_service_inner(_arguments: Vec<OsString>) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let svc_name = get_service_name();
    let status_handle = service_control_handler::register(svc_name, event_handler)
        .map_err(|e| format!("Failed to register service control handler: {:?}", e))?;

    let mut checkpoint = 1u32;
    status_handle
        .set_service_status(make_pending_status(checkpoint, 15))
        .map_err(|e| format!("Failed to set status: {:?}", e))?;

    // Keep telemetry alive for this host instance, including its startup retries.
    // A telemetry failure must not prevent an otherwise healthy proxy from running.
    let _telemetry = crate::params::read_panel_sid(svc_name)
        .and_then(|sid| crate::telemetry::Server::start(svc_name, &sid).map_err(|e| e.to_string()));

    let (singbox_path, config_path, working_dir) = match read_service_params(svc_name) {
        Ok(v) => v,
        Err(e) => {
            status_handle.set_service_status(make_stopped_status()).ok();
            return Err(e);
        }
    };
    // 启动前清除旧的错误日志
    remove_error_log(svc_name);

    let work_dir = match resolve_work_dir(&singbox_path, &config_path, &working_dir) {
        Ok(dir) => dir,
        Err(e) => {
            save_error_log(svc_name, &e);
            status_handle.set_service_status(make_stopped_status()).ok();
            return Err(e);
        }
    };
    let probes = load_startup_probes(&config_path, &work_dir);

    let mut child: Option<CoreProcess> = None;
    let mut startup_succeeded = false;
    let mut last_stderr: Option<String> = None;

    for attempt in 1..=STARTUP_MAX_ATTEMPTS {
        checkpoint = checkpoint.saturating_add(1);
        let _ = status_handle.set_service_status(make_pending_status(checkpoint, 15));

        if let Some(cache_path) = probes.cache_db.as_deref() {
            let unlocked = wait_for_file_unlocked(
                cache_path,
                Duration::from_secs(CACHE_LOCK_WAIT_SECONDS),
                &rx,
                || {
                    checkpoint = checkpoint.saturating_add(1);
                    let _ = status_handle.set_service_status(make_pending_status(checkpoint, 15));
                },
            );
            if unlocked.is_err() {
                status_handle.set_service_status(make_stopped_status()).ok();
                return Ok(());
            }
        }

        let port_busy_before_spawn = probes.clash_api.is_some_and(probe_tcp_ready);
        let mut current_child = match spawn_singbox(&singbox_path, &config_path, &work_dir) {
            Ok(c) => c,
            Err(e) => {
                save_error_log(svc_name, &e);
                status_handle.set_service_status(make_stopped_status()).ok();
                return Err(e);
            }
        };

        let outcome = wait_for_core_ready(
            &mut current_child,
            probes.clash_api,
            port_busy_before_spawn,
            Duration::from_secs(STARTUP_VERIFY_SECONDS),
            Duration::from_secs(STARTUP_READY_TIMEOUT_SECONDS),
            &rx,
            || {
                checkpoint = checkpoint.saturating_add(1);
                let _ = status_handle.set_service_status(make_pending_status(checkpoint, 15));
            },
        );
        match outcome {
            ReadyOutcome::Ready => {
                child = Some(current_child);
                startup_succeeded = true;
                break;
            }
            ReadyOutcome::Stopped => {
                stop_core_gracefully(&mut current_child, &status_handle);
                status_handle.set_service_status(make_stopped_status()).ok();
                return Ok(());
            }
            ReadyOutcome::Error(e) => {
                stop_core_gracefully(&mut current_child, &status_handle);
                status_handle.set_service_status(make_stopped_status()).ok();
                return Err(e);
            }
            ReadyOutcome::Exited | ReadyOutcome::TimedOut(_) => {
                let mut fatal = false;
                match outcome {
                    ReadyOutcome::Exited => {
                        if let Some(stderr_output) = read_stderr_output(&current_child) {
                            if is_fatal_config_error(&stderr_output) {
                                save_error_log(svc_name, &stderr_output);
                                fatal = true;
                            }
                            last_stderr = Some(stderr_output);
                        }
                    }
                    ReadyOutcome::TimedOut(detail) => {
                        stop_core_gracefully(&mut current_child, &status_handle);
                        last_stderr = Some(detail);
                    }
                    _ => {}
                }
                if fatal {
                    status_handle.set_service_status(make_stopped_status()).ok();
                    return Err("sing-box 启动失败，核心已输出错误日志".into());
                }

                if attempt < STARTUP_MAX_ATTEMPTS {
                    let delay = STARTUP_RETRY_DELAYS
                        .get((attempt - 1) as usize)
                        .copied()
                        .unwrap_or(5);
                    checkpoint = checkpoint.saturating_add(1);
                    let _ = status_handle
                        .set_service_status(make_pending_status(checkpoint, delay + 10));
                    for _ in 0..delay {
                        match rx.recv_timeout(Duration::from_secs(1)) {
                            Ok(()) => {
                                status_handle.set_service_status(make_stopped_status()).ok();
                                return Ok(());
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    continue;
                }

                if let Some(stderr_output) = last_stderr.as_deref() {
                    save_error_log(svc_name, stderr_output);
                    status_handle.set_service_status(make_stopped_status()).ok();
                    return Err("sing-box 启动失败，核心已输出错误日志".into());
                }
                save_error_log(svc_name, "sing-box 启动后立即退出，未捕获到错误输出");
                status_handle.set_service_status(make_stopped_status()).ok();
                return Err("sing-box 启动失败，已多次重试".into());
            }
        }
    }

    if !startup_succeeded {
        status_handle.set_service_status(make_stopped_status()).ok();
        return Err("sing-box 启动失败，未进入运行状态".into());
    }
    let mut child = match child {
        Some(c) => c,
        None => {
            status_handle.set_service_status(make_stopped_status()).ok();
            return Err("sing-box 启动失败，未获取到核心进程句柄".into());
        }
    };

    // 进程存活，设置为 Running
    status_handle
        .set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })
        .map_err(|e| format!("Failed to set status: {:?}", e))?;

    let mut crashed = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(()) => {
                // 收到停止信号
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                match child.child.try_wait() {
                    Ok(Some(_status)) => {
                        // 核心进程退出，收集错误信息，不再重试
                        crashed = true;
                        if let Some(stderr_output) = read_stderr_output(&child) {
                            save_error_log(svc_name, &stderr_output);
                        } else {
                            save_error_log(svc_name, "sing-box 异常退出，未捕获到错误输出");
                        }
                        break;
                    }
                    Ok(None) => {
                        // 核心正常运行
                    }
                    Err(_) => {
                        crashed = true;
                        break;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    status_handle
        .set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::StopPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 1,
            wait_hint: Duration::from_secs(10),
            process_id: None,
        })
        .ok();

    if !crashed {
        stop_core_gracefully(&mut child, &status_handle);
    }
    drop(child);

    status_handle
        .set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(if crashed { 1 } else { 0 }),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })
        .ok();

    Ok(())
}

fn resolve_work_dir(
    singbox_path: &str,
    config_path: &str,
    working_dir: &str,
) -> Result<PathBuf, String> {
    let work_dir = if working_dir.trim().is_empty() {
        let config = std::path::Path::new(config_path);
        config
            .parent()
            .map(|p| p.to_path_buf())
            .or_else(|| {
                let singbox = std::path::Path::new(singbox_path);
                singbox.parent().map(|p| p.to_path_buf())
            })
            .ok_or_else(|| {
                "WorkingDir is empty and cannot be inferred from configPath or singboxPath"
                    .to_string()
            })?
    } else {
        std::path::PathBuf::from(working_dir.trim())
    };

    if !work_dir.is_dir() {
        return Err(format!(
            "Working directory does not exist: {}",
            work_dir.display()
        ));
    }
    Ok(work_dir)
}

fn spawn_singbox(
    singbox_path: &str,
    config_path: &str,
    work_dir: &Path,
) -> Result<CoreProcess, String> {
    let mut cmd = Command::new(singbox_path);
    cmd.args(["run", "-c", config_path, "-D", &work_dir.to_string_lossy()])
        .current_dir(
            std::path::Path::new(singbox_path)
                .parent()
                .ok_or("Invalid sing-box path")?,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);

    CoreProcess::spawn(&mut cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::time::Instant;

    #[test]
    fn stderr_writer_fixture() {
        if std::env::var("SINGBOARD_STDERR_TEST_WRITER").as_deref() != Ok("1") {
            return;
        }
        let mut stderr = std::io::stderr().lock();
        stderr
            .write_all(&vec![b'x'; STDERR_TAIL_BYTES * 16])
            .unwrap();
        stderr.write_all(b"\xfffinal diagnostic").unwrap();
    }

    #[test]
    fn drains_stderr_while_the_core_runs_and_keeps_a_bounded_tail() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "wrapper::tests::stderr_writer_fixture",
                "--nocapture",
            ])
            .env("SINGBOARD_STDERR_TEST_WRITER", "1")
            .stdout(Stdio::null())
            .creation_flags(0x08000000);
        let mut process = CoreProcess::spawn(&mut command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = process.child.try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() >= deadline {
                let _ = process.child.kill();
                let _ = process.child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            status.is_some_and(|status| status.success()),
            "stderr output blocked the core"
        );
        let output = read_stderr_output(&process).unwrap();
        assert!(output.ends_with("\u{fffd}final diagnostic"));
        assert!(output.len() <= STDERR_TAIL_BYTES + 2);
    }

    #[test]
    fn stderr_tail_preserves_diagnostics_after_invalid_utf8() {
        let output = read_stderr_tail(std::io::Cursor::new(b"before\xffafter"));
        assert_eq!(output, "before\u{fffd}after");
    }

    #[test]
    fn classifies_config_errors_as_fatal_and_boot_network_errors_as_retryable() {
        assert!(is_fatal_config_error("FATAL decode config: unknown field"));
        assert!(is_fatal_config_error("open config.json: cannot find the file"));
        assert!(!is_fatal_config_error("FATAL start inbound/tun: wintun adapter not ready"));
        assert!(!is_fatal_config_error("FATAL listen tcp 127.0.0.1:9090: bind failed"));
    }

    #[test]
    fn parses_clash_api_and_cache_file_from_jsonc_config() {
        let work_dir = Path::new(r"C:\sing-box");
        let cfg = r#"{
            // comment with // inside
            "experimental": {
                "clash_api": {
                    "external_controller": "0.0.0.0:9090", /* block */
                },
                "cache_file": {
                    "enabled": true,
                    "path": "data/cache.db",
                },
            },
        }"#;
        let probes = parse_startup_probes(cfg, work_dir);
        assert_eq!(
            probes.clash_api,
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9090))
        );
        assert_eq!(probes.cache_db, Some(work_dir.join("data/cache.db")));

        assert_eq!(
            parse_loopback_controller(":9091"),
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9091))
        );
        assert_eq!(
            parse_loopback_controller("[::]:9092"),
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 9092))
        );
        assert_eq!(parse_loopback_controller("192.168.1.5:9090"), None);
    }

    #[test]
    fn wait_for_file_unlocked_waits_until_lock_releases() {
        let path = std::env::temp_dir().join(format!(
            "singboard-cache-lock-test-{}.db",
            std::process::id()
        ));
        fs::write(&path, b"lock").unwrap();
        let locked = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let (_tx, rx) = mpsc::channel();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(350));
            drop(locked);
        });
        let unlocked =
            wait_for_file_unlocked(&path, Duration::from_secs(3), &rx, || {}).unwrap();
        releaser.join().unwrap();
        let _ = fs::remove_file(&path);
        assert!(unlocked);
    }

    #[test]
    fn hung_startup_fixture() {
        if std::env::var("SINGBOARD_HUNG_STARTUP_FIXTURE").as_deref() != Ok("1") {
            return;
        }
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "DEBUG[0000] router: updating rule-set stalled");
        let _ = stderr.flush();
        std::thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn detects_hung_startup_and_captures_live_stderr_before_process_exits() {
        let probe_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let unused_addr = probe_listener.local_addr().unwrap();
        drop(probe_listener);

        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "wrapper::tests::hung_startup_fixture",
                "--nocapture",
            ])
            .env("SINGBOARD_HUNG_STARTUP_FIXTURE", "1")
            .stdout(Stdio::null())
            .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        let mut process = CoreProcess::spawn(&mut command).unwrap();
        let (_tx, rx) = mpsc::channel();
        let outcome = wait_for_core_ready(
            &mut process,
            Some(unused_addr),
            false,
            Duration::from_millis(200),
            Duration::from_millis(900),
            &rx,
            || {},
        );
        let _ = process.child.kill();
        let _ = process.child.wait();

        match outcome {
            ReadyOutcome::TimedOut(detail) => {
                assert!(detail.contains("router: updating rule-set stalled"), "detail={detail}");
            }
            other => panic!("expected TimedOut, got {other:?}"),
        }
    }

    #[test]
    fn detects_ready_when_clash_api_port_accepts_connections() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "wrapper::tests::hung_startup_fixture",
                "--nocapture",
            ])
            .env("SINGBOARD_HUNG_STARTUP_FIXTURE", "1")
            .stdout(Stdio::null())
            .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        let mut process = CoreProcess::spawn(&mut command).unwrap();
        let (_tx, rx) = mpsc::channel();
        let outcome = wait_for_core_ready(
            &mut process,
            Some(addr),
            false,
            Duration::from_secs(5),
            Duration::from_secs(5),
            &rx,
            || {},
        );
        let _ = process.child.kill();
        let _ = process.child.wait();
        drop(listener);

        assert_eq!(outcome, ReadyOutcome::Ready);
    }
}
