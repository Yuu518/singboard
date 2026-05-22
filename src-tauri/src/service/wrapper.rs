use std::ffi::OsString;
use std::fs;
use std::io::Read as _;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

use super::scm::{read_service_params, resolve_service_error_log_path};

static SERVICE_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const STARTUP_VERIFY_SECONDS: u64 = 2;
const STARTUP_RETRY_DELAY_SECONDS: u64 = 5;
const STARTUP_MAX_ATTEMPTS: u32 = 3;

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

fn read_stderr_output(child: &mut Child) -> Option<String> {
    if let Some(ref mut stderr) = child.stderr {
        let mut output = String::new();
        let _ = stderr.read_to_string(&mut output);
        if !output.trim().is_empty() {
            return Some(output.trim().to_string());
        }
    }
    None
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

    status_handle
        .set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::StartPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(10),
            process_id: None,
        })
        .map_err(|e| format!("Failed to set status: {:?}", e))?;

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

    let mut child: Option<Child> = None;
    let mut startup_succeeded = false;

    for attempt in 1..=STARTUP_MAX_ATTEMPTS {
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
        match current_child.try_wait() {
            Ok(Some(_)) => {
                if let Some(stderr_output) = read_stderr_output(&mut current_child) {
                    // 核心有明确报错，直接失败，不做重试
                    save_error_log(&log_path, &stderr_output);
                    status_handle.set_service_status(make_stopped_status()).ok();
                    return Err("sing-box 启动失败，核心已输出错误日志".into());
                }

                // 未捕获到核心错误输出，仅在这种情况下进行重试
                if attempt < STARTUP_MAX_ATTEMPTS {
                    for _ in 0..STARTUP_RETRY_DELAY_SECONDS {
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

                save_error_log(&log_path, "sing-box 启动后立即退出，未捕获到错误输出");
                status_handle.set_service_status(make_stopped_status()).ok();
                return Err("sing-box 启动失败，已重试 2 次".into());
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
                match child.try_wait() {
                    Ok(Some(_status)) => {
                        // 核心进程退出，收集错误信息，不再重试
                        if let Some(stderr_output) = read_stderr_output(&mut child) {
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

    let _ = child.kill();
    let _ = child.wait();

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
) -> Result<Child, String> {
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
        .current_dir(&work_dir)
        .stderr(Stdio::piped());

    cmd.spawn()
        .map_err(|e| format!("Failed to spawn sing-box: {}", e))
}
