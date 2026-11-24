use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::root::{self, SERVICE_HOME, quote};

use super::{
    AssetTarget, CORE_PROGRESS_EVENT, CoreUpdateResult, STAGED_MANIFEST, StagedCore, TargetArch,
    TargetOs, UPDATE_LOCK, apply_mirror, core_asset_hash, download_asset, emit_progress,
    staged_matches, verify_archive,
};

const CORE_BIN_NAME: &str = "sing-box";
const STAGED_ASSET_FILE: &str = "core.asset";
const MAX_CORE_EXTRACT_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn asset_target() -> Result<AssetTarget, String> {
    match std::env::consts::ARCH {
        "aarch64" => Ok(AssetTarget {
            os: TargetOs::Android,
            arch: TargetArch::Arm64,
        }),
        "arm" => Ok(AssetTarget {
            os: TargetOs::Android,
            arch: TargetArch::Arm,
        }),
        "x86_64" => Ok(AssetTarget {
            os: TargetOs::Android,
            arch: TargetArch::Amd64,
        }),
        "x86" => Ok(AssetTarget {
            os: TargetOs::Android,
            arch: TargetArch::X86,
        }),
        other => Err(format!("不支持的 CPU 架构: {}", other)),
    }
}

fn is_tar_gz_asset(asset_name: &str) -> bool {
    let lower = asset_name.trim().to_ascii_lowercase();
    lower.ends_with(".tar.gz") || lower.ends_with(".tgz")
}

fn is_candidate_core_bin(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if !(lower.starts_with("sing-box") || lower.starts_with("singbox")) {
        return false;
    }
    !matches!(
        lower.rsplit_once('.').map(|(_, ext)| ext),
        Some(
            "md" | "txt"
                | "json"
                | "yaml"
                | "yml"
                | "service"
                | "sh"
                | "conf"
                | "cfg"
                | "ini"
                | "toml"
                | "sig"
                | "asc"
                | "sha256"
                | "sha512"
                | "exe"
                | "dll"
                | "so"
                | "dylib"
                | "plist"
        )
    )
}

pub(crate) async fn probe_core_version(target: &Path) -> Result<String, String> {
    let target = target.to_string_lossy();
    let target = target.trim();
    if target.is_empty() {
        return Err("核心路径为空".into());
    }
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        root::run_async(format!("{} version", quote(target))),
    )
    .await
    .map_err(|_| "核心版本检测超时".to_string())??;
    Ok(output.lines().next().unwrap_or("").trim().to_string())
}

fn staging_dir(app: &super::UpdateContext) -> Result<PathBuf, String> {
    Ok(app.directory.join("core-update"))
}

fn take_staged(
    staging: &Path,
    asset_name: &str,
    asset_url: &str,
    asset_size: u64,
    asset_digest: &str,
) -> bool {
    staged_matches(staging, asset_url, asset_size, asset_digest)
        && extract_core(
            &staging.join(STAGED_ASSET_FILE),
            asset_name,
            &staging.join("files"),
            asset_digest,
        )
        .is_ok()
}

fn write_staged_binary(files_dir: &Path, data: &[u8]) -> Result<(), String> {
    if files_dir.exists() {
        std::fs::remove_dir_all(files_dir).map_err(|e| format!("清理临时目录失败: {}", e))?;
    }
    std::fs::create_dir_all(files_dir).map_err(|e| format!("创建临时目录失败: {}", e))?;
    std::fs::write(files_dir.join(CORE_BIN_NAME), data)
        .map_err(|e| format!("写入临时文件失败: {}", e))
}

