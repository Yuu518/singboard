use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{
    AssetTarget, CORE_PROGRESS_EVENT, CoreUpdateResult, TargetArch, TargetOs, UPDATE_LOCK,
    apply_mirror, core_asset_hash, download_asset, emit_progress, staged_matches, verify_archive,
};

const CORE_EXE_NAME: &str = "sing-box.exe";
const STAGED_ASSET_FILE: &str = "core.asset";
const MAX_CORE_EXTRACT_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn asset_target() -> Result<AssetTarget, String> {
    match std::env::consts::ARCH {
        "x86_64" => Ok(AssetTarget {
            os: TargetOs::Windows,
            arch: TargetArch::Amd64,
        }),
        "aarch64" => Ok(AssetTarget {
            os: TargetOs::Windows,
            arch: TargetArch::Arm64,
        }),
        other => Err(format!("不支持的 CPU 架构: {}", other)),
    }
}

fn is_raw_exe_asset(asset_name: &str) -> bool {
    asset_name.trim().to_ascii_lowercase().ends_with(".exe")
}

fn is_candidate_core_exe(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".exe") else {
        return false;
    };
    if !(stem.starts_with("sing-box") || stem.starts_with("singbox")) {
        return false;
    }
    !stem
        .split(['-', '_', '.'])
        .any(|token| matches!(token, "updater" | "installer" | "setup" | "service"))
}

fn take_staged(
    staging: &Path,
    asset_name: &str,
    asset_url: &str,
    asset_size: u64,
    asset_digest: &str,
) -> Option<Vec<String>> {
    if !staged_matches(staging, asset_url, asset_size, asset_digest) {
        return None;
    }
    extract_core_files(
        &staging.join(STAGED_ASSET_FILE),
        asset_name,
        &staging.join("files"),
        asset_digest,
    )
    .ok()
}

fn extract_core_files(
    asset_path: &Path,
    asset_name: &str,
    staging: &Path,
    asset_digest: &str,
) -> Result<Vec<String>, String> {
    let bytes = verify_archive(asset_path, asset_digest)?;
    if is_raw_exe_asset(asset_name) {
        if bytes.len() as u64 > MAX_CORE_EXTRACT_BYTES {
            return Err("核心文件超过大小上限".into());
        }
        if !bytes.starts_with(b"MZ") {
            return Err("下载的文件不是有效的 Windows 可执行文件".into());
        }
        if staging.exists() {
            std::fs::remove_dir_all(staging).map_err(|e| format!("清理临时目录失败: {}", e))?;
        }
        std::fs::create_dir_all(staging).map_err(|e| format!("创建临时目录失败: {}", e))?;
        std::fs::write(staging.join(CORE_EXE_NAME), &bytes)
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
        return Ok(Vec::new());
    }

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("读取压缩包失败: {}", e))?;

    let mut exact_exe_index: Option<usize> = None;
    let mut candidate_exe_indices: Vec<usize> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("读取压缩包失败: {}", e))?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.name().rsplit(['/', '\\']).next() else {
            continue;
        };
        if name.eq_ignore_ascii_case(CORE_EXE_NAME) {
            exact_exe_index.get_or_insert(i);
        } else if is_candidate_core_exe(name) {
            candidate_exe_indices.push(i);
        }
    }
    let exe_index = exact_exe_index
        .or_else(|| (candidate_exe_indices.len() == 1).then(|| candidate_exe_indices[0]))
        .ok_or_else(|| format!("压缩包内未找到 {}", CORE_EXE_NAME))?;

    if staging.exists() {
        std::fs::remove_dir_all(staging).map_err(|e| format!("清理临时目录失败: {}", e))?;
    }
    std::fs::create_dir_all(staging).map_err(|e| format!("创建临时目录失败: {}", e))?;

    let mut dlls: Vec<String> = Vec::new();
    let mut total_extracted: u64 = 0;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("读取压缩包失败: {}", e))?;
        if entry.is_dir() {
            continue;
        }
        // 只取文件名部分，天然免疫 zip 路径穿越
        let Some(name) = entry.name().rsplit(['/', '\\']).next().map(str::to_string) else {
            continue;
        };
        let dest = if i == exe_index {
            staging.join(CORE_EXE_NAME)
        } else if name.to_ascii_lowercase().ends_with(".dll") {
            if !dlls.iter().any(|existing| existing.eq_ignore_ascii_case(&name)) {
                dlls.push(name.clone());
            }
            staging.join(&name)
        } else {
            continue;
        };
        let remaining = MAX_CORE_EXTRACT_BYTES.saturating_sub(total_extracted);
        let mut out =
            std::fs::File::create(&dest).map_err(|e| format!("写入临时文件失败: {}", e))?;
        let mut limited = entry.take(remaining + 1);
        let written = std::io::copy(&mut limited, &mut out).map_err(|e| format!("解压失败: {}", e))?;
        total_extracted = total_extracted.saturating_add(written);
        if total_extracted > MAX_CORE_EXTRACT_BYTES {
            return Err("解压后的核心文件超过大小上限".into());
        }
    }

    Ok(dlls)
}


pub(crate) async fn probe_core_version(exe: &Path) -> Result<String, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(exe)
            .args(["version"])
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .output(),
    )
    .await
    .map_err(|_| "核心版本检测超时".to_string())?
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

