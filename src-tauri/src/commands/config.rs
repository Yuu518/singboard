use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::Manager;

const MAX_SCAN_DEPTH: usize = 8;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedRuntimeFiles {
    pub base_dir: String,
    pub singbox_path: Option<String>,
    pub config_path: Option<String>,
    pub found: bool,
}

fn is_singbox_binary(file_name: &str) -> bool {
    matches!(
        file_name.to_ascii_lowercase().as_str(),
        "sing-box.exe" | "sing-box" | "singbox.exe" | "singbox"
    )
}

#[cfg(windows)]
fn normalize_path_for_client(path: &Path) -> String {
    let raw = path.to_string_lossy().to_string();
    if let Some(stripped) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", stripped)
    } else if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        raw
    }
}

#[cfg(not(windows))]
fn normalize_path_for_client(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn scan_dir(
    dir: &Path,
    depth: usize,
    singbox_path: &mut Option<PathBuf>,
    config_path: &mut Option<PathBuf>,
) -> Result<(), String> {
    if depth > MAX_SCAN_DEPTH || (singbox_path.is_some() && config_path.is_some()) {
        return Ok(());
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    entries.sort();

    for path in entries {
        if singbox_path.is_some() && config_path.is_some() {
            break;
        }

        if path.is_file() {
            if config_path.is_none()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.eq_ignore_ascii_case("config.json"))
                    .unwrap_or(false)
            {
                *config_path = Some(path.clone());
            }

            if singbox_path.is_none() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if is_singbox_binary(file_name) {
                        *singbox_path = Some(path.clone());
                    }
                }
            }
        } else if path.is_dir() {
            scan_dir(&path, depth + 1, singbox_path, config_path)?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn detect_runtime_files(
    base_dir: Option<String>,
) -> Result<DetectedRuntimeFiles, String> {
    tokio::task::spawn_blocking(move || {
        let base_dir = if let Some(input) = base_dir {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                std::env::current_dir()
                    .map_err(|e| format!("Failed to get current directory: {}", e))?
            } else {
                let path = PathBuf::from(trimmed);
                if !path.exists() {
                    return Err(format!(
                        "Working directory does not exist: {}",
                        path.display()
                    ));
                }
                if !path.is_dir() {
                    return Err(format!(
                        "Working directory is not a directory: {}",
                        path.display()
                    ));
                }
                path
            }
        } else {
            std::env::current_dir()
                .map_err(|e| format!("Failed to get current directory: {}", e))?
        };

        let base_dir = std::fs::canonicalize(&base_dir).unwrap_or(base_dir);
        let mut singbox_path = None;
        let mut config_path = None;

        scan_dir(&base_dir, 0, &mut singbox_path, &mut config_path)?;
        let found = singbox_path.is_some() && config_path.is_some();

        Ok(DetectedRuntimeFiles {
            base_dir: normalize_path_for_client(&base_dir),
            singbox_path: singbox_path
                .as_ref()
                .map(|p| normalize_path_for_client(p.as_path())),
            config_path: config_path
                .as_ref()
                .map(|p| normalize_path_for_client(p.as_path())),
            found,
        })
    })
    .await
    .map_err(|e| format!("Failed to run detection task: {}", e))?
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

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub fn config_backup_path(path: &Path) -> PathBuf {
    sibling_path(path, ".bak")
}

fn write_atomic(path: &Path, content: &[u8]) -> Result<(), String> {
    let temp = sibling_path(path, ".tmp");
    std::fs::write(&temp, content).map_err(|e| format!("写入临时文件失败：{e}"))?;
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("替换配置文件失败：{e}"));
    }
    Ok(())
}

pub fn replace_with_backup(path: &Path, content: &[u8]) -> Result<bool, String> {
    match std::fs::read(path) {
        Ok(previous) if previous == content => return Ok(false),
        Ok(previous) => write_atomic(&config_backup_path(path), &previous)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("读取原配置失败：{e}")),
    }
    write_atomic(path, content)?;
    Ok(true)
}

