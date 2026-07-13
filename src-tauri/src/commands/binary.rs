use sha2::{Digest, Sha256};

#[tauri::command]
pub async fn get_file_hash(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let content = std::fs::read(&path).map_err(|e| format!("读取文件失败: {}", e))?;
        Ok(format!("{:x}", Sha256::digest(&content)))
    })
    .await
    .map_err(|e| format!("任务执行失败: {}", e))?
}

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
