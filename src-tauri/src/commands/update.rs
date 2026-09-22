use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

use crate::service::scm;

/// 防止并发执行更新
pub(crate) static UPDATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) const CORE_PROGRESS_EVENT: &str = "core-update-progress";

const CORE_EXE_NAME: &str = "sing-box.exe";
/// 一致性校验阶段解压结果的清单，供随后的安装复用（同一资产不下载两次）
const STAGED_MANIFEST: &str = "staged.json";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CoreUpdateInfo {
    version: String,
    prerelease: bool,
    published_at: String,
    asset_name: String,
    asset_url: String,
    asset_size: u64,
    /// GitHub 资产的 SHA-256（形如 "sha256:..."），个别源可能缺失则为空串
    asset_digest: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreUpdateResult {
    version: String,
    restarted: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateProgress {
    phase: &'static str,
    downloaded: u64,
    total: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedCore {
    asset_url: String,
    asset_size: u64,
    asset_digest: String,
}

#[derive(Deserialize)]
pub(crate) struct GhAsset {
    pub(crate) name: String,
    pub(crate) browser_download_url: String,
    pub(crate) size: u64,
    #[serde(default)]
    pub(crate) digest: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct GhRelease {
    pub(crate) tag_name: String,
    pub(crate) prerelease: bool,
    #[serde(default)]
    pub(crate) draft: bool,
    #[serde(default)]
    pub(crate) published_at: Option<String>,
    pub(crate) assets: Vec<GhAsset>,
}

fn validate_repo(repo: &str) -> Result<(), String> {
    let parts: Vec<&str> = repo.split('/').collect();
    let valid = parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        });
    if valid {
        Ok(())
    } else {
        Err("仓库格式应为 owner/repo".into())
    }
}

fn windows_arch_suffix() -> Result<String, String> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("windows-amd64.zip".into()),
        "aarch64" => Ok("windows-arm64.zip".into()),
        other => Err(format!("不支持的 CPU 架构: {}", other)),
    }
}

/// 下载与解压产物放在 %TEMP%\singboard（整个目录由核心更新独占，会被清空重建）。
/// 校验与安装共用它，安装才能复用校验解压出来的文件
fn staging_dir() -> PathBuf {
    std::env::temp_dir().join("singboard")
}

fn core_asset_hash(asset_digest: &str) -> Result<&str, String> {
    asset_digest
        .trim()
        .strip_prefix("sha256:")
        .map(str::trim)
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| "上游未提供有效的 SHA-256 校验信息，已中止核心更新".to_string())
}

fn take_staged(
    staging: &Path,
    asset_url: &str,
    asset_size: u64,
    asset_digest: &str,
) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(staging.join(STAGED_MANIFEST)).ok()?;
    let staged: StagedCore = serde_json::from_str(&text).ok()?;
    if staged.asset_url != asset_url
        || staged.asset_size != asset_size
        || !core_asset_hash(&staged.asset_digest)
            .ok()?
            .eq_ignore_ascii_case(core_asset_hash(asset_digest).ok()?)
    {
        return None;
    }
    extract_core_files(
        &staging.join("core.zip"),
        &staging.join("files"),
        asset_digest,
    )
    .ok()
}

pub(crate) fn emit_progress(
    app: &tauri::AppHandle,
    event: &str,
    phase: &'static str,
    downloaded: u64,
    total: u64,
) {
    let _ = app.emit(
        event,
        UpdateProgress {
            phase,
            downloaded,
            total,
        },
    );
}

/// ghproxy 风格镜像：前缀 + 完整原始 URL
pub(crate) fn apply_mirror(mirror: &Option<String>, url: &str) -> String {
    match mirror.as_deref().map(str::trim) {
        Some(m) if !m.is_empty() => format!("{}/{}", m.trim_end_matches('/'), url),
        _ => url.to_string(),
    }
}

pub(crate) async fn github_get(url: &str) -> Result<reqwest::Response, String> {
    let client = super::network::build_client(Some(Duration::from_secs(30)))
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let resp = client
        .get(url)
        .header("User-Agent", "singboard")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("请求 GitHub API 失败: {}", e))?;

    let status = resp.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        let body = resp.text().await.unwrap_or_default();
        if body.contains("rate limit") {
            return Err("GitHub API 限流，请稍后再试".into());
        }
        return Err("GitHub API 拒绝访问 (403)".into());
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("仓库不存在或没有发布版本".into());
    }
    if !status.is_success() {
        return Err(format!("GitHub API 返回错误: {}", status));
    }
    Ok(resp)
}