pub fn swap_with_backup(path: &Path) -> Result<(), String> {
    let backup = config_backup_path(path);
    let previous = match std::fs::read(&backup) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("没有可恢复的上一版本".into());
        }
        Err(e) => return Err(format!("读取上一版本失败：{e}")),
    };
    let current = match std::fs::read(path) {
        Ok(content) => Some(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("读取当前配置失败：{e}")),
    };
    let staged = sibling_path(path, ".bak.swap");
    if let Some(content) = &current {
        std::fs::write(&staged, content).map_err(|e| format!("暂存当前版本失败：{e}"))?;
    }
    if let Err(e) = write_atomic(path, &previous) {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }
    match current {
        Some(_) => std::fs::rename(&staged, &backup).map_err(|e| format!("保存换下来的版本失败：{e}，它还在 {}", staged.display())),
        None => std::fs::remove_file(&backup).map_err(|e| format!("清理上一版本失败：{e}")),
    }
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

fn resolve_working_dir(working_dir: Option<&str>, config_path: &str) -> Result<PathBuf, String> {
    if let Some(dir) = working_dir.map(str::trim).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(dir);
        if !path.exists() {
            return Err(format!(
                "Working directory does not exist: {}",
                path.display()
            ));
        }
        if !path.is_dir() {
            return Err(format!(
                "Working directory is not a directory: {}",
                path.display()
            ));
        }
        return Ok(path);
    }

    if let Some(parent) = Path::new(config_path).parent() {
        if !parent.as_os_str().is_empty() {
            return Ok(parent.to_path_buf());
        }
    }

    std::env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))
}

async fn run_singbox_check(
    singbox_path: &str,
    config_path: &str,
    working_dir: Option<&str>,
) -> Result<String, String> {
    let cwd = resolve_working_dir(working_dir, config_path)?;
    let output = tokio::process::Command::new(&singbox_path)
        .args(["check", "-c", &config_path])
        .current_dir(&cwd)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .await
        .map_err(|e| format!("无法运行 sing-box 校验：{}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok("Configuration is valid".into())
    } else {
        Err(format!("{}\n{}", stdout, stderr).trim().to_string())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "singboard-config-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn replacing_keeps_the_previous_version_as_backup() {
        let dir = temp_dir("replace");
        let path = dir.join("remote.json");
        std::fs::write(&path, b"old").unwrap();

        assert!(replace_with_backup(&path, b"new").unwrap());

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read(config_backup_path(&path)).unwrap(), b"old");
        assert!(!sibling_path(&path, ".tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replacing_with_identical_content_keeps_the_older_backup() {
        let dir = temp_dir("same");
        let path = dir.join("remote.json");
        std::fs::write(&path, b"current").unwrap();
        std::fs::write(config_backup_path(&path), b"older").unwrap();

        assert!(!replace_with_backup(&path, b"current").unwrap());

        assert_eq!(std::fs::read(config_backup_path(&path)).unwrap(), b"older");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn first_download_writes_without_a_backup() {
        let dir = temp_dir("first");
        let path = dir.join("remote.json");

        replace_with_backup(&path, b"fresh").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"fresh");
        assert!(!config_backup_path(&path).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn restoring_swaps_current_and_previous_so_it_can_be_undone() {
        let dir = temp_dir("restore");
        let path = dir.join("remote.json");
        std::fs::write(&path, b"broken").unwrap();
        std::fs::write(config_backup_path(&path), b"working").unwrap();

        swap_with_backup(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"working");
        assert_eq!(std::fs::read(config_backup_path(&path)).unwrap(), b"broken");

        swap_with_backup(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        assert!(!sibling_path(&path, ".bak.swap").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn restoring_without_a_backup_reports_it() {
        let dir = temp_dir("missing");
        let path = dir.join("remote.json");
        std::fs::write(&path, b"only").unwrap();

        assert_eq!(swap_with_backup(&path).unwrap_err(), "没有可恢复的上一版本");
        assert_eq!(std::fs::read(&path).unwrap(), b"only");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