fn extract_core(
    asset_path: &Path,
    asset_name: &str,
    files_dir: &Path,
    asset_digest: &str,
) -> Result<(), String> {
    let bytes = verify_archive(asset_path, asset_digest)?;
    if !is_tar_gz_asset(asset_name) {
        if bytes.len() as u64 > MAX_CORE_EXTRACT_BYTES {
            return Err("核心文件超过大小上限".into());
        }
        if !bytes.starts_with(b"\x7fELF") {
            return Err("下载的文件不是有效的 Android 可执行文件".into());
        }
        return write_staged_binary(files_dir, &bytes);
    }

    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|e| format!("读取压缩包失败: {}", e))?;
    let mut fallback: Option<Vec<u8>> = None;
    let mut ambiguous = false;
    for entry in entries {
        let entry = entry.map_err(|e| format!("读取压缩包失败: {}", e))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        if name == CORE_BIN_NAME {
            let mut data = Vec::new();
            entry
                .take(MAX_CORE_EXTRACT_BYTES + 1)
                .read_to_end(&mut data)
                .map_err(|e| format!("解压失败: {}", e))?;
            if data.len() as u64 > MAX_CORE_EXTRACT_BYTES {
                return Err("解压后的核心文件超过大小上限".into());
            }
            return write_staged_binary(files_dir, &data);
        }
        if is_candidate_core_bin(&name) {
            if fallback.is_some() || ambiguous {
                ambiguous = true;
                fallback = None;
                continue;
            }
            let mut data = Vec::new();
            entry
                .take(MAX_CORE_EXTRACT_BYTES + 1)
                .read_to_end(&mut data)
                .map_err(|e| format!("解压失败: {}", e))?;
            if data.len() as u64 > MAX_CORE_EXTRACT_BYTES {
                return Err("解压后的核心文件超过大小上限".into());
            }
            fallback = Some(data);
        }
    }
    if !ambiguous
        && let Some(data) = fallback
    {
        return write_staged_binary(files_dir, &data);
    }
    Err(format!("压缩包内未找到 {}", CORE_BIN_NAME))
}

async fn download_and_extract(
    app: &super::UpdateContext,
    staging: &Path,
    asset_name: &str,
    asset_url: &str,
    asset_size: u64,
    asset_digest: &str,
    mirror: &Option<String>,
) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(staging);
    std::fs::create_dir_all(staging).map_err(|e| format!("创建临时目录失败: {}", e))?;
    let archive = staging.join(STAGED_ASSET_FILE);
    download_asset(
        app,
        CORE_PROGRESS_EVENT,
        &apply_mirror(mirror, asset_url),
        asset_size,
        &archive,
    )
    .await?;

    emit_progress(app, CORE_PROGRESS_EVENT, "extract", 0, 0);
    let staging = staging.to_path_buf();
    let asset_name = asset_name.to_string();
    let asset_url = asset_url.to_string();
    let asset_digest = asset_digest.to_string();
    tokio::task::spawn_blocking(move || {
        extract_core(&archive, &asset_name, &staging.join("files"), &asset_digest)?;
        let manifest = serde_json::to_string(&StagedCore {
            asset_url,
            asset_size,
            asset_digest,
        })
        .map_err(|e| format!("写入清单失败: {}", e))?;
        std::fs::write(staging.join(STAGED_MANIFEST), manifest)
            .map_err(|e| format!("写入清单失败: {}", e))
    })
    .await
    .map_err(|e| format!("任务执行失败: {}", e))?
}

