use crate::service::scm;

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
    service_name: String,
    singbox_path: String,
    config_path: String,
    working_dir: String,
    startup_delay_seconds: u32,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Failed to get current exe path: {}", e))?;
        let bin_path = format!("\"{}\" service \"{}\"", exe_path.display(), service_name);
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
pub async fn service_uninstall(service_name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        scm::delete_startup_task(&service_name);
        scm::uninstall_service(&service_name)
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
