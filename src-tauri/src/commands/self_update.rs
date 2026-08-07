use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::update::{
    GhRelease, UPDATE_LOCK, apply_mirror, download_asset, emit_progress, github_get, retry_io,
};

const PANEL_REPO: &str = "Yuu518/singboard";
const PANEL_ASSET: &str = "singboard.exe";
const PROGRESS_EVENT: &str = "panel-update-progress";
pub const APPLY_UPDATE_FLAG: &str = "--apply-update";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelUpdateInfo {
    current_version: String,
    latest_version: String,
    has_update: bool,
    /// Same version, but the local exe does not match the release asset hash.
    out_of_sync: bool,
    published_at: String,
    asset_url: String,
    asset_size: u64,
    asset_digest: String,
}

fn staging_dir() -> PathBuf {
    std::env::temp_dir().join("singboard-panel")
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_raw = parts.next().unwrap_or("0");
    let digits: String = patch_raw.chars().take_while(char::is_ascii_digit).collect();
    let patch = digits.parse().ok()?;
    Some((major, minor, patch))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => latest.trim().trim_start_matches(['v', 'V']) != current.trim(),
    }
}

fn digest_hash(asset_digest: &str) -> Option<&str> {
    asset_digest
        .trim()
        .strip_prefix("sha256:")
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn local_out_of_sync(asset_digest: &str) -> bool {
    let Some(expected) = digest_hash(asset_digest) else {
        return false;
    };
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    match crate::service::helper::sha256_file(&exe) {
        Ok(actual) => !actual.eq_ignore_ascii_case(expected),
        Err(_) => false,
    }
}

#[tauri::command]
pub async fn check_panel_update() -> Result<PanelUpdateInfo, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", PANEL_REPO);
    let release: GhRelease = github_get(&url)
        .await?
        .json()
        .await
        .map_err(|e| format!("解析 GitHub API 响应失败: {}", e))?;

    let asset = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(PANEL_ASSET))
        .ok_or_else(|| format!("该版本未提供 {} 资产", PANEL_ASSET))?;

    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let latest_version = release.tag_name.trim_start_matches(['v', 'V']).to_string();
    let asset_digest = asset.digest.clone().unwrap_or_default();
    let has_update = is_newer(&latest_version, &current_version);

    let out_of_sync = if has_update {
        false
    } else {
        let digest = asset_digest.clone();
        tokio::task::spawn_blocking(move || local_out_of_sync(&digest))
            .await
            .unwrap_or(false)
    };

    Ok(PanelUpdateInfo {
        has_update,
        out_of_sync,
        current_version,
        latest_version,
        published_at: release.published_at.unwrap_or_default(),
        asset_url: asset.browser_download_url.clone(),
        asset_size: asset.size,
        asset_digest,
    })
}

fn verify_digest(file: &Path, asset_digest: &str, mirror: &Option<String>) -> Result<(), String> {
    let Some(expected) = digest_hash(asset_digest) else {
        let mirrored = mirror.as_deref().map(str::trim).is_some_and(|m| !m.is_empty());
        if mirrored {
            return Err("该版本缺少校验信息，已中止更新".into());
        }
        return Ok(());
    };

    let actual = crate::service::helper::sha256_file(file)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("文件校验失败，已中止更新".into())
    }
}

#[tauri::command]
pub async fn perform_panel_update(
    app: tauri::AppHandle,
    asset_url: String,
    asset_size: u64,
    asset_digest: String,
    mirror: Option<String>,
) -> Result<(), String> {
    let _guard = UPDATE_LOCK
        .try_lock()
        .map_err(|_| "更新正在进行中".to_string())?;

    let target = std::env::current_exe().map_err(|e| format!("获取面板路径失败: {}", e))?;

    let staging = staging_dir();
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("创建临时目录失败: {}", e))?;
    let cleanup = |msg: String| {
        let _ = std::fs::remove_dir_all(&staging);
        msg
    };

    let staged_exe = staging.join(PANEL_ASSET);
    let download_url = apply_mirror(&mirror, &asset_url);
    download_asset(
        &app,
        PROGRESS_EVENT,
        &download_url,
        asset_size,
        &staged_exe,
    )
    .await
    .map_err(cleanup)?;

    emit_progress(&app, PROGRESS_EVENT, "verify", 0, 0);
    {
        let staged_exe = staged_exe.clone();
        tokio::task::spawn_blocking(move || verify_digest(&staged_exe, &asset_digest, &mirror))
            .await
            .map_err(|e| format!("任务执行失败: {}", e))
            .and_then(|r| r)
            .map_err(cleanup)?;
    }

    emit_progress(&app, PROGRESS_EVENT, "replace", 0, 0);
    std::process::Command::new(&staged_exe)
        .arg(APPLY_UPDATE_FLAG)
        .arg(&target)
        .arg(std::process::id().to_string())
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map_err(|e| cleanup(format!("启动更新程序失败: {}", e)))?;

    tokio::time::sleep(Duration::from_millis(200)).await;
    app.exit(0);
    Ok(())
}

