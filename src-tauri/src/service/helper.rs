use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tauri::Manager;

const HELPER_EXE_NAME: &str = "singboard-service.exe";

/// 服务实际注册使用的 helper 副本位置（用户数据目录根下）
pub fn deployed_helper_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    Ok(data_dir.join(HELPER_EXE_NAME))
}

/// 随面板一起分发的 helper（与主程序同目录）
pub fn bundled_helper_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("Failed to get current exe path: {}", e))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "Failed to get exe directory".to_string())?;
    Ok(dir.join(HELPER_EXE_NAME))
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let content = std::fs::read(path).map_err(|e| format!("读取文件失败: {}", e))?;
    Ok(format!("{:x}", Sha256::digest(&content)))
}

/// 把随包 helper 复制到部署位置。调用方负责先停止服务。
pub fn deploy_helper(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let src = bundled_helper_path()?;
    if !src.is_file() {
        return Err(format!(
            "未找到 {}，请确保它与主程序位于同一目录",
            HELPER_EXE_NAME
        ));
    }
    let dest = deployed_helper_path(app)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create app data dir: {}", e))?;
    }

    // 服务刚停止时旧文件可能仍被短暂占用，重试几次
    let mut last_err = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(500));
        }
        match std::fs::copy(&src, &dest) {
            Ok(_) => return Ok(dest),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(format!("复制服务组件失败: {}", last_err))
}

#[derive(PartialEq)]
pub enum SyncNeed {
    /// 随包 helper 不存在，跳过（旧服务继续运行）
    Skip,
    /// 已指向部署副本且哈希一致
    UpToDate,
    /// 服务仍指向旧的面板 exe，需要改指向
    Migrate,
    /// 部署副本缺失或与随包 helper 哈希不一致
    Update,
}

fn service_image_path(service_name: &str) -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::*;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(format!(
            "SYSTEM\\CurrentControlSet\\Services\\{}",
            service_name
        ))
        .ok()?;
    key.get_value("ImagePath").ok()
}

pub fn sync_needed(app: &tauri::AppHandle, service_name: &str) -> Result<SyncNeed, String> {
    let bundled = bundled_helper_path()?;
    if !bundled.is_file() {
        return Ok(SyncNeed::Skip);
    }

    let deployed = deployed_helper_path(app)?;
    let image_path = service_image_path(service_name).unwrap_or_default();
    let points_at_deployed = image_path
        .to_lowercase()
        .contains(&deployed.to_string_lossy().to_lowercase());
    if !points_at_deployed {
        return Ok(SyncNeed::Migrate);
    }

    if !deployed.is_file() {
        return Ok(SyncNeed::Update);
    }
    if sha256_file(&bundled)? != sha256_file(&deployed)? {
        return Ok(SyncNeed::Update);
    }
    Ok(SyncNeed::UpToDate)
}
