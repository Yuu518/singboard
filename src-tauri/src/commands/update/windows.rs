use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::service::scm;

use super::{
    CORE_PROGRESS_EVENT, CoreUpdateResult, STAGED_MANIFEST, StagedCore, UPDATE_LOCK, apply_mirror,
    core_asset_hash, download_asset, emit_progress, staged_matches, verify_archive,
};

const CORE_EXE_NAME: &str = "sing-box.exe";

pub(super) fn asset_suffixes() -> Result<Vec<String>, String> {
    match std::env::consts::ARCH {
        "x86_64" => Ok(vec!["windows-amd64.zip".into()]),
        "aarch64" => Ok(vec!["windows-arm64.zip".into()]),
        other => Err(format!("不支持的 CPU 架构: {}", other)),
    }
}

/// 下载与解压产物放在 %TEMP%\singboard（整个目录由核心更新独占，会被清空重建）。
/// 校验与安装共用它，安装才能复用校验解压出来的文件
fn staging_dir() -> PathBuf {
    std::env::temp_dir().join("singboard")
}

fn take_staged(
    staging: &Path,
    asset_url: &str,
    asset_size: u64,
    asset_digest: &str,
) -> Option<Vec<String>> {
    if !staged_matches(staging, asset_url, asset_size, asset_digest) {
        return None;
    }
    extract_core_files(
        &staging.join("core.zip"),
        &staging.join("files"),
        asset_digest,
    )
    .ok()
}

/// 从 zip 中解出 sing-box.exe（必需）与随附的 dll 依赖（如 naive 需要的
/// libcronet.dll）。按文件名匹配、平铺写入 staging，不依赖目录结构。
/// 返回解出的 dll 文件名列表。
fn extract_core_files(
    zip_path: &Path,
    staging: &Path,
    asset_digest: &str,
) -> Result<Vec<String>, String> {
    let bytes = verify_archive(zip_path, asset_digest)?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("读取压缩包失败: {}", e))?;
    if staging.exists() {
        std::fs::remove_dir_all(staging).map_err(|e| format!("清理临时目录失败: {}", e))?;
    }
    std::fs::create_dir_all(staging).map_err(|e| format!("创建临时目录失败: {}", e))?;

    let mut found_exe = false;
    let mut dlls: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取压缩包失败: {}", e))?;
        if entry.is_dir() {
            continue;
        }
        // 只取文件名部分，天然免疫 zip 路径穿越
        let Some(name) = entry.name().rsplit(['/', '\\']).next().map(str::to_string) else {
            continue;
        };
        let dest = if name.eq_ignore_ascii_case(CORE_EXE_NAME) {
            found_exe = true;
            staging.join(CORE_EXE_NAME)
        } else if name.to_ascii_lowercase().ends_with(".dll") {
            dlls.push(name.clone());
            staging.join(&name)
        } else {
            continue;
        };
        let mut out =
            std::fs::File::create(&dest).map_err(|e| format!("写入临时文件失败: {}", e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压失败: {}", e))?;
    }

    if !found_exe {
        return Err(format!("压缩包内未找到 {}", CORE_EXE_NAME));
    }
    Ok(dlls)
}


