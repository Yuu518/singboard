use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::Manager;

#[path = "config_files.rs"]
mod config_files;
pub use config_files::{config_backup_path, replace_with_backup, swap_with_backup};
use config_files::{run_singbox_check, write_atomic};

#[path="runtime_files.rs"]
mod runtime_files;
use runtime_files::{DetectedRuntimeFiles,normalize_path_for_client};

#[tauri::command]
pub async fn detect_runtime_files(base_dir:Option<String>)->Result<DetectedRuntimeFiles,String>{
    runtime_files::detect_runtime_files(base_dir).await
}

#[tauri::command]
pub async fn read_config(path: String) -> Result<String, String> {
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("读取配置失败：{}", e))
}

#[tauri::command]
pub async fn write_config(path: String, content: String) -> Result<(), String> {
    tokio::fs::write(&path, &content)
        .await
        .map_err(|e| format!("写入配置失败：{}", e))
}

#[tauri::command]
pub async fn replace_config(path: String, content: String) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || replace_with_backup(Path::new(&path), content.as_bytes()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn restore_config_backup(path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || swap_with_backup(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn has_config_backup(path: String) -> bool {
    tokio::fs::metadata(config_backup_path(Path::new(&path)))
        .await
        .map(|meta| meta.is_file())
        .unwrap_or(false)
}

#[tauri::command]
pub async fn delete_file(path: String) -> Result<(), String> {
    match tokio::fs::remove_file(&path).await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("删除文件失败：{}", e)),
    }
}

#[tauri::command]
pub async fn validate_config(
    singbox_path: String,
    config_path: String,
    working_dir: Option<String>,
) -> Result<String, String> {
    run_singbox_check(&singbox_path, &config_path, working_dir.as_deref()).await
}

fn make_validate_temp_path(config_path: &Path) -> PathBuf {
    let parent = config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let pid = std::process::id();
    parent.join(format!(".singboard-validate-{}-{}.json", pid, millis))
}

#[tauri::command]
pub async fn validate_config_content(
    singbox_path: String,
    config_path: String,
    content: String,
    working_dir: Option<String>,
) -> Result<String, String> {
    let temp_path = make_validate_temp_path(Path::new(&config_path));
    let temp_path_str = temp_path.to_string_lossy().to_string();

    tokio::fs::write(&temp_path, &content)
        .await
        .map_err(|e| format!("写入校验用的临时文件失败：{}", e))?;

    let check_result =
        run_singbox_check(&singbox_path, &temp_path_str, working_dir.as_deref()).await;
    let _ = tokio::fs::remove_file(&temp_path).await;
    check_result
}

fn resolve_remote_config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let remote_dir = data_dir.join("Remote Config");
    if !remote_dir.exists() {
        std::fs::create_dir_all(&remote_dir)
            .map_err(|e| format!("Failed to create remote config dir: {}", e))?;
    }
    Ok(remote_dir)
}

#[tauri::command]
pub async fn get_remote_config_dir(app: tauri::AppHandle) -> Result<String, String> {
    let dir = resolve_remote_config_dir(&app)?;
    Ok(normalize_path_for_client(&dir))
}

#[tauri::command]
pub async fn get_remote_config_path(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<String, String> {
    let dir = resolve_remote_config_dir(&app)?;
    let path = dir.join(format!("{}.json", profile_id));
    Ok(normalize_path_for_client(&path))
}

fn resolve_running_config_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create app data dir: {}", e))?;
    }
    Ok(data_dir.join("running-config.json"))
}

#[tauri::command]
pub async fn get_running_config_path(app: tauri::AppHandle) -> Result<String, String> {
    let running_config_path = resolve_running_config_path(&app)?;
    Ok(normalize_path_for_client(&running_config_path))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningConfigCopy {
    pub path: String,
    pub changed: bool,
}

#[tauri::command]
pub async fn copy_to_running_config(
    app: tauri::AppHandle,
    source_path: String,
) -> Result<RunningConfigCopy, String> {
    let running_config_path = resolve_running_config_path(&app)?;

    let source_content = tokio::fs::read(&source_path)
        .await
        .map_err(|e| format!("读取源配置失败：{}", e))?;

    let source_hash: [u8; 32] = Sha256::digest(&source_content).into();
    let target_hash = match tokio::fs::read(&running_config_path).await {
        Ok(existing) => Some(<[u8; 32]>::from(Sha256::digest(&existing))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("读取运行配置失败：{}", e)),
    };

    let changed = target_hash.map(|h| h != source_hash).unwrap_or(true);
    if changed {
        let target = running_config_path.clone();
        tokio::task::spawn_blocking(move || write_atomic(&target, &source_content))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("写入运行配置失败：{}", e))?;
    }

    Ok(RunningConfigCopy {
        path: normalize_path_for_client(&running_config_path),
        changed,
    })
}
