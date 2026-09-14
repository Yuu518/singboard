use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

const HELPER_EXE_NAME: &str = "singboard.service";
const HELPER_VERSION_FILE: &str = "singboard.service.version";

// The release helper is built before the panel by the Tauri build hooks.
// Development embeds the same payload so opening a dev panel cannot replace
// an installed release service with a debug build.
const EMBEDDED_HELPER: &[u8] = include_bytes!("../../target/release/singboard.service");

/// 服务实际注册使用的 helper 副本位置（用户数据目录根下）
pub fn deployed_helper_path(data_dir: &Path) -> PathBuf {
    data_dir.join(HELPER_EXE_NAME)
}

/// 记录已部署副本版本的标记文件位置
pub fn deployed_version_path(data_dir: &Path) -> PathBuf {
    data_dir.join(HELPER_VERSION_FILE)
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let content = std::fs::read(path).map_err(|e| format!("读取文件失败: {}", e))?;
    Ok(format!("{:x}", Sha256::digest(&content)))
}

fn deployed_version(data_dir: &Path) -> Option<String> {
    let path = deployed_version_path(data_dir);
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.trim().to_string())
}

/// 把内嵌的 helper 释放到部署位置。调用方负责先停止服务。
pub fn deploy_helper(data_dir: &Path) -> Result<PathBuf, String> {
    let dest = deployed_helper_path(data_dir);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create app data dir: {}", e))?;
    }

    let staged = dest.with_extension("service.new");
    let backup = dest.with_extension("service.bak");
    std::fs::write(&staged, EMBEDDED_HELPER).map_err(|e| format!("暂存服务组件失败: {e}"))?;
    let had_old = dest.is_file();
    let result = (|| {
        if had_old {
            if backup.exists() {
                std::fs::remove_file(&backup).map_err(|e| e.to_string())?;
            }
            let mut moved = false;
            for attempt in 0..3 {
                if attempt > 0 {
                    thread::sleep(Duration::from_millis(500));
                }
                match std::fs::rename(&dest, &backup) {
                    Ok(()) => {
                        moved = true;
                        break;
                    }
                    Err(e) if attempt == 2 => return Err(format!("备份服务组件失败: {e}")),
                    Err(_) => {}
                }
            }
            if !moved {
                return Err("无法备份服务组件".into());
            }
        }
        if let Err(e) = std::fs::rename(&staged, &dest) {
            if had_old {
                std::fs::rename(&backup, &dest)
                    .map_err(|restore| format!("安装服务组件失败: {e}; 恢复失败: {restore}"))?;
            }
            return Err(format!("安装服务组件失败: {e}"));
        }
        let _ = std::fs::write(
            deployed_version_path(data_dir),
            singboard_service::HELPER_VERSION,
        );
        if had_old {
            let _ = std::fs::remove_file(&backup);
        }
        Ok(dest.clone())
    })();
    let _ = std::fs::remove_file(staged);
    result
}

#[derive(PartialEq)]
pub enum SyncNeed {
    UpToDate,
    /// 服务仍指向旧的面板 exe，需要改指向
    Migrate,
    /// 部署副本缺失或版本与内嵌 helper 不一致
    Update,
}

pub fn service_image_path(service_name: &str) -> Option<String> {
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

pub fn sync_needed(data_dir: &Path, service_name: &str) -> Result<SyncNeed, String> {
    let deployed = deployed_helper_path(data_dir);
    let image_path = service_image_path(service_name).unwrap_or_default();
    let points_at_deployed = image_points_to(&image_path, &deployed);
    if !points_at_deployed {
        return Ok(SyncNeed::Migrate);
    }

    if deployed_helper_needs_update(data_dir) {
        return Ok(SyncNeed::Update);
    }
    let sid = singboard_service::ipc::current_user_sid().map_err(|e| e.to_string())?;
    if singboard_service::params::read_panel_sid(service_name)
        .ok()
        .as_deref()
        != Some(&sid)
    {
        return Ok(SyncNeed::Update);
    }
    Ok(SyncNeed::UpToDate)
}

fn deployed_helper_needs_update(data_dir: &Path) -> bool {
    !deployed_helper_path(data_dir).is_file()
        || deployed_version(data_dir).as_deref() != Some(singboard_service::HELPER_VERSION)
}

pub fn image_points_to(image: &str, path: &Path) -> bool {
    let image = image.trim();
    let executable = if let Some(quoted) = image.strip_prefix('"') {
        quoted.split('"').next().unwrap_or_default()
    } else {
        image.split_whitespace().next().unwrap_or_default()
    };
    executable.eq_ignore_ascii_case(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn unchanged_service_version_does_not_require_redeployment_after_panel_update() {
        let dir =
            std::env::temp_dir().join(format!("singboard-helper-version-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = deploy_helper(&dir).unwrap();
        let same_payload = deployed_helper_needs_update(&dir);
        let mut rebuilt = EMBEDDED_HELPER.to_vec();
        rebuilt.extend_from_slice(b"different release signature");
        std::fs::write(dest, rebuilt).unwrap();
        let rebuilt_payload = deployed_helper_needs_update(&dir);
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(!same_payload);
        assert!(
            !rebuilt_payload,
            "an unchanged service version must survive panel-only updates"
        );
    }

    #[test]
    fn missing_or_outdated_service_components_require_redeployment() {
        let dir =
            std::env::temp_dir().join(format!("singboard-helper-outdated-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = deploy_helper(&dir).unwrap();
        std::fs::write(deployed_version_path(&dir), "old-version").unwrap();
        let outdated_version = deployed_helper_needs_update(&dir);
        std::fs::remove_file(deployed_version_path(&dir)).unwrap();
        let missing_version = deployed_helper_needs_update(&dir);
        std::fs::write(
            deployed_version_path(&dir),
            singboard_service::HELPER_VERSION,
        )
        .unwrap();
        std::fs::remove_file(dest).unwrap();
        let missing_payload = deployed_helper_needs_update(&dir);
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(outdated_version);
        assert!(missing_version);
        assert!(missing_payload);
    }

    #[test]
    fn locked_host_is_preserved_when_component_deployment_fails() {
        let dir =
            std::env::temp_dir().join(format!("singboard-helper-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = deployed_helper_path(&dir);
        std::fs::write(&dest, b"original host").unwrap();
        std::fs::write(deployed_version_path(&dir), "old-version").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&dest)
            .unwrap();
        assert!(deploy_helper(&dir).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"original host");
        assert_eq!(deployed_version(&dir).as_deref(), Some("old-version"));
        assert!(!dest.with_extension("service.new").exists());
        drop(locked);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn service_image_matching_uses_the_executable_not_arguments_or_prefixes() {
        let path = Path::new(r"C:\App Data\singboard.service");
        assert!(image_points_to(
            r#""c:\app data\singboard.service" service "singboard.service""#,
            path
        ));
        assert!(!image_points_to(
            r#""C:\App Data\singboard.service.old""#,
            path
        ));
        assert!(!image_points_to(
            r#""C:\other.exe" "C:\App Data\singboard.service""#,
            path
        ));
    }
}