async fn probe_core_version(exe: &Path) -> Result<String, String> {
    let output = tokio::process::Command::new(exe)
        .args(["version"])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .await
        .map_err(|e| format!("下载的核心无法运行: {}", e))?;
    if !output.status.success() {
        return Err("下载的核心无法运行（version 命令执行失败）".into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().next().unwrap_or("unknown").to_string())
}

/// 服务刚停止时文件锁释放有延迟，重试几次（同 helper::deploy_helper）
pub(crate) fn retry_io<F: FnMut() -> std::io::Result<()>>(
    attempts: u32,
    mut f: F,
) -> std::io::Result<()> {
    let mut last_err = None;
    for attempt in 0..attempts.max(1) {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        match f() {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap())
}

/// 停服务 → 备份旧核心（只留一份 .bak）→ 替换 exe 并覆盖随附 dll → 重启，
/// 失败自动回滚（dll 按需求不备份，直接覆盖）
pub(crate) fn swap_and_restart(
    progress: &impl Fn(&str),
    files: &[(String, Vec<u8>)],
    target: &Path,
    service_name: &str,
) -> Result<bool, String> {
    let exe_bytes = &files
        .iter()
        .find(|(name, _)| name == CORE_EXE_NAME)
        .ok_or("更新清单缺少 sing-box.exe")?
        .1;
    let target_dir = target.parent().ok_or("sing-box 路径无效")?;
    let new_path = target.with_extension("exe.new");
    let bak_path = target.with_extension("exe.bak");

    // 先落到目标同卷，后续 rename 才是原子操作
    std::fs::write(&new_path, exe_bytes).map_err(|e| format!("复制新核心失败: {}", e))?;
    let cleanup_new = || {
        let _ = std::fs::remove_file(&new_path);
    };

    let was_running = match scm::query_service_status(service_name) {
        Ok(status) => status.state == "running" || status.state == "starting",
        Err(e) => {
            cleanup_new();
            return Err(e);
        }
    };

    if was_running {
        progress("replace");
        if let Err(e) = scm::stop_service(service_name) {
            cleanup_new();
            return Err(format!("停止服务失败: {}", e));
        }
    } else {
        progress("replace");
    }

    // 备份：旧核心存在才做；只保留一份备份
    let had_old = target.exists();
    if had_old {
        let _ = std::fs::remove_file(&bak_path);
        if let Err(e) = retry_io(3, || std::fs::rename(target, &bak_path)) {
            cleanup_new();
            if was_running {
                let _ = scm::start_service(service_name);
            }
            return Err(format!("备份旧核心失败: {}", e));
        }
    }

    if let Err(e) = std::fs::rename(&new_path, target) {
        // 还原备份
        if had_old {
            let _ = std::fs::rename(&bak_path, target);
        }
        cleanup_new();
        if was_running {
            let _ = scm::start_service(service_name);
        }
        return Err(format!("替换核心失败: {}", e));
    }

    // 覆盖随附 dll（如 naive 依赖的 libcronet.dll）：不备份，直接覆盖。
    // 失败则回滚 exe，避免 exe 与 dll 版本不一致
    for (dll, bytes) in files.iter().filter(|(name, _)| name != CORE_EXE_NAME) {
        let dll_dest = target_dir.join(dll);
        if let Err(e) = retry_io(3, || std::fs::write(&dll_dest, bytes)) {
            if had_old {
                let _ = retry_io(3, || {
                    std::fs::remove_file(target)?;
                    std::fs::rename(&bak_path, target)
                });
            }
            if was_running {
                let _ = scm::start_service(service_name);
            }
            return Err(format!("更新 {} 失败: {}", dll, e));
        }
    }

    if was_running {
        progress("restart");
        let start_result = scm::start_service(service_name).and_then(|_| {
            std::thread::sleep(Duration::from_secs(2));
            match scm::query_service_status(service_name) {
                Ok(status) if status.state == "running" => Ok(()),
                Ok(status) => Err(format!("服务未能保持运行（状态: {}）", status.state)),
                Err(e) => Err(e),
            }
        });
        if let Err(e) = start_result {
            // 回滚到旧核心（copy 保留 .bak）
            let _ = scm::stop_service(service_name);
            let rollback = if had_old {
                retry_io(3, || std::fs::copy(&bak_path, target).map(|_| ()))
                    .map_err(|e| e.to_string())
                    .and_then(|_| scm::start_service(service_name))
            } else {
                Err("无可用备份".into())
            };
            return match rollback {
                Ok(()) => Err(format!("新核心启动失败，已回滚到旧版本。原因: {}", e)),
                Err(re) => Err(format!("新核心启动失败: {}；回滚也失败: {}", e, re)),
            };
        }
    }

    Ok(was_running)
}

/// 下载资产并解压，返回其中 sing-box.exe 的 SHA-256。
/// 用于版本号相同时比对本地核心与上游资产是否一致（缓存未命中时调用）
#[tauri::command]
pub async fn probe_asset_core_hash(
    app: tauri::AppHandle,
    asset_url: String,
    asset_size: u64,
    asset_digest: String,
    mirror: Option<String>,
) -> Result<String, String> {
    core_asset_hash(&asset_digest)?;
    let _guard = UPDATE_LOCK
        .try_lock()
        .map_err(|_| "更新正在进行中".to_string())?;

    let staging = staging_dir();
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("创建临时目录失败: {}", e))?;
    let cleanup = |msg: String| {
        let _ = std::fs::remove_dir_all(&staging);
        msg
    };

    let download_url = apply_mirror(&mirror, &asset_url);
    let zip_path = staging.join("core.zip");
    download_asset(
        &app,
        CORE_PROGRESS_EVENT,
        &download_url,
        asset_size,
        &zip_path,
    )
    .await
    .map_err(cleanup)?;

    emit_progress(&app, CORE_PROGRESS_EVENT, "extract", 0, 0);
    // 解压结果与清单留在 staging：用户确认重新安装时直接取用，不再下载一遍
    let hash = {
        let staging = staging.clone();
        let asset_url = asset_url.clone();
        tokio::task::spawn_blocking(move || {
            let files_dir = staging.join("files");
            extract_core_files(&zip_path, &files_dir, &asset_digest)?;
            let exe_hash = crate::service::helper::sha256_file(&files_dir.join(CORE_EXE_NAME))?;
            let manifest = serde_json::to_string(&StagedCore {
                asset_url,
                asset_size,
                asset_digest,
            })
            .map_err(|e| format!("写入清单失败: {}", e))?;
            std::fs::write(staging.join(STAGED_MANIFEST), manifest)
                .map_err(|e| format!("写入清单失败: {}", e))?;
            Ok::<String, String>(exe_hash)
        })
        .await
        .map_err(|e| format!("任务执行失败: {}", e))
        .and_then(|r| r)
        .map_err(cleanup)?
    };

    Ok(hash)
}

#[tauri::command]
pub async fn perform_core_update(
    app: tauri::AppHandle,
    asset_url: String,
    asset_size: u64,
    asset_digest: String,
    mirror: Option<String>,
    singbox_path: String,
) -> Result<CoreUpdateResult, String> {
    core_asset_hash(&asset_digest)?;
    let service_name = crate::service::component::app_service_name(&app)?;
    let _guard = UPDATE_LOCK
        .try_lock()
        .map_err(|_| "更新正在进行中".to_string())?;

    // 步骤 0：校验目标路径
    let target = PathBuf::from(singbox_path.trim());
    if singbox_path.trim().is_empty() {
        return Err("请先在服务配置中设置 sing-box 路径".into());
    }
    if !target.parent().is_some_and(|p| p.is_dir()) {
        return Err("sing-box 路径所在目录不存在".into());
    }

    // 临时目录
    let staging = staging_dir();
    let cleanup = |msg: String| {
        let _ = std::fs::remove_dir_all(&staging);
        msg
    };

    let files_dir = staging.join("files");
    let staged_exe = files_dir.join(CORE_EXE_NAME);
    let reusable = {
        let staging = staging.clone();
        let asset_url = asset_url.clone();
        let asset_digest = asset_digest.clone();
        tokio::task::spawn_blocking(move || {
            take_staged(&staging, &asset_url, asset_size, &asset_digest)
        })
        .await
        .map_err(|e| format!("任务执行失败: {}", e))?
    };
    let dlls = match reusable {
        Some(dlls) => dlls,
        None => {
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).map_err(|e| format!("创建临时目录失败: {}", e))?;

            let download_url = apply_mirror(&mirror, &asset_url);
            let zip_path = staging.join("core.zip");
            download_asset(
                &app,
                CORE_PROGRESS_EVENT,
                &download_url,
                asset_size,
                &zip_path,
            )
            .await
            .map_err(cleanup)?;

            emit_progress(&app, CORE_PROGRESS_EVENT, "extract", 0, 0);
            let files_dir = files_dir.clone();
            tokio::task::spawn_blocking(move || {
                extract_core_files(&zip_path, &files_dir, &asset_digest)
            })
            .await
            .map_err(|e| format!("任务执行失败: {}", e))
            .and_then(|r| r)
            .map_err(cleanup)?
        }
    };

    // 步骤 3：健全性检查（staging 里 dll 就在 exe 旁边，加载依赖不受影响）
    let version = probe_core_version(&staged_exe).await.map_err(cleanup)?;

    // One consent covers replacement, restart and any rollback.
    let mut files = Vec::new();
    for name in std::iter::once("sing-box.exe".to_string()).chain(dlls) {
        let hash = crate::service::helper::sha256_file(&files_dir.join(&name)).map_err(cleanup)?;
        files.push((name, hash));
    }
    emit_progress(&app, CORE_PROGRESS_EVENT, "replace", 0, 0);
    let restarted = crate::service::elevation::request(
        &app,
        crate::service::elevation::Operation::UpdateCore {
            service: service_name,
            staging: files_dir,
            target,
            files,
        },
    )
    .await
    .map_err(cleanup)?
    .as_bool()
    .ok_or("更新进程返回了无效结果")?;
    // 步骤 10：清理
    let _ = std::fs::remove_dir_all(&staging);

    Ok(CoreUpdateResult { version, restarted })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);
    const ASSET_URL: &str = "https://example.invalid/sing-box.zip";

    struct TestStaging(PathBuf);

    impl TestStaging {
        fn new() -> Self {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "singboard-core-update-test-{}-{}-{}",
                std::process::id(),
                timestamp,
                NEXT_CASE.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn write_archive(&self, exe: &[u8], dll: &[u8]) -> (u64, String) {
            let path = self.0.join("core.zip");
            let file = std::fs::File::create(&path).unwrap();
            let mut archive = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            archive.start_file("release/sing-box.exe", options).unwrap();
            archive.write_all(exe).unwrap();
            archive
                .start_file("release/libcronet.dll", options)
                .unwrap();
            archive.write_all(dll).unwrap();
            archive.finish().unwrap();
            (
                std::fs::metadata(&path).unwrap().len(),
                format!(
                    "sha256:{}",
                    crate::service::helper::sha256_file(&path).unwrap()
                ),
            )
        }

        fn write_manifest(&self, asset_size: u64, asset_digest: &str) {
            let manifest = StagedCore {
                asset_url: ASSET_URL.to_string(),
                asset_size,
                asset_digest: asset_digest.to_string(),
            };
            std::fs::write(
                self.0.join(STAGED_MANIFEST),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
        }
    }

    impl Drop for TestStaging {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn mismatched_archive_is_rejected_before_extracting_any_files() {
        let staging = TestStaging::new();
        let (size, digest) = staging.write_archive(b"trusted exe", b"trusted dll");
        let (altered_size, _) = staging.write_archive(b"changed exe", b"changed dll");
        assert_eq!(size, altered_size);
        let files = staging.0.join("files");

        let error = extract_core_files(&staging.0.join("core.zip"), &files, &digest).unwrap_err();

        assert!(error.contains("SHA-256"));
        assert!(!files.exists());
    }

    #[test]
    fn cached_executable_and_dlls_are_restored_from_the_verified_archive() {
        let staging = TestStaging::new();
        let (size, digest) = staging.write_archive(b"trusted exe", b"trusted dll");
        staging.write_manifest(size, &digest);
        let files = staging.0.join("files");
        std::fs::create_dir(&files).unwrap();
        std::fs::write(files.join(CORE_EXE_NAME), b"changed exe").unwrap();
        std::fs::write(files.join("libcronet.dll"), b"changed dll").unwrap();
        std::fs::write(files.join("untrusted.dll"), b"extra dll").unwrap();

        let dlls = take_staged(&staging.0, ASSET_URL, size, &digest).unwrap();

        assert_eq!(dlls, vec!["libcronet.dll"]);
        assert_eq!(
            std::fs::read(files.join(CORE_EXE_NAME)).unwrap(),
            b"trusted exe"
        );
        assert_eq!(
            std::fs::read(files.join("libcronet.dll")).unwrap(),
            b"trusted dll"
        );
        assert!(!files.join("untrusted.dll").exists());
    }

    #[test]
    fn a_manifest_cannot_authorize_an_altered_cached_archive() {
        let staging = TestStaging::new();
        let (size, digest) = staging.write_archive(b"trusted exe", b"trusted dll");
        staging.write_archive(b"changed exe", b"changed dll");
        staging.write_manifest(size, &digest);

        assert!(take_staged(&staging.0, ASSET_URL, size, &digest).is_none());
        assert!(!staging.0.join("files").exists());
    }

    #[test]
    fn cached_archive_must_match_the_requested_asset_and_digest() {
        let staging = TestStaging::new();
        let (size, digest) = staging.write_archive(b"trusted exe", b"trusted dll");
        staging.write_manifest(size, &digest);

        assert!(
            take_staged(
                &staging.0,
                "https://example.invalid/other.zip",
                size,
                &digest
            )
            .is_none()
        );
        assert!(take_staged(&staging.0, ASSET_URL, size + 1, &digest).is_none());
        assert!(
            take_staged(
                &staging.0,
                ASSET_URL,
                size,
                &format!("sha256:{}", "0".repeat(64))
            )
            .is_none()
        );
        assert!(!staging.0.join("files").exists());
    }

    #[test]
    fn legacy_unverified_cache_is_not_reused() {
        let staging = TestStaging::new();
        let (size, digest) = staging.write_archive(b"trusted exe", b"trusted dll");
        let legacy_manifest = serde_json::json!({
            "assetUrl": ASSET_URL,
            "assetSize": size,
            "exeHash": "unverified",
            "dlls": ["libcronet.dll"],
        });
        std::fs::write(
            staging.0.join(STAGED_MANIFEST),
            serde_json::to_vec(&legacy_manifest).unwrap(),
        )
        .unwrap();

        assert!(take_staged(&staging.0, ASSET_URL, size, &digest).is_none());
        assert!(!staging.0.join("files").exists());
    }
}
