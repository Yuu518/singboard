//! Explicit, one-shot management requests. The elevated child never opens a GUI.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use singboard_service::ipc;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::Manager;
use tokio::net::windows::named_pipe::ClientOptions;
use windows_sys::Win32::System::Threading::{GetProcessId, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::*;

use super::{helper, scm};

pub const ADMIN_FLAG: &str = "--admin-operation";
pub const CANCELLED: &str = "elevation_cancelled";
static ADMIN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize, Deserialize)]
pub enum Operation {
    Start {
        service: String,
    },
    Stop {
        service: String,
    },
    Restart {
        service: String,
    },
    Install {
        service: String,
        core: PathBuf,
        config: PathBuf,
        working_dir: PathBuf,
        delay: u32,
    },
    Uninstall {
        service: String,
    },
    Sync {
        service: String,
    },
    Migrate {
        service: String,
    },
    CreateTask {
        service: String,
        delay: u32,
    },
    DeleteTask {
        service: String,
    },
    UpdateCore {
        service: String,
        staging: PathBuf,
        target: PathBuf,
        files: Vec<(String, String)>,
    },
    ReplacePanel {
        source: PathBuf,
        target: PathBuf,
        hash: String,
    },
}

#[derive(Serialize, Deserialize)]
struct Request {
    operation: Operation,
    data_dir: PathBuf,
    user_sid: String,
}

#[derive(Serialize, Deserialize)]
enum Message {
    Progress(String),
    Finished(Result<Value, String>),
}

fn io_error(e: impl std::fmt::Display) -> String {
    format!("管理操作通信失败: {e}")
}

// CommandLineToArgvW-compatible quoting, including trailing backslashes.
pub fn quote_arg(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        result.push_str(&"\\".repeat(if c == '"' { slashes * 2 + 1 } else { slashes }));
        slashes = 0;
        result.push(c);
    }
    result.push_str(&"\\".repeat(slashes * 2));
    result.push('"');
    result
}

fn launch_admin(exe: &Path, parameters: &str) -> Result<OwnedHandle, String> {
    let verb = ipc::wide("runas");
    let file = ipc::wide(&exe.to_string_lossy());
    let args = ipc::wide(parameters);
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of_val(&info) as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = args.as_ptr();
    info.nShow = 0;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(1223) {
            CANCELLED.into()
        } else {
            format!("请求管理员权限失败: {error}")
        });
    }
    if info.hProcess.is_null() {
        return Err("未获取到管理进程句柄".into());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(info.hProcess) })
}

pub async fn request(app: &tauri::AppHandle, operation: Operation) -> Result<Value, String> {
    let data_dir = app.path().app_data_dir().map_err(io_error)?;
    request_with_context(
        operation,
        data_dir,
        ipc::current_user_sid().map_err(io_error)?,
        Some(app),
    )
    .await
}

pub async fn request_with_context(
    operation: Operation,
    data_dir: PathBuf,
    user_sid: String,
    app: Option<&tauri::AppHandle>,
) -> Result<Value, String> {
    let _guard = ADMIN_LOCK
        .try_lock()
        .map_err(|_| "另一个管理操作正在进行中".to_string())?;
    let request = Request {
        operation,
        data_dir,
        user_sid,
    };
    validate(&request)?;
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(io_error)?
        .as_nanos();
    let name = format!(r"\\.\pipe\singboard-admin-{}-{nonce}", std::process::id());
    let mut pipe = ipc::server(&name, &request.user_sid, false).map_err(io_error)?;
    let args = format!("{} {} {}", ADMIN_FLAG, quote_arg(&name), std::process::id());
    let exe = std::env::current_exe().map_err(io_error)?;
    let child = tokio::task::spawn_blocking(move || launch_admin(&exe, &args))
        .await
        .map_err(io_error)??;
    let child_pid = unsafe { GetProcessId(child.as_raw_handle()) };
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            tokio::select! {
                connected = pipe.connect() => {
                    connected.map_err(io_error)?;
                    if ipc::client_pid(&pipe).map_err(io_error)? == child_pid { break; }
                    pipe.disconnect().map_err(io_error)?;
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } == 0 { return Err("管理进程在连接前退出".to_string()); }
                }
            }
        }
        ipc::send(&mut pipe, &request).await.map_err(io_error)
    }).await.map_err(|_| "等待管理进程连接超时".to_string())??;

    // Do not kill an in-flight transaction on timeout: it may be rolling back.
    loop {
        let message: Message =
            tokio::time::timeout(Duration::from_secs(180), ipc::receive(&mut pipe))
                .await
                .map_err(|_| "管理操作仍未返回结果，请刷新状态后检查，勿重复执行".to_string())?
                .map_err(io_error)?;
        match message {
            Message::Finished(result) => {
                let _ = ipc::send(&mut pipe, &true).await;
                return result;
            }
            Message::Progress(phase) => {
                if let Some(app) = app {
                    let phase = if phase == "restart" {
                        "restart"
                    } else {
                        "replace"
                    };
                    crate::commands::update::emit_progress(
                        app,
                        crate::commands::update::CORE_PROGRESS_EVENT,
                        phase,
                        0,
                        0,
                    );
                }
            }
        }
    }
}