async fn swap_core(staged: &Path, target: &str) -> Result<(String, bool), String> {
    use crate::commands::service;

    if let Some(installed) = service::installed_core_path().await {
        if installed != target {
            return Err("所选核心路径与已登记服务不一致，请先在设置中重新识别工作目录".into());
        }
    }

    let pending = format!("{SERVICE_HOME}/.staging/{CORE_BIN_NAME}");
    let version_line = root::run_async(format!(
        "set -e\nmkdir -p {stage_dir}\ncp -f {src} {pending}\nchmod 755 {pending}\n{pending} version",
        stage_dir = quote(&format!("{SERVICE_HOME}/.staging")),
        src = quote(&staged.to_string_lossy()),
        pending = quote(&pending)
    ))
    .await
    .map_err(|e| format!("下载的核心无法运行: {e}"))?;
    let version = version_line.lines().next().unwrap_or("unknown").to_string();

    let was_running = service::is_running().await;
    if was_running {
        if let Err(e) = service::stop().await {
            let _ = root::run_async(format!("rm -f {}", quote(&pending))).await;
            return Err(e);
        }
    }

    let backup = format!("{target}.bak");
    if let Err(e) = root::run_async(format!(
        "set -e\nmkdir -p \"$(dirname {t})\"\nif [ -f {t} ]; then cp -f {t} {b}; fi\nmv -f {p} {t}\nchmod 755 {t}",
        t = quote(target),
        b = quote(&backup),
        p = quote(&pending)
    ))
    .await
    {
        let _ = root::run_async(format!("rm -f {}", quote(&pending))).await;
        if was_running {
            let _ = service::start().await;
        }
        return Err(format!("替换核心失败: {e}"));
    }

    if !was_running {
        let _ = root::run_async(format!("rm -f {}", quote(&backup))).await;
        return Ok((version, false));
    }

    let started = match service::start().await {
        Ok(()) => {
            tokio::time::sleep(Duration::from_secs(2)).await;
            service::is_running().await
        }
        Err(_) => false,
    };
    if started {
        let _ = root::run_async(format!("rm -f {}", quote(&backup))).await;
        return Ok((version, true));
    }

    let _ = service::stop().await;
    let _ = root::run_async(format!(
        "if [ -f {b} ]; then mv -f {b} {t}; chmod 755 {t}; fi",
        b = quote(&backup),
        t = quote(target)
    ))
    .await;
    let _ = service::start().await;
    Err("新核心启动失败，已恢复旧版本".into())
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
    if !root::enabled() {
        return Err("核心更新需要启用 root 模式".into());
    }
    let _guard = UPDATE_LOCK
        .try_lock()
        .map_err(|_| "更新正在进行中".to_string())?;

    let target = singbox_path.trim().to_string();
    if target.is_empty() {
        return Err("请先在服务配置中设置 sing-box 路径".into());
    }

    let staging = staging_dir(&app)?;
    let cleanup = |msg: String| {
        let _ = std::fs::remove_dir_all(&staging);
        msg
    };

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
    if !reusable {
        download_and_extract(
            &app,
            &staging,
            &asset_name,
            &asset_url,
            asset_size,
            &asset_digest,
            &mirror,
        )
        .await
        .map_err(cleanup)?;
    }

    emit_progress(&app, CORE_PROGRESS_EVENT, "replace", 0, 0);
    let staged = staging.join("files").join(CORE_BIN_NAME);
    let result = swap_core(&staged, &target).await;
    let _ = std::fs::remove_dir_all(&staging);
    let (version, restarted) = result?;
    Ok(CoreUpdateResult { version, restarted })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn extracts_only_the_core_binary_from_a_verified_archive() {
        let dir = std::env::temp_dir().join(format!("singboard-core-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        for (name, body) in [
            ("sing-box-1.0.0-android-arm64/LICENSE", b"license".as_slice()),
            ("sing-box-1.0.0-android-arm64/sing-box", b"core-binary".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, body).unwrap();
        }
        let archive = builder.into_inner().unwrap().finish().unwrap();
        let archive_path = dir.join(STAGED_ASSET_FILE);
        std::fs::write(&archive_path, &archive).unwrap();
        let digest = format!("sha256:{:x}", Sha256::digest(&archive));

        let files = dir.join("files");
        extract_core(
            &archive_path,
            "sing-box-1.0.0-android-arm64.tar.gz",
            &files,
            &digest,
        )
        .unwrap();
        assert_eq!(std::fs::read(files.join("sing-box")).unwrap(), b"core-binary");
        assert!(!files.join("LICENSE").exists());

        let wrong = format!("sha256:{}", "0".repeat(64));
        assert!(
            extract_core(
                &archive_path,
                "sing-box-1.0.0-android-arm64.tar.gz",
                &files,
                &wrong,
            )
            .is_err()
        );

        let raw = b"\x7fELF-raw-android-core";
        std::fs::write(&archive_path, raw).unwrap();
        let raw_digest = format!("sha256:{:x}", Sha256::digest(raw));
        extract_core(
            &archive_path,
            "sing-box_v1.15.0_android_arm64",
            &files,
            &raw_digest,
        )
        .unwrap();
        assert_eq!(std::fs::read(files.join("sing-box")).unwrap(), raw);

        let bad_raw = b"not-elf";
        std::fs::write(&archive_path, bad_raw).unwrap();
        let bad_digest = format!("sha256:{:x}", Sha256::digest(bad_raw));
        assert!(
            extract_core(
                &archive_path,
                "sing-box_v1.15.0_android_arm64",
                &files,
                &bad_digest,
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