fn wait_for_pid_exit(pid: u32, timeout: Duration) {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
    };

    const SYNCHRONIZE: u32 = 0x0010_0000;

    unsafe {
        let handle = OpenProcess(SYNCHRONIZE | PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return;
        }
        if WaitForSingleObject(handle, timeout.as_millis() as u32) == WAIT_TIMEOUT {
            TerminateProcess(handle, 0);
            WaitForSingleObject(handle, 5_000);
        }
        CloseHandle(handle);
    }
}

pub fn run_apply_update(target: &Path, pid: u32) -> Result<(), String> {
    wait_for_pid_exit(pid, Duration::from_secs(10));
    let source = std::env::current_exe().map_err(|e| format!("获取更新程序路径失败: {}", e))?;
    overwrite_with_backup(&source, target)
}

fn overwrite_with_backup(source: &Path, target: &Path) -> Result<(), String> {
    let bak_path = target.with_extension("exe.bak");
    let had_backup = target.is_file() && std::fs::copy(target, &bak_path).is_ok();

    match retry_io(5, || std::fs::copy(source, target).map(|_| ())) {
        Ok(()) => {
            if had_backup {
                let _ = std::fs::remove_file(&bak_path);
            }
            Ok(())
        }
        Err(e) => {
            if had_backup {
                let _ = retry_io(5, || std::fs::copy(&bak_path, target).map(|_| ()));
                let _ = std::fs::remove_file(&bak_path);
            }
            Err(format!("覆盖面板失败: {}", e))
        }
    }
}

pub fn launch_panel(target: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(target);
    if let Some(dir) = target.parent() {
        cmd.current_dir(dir);
    }
    cmd.creation_flags(DETACHED_PROCESS)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动面板失败: {}", e))
}

pub fn cleanup_staging() {
    let staging = staging_dir();
    if staging.is_dir() {
        let _ = retry_io(3, || std::fs::remove_dir_all(&staging));
    }
    if let Ok(exe) = std::env::current_exe() {
        let bak = exe.with_extension("exe.bak");
        if bak.is_file() {
            let _ = std::fs::remove_file(&bak);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_prefixed_versions() {
        assert_eq!(parse_version("3.2.4"), Some((3, 2, 4)));
        assert_eq!(parse_version("v3.2.4"), Some((3, 2, 4)));
        assert_eq!(parse_version(" 3.2.4 "), Some((3, 2, 4)));
        assert_eq!(parse_version("3.2"), Some((3, 2, 0)));
        assert_eq!(parse_version("3.2.4-beta1"), Some((3, 2, 4)));
    }

    #[test]
    fn rejects_unparsable_versions() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("nightly"), None);
        assert_eq!(parse_version("3"), None);
    }

    #[test]
    fn compares_numerically_not_lexically() {
        assert!(is_newer("3.10.0", "3.2.4"));
        assert!(is_newer("v3.2.5", "3.2.4"));
        assert!(is_newer("4.0.0", "3.99.99"));
    }

    #[test]
    fn same_or_older_is_not_newer() {
        assert!(!is_newer("3.2.4", "3.2.4"));
        assert!(!is_newer("v3.2.4", "3.2.4"));
        assert!(!is_newer("3.2.3", "3.2.4"));
        assert!(!is_newer("2.9.9", "3.0.0"));
    }

    #[test]
    fn unparsable_falls_back_to_string_inequality() {
        assert!(is_newer("nightly", "3.2.4"));
        assert!(!is_newer("nightly", "nightly"));
        assert!(!is_newer("vnightly", "nightly"));
    }

    fn temp_case(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("singboard-test-{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn overwrite_replaces_target_and_leaves_no_backup() {
        let dir = temp_case("overwrite-ok");
        let source = dir.join("new.exe");
        let target = dir.join("panel.exe");
        std::fs::write(&source, b"new").unwrap();
        std::fs::write(&target, b"old").unwrap();

        overwrite_with_backup(&source, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!target.with_extension("exe.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overwrite_creates_target_when_missing() {
        let dir = temp_case("overwrite-fresh");
        let source = dir.join("new.exe");
        let target = dir.join("panel.exe");
        std::fs::write(&source, b"new").unwrap();

        overwrite_with_backup(&source, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_overwrite_restores_original() {
        let dir = temp_case("overwrite-fail");
        let source = dir.join("missing.exe");
        let target = dir.join("panel.exe");
        std::fs::write(&target, b"old").unwrap();

        assert!(overwrite_with_backup(&source, &target).is_err());

        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert!(!target.with_extension("exe.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