pub fn run_child(name: &str, parent_pid: u32) -> Result<(), String> {
    if !ipc::is_elevated().map_err(io_error)? {
        return Err("管理入口需要管理员权限".into());
    }
    if parent_pid == 0 || !name.starts_with(&format!(r"\\.\pipe\singboard-admin-{parent_pid}-")) {
        return Err("无效的管理端点".into());
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io_error)?
        .block_on(async {
            let mut pipe = ClientOptions::new().open(name).map_err(io_error)?;
            if ipc::server_pid(&pipe).map_err(io_error)? != parent_pid {
                return Err("管理端点进程不匹配".into());
            }
            let request: Request =
                tokio::time::timeout(Duration::from_secs(30), ipc::receive(&mut pipe))
                    .await
                    .map_err(io_error)?
                    .map_err(io_error)?;
            validate(&request)?;
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            let worker = tokio::task::spawn_blocking(move || {
                let result = execute(request, |phase| {
                    let _ = sender.send(Message::Progress(phase.to_string()));
                });
                let _ = sender.send(Message::Finished(result));
            });
            while let Some(message) = receiver.recv().await {
                let _ =
                    tokio::time::timeout(Duration::from_secs(5), ipc::send(&mut pipe, &message))
                        .await;
                if matches!(message, Message::Finished(_)) {
                    let _ = tokio::time::timeout(
                        Duration::from_secs(5),
                        ipc::receive::<bool>(&mut pipe),
                    )
                    .await;
                    break;
                }
            }
            worker.await.map_err(io_error)?;
            Ok(())
        })
}

fn absolute(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.as_os_str().to_string_lossy().contains('\0') {
        return Err("管理操作必须使用绝对路径".into());
    }
    Ok(())
}

fn valid_service(name: &str) -> bool {
    !name.is_empty()
        && name.encode_utf16().count() <= 256
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | '"'))
}

fn validate(request: &Request) -> Result<(), String> {
    absolute(&request.data_dir)?;
    if !ipc::valid_sid(&request.user_sid) {
        return Err("无效的原用户身份".into());
    }
    let service = match &request.operation {
        Operation::Start { service }
        | Operation::Stop { service }
        | Operation::Restart { service }
        | Operation::Uninstall { service }
        | Operation::Sync { service }
        | Operation::Migrate { service }
        | Operation::CreateTask { service, .. }
        | Operation::DeleteTask { service } => Some(service),
        Operation::Install {
            service,
            core,
            config,
            working_dir,
            ..
        } => {
            absolute(core)?;
            absolute(config)?;
            absolute(working_dir)?;
            Some(service)
        }
        Operation::UpdateCore {
            service,
            staging,
            target,
            files,
        } => {
            absolute(staging)?;
            absolute(target)?;
            if files.is_empty()
                || files.iter().any(|(name, hash)| {
                    !valid_asset_name(name)
                        || hash.len() != 64
                        || !hash.bytes().all(|b| b.is_ascii_hexdigit())
                })
            {
                return Err("无效的更新文件清单".into());
            }
            Some(service)
        }
        Operation::ReplacePanel {
            source,
            target,
            hash,
        } => {
            absolute(source)?;
            absolute(target)?;
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("无效的面板校验值".into());
            }
            None
        }
    };
    if service.is_some_and(|name| !valid_service(name)) {
        return Err("无效的服务名称".into());
    }
    Ok(())
}

fn valid_asset_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '"'))
        && (name.eq_ignore_ascii_case("sing-box.exe")
            || name.to_ascii_lowercase().ends_with(".dll"))
}

