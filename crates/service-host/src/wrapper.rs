use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
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
const STDERR_TAIL_BYTES: usize = 64 * 1024;
const GRACEFUL_STOP_SECONDS: u64 = 15;
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
}

struct CoreProcess {
    child: Child,
    stderr: StderrBuffer,
    job: JobHandle,
}

impl CoreProcess {
    fn spawn(command: &mut Command) -> Result<Self, String> {
        let mut child = command
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn sing-box: {e}"))?;
        let job = match attach_kill_on_close_job(&child) {
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
            job,
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
    buf.wait_and_snapshot(Duration::ZERO).unwrap_or_default()
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
    mut child: CoreProcess,
    mut report_checkpoint: impl FnMut(u32),
) -> Result<(), String> {
    if matches!(child.child.try_wait(), Ok(Some(_))) {
        return Ok(());
    }
    if send_ctrl_break(child.child.id()) {
        let deadline = Instant::now() + Duration::from_secs(GRACEFUL_STOP_SECONDS);
        let mut last_tick = Instant::now();
        let mut checkpoint = 1u32;
        while Instant::now() < deadline {
            if matches!(child.child.try_wait(), Ok(Some(_))) {
                return Ok(());
            }
            if last_tick.elapsed() >= Duration::from_secs(1) {
                checkpoint += 1;
                report_checkpoint(checkpoint);
                last_tick = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }
    drop(child.job);
    child.child.wait().map(|_| ()).map_err(|e| format!("等待 sing-box 核心退出失败: {e}"))
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

    status_handle
        .set_service_status(make_pending_status(1, 15))
        .map_err(|e| format!("Failed to set status: {:?}", e))?;

    let (singbox_path, config_path, working_dir) = match read_service_params(svc_name) {
        Ok(v) => v,
        Err(e) => {
            save_error_log(svc_name, &e);
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
    let mut child = match spawn_singbox(&singbox_path, &config_path, &work_dir) {
        Ok(child) => child,
        Err(error) => {
            save_error_log(svc_name, &error);
            status_handle.set_service_status(make_stopped_status()).ok();
            return Err(error);
        }
    };

    // A telemetry failure must not prevent an otherwise healthy proxy from running.
    let telemetry = crate::params::read_panel_sid(svc_name)
        .and_then(|sid| crate::telemetry::Server::start(svc_name, &sid).map_err(|e| e.to_string()));

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
                    Ok(Some(status)) => {
                        // 核心进程退出，收集错误信息，不再重试
                        crashed = true;
                        if let Some(stderr_output) = read_stderr_output(&child) {
                            save_error_log(svc_name, &stderr_output);
                        } else {
                            save_error_log(svc_name, &format!("sing-box 异常退出 ({status})，未捕获到错误输出"));
                        }
                        break;
                    }
                    Ok(None) => {
                        // 核心正常运行
                    }
                    Err(error) => {
                        save_error_log(svc_name, &format!("检查 sing-box 进程状态失败: {error}"));
                        crashed = true;
                        break;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let report_stop = |checkpoint| {
        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::StopPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint,
            wait_hint: Duration::from_secs(10),
            process_id: None,
        });
    };
    report_stop(1);
    let stopped = stop_core_gracefully(child, report_stop);
    drop(telemetry);
    if let Err(error) = stopped {
        save_error_log(svc_name, &error);
        status_handle.set_service_status(make_stopped_status()).ok();
        return Err(error);
    }

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
    fn stop_waits_for_the_old_process_to_exit() {
        use std::os::windows::io::AsHandle;
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::WaitForSingleObject;

        if std::env::var_os("SINGBOARD_TEST_WAIT_FOR_STOP").is_some() {
            std::thread::sleep(Duration::from_secs(60));
            return;
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "wrapper::tests::stop_waits_for_the_old_process_to_exit"])
            .env("SINGBOARD_TEST_WAIT_FOR_STOP", "1")
            .stdout(Stdio::null())
            .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        let core = CoreProcess::spawn(&mut command).unwrap();
        let handle = core.child.as_handle().try_clone_to_owned().unwrap();
        assert_eq!(unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) }, WAIT_TIMEOUT);
        stop_core_gracefully(core, |_| {}).unwrap();
        assert_eq!(unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) }, WAIT_OBJECT_0);
    }

    #[test]
    fn stderr_tail_preserves_diagnostics_after_invalid_utf8() {
        let output = read_stderr_tail(std::io::Cursor::new(b"before\xffafter"));
        assert_eq!(output, "before\u{fffd}after");
    }
}
