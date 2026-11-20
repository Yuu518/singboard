#![allow(dead_code)]
mod quoting;
mod elevation {
    pub use super::quoting::quote_arg;
}
pub mod admin;
mod managed;
use singboard_service::SERVICE_NAME;
pub(crate) mod helper;
mod install;
mod protected;
mod runtime;
pub(crate) mod scm;
mod snapshot;
mod source_files;

use std::path::Path;

pub use runtime::Snapshot;

pub fn snapshot() -> Result<Option<Snapshot>, String> {
    let name = service_name()?;
    if scm::query_service_status(&name)?.state == "not_installed" {
        return Ok(None);
    }
    Snapshot::read(&name).map(Some)
}

pub fn install(core: &Path, config: &Path) -> Result<(), String> {
    let name = singboard_service::SERVICE_NAME;
    if scm::query_service_status(name)?.state != "not_installed" {
        return Err("服务已经存在，请在干净的测试系统上验证安装".into());
    }
    let core = core.canonicalize().map_err(|e| e.to_string())?;
    let config = config.canonicalize().map_err(|e| e.to_string())?;
    let working_dir = config.parent().ok_or("配置目录无效")?;
    let sid = singboard_service::ipc::current_user_sid().map_err(|e| e.to_string())?;
    let snapshot = runtime::capture(&core, &config, &working_dir.to_string_lossy())?;
    install::configure(name, &snapshot, &sid, false)
}

pub fn data_dir() -> Result<std::path::PathBuf, String> {
    std::env::var_os("APPDATA")
        .map(|dir| std::path::PathBuf::from(dir).join("singboard"))
        .ok_or_else(|| "找不到应用数据目录".into())
}

pub fn service_name() -> Result<String, String> {
    managed::current_service_name(&data_dir()?)
}

pub struct Status {
    pub name: String,
    pub state: String,
    pub pid: Option<u32>,
    pub autostart: bool,
}

pub fn status() -> Result<Status, String> {
    let name = service_name()?;
    let status = scm::query_service_status(&name)?;
    let autostart = status.state != "not_installed" && scm::startup_task_exists(&name);
    Ok(Status {
        name,
        state: status.state,
        pid: status.pid,
        autostart,
    })
}

static STARTUP_SYNC: tokio::sync::OnceCell<Result<String, String>> =
    tokio::sync::OnceCell::const_new();

pub async fn sync() -> Result<String, String> {
    STARTUP_SYNC
        .get_or_init(|| async {
            let dir = data_dir()?;
            let operation = tokio::task::spawn_blocking(move || {
                if scm::query_service_status(SERVICE_NAME)?.state == "not_installed" {
                    return Ok::<_, String>(
                        managed::legacy_service_name(&dir)?
                            .map(|service| admin::Operation::Migrate { service }),
                    );
                }
                Ok(match helper::sync_needed(SERVICE_NAME)? {
                    helper::SyncNeed::UpToDate => None,
                    _ => Some(admin::Operation::Sync {
                        service: SERVICE_NAME.into(),
                    }),
                })
            })
            .await
            .map_err(|e| e.to_string())??;
            if let Some(operation) = operation {
                let result = admin::request(
                    operation,
                    data_dir()?,
                    singboard_service::ipc::current_user_sid().map_err(|e| e.to_string())?,
                    |_| {},
                )
                .await?;
                Ok(result.as_str().unwrap_or("updated").to_string())
            } else {
                Ok("ok".into())
            }
        })
        .await
        .clone()
}

pub fn error_log() -> Result<String, String> {
    scm::read_service_error_log(&service_name()?)
}

pub async fn request(operation: admin::Operation) -> Result<(), String> {
    request_value(operation).await.map(|_| ())
}

pub async fn control(action: &'static str) -> Result<(), String> {
    let service = service_name()?;
    let operation = match action {
        "start" => admin::Operation::Start { service },
        "stop" => admin::Operation::Stop { service },
        _ => admin::Operation::Restart { service },
    };
    request(operation).await
}

pub async fn request_value(operation: admin::Operation) -> Result<serde_json::Value, String> {
    admin::request(
        operation,
        data_dir()?,
        singboard_service::ipc::current_user_sid().map_err(|e| e.to_string())?,
        |_| {},
    )
    .await
    .map_err(|e| {
        if e == admin::CANCELLED {
            "已取消管理员授权".into()
        } else {
            e
        }
    })
}

pub fn run_child(pipe: &str, pid: u32) -> Result<(), String> {
    admin::run_child(pipe, pid, |_, _, _, _| Err("该操作不属于服务管理".into()))
}

pub async fn runtime_status() -> Result<(String, Option<u64>), String> {
    let (name, mut status) = tokio::task::spawn_blocking(|| {
        let name = service_name()?;
        let status = scm::query_service_status(&name)?;
        Ok::<_, String>((name, status))
    })
    .await
    .map_err(|err| err.to_string())??;
    if status.state == "running" {
        if let Some(pid) = status.pid {
            let uptime = singboard_service::telemetry::query(&name, pid).await;
            status = tokio::task::spawn_blocking(move || scm::query_service_status(&name))
                .await
                .map_err(|err| err.to_string())??;
            if status.pid == Some(pid) && status.state == "running" {
                status.uptime_seconds = uptime;
            }
        }
    }
    let state = match status.state.as_str() {
        "running" => "运行中",
        "stopped" => "已停止",
        "not_installed" => "未安装",
        "starting" => "启动中",
        "stopping" => "停止中",
        s => s,
    };
    let description = format!(
        "本机服务 · {state}{}",
        status
            .uptime_seconds
            .map(|t| format!(" · 运行 {} 小时 {} 分钟", t / 3600, (t % 3600) / 60))
            .unwrap_or_default()
    );
    Ok((description, status.uptime_seconds))
}