pub async fn perform_core_update(
    app: super::UpdateContext,
    asset_name: String,
    asset_url: String,
    asset_size: u64,
    asset_digest: String,
    mirror: Option<String>,
    singbox_path: String,
) -> Result<CoreUpdateResult, String> {
    core_asset_hash(&asset_digest)?;
    let service_name = crate::service::service_name()?;
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
    let staging = app.directory.join("core-update");
    let cleanup = |msg: String| {
        let _ = std::fs::remove_dir_all(&staging);
        msg
    };

    let files_dir = staging.join("files");
    let staged_exe = files_dir.join(CORE_EXE_NAME);
    let reusable = {
        let staging = staging.clone();
        let asset_name = asset_name.clone();
        let asset_url = asset_url.clone();
        let asset_digest = asset_digest.clone();
        tokio::task::spawn_blocking(move || {
            take_staged(&staging, &asset_name, &asset_url, asset_size, &asset_digest)
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
            let asset_path = staging.join(STAGED_ASSET_FILE);
            download_asset(
                &app,
                CORE_PROGRESS_EVENT,
                &download_url,
                asset_size,
                &asset_path,
            )
            .await
            .map_err(cleanup)?;

            emit_progress(&app, CORE_PROGRESS_EVENT, "extract", 0, 0);
            let files_dir = files_dir.clone();
            tokio::task::spawn_blocking(move || {
                extract_core_files(&asset_path, &asset_name, &files_dir, &asset_digest)
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
    let operation = crate::service::admin::Operation::UpdateCore {
            service: service_name,
            staging: files_dir,
            target,
            files,
        };
    let result = crate::service::request_value(operation).await;
    let restarted=result.map_err(cleanup)?.as_bool()
    .ok_or("更新进程返回了无效结果")?;
    // 步骤 10：清理
    let _ = std::fs::remove_dir_all(&staging);

    Ok(CoreUpdateResult { version, restarted })
}

#[cfg(test)]
mod tests {
    use super::super::{STAGED_MANIFEST, StagedCore};
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);
    const ASSET_NAME: &str = "sing-box-windows-amd64.zip";
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
            self.write_archive_named("release/sing-box.exe", exe, dll)
        }

        fn write_archive_named(&self, exe_entry: &str, exe: &[u8], dll: &[u8]) -> (u64, String) {
            let path = self.0.join(STAGED_ASSET_FILE);
            let file = std::fs::File::create(&path).unwrap();
            let mut archive = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            archive.start_file(exe_entry, options).unwrap();
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

        fn write_raw_exe(&self, exe: &[u8]) -> (u64, String) {
            let path = self.0.join(STAGED_ASSET_FILE);
            std::fs::write(&path, exe).unwrap();
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

        let error = extract_core_files(
            &staging.0.join(STAGED_ASSET_FILE),
            ASSET_NAME,
            &files,
            &digest,
        )
        .unwrap_err();

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

        let dlls = take_staged(&staging.0, ASSET_NAME, ASSET_URL, size, &digest).unwrap();

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
    fn raw_exe_and_renamed_zip_exe_are_staged_as_sing_box_exe() {
        let staging = TestStaging::new();
        let (_, digest) = staging.write_raw_exe(b"MZraw-binary");
        let files = staging.0.join("files");
        let dlls = extract_core_files(
            &staging.0.join(STAGED_ASSET_FILE),
            "sing-box_v1.15.0-alpha.13-xireiki.1_windows_amd64.exe",
            &files,
            &digest,
        )
        .unwrap();
        assert!(dlls.is_empty());
        assert_eq!(
            std::fs::read(files.join(CORE_EXE_NAME)).unwrap(),
            b"MZraw-binary"
        );

        let (_, bad_digest) = staging.write_raw_exe(b"not-a-pe");
        assert!(
            extract_core_files(
                &staging.0.join(STAGED_ASSET_FILE),
                "sing-box_windows_amd64.exe",
                &files,
                &bad_digest,
            )
            .is_err()
        );

        let (_, renamed_digest) = staging.write_archive_named(
            "release/sing-box_windows_amd64.exe",
            b"renamed exe",
            b"trusted dll",
        );
        let dlls = extract_core_files(
            &staging.0.join(STAGED_ASSET_FILE),
            ASSET_NAME,
            &files,
            &renamed_digest,
        )
        .unwrap();
        assert_eq!(dlls, vec!["libcronet.dll"]);
        assert_eq!(
            std::fs::read(files.join(CORE_EXE_NAME)).unwrap(),
            b"renamed exe"
        );
    }

    #[test]
    fn a_manifest_cannot_authorize_an_altered_cached_archive() {
        let staging = TestStaging::new();
        let (size, digest) = staging.write_archive(b"trusted exe", b"trusted dll");
        staging.write_archive(b"changed exe", b"changed dll");
        staging.write_manifest(size, &digest);

        assert!(take_staged(&staging.0, ASSET_NAME, ASSET_URL, size, &digest).is_none());
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
                ASSET_NAME,
                "https://example.invalid/other.zip",
                size,
                &digest
            )
            .is_none()
        );
        assert!(take_staged(&staging.0, ASSET_NAME, ASSET_URL, size + 1, &digest).is_none());
        assert!(
            take_staged(
                &staging.0,
                ASSET_NAME,
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

        assert!(take_staged(&staging.0, ASSET_NAME, ASSET_URL, size, &digest).is_none());
        assert!(!staging.0.join("files").exists());
    }
}