fn execute(request: Request, progress: impl Fn(&str)) -> Result<Value, String> {
    let Request {
        operation,
        data_dir,
        user_sid,
    } = request;
    match operation {
        Operation::Start { service } => scm::start_service(&service)?,
        Operation::Stop { service } => scm::stop_service(&service)?,
        Operation::Restart { service } => scm::restart_service(&service)?,
        Operation::Install {
            service,
            core,
            config,
            working_dir,
            delay,
        } => {
            if scm::query_service_status(super::SERVICE_NAME)?.state == "not_installed" {
                if let Some(old_name) = super::component::legacy_service_name(&data_dir)? {
                    super::component::migrate(&data_dir, &old_name, &user_sid)?;
                }
            }
            if scm::query_service_status(&service)?.state != "not_installed" {
                scm::stop_service(&service)?;
            }
            let deployed = helper::deploy_helper(&data_dir)?;
            let bin = quote_arg(&deployed.to_string_lossy());
            scm::install_service(&service, &bin, &service)?;
            scm::write_service_params(
                &service,
                &core.to_string_lossy(),
                &config.to_string_lossy(),
                &working_dir.to_string_lossy(),
            )?;
            singboard_service::params::write_panel_sid(&service, &user_sid)?;
            scm::create_startup_task(&service, delay, &user_sid)?;
        }
        Operation::Uninstall { service } => {
            scm::delete_startup_task(&service)?;
            scm::uninstall_service(&service)?;
            for path in [
                helper::deployed_helper_path(&data_dir),
                helper::deployed_version_path(&data_dir),
            ] {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
            super::component::cleanup_legacy_files(&data_dir)?;
        }
        Operation::Sync { service } => {
            let was_running = matches!(
                scm::query_service_status(&service)?.state.as_str(),
                "running" | "starting"
            );
            if was_running {
                scm::stop_service(&service)?;
            }
            let result = (|| {
                let deployed = helper::deploy_helper(&data_dir)?;
                let bin = quote_arg(&deployed.to_string_lossy());
                scm::update_service_bin_path(&service, &bin)?;
                singboard_service::params::write_panel_sid(&service, &user_sid)
            })();
            let restart = if was_running {
                scm::start_service(&service)
            } else {
                Ok(())
            };
            result.and(restart)?;
            super::component::cleanup_legacy_files(&data_dir)?;
            return Ok(Value::String("updated".into()));
        }
        Operation::Migrate { service } => {
            super::component::migrate(&data_dir, &service, &user_sid)?;
            return Ok(Value::String("migrated".into()));
        }
        Operation::CreateTask { service, delay } => {
            scm::create_startup_task(&service, delay, &user_sid)?
        }
        Operation::DeleteTask { service } => scm::delete_startup_task(&service)?,
        Operation::UpdateCore {
            service,
            staging,
            target,
            files,
        } => {
            use sha2::{Digest, Sha256};
            let mut verified = Vec::new();
            for (name, expected) in &files {
                let bytes = std::fs::read(staging.join(name)).map_err(|e| e.to_string())?;
                if format!("{:x}", Sha256::digest(&bytes)) != *expected {
                    return Err(format!("更新文件已发生变化: {name}"));
                }
                verified.push((name.clone(), bytes));
            }
            // Apply precisely the bytes authenticated above, not mutable staging files.
            return crate::commands::update::swap_and_restart(
                &progress, &verified, &target, &service,
            )
            .map(Value::Bool);
        }
        Operation::ReplacePanel {
            source,
            target,
            hash,
        } => {
            use sha2::{Digest, Sha256};
            let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
            if format!("{:x}", Sha256::digest(&bytes)) != hash {
                return Err("面板更新文件已发生变化".into());
            }
            crate::commands::self_update::overwrite_bytes_with_backup(&bytes, &target)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_asset_names_and_service_names() {
        assert!(valid_asset_name("libcronet.dll"));
        assert!(!valid_asset_name("../evil.dll"));
        assert!(!valid_asset_name("C:evil.dll"));
        assert!(!valid_service("x\" stop other"));
        assert!(valid_service("sing-box 测试"));
    }
    #[test]
    fn quotes_paths_without_losing_trailing_slashes() {
        assert_eq!(quote_arg(r"C:\App Data\"), "\"C:\\App Data\\\\\"");
        assert_eq!(quote_arg("a\"b"), "\"a\\\"b\"");
    }
}
