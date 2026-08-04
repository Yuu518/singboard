use crate::service::{helper, scm};

fn helper_bin_path(deployed: &std::path::Path, service_name: &str) -> String {
    format!("\"{}\" service \"{}\"", deployed.display(), service_name)
}

#[tauri::command]
pub async fn service_status(service_name: String) -> Result<scm::ServiceStatus, String> {
    tokio::task::spawn_blocking(move || scm::query_service_status(&service_name))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn service_start(service_name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || scm::start_service(&service_name))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn service_stop(service_name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || scm::stop_service(&service_name))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn service_restart(service_name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || scm::restart_service(&service_name))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn service_install(
    app: tauri::AppHandle,
    service_name: String,
    singbox_path: String,
    config_path: String,
    working_dir: String,
    startup_delay_seconds: u32,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        // 服务在运行时旧的宿主副本被锁定，先停下来再部署
        if scm::query_service_status(&service_name)?.state == "running" {
            scm::stop_service(&service_name)?;
        }
        let deployed = helper::deploy_helper(&app)?;
        let bin_path = helper_bin_path(&deployed, &service_name);
        let display_name = format!("{} (singboard)", service_name);

        scm::write_service_params(&service_name, &singbox_path, &config_path, &working_dir)?;

        scm::install_service(&service_name, &bin_path, &display_name)?;

        scm::create_startup_task(&service_name, startup_delay_seconds)?;

        Ok(())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn service_uninstall(app: tauri::AppHandle, service_name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        scm::delete_startup_task(&service_name);
        scm::uninstall_service(&service_name)?;
        if let Ok(deployed) = helper::deployed_helper_path(&app) {
            let _ = std::fs::remove_file(deployed);
        }
        if let Ok(marker) = helper::deployed_version_path(&app) {
            let _ = std::fs::remove_file(marker);
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// 面板启动时静默调用：服务仍指向旧的面板 exe 时迁移到独立宿主；
/// 部署副本与内嵌 helper 哈希不一致时热换。
#[tauri::command]
pub async fn service_component_sync(
    app: tauri::AppHandle,
    service_name: String,
) -> Result<String, String> {
    // 主窗口与托盘窗口各自的前端都会在启动时调用,串行化避免并发停/启服务
    static SYNC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    tokio::task::spawn_blocking(move || {
        let _guard = SYNC_LOCK.lock().map_err(|_| "sync lock poisoned".to_string())?;

        let status = scm::query_service_status(&service_name)?;
        if status.state == "not_installed" {
            return Ok("not_installed".to_string());
        }

        let need = helper::sync_needed(&app, &service_name)?;
        let result = match need {
            helper::SyncNeed::UpToDate => return Ok("ok".to_string()),
            helper::SyncNeed::Migrate => "migrated",
            helper::SyncNeed::Update => "updated",
        };

        let was_running = status.state == "running" || status.state == "starting";
        if was_running {
            scm::stop_service(&service_name)?;
        }
        let deployed = helper::deploy_helper(&app)?;
        scm::update_service_bin_path(&service_name, &helper_bin_path(&deployed, &service_name))?;
        if was_running {
            scm::start_service(&service_name)?;
        }
        Ok(result.to_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn service_startup_task_exists(service_name: String) -> bool {
    tokio::task::spawn_blocking(move || scm::startup_task_exists(&service_name))
        .await
        .unwrap_or(false)
}

#[tauri::command]
pub async fn service_create_startup_task(
    service_name: String,
    startup_delay_seconds: u32,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || scm::create_startup_task(&service_name, startup_delay_seconds))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn service_delete_startup_task(service_name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        scm::delete_startup_task(&service_name);
        Ok(())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn service_error_log(service_name: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || scm::read_service_error_log(&service_name))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}
