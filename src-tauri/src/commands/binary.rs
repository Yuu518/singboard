#[tauri::command]
pub async fn get_file_hash(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&path);
        #[cfg(windows)]
        return crate::service::helper::sha256_file(path);
        #[cfg(not(windows))]
        return crate::root::fs::sha256(path);
    })
    .await
    .map_err(|e| format!("任务执行失败: {}", e))?
}

#[cfg(windows)]
#[tauri::command]
pub async fn get_singbox_version(singbox_path: String) -> Result<String, String> {
    let output = tokio::process::Command::new(&singbox_path)
        .args(["version"])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .await
        .map_err(|e| format!("Failed to run sing-box version: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().next().unwrap_or("unknown").to_string())
    } else {
        Err("Failed to get version".into())
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn get_singbox_version(singbox_path: String) -> Result<String, String> {
    use crate::root::{self, quote};

    let output = root::run_async(format!("{} version", quote(&singbox_path)))
        .await
        .map_err(|e| format!("Failed to run sing-box version: {}", e))?;
    Ok(output.lines().next().unwrap_or("unknown").to_string())
}
