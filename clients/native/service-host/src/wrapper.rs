use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

use crate::params::{read_service_params, resolve_service_error_log_path};

static SERVICE_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const STARTUP_VERIFY_SECONDS: u64 = 2;
const STARTUP_RETRY_DELAYS: [u64; 3] = [3, 5, 8];
const STARTUP_MAX_ATTEMPTS: u32 = 4;
const STDERR_TAIL_BYTES: usize = 64 * 1024;

struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

fn attach_kill_on_close_job(child: &Child) -> Option<JobHandle> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::*;

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
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
            return None;
        }
        if AssignProcessToJobObject(handle.0, child.as_raw_handle()) == 0 {
            return None;
        }
        Some(handle)
    }
}

struct CoreProcess {
    child: Child,
    stderr: mpsc::Receiver<String>,
    _job: Option<JobHandle>,
}

impl CoreProcess {
    fn spawn(command: &mut Command) -> Result<Self, String> {
        let mut child = command
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn sing-box: {e}"))?;
        let _job = attach_kill_on_close_job(&child);
        let stderr = child
            .stderr
            .take()
            .ok_or("Failed to capture sing-box stderr")?;
        let (sender, receiver) = mpsc::channel();
        if let Err(error) = std::thread::Builder::new()
            .name("singbox-stderr".into())
            .spawn(move || {
                let _ = sender.send(read_stderr_tail(stderr));
            })
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Failed to capture sing-box stderr: {error}"));
        }
        Ok(Self {
            child,
            stderr: receiver,
            _job,
        })
    }
}

fn read_stderr_tail(mut reader: impl std::io::Read) -> String {
    let mut tail = VecDeque::with_capacity(STDERR_TAIL_BYTES);
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let excess = (tail.len() + count).saturating_sub(STDERR_TAIL_BYTES);
                tail.drain(..excess);
                tail.extend(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(tail.make_contiguous())
        .trim()
        .to_string()
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
    child
        .stderr
        .recv_timeout(Duration::from_secs(2))
        .ok()
        .filter(|output| !output.is_empty())
}

fn save_error_log(log_path: &std::path::Path, message: &str) {
    let _ = fs::write(log_path, message);
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
    let log_path = resolve_service_error_log_path(svc_name);

    // 启动前清除旧的错误日志
    let _ = fs::remove_file(&log_path);

    let mut child: Option<CoreProcess> = None;
    let mut startup_succeeded = false;
    let mut last_stderr: Option<String> = None;

    for attempt in 1..=STARTUP_MAX_ATTEMPTS {
        checkpoint = checkpoint.saturating_add(1);
        let _ = status_handle.set_service_status(make_pending_status(checkpoint, 15));
        let mut current_child = match spawn_singbox(&singbox_path, &config_path, &working_dir) {
            Ok(c) => c,
            Err(e) => {
                save_error_log(&log_path, &e);
                status_handle.set_service_status(make_stopped_status()).ok();
                return Err(e);
            }
        };

        // 等待 2 秒验证进程是否存活
        std::thread::sleep(Duration::from_secs(STARTUP_VERIFY_SECONDS));
        match current_child.child.try_wait() {
            Ok(Some(_)) => {
                if let Some(stderr_output) = read_stderr_output(&current_child) {
                    if is_fatal_config_error(&stderr_output) {
                        save_error_log(&log_path, &stderr_output);
                        status_handle.set_service_status(make_stopped_status()).ok();
                        return Err("sing-box 启动失败，核心已输出错误日志".into());
                    }
                    last_stderr = Some(stderr_output);
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
                    save_error_log(&log_path, stderr_output);
                    status_handle.set_service_status(make_stopped_status()).ok();
                    return Err("sing-box 启动失败，核心已输出错误日志".into());
                }
                save_error_log(&log_path, "sing-box 启动后立即退出，未捕获到错误输出");
                status_handle.set_service_status(make_stopped_status()).ok();
                return Err("sing-box 启动失败，已多次重试".into());
            }
            Ok(None) => {
                child = Some(current_child);
                startup_succeeded = true;
                break;
            }
            Err(e) => {
                status_handle.set_service_status(make_stopped_status()).ok();
                return Err(format!("检查 sing-box 进程状态失败: {}", e));
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
                        if let Some(stderr_output) = read_stderr_output(&child) {
                            save_error_log(&log_path, &stderr_output);
                        } else {
                            save_error_log(&log_path, "sing-box 异常退出，未捕获到错误输出");
                        }
                        break;
                    }
                    Ok(None) => {
                        // 核心正常运行
                    }
                    Err(_) => break,
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
            checkpoint: 0,
            wait_hint: Duration::from_secs(10),
            process_id: None,
        })
        .ok();

    let _ = child.child.kill();
    let _ = child.child.wait();
    drop(child);

    status_handle
        .set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })
        .ok();

    Ok(())
}

fn spawn_singbox(
    singbox_path: &str,
    config_path: &str,
    working_dir: &str,
) -> Result<CoreProcess, String> {
    let work_dir = if working_dir.is_empty() {
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
        std::path::PathBuf::from(working_dir)
    };

    if !work_dir.is_dir() {
        return Err(format!(
            "Working directory does not exist: {}",
            work_dir.display()
        ));
    }

    let mut cmd = Command::new(singbox_path);
    cmd.args(["run", "-c", config_path, "-D", &work_dir.to_string_lossy()])
        .current_dir(
            std::path::Path::new(singbox_path)
                .parent()
                .ok_or("Invalid sing-box path")?,
        );

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
}
