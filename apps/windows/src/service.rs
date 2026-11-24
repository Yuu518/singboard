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

static STARTUP_SYNC: tokio::sync::Mutex<Option<String>> = tokio::sync::Mutex::const_new(None);

pub async fn sync() -> Result<String, String> {
    let mut guard = STARTUP_SYNC.lock().await;
    if let Some(cached) = guard.as_ref() {
        return Ok(cached.clone());
    }
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
    let outcome = if let Some(operation) = operation {
        let result = admin::request(
            operation,
            data_dir()?,
            singboard_service::ipc::current_user_sid().map_err(|e| e.to_string())?,
            |_| {},
        )
        .await?;
        result.as_str().unwrap_or("updated").to_string()
    } else {
        "ok".into()
    };
    *guard = Some(outcome.clone());
    Ok(outcome)
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum CoreState {
    #[default]
    Unknown,
    NotInstalled,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed(u32),
}

impl CoreState {
    pub fn is_stopped(self) -> bool {
        matches!(self, Self::Stopped | Self::Failed(_))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "核心状态未知",
            Self::NotInstalled => "未安装服务",
            Self::Starting => "正在启动",
            Self::Running => "核心已启动",
            Self::Stopping => "正在停止",
            Self::Stopped | Self::Failed(_) => "核心已停止",
        }
    }
}

pub async fn runtime_status() -> Result<(CoreState, Option<u64>, Option<String>), String> {
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
            let service = name.clone();
            status = tokio::task::spawn_blocking(move || scm::query_service_status(&service))
                .await
                .map_err(|err| err.to_string())??;
            if status.pid == Some(pid) && status.state == "running" {
                status.uptime_seconds = uptime;
            }
        }
    }
    let state = match status.state.as_str() {
        "running" => CoreState::Running,
        "stopped" if status.exit_code != 0 => CoreState::Failed(status.exit_code),
        "stopped" => CoreState::Stopped,
        "not_installed" => CoreState::NotInstalled,
        "starting" => CoreState::Starting,
        "stopping" => CoreState::Stopping,
        _ => CoreState::Unknown,
    };
    let error = if let CoreState::Failed(code) = state {
        let log = tokio::task::spawn_blocking(move || scm::read_service_error_log(&name))
            .await.ok().and_then(Result::ok);
        Some(log.as_deref().and_then(|text| text.lines().rfind(|line| !line.trim().is_empty()))
            .map(str::to_owned).unwrap_or_else(|| format!("服务退出码 {code}")))
    } else {
        None
    };
    Ok((state, status.uptime_seconds, error))
}
