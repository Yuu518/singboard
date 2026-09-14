use crate::service::{
    SERVICE_NAME, component,
    elevation::{self, Operation},
    scm,
};
use std::path::PathBuf;

#[tauri::command]
pub async fn service_status(app: tauri::AppHandle) -> Result<scm::ServiceStatus, String> {
    let service_name = component::app_service_name(&app)?;
    let name = service_name.clone();
    let mut status = tokio::task::spawn_blocking(move || scm::query_service_status(&name))
        .await
        .map_err(|e| e.to_string())??;
    if status.state == "running" {
        if let Some(pid) = status.pid {
            status.uptime_seconds = singboard_service::telemetry::query(&service_name, pid).await;
            // Do not attach the previous instance's time across a concurrent restart.
            let latest =
                tokio::task::spawn_blocking(move || scm::query_service_status(&service_name))
                    .await
                    .map_err(|e| e.to_string())??;
            if latest.pid != status.pid || latest.state != status.state {
                return Ok(latest);
            }
        }
    }
    Ok(status)
}

#[tauri::command]
pub async fn service_start(app: tauri::AppHandle) -> Result<(), String> {
    let service_name = component::app_service_name(&app)?;
    elevation::request(
        &app,
        Operation::Start {
            service: service_name,
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn service_stop(app: tauri::AppHandle) -> Result<(), String> {
    let service_name = component::app_service_name(&app)?;
    elevation::request(
        &app,
        Operation::Stop {
            service: service_name,
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn service_restart(app: tauri::AppHandle) -> Result<(), String> {
    let service_name = component::app_service_name(&app)?;
    elevation::request(
        &app,
        Operation::Restart {
            service: service_name,
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn service_install(
    app: tauri::AppHandle,
    singbox_path: String,
    config_path: String,
    working_dir: String,
    startup_delay_seconds: u32,
) -> Result<(), String> {
    let core = std::fs::canonicalize(&singbox_path).map_err(|e| format!("核心路径无效: {e}"))?;
    let config = PathBuf::from(config_path);
    let working_dir = if working_dir.trim().is_empty() {
        config.parent().ok_or("配置路径无效")?.to_path_buf()
    } else {
        std::fs::canonicalize(working_dir).map_err(|e| format!("工作目录无效: {e}"))?
    };
    elevation::request(
        &app,
        Operation::Install {
            service: SERVICE_NAME.into(),
            core,
            config,
            working_dir,
            delay: startup_delay_seconds,
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn service_uninstall(app: tauri::AppHandle) -> Result<(), String> {
    let service_name = component::app_service_name(&app)?;
    elevation::request(
        &app,
        Operation::Uninstall {
            service: service_name,
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn service_component_sync(app: tauri::AppHandle) -> Result<String, String> {
    component::sync(&app).await
}

#[tauri::command]
pub async fn service_startup_task_exists(app: tauri::AppHandle) -> bool {
    let Ok(service_name) = component::app_service_name(&app) else {
        return false;
    };
    tokio::task::spawn_blocking(move || scm::startup_task_exists(&service_name))
        .await
        .unwrap_or(false)
}

#[tauri::command]
pub async fn service_create_startup_task(
    app: tauri::AppHandle,
    startup_delay_seconds: u32,
) -> Result<(), String> {
    let service_name = component::app_service_name(&app)?;
    elevation::request(
        &app,
        Operation::CreateTask {
            service: service_name,
            delay: startup_delay_seconds,
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn service_delete_startup_task(app: tauri::AppHandle) -> Result<(), String> {
    let service_name = component::app_service_name(&app)?;
    elevation::request(
        &app,
        Operation::DeleteTask {
            service: service_name,
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn service_error_log(app: tauri::AppHandle) -> Result<String, String> {
    let service_name = component::app_service_name(&app)?;
    tokio::task::spawn_blocking(move || scm::read_service_error_log(&service_name))
        .await
        .map_err(|e| e.to_string())?
}