fn pick_windows_asset(release: &GhRelease, suffix: &str) -> Result<CoreUpdateInfo, String> {
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(suffix))
        .ok_or_else(|| {
            format!(
                "该版本未提供适用于 {} 的资产",
                suffix.trim_end_matches(".zip")
            )
        })?;
    Ok(CoreUpdateInfo {
        version: release.tag_name.clone(),
        prerelease: release.prerelease,
        published_at: release.published_at.clone().unwrap_or_default(),
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
        asset_size: asset.size,
        asset_digest: asset.digest.clone().unwrap_or_default(),
    })
}

#[tauri::command]
pub async fn check_core_update(repo: String, channel: String) -> Result<CoreUpdateInfo, String> {
    let repo = repo.trim().to_string();
    validate_repo(&repo)?;
    let suffix = windows_arch_suffix()?;

    let release = if channel == "testing" {
        let url = format!("https://api.github.com/repos/{}/releases?per_page=10", repo);
        let releases: Vec<GhRelease> = github_get(&url)
            .await?
            .json()
            .await
            .map_err(|e| format!("解析 GitHub API 响应失败: {}", e))?;
        releases
            .into_iter()
            .find(|r| !r.draft)
            .ok_or("该仓库暂无发布版本")?
    } else {
        let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
        github_get(&url)
            .await?
            .json()
            .await
            .map_err(|e| format!("解析 GitHub API 响应失败: {}", e))?
    };

    pick_windows_asset(&release, &suffix)
}

pub(crate) async fn download_asset(
    app: &tauri::AppHandle,
    event: &str,
    url: &str,
    expected_size: u64,
    dest: &Path,
) -> Result<(), String> {
    // 下载不设总超时（大文件慢速网络下会中途截断），连接问题由系统层面报错
    let client =
        super::network::build_client(None).map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let mut resp = client
        .get(url)
        .header("User-Agent", "singboard")
        .send()
        .await
        .map_err(|e| format!("下载失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }

    let total = resp.content_length().unwrap_or(expected_size);
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("创建临时文件失败: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();
    emit_progress(app, event, "download", 0, total);
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("下载中断: {}", e))? {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(200) {
            emit_progress(app, event, "download", downloaded, total);
            last_emit = Instant::now();
        }
    }
    file.flush()
        .await
        .map_err(|e| format!("写入临时文件失败: {}", e))?;
    emit_progress(app, event, "download", downloaded, total);

    if expected_size > 0 && downloaded != expected_size {
        return Err(format!(
            "下载文件不完整（{} / {} 字节）",
            downloaded, expected_size
        ));
    }
    Ok(())
}

/// 从 zip 中解出 sing-box.exe（必需）与随附的 dll 依赖（如 naive 需要的
/// libcronet.dll）。按文件名匹配、平铺写入 staging，不依赖目录结构。
/// 返回解出的 dll 文件名列表。
fn extract_core_files(
    zip_path: &Path,
    staging: &Path,
    asset_digest: &str,
) -> Result<Vec<String>, String> {
    let expected = core_asset_hash(asset_digest)?;
    let bytes = std::fs::read(zip_path).map_err(|e| format!("打开压缩包失败: {}", e))?;
    if !format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(expected) {
        return Err("核心文件 SHA-256 校验失败，已中止更新".into());
    }
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

/// 对下载的核心跑一次 `version`，确认能运行并取版本串
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
pub async fn probe_asset_exe_hash(
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
    fn core_digest_requires_a_complete_sha256_hash() {
        for digest in ["", "sha256:", "sha256:1234", "sha512:abcd"] {
            assert!(core_asset_hash(digest).is_err(), "{digest}");
        }
        assert!(core_asset_hash(&format!("sha256:{}", "g".repeat(64))).is_err());
        let hash = "AB".repeat(32);
        assert_eq!(core_asset_hash(&format!(" sha256:{hash} ")).unwrap(), hash);
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
