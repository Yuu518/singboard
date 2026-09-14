use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tauri::Manager;

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
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        PANEL_REPO
    );
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
        let mirrored = mirror
            .as_deref()
            .map(str::trim)
            .is_some_and(|m| !m.is_empty());
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
    download_asset(&app, PROGRESS_EVENT, &download_url, asset_size, &staged_exe)
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
    // Run our known coordinator, not code from the downloaded release. It stays
    // at the caller's privilege level and relaunches the GUI after any elevated copy.
    let coordinator = staging.join("singboard-updater.exe");
    std::fs::copy(&target, &coordinator).map_err(|e| cleanup(e.to_string()))?;
    // GNU builds use a dynamic WebView2 loader; official MSVC builds embed it.
    if let Some(parent) = target.parent() {
        let loader = parent.join("WebView2Loader.dll");
        if loader.is_file() {
            std::fs::copy(loader, staging.join("WebView2Loader.dll"))
                .map_err(|e| cleanup(e.to_string()))?;
        }
    }
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::process::Command::new(&coordinator)
        .arg(APPLY_UPDATE_FLAG)
        .arg(&target)
        .arg(std::process::id().to_string())
        .arg(&staged_exe)
        .arg(&data_dir)
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

pub fn run_apply_update(
    target: &Path,
    pid: u32,
    source: &Path,
    data_dir: &Path,
) -> Result<(), String> {
    wait_for_pid_exit(pid, Duration::from_secs(10));
    let result = overwrite_with_backup(source, target);
    match result {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(e) => {
            let message = format!("覆盖面板失败: {e}");
            let _ = std::fs::create_dir_all(data_dir);
            let _ = std::fs::write(data_dir.join("panel-update-error.txt"), &message);
            return Err(message);
        }
    }
    // Retry protected-directory replacement with a single elevated child. The
    // coordinator remains unelevated, including when the user cancels UAC.
    let hash = crate::service::helper::sha256_file(source)?;
    let sid = singboard_service::ipc::current_user_sid().map_err(|e| e.to_string())?;
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(crate::service::elevation::request_with_context(
            crate::service::elevation::Operation::ReplacePanel {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                hash,
            },
            data_dir.to_path_buf(),
            sid,
            None,
        ))
        .map(|_| ());
    if let Err(error) = &result {
        let _ = std::fs::create_dir_all(data_dir);
        let _ = std::fs::write(data_dir.join("panel-update-error.txt"), error);
    }
    result
}

fn overwrite_with_backup(source: &Path, target: &Path) -> std::io::Result<()> {
    let bytes = std::fs::read(source)?;
    overwrite_bytes_with_backup(&bytes, target)
}

pub(crate) fn overwrite_bytes_with_backup(bytes: &[u8], target: &Path) -> std::io::Result<()> {
    let bak_path = target.with_extension("exe.bak");
    let had_backup = target.is_file();
    if had_backup {
        std::fs::copy(target, &bak_path)?;
    }

    match retry_io(5, || std::fs::write(target, bytes)) {
        Ok(()) => {
            if had_backup {
                let _ = std::fs::remove_file(&bak_path);
            }
            Ok(())
        }
        Err(e) => {
            if had_backup {
                if let Err(rollback) = retry_io(5, || std::fs::copy(&bak_path, target).map(|_| ()))
                {
                    return Err(std::io::Error::new(
                        e.kind(),
                        format!(
                            "覆盖失败: {e}; 恢复失败: {rollback}; 备份保留在 {}",
                            bak_path.display()
                        ),
                    ));
                }
                let _ = std::fs::remove_file(&bak_path);
            }
            Err(e)
        }
    }
}

#[tauri::command]
pub fn take_panel_update_error(app: tauri::AppHandle) -> Option<String> {
    let path = app
        .path()
        .app_data_dir()
        .ok()?
        .join("panel-update-error.txt");
    let error = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(path);
    Some(error)
}

pub fn launch_panel(target: &Path) -> Result<(), String> {
    if singboard_service::ipc::is_elevated().map_err(|e| e.to_string())? {
        return launch_panel_from_desktop(target);
    }
    let mut cmd = std::process::Command::new(target);
    if let Some(dir) = target.parent() {
        cmd.current_dir(dir);
    }
    cmd.creation_flags(DETACHED_PROCESS)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动面板失败: {}", e))
}

// Also handles an upgrade launched by an older, always-elevated panel.
fn launch_panel_from_desktop(target: &Path) -> Result<(), String> {
    use singboard_service::ipc::wide;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::System::Environment::{
        CreateEnvironmentBlock, DestroyEnvironmentBlock,
    };
    use windows_sys::Win32::System::Threading::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};
    unsafe {
        let shell = GetShellWindow();
        let mut shell_pid = 0;
        GetWindowThreadProcessId(shell, &mut shell_pid);
        if shell_pid == 0 {
            return Err("未找到普通权限桌面进程".into());
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, shell_pid);
        if process.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let process = OwnedHandle::from_raw_handle(process);
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(
            process.as_raw_handle(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut token,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut primary = std::ptr::null_mut();
        if DuplicateTokenEx(
            token.as_raw_handle(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
            std::ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &mut primary,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let primary = OwnedHandle::from_raw_handle(primary);
        let application = wide(&target.to_string_lossy());
        let mut command = wide(&crate::service::elevation::quote_arg(
            &target.to_string_lossy(),
        ));
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = std::mem::size_of_val(&startup) as u32;
        let mut result: PROCESS_INFORMATION = std::mem::zeroed();
        let mut environment = std::ptr::null_mut();
        if CreateEnvironmentBlock(&mut environment, primary.as_raw_handle(), 0) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let launched = CreateProcessWithTokenW(
            primary.as_raw_handle(),
            0,
            application.as_ptr(),
            command.as_mut_ptr(),
            DETACHED_PROCESS | CREATE_UNICODE_ENVIRONMENT,
            environment,
            std::ptr::null(),
            &startup,
            &mut result,
        );
        let error = std::io::Error::last_os_error();
        DestroyEnvironmentBlock(environment);
        if launched == 0 {
            return Err(format!("以桌面用户身份启动面板失败: {error}"));
        }
        drop(OwnedHandle::from_raw_handle(result.hThread));
        drop(OwnedHandle::from_raw_handle(result.hProcess));
        Ok(())
    }
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
