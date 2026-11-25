use std::cmp::Ordering;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub struct UpdateContext {
    pub directory: std::path::PathBuf,
    pub progress: Box<dyn Fn(&str, u64, u64) + Send + Sync>,
}

#[cfg_attr(windows, path = "update/windows.rs")]
#[cfg_attr(target_os = "android", path = "update/android.rs")]
pub mod platform;

/// 防止并发执行更新
pub(crate) static UPDATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) const CORE_PROGRESS_EVENT: &str = "core-update-progress";

/// 一致性校验阶段解压结果的清单，供随后的安装复用（同一资产不下载两次）
pub(crate) const STAGED_MANIFEST: &str = "staged.json";
const STAGED_ASSET_FILE: &str = "core.asset";
const CORE_HASH_RECORD: &str = "core-update-hash.json";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CoreUpdateInfo {
    pub version: String,
    pub prerelease: bool,
    pub published_at: String,
    pub asset_name: String,
    pub asset_url: String,
    pub asset_size: u64,
    /// GitHub 资产的 SHA-256（形如 "sha256:..."），个别源可能缺失则为空串
    pub asset_digest: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreUpdateResult {
    pub(crate) version: String,
    pub(crate) restarted: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StagedCore {
    pub(crate) asset_url: String,
    pub(crate) asset_size: u64,
    pub(crate) asset_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoreHashRecord {
    asset_digest: String,
    core_hash: String,
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

fn is_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn core_asset_hash(asset_digest: &str) -> Result<&str, String> {
    asset_digest
        .trim()
        .strip_prefix("sha256:")
        .map(str::trim)
        .filter(|hash| is_sha256(hash))
        .ok_or_else(|| "上游未提供有效的 SHA-256 校验信息，已中止核心更新".to_string())
}

pub(crate) fn verify_archive(archive: &Path, asset_digest: &str) -> Result<Vec<u8>, String> {
    let expected = core_asset_hash(asset_digest)?;
    let bytes = std::fs::read(archive).map_err(|e| format!("打开下载文件失败: {}", e))?;
    if !format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(expected) {
        return Err("核心文件 SHA-256 校验失败，已中止更新".into());
    }
    Ok(bytes)
}

pub(crate) fn staged_matches(
    staging: &Path,
    asset_url: &str,
    asset_size: u64,
    asset_digest: &str,
) -> bool {
    let Ok(text) = std::fs::read_to_string(staging.join(STAGED_MANIFEST)) else {
        return false;
    };
    let Ok(staged) = serde_json::from_str::<StagedCore>(&text) else {
        return false;
    };
    let same_digest = match (
        core_asset_hash(&staged.asset_digest),
        core_asset_hash(asset_digest),
    ) {
        (Ok(a), Ok(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    };
    staged.asset_url == asset_url && staged.asset_size == asset_size && same_digest
}

fn take_staged(
    staging: &Path,
    asset_name: &str,
    asset_url: &str,
    asset_size: u64,
    asset_digest: &str,
) -> Option<platform::CoreFiles> {
    if !staged_matches(staging, asset_url, asset_size, asset_digest) {
        return None;
    }
    platform::extract_core_files(
        &staging.join(STAGED_ASSET_FILE),
        asset_name,
        &staging.join("files"),
        asset_digest,
    )
    .ok()
}

async fn prepare_core(
    app: &UpdateContext,
    info: &CoreUpdateInfo,
    mirror: &Option<String>,
) -> Result<(platform::CoreFiles, String), String> {
    let staging = app.directory.join("core-update");
    let reusable = {
        let staging = staging.clone();
        let info = info.clone();
        tokio::task::spawn_blocking(move || {
            take_staged(
                &staging,
                &info.asset_name,
                &info.asset_url,
                info.asset_size,
                &info.asset_digest,
            )
        })
        .await
        .map_err(|e| format!("任务执行失败: {e}"))?
    };
    if reusable.is_none() {
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|e| format!("清理临时目录失败: {e}"))?;
        }
        std::fs::create_dir_all(&staging).map_err(|e| format!("创建临时目录失败: {e}"))?;
        download_asset(
            app,
            CORE_PROGRESS_EVENT,
            &apply_mirror(mirror, &info.asset_url),
            info.asset_size,
            &staging.join(STAGED_ASSET_FILE),
        )
        .await?;
        emit_progress(app, CORE_PROGRESS_EVENT, "extract", 0, 0);
    }
    let record_path = app.directory.join(CORE_HASH_RECORD);
    let info = info.clone();
    tokio::task::spawn_blocking(move || {
        let files = staging.join("files");
        let extracted = match reusable {
            Some(extracted) => extracted,
            None => {
                let extracted = platform::extract_core_files(
                    &staging.join(STAGED_ASSET_FILE),
                    &info.asset_name,
                    &files,
                    &info.asset_digest,
                )?;
                let manifest = serde_json::to_vec(&StagedCore {
                    asset_url: info.asset_url,
                    asset_size: info.asset_size,
                    asset_digest: info.asset_digest.clone(),
                })
                .map_err(|e| e.to_string())?;
                std::fs::write(staging.join(STAGED_MANIFEST), manifest)
                    .map_err(|e| format!("写入清单失败: {e}"))?;
                extracted
            }
        };
        let core_hash = if platform::is_raw_asset(&info.asset_name) {
            core_asset_hash(&info.asset_digest)?.to_ascii_lowercase()
        } else {
            let core_hash = platform::core_file_hash(&files.join(platform::CORE_NAME))?;
            let record = CoreHashRecord {
                asset_digest: info.asset_digest,
                core_hash: core_hash.clone(),
            };
            if let Ok(bytes) = serde_json::to_vec(&record) {
                let _ = std::fs::write(record_path, bytes);
            }
            core_hash
        };
        Ok((extracted, core_hash))
    })
    .await
    .map_err(|e| format!("任务执行失败: {e}"))?
}

pub(crate) async fn core_matches_release(
    app: &UpdateContext,
    target: &Path,
    info: &CoreUpdateInfo,
    mirror: &Option<String>,
) -> Result<bool, String> {
    let _guard = UPDATE_LOCK.lock().await;
    let target = target.to_owned();
    let installed = tokio::task::spawn_blocking(move || platform::core_file_hash(&target))
        .await
        .map_err(|e| format!("任务执行失败: {e}"))??;
    let digest = core_asset_hash(&info.asset_digest)?;
    if platform::is_raw_asset(&info.asset_name) {
        return Ok(installed.eq_ignore_ascii_case(digest));
    }
    if let Ok(bytes) = std::fs::read(app.directory.join(CORE_HASH_RECORD))
        && let Ok(record) = serde_json::from_slice::<CoreHashRecord>(&bytes)
        && record.asset_digest.eq_ignore_ascii_case(&info.asset_digest)
        && is_sha256(&record.core_hash)
    {
        return Ok(installed.eq_ignore_ascii_case(&record.core_hash));
    }
    let result = prepare_core(app, info, mirror)
        .await
        .map(|(_, hash)| installed.eq_ignore_ascii_case(&hash));
    if !matches!(result, Ok(false)) {
        let _ = std::fs::remove_dir_all(app.directory.join("core-update"));
    }
    result
}

pub(crate) fn emit_progress(
    app: &UpdateContext,
    event: &str,
    phase: &'static str,
    downloaded: u64,
    total: u64,
) {
    let _ = event;
    (app.progress)(phase, downloaded, total);
}

/// ghproxy 风格镜像：前缀 + 完整原始 URL
pub(crate) fn apply_mirror(mirror: &Option<String>, url: &str) -> String {
    match mirror.as_deref().map(str::trim) {
        Some(m) if !m.is_empty() => format!("{}/{}", m.trim_end_matches('/'), url),
        _ => url.to_string(),
    }
}

const GITHUB_API: &str = "https://api.github.com/";
const INVALID_TOKEN: &str = "GitHub Token 无效或已过期 (401)";

pub(crate) fn github_token(token: Option<&str>) -> Option<&str> {
    token.map(str::trim).filter(|token| !token.is_empty())
}

fn describe_github_error(err: reqwest::Error) -> String {
    use std::error::Error as _;
    let mut detail = String::new();
    let mut current = err.source();
    while let Some(cause) = current {
        detail = cause.to_string();
        current = cause.source();
    }
    let prefix = if err.is_timeout() {
        "请求超时"
    } else if err.is_connect() {
        "连接失败"
    } else if err.is_builder() {
        "请求构造失败"
    } else {
        "网络请求失败"
    };
    if detail.is_empty() {
        format!("请求 GitHub API 失败: {prefix} ({})", err.without_url())
    } else {
        format!("请求 GitHub API 失败: {prefix} ({detail})")
    }
}

pub(crate) async fn github_get_with_token(
    url: &str,
    token: Option<&str>,
) -> Result<reqwest::Response, String> {
    let token = github_token(token);
    if let Some(tok) = token
        && (!tok.is_ascii() || tok.chars().any(char::is_control))
    {
        return Err("GitHub Token 包含无效字符，请重新填写".into());
    }
    let client = super::network::build_client(Some(Duration::from_secs(30)))
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let mut attempt = 0u8;
    let resp = loop {
        let mut request = client
            .get(url)
            .header("User-Agent", "singboard")
            .header("Accept", "application/vnd.github+json");
        if let Some(token) = token.filter(|_| url.starts_with(GITHUB_API)) {
            request = request.bearer_auth(token);
        }
        match request.send().await {
            Ok(resp) => break resp,
            Err(e) if attempt == 0 && !e.is_builder() && (e.is_connect() || e.is_timeout() || e.is_request()) => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => return Err(describe_github_error(e)),
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(INVALID_TOKEN.into());
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TargetOs {
    Windows,
    Android,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TargetArch {
    Amd64,
    Arm64,
    Arm,
    X86,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AssetTarget {
    pub(crate) os: TargetOs,
    pub(crate) arch: TargetArch,
}

impl AssetTarget {
    fn label(self) -> &'static str {
        match (self.os, self.arch) {
            (TargetOs::Windows, TargetArch::Amd64) => "windows-amd64",
            (TargetOs::Windows, TargetArch::Arm64) => "windows-arm64",
            (TargetOs::Windows, TargetArch::Arm) => "windows-arm",
            (TargetOs::Windows, TargetArch::X86) => "windows-386",
            (TargetOs::Android, TargetArch::Arm64) => "android-arm64",
            (TargetOs::Android, TargetArch::Arm) => "android-arm",
            (TargetOs::Android, TargetArch::Amd64) => "android-amd64",
            (TargetOs::Android, TargetArch::X86) => "android-386",
        }
    }
}

const BLOCKED_EXTENSIONS: &[&str] = &[
    ".sha256",
    ".sha256sum",
    ".sha512",
    ".sha512sum",
    ".md5",
    ".sig",
    ".asc",
    ".txt",
    ".json",
    ".yaml",
    ".yml",
    ".sbom",
    ".spdx",
    ".pem",
    ".crt",
    ".deb",
    ".rpm",
    ".pkg",
    ".dmg",
    ".appimage",
    ".apk",
    ".aab",
    ".ipa",
    ".msi",
    ".msix",
    ".pdb",
    ".dsym",
    ".db",
    ".srs",
    ".mmdb",
    ".dat",
    ".tar.xz",
    ".tar.zst",
    ".tar.bz2",
    ".7z",
    ".rar",
    ".xz",
    ".zst",
    ".bz2",
];

const NON_BINARY_EXTENSIONS: &[&str] = &[
    "zip", "exe", "gz", "tar", "so", "dll", "dylib", "a", "o", "lib", "md", "sh", "bat", "cmd",
    "ps1", "html", "htm", "xml", "pdf", "png", "jpg", "jpeg", "webp", "svg", "ico", "log", "conf",
    "cfg", "ini", "toml", "csv", "service", "plist", "go", "rs", "py", "js", "ts", "java", "kt",
    "swift", "c", "h", "cpp", "hpp",
];

fn valid_asset_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | '?' | '#'))
}

fn parse_asset_format(lower: &str, os: TargetOs) -> Option<(&str, u8)> {
    if BLOCKED_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return None;
    }
    match os {
        TargetOs::Windows => {
            if let Some(stem) = lower.strip_suffix(".zip") {
                Some((stem, 0))
            } else {
                lower.strip_suffix(".exe").map(|stem| (stem, 1))
            }
        }
        TargetOs::Android => {
            if let Some(stem) = lower
                .strip_suffix(".tar.gz")
                .or_else(|| lower.strip_suffix(".tgz"))
            {
                return Some((stem, 0));
            }
            if let Some(stem) = lower.strip_suffix(".bin") {
                return Some((stem, 1));
            }
            if let Some((_, ext)) = lower.rsplit_once('.')
                && NON_BINARY_EXTENSIONS.contains(&ext)
            {
                return None;
            }
            Some((lower, 1))
        }
    }
}

fn is_microarch_suffix(tokens: &[&str], index: usize) -> bool {
    let Some(token) = tokens.get(index) else {
        return false;
    };
    if !matches!(*token, "v2" | "v3" | "v4") {
        return false;
    }
    !tokens
        .get(index + 1)
        .is_some_and(|next| !next.is_empty() && next.bytes().all(|b| b.is_ascii_digit()))
}

fn score_asset(name: &str, target: AssetTarget) -> Option<(u8, u8, u8, u8, usize)> {
    let trimmed = name.trim();
    if !valid_asset_name(trimmed) {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let (stem, format_rank) = parse_asset_format(&lower, target.os)?;
    let tokens: Vec<&str> = stem
        .split(['-', '_', '.'])
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    if tokens.iter().any(|t| {
        matches!(
            *t,
            "source"
                | "src"
                | "debug"
                | "symbols"
                | "sym"
                | "updater"
                | "installer"
                | "setup"
                | "gui"
                | "desktop"
                | "service"
                | "checksum"
                | "checksums"
                | "sha256sums"
                | "geoip"
                | "geosite"
                | "ruleset"
        )
    }) {
        return None;
    }

    match target.os {
        TargetOs::Windows => {
            if tokens.iter().any(|t| {
                matches!(
                    *t,
                    "linux"
                        | "darwin"
                        | "macos"
                        | "osx"
                        | "android"
                        | "androideabi"
                        | "freebsd"
                        | "openbsd"
                        | "netbsd"
                        | "ios"
                        | "illumos"
                        | "solaris"
                        | "openwrt"
                )
            }) {
                return None;
            }
            let has_win = tokens
                .iter()
                .any(|t| matches!(*t, "windows" | "win" | "win64" | "win32"));
            if !has_win {
                return None;
            }
            if tokens.contains(&"win64") && target.arch != TargetArch::Amd64 {
                return None;
            }
            if tokens.contains(&"win32") && target.arch != TargetArch::X86 {
                return None;
            }
        }
        TargetOs::Android => {
            if tokens.iter().any(|t| {
                matches!(
                    *t,
                    "windows"
                        | "win"
                        | "win32"
                        | "win64"
                        | "darwin"
                        | "macos"
                        | "osx"
                        | "freebsd"
                        | "openbsd"
                        | "netbsd"
                        | "ios"
                        | "illumos"
                        | "solaris"
                        | "openwrt"
                )
            }) {
                return None;
            }
            if !tokens.iter().any(|t| matches!(*t, "android" | "androideabi")) {
                return None;
            }
        }
    }

    let mut matched_arch_rank: Option<u8> = None;
    let mut has_other_arch = false;
    let mut has_microarch_variant = false;

    let mut i = 0;
    while i < tokens.len() {
        let mut detected: Option<(TargetArch, u8)> = None;
        match tokens[i] {
            "x86" if tokens.get(i + 1) == Some(&"64") => {
                if is_microarch_suffix(&tokens, i + 2) {
                    has_microarch_variant = true;
                    i += 3;
                } else {
                    detected = Some((TargetArch::Amd64, 1));
                    i += 2;
                }
            }
            "arm64" => {
                if tokens.get(i + 1) == Some(&"v8a") {
                    detected = Some((TargetArch::Arm64, 1));
                    i += 2;
                } else {
                    detected = Some((TargetArch::Arm64, 0));
                    i += 1;
                }
            }
            "armeabi" => {
                detected = Some((TargetArch::Arm, 2));
                i += if tokens.get(i + 1) == Some(&"v7a") {
                    2
                } else {
                    1
                };
            }
            "amd64" => {
                if is_microarch_suffix(&tokens, i + 1) {
                    has_microarch_variant = true;
                    i += 2;
                } else {
                    detected = Some((TargetArch::Amd64, 0));
                    i += 1;
                }
            }
            "x64" => {
                if is_microarch_suffix(&tokens, i + 1) {
                    has_microarch_variant = true;
                    i += 2;
                } else {
                    detected = Some((TargetArch::Amd64, 2));
                    i += 1;
                }
            }
            "amd64v2" | "amd64v3" | "amd64v4" | "x64v2" | "x64v3" | "x64v4" | "x86_64v2"
            | "x86_64v3" | "x86_64v4" => {
                has_microarch_variant = true;
                i += 1;
            }
            "aarch64" => {
                detected = Some((TargetArch::Arm64, 2));
                i += 1;
            }
            "arm" => {
                detected = Some((TargetArch::Arm, 0));
                i += 1;
            }
            "armv7" | "armv7a" | "armv7l" => {
                detected = Some((TargetArch::Arm, 1));
                i += 1;
            }
            "386" => {
                detected = Some((TargetArch::X86, 0));
                i += 1;
            }
            "i386" | "i686" | "ia32" | "x86" => {
                detected = Some((TargetArch::X86, 1));
                i += 1;
            }
            "armv5" | "armv6" | "armel" | "armhf" | "mips" | "mips64" | "mipsle" | "mips64le"
            | "riscv64" | "s390x" | "ppc64" | "ppc64le" | "loong64" => {
                has_other_arch = true;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
        if let Some((arch, rank)) = detected {
            if arch == target.arch {
                matched_arch_rank = Some(matched_arch_rank.map_or(rank, |cur| cur.min(rank)));
            } else {
                has_other_arch = true;
            }
        }
    }

    if matched_arch_rank.is_none()
        && !has_other_arch
        && !has_microarch_variant
        && target.os == TargetOs::Windows
    {
        if target.arch == TargetArch::Amd64 && tokens.contains(&"win64") {
            matched_arch_rank = Some(3);
        } else if target.arch == TargetArch::X86 && tokens.contains(&"win32") {
            matched_arch_rank = Some(3);
        }
    }

    if has_other_arch || has_microarch_variant {
        return None;
    }
    let arch_rank = matched_arch_rank?;
    let variant_rank = u8::from(tokens.iter().any(|t| {
        matches!(
            *t,
            "legacy" | "fips" | "musl" | "lite" | "mini" | "win7" | "compat"
        )
    }));
    let core_name_rank = u8::from(!(lower.contains("sing-box") || lower.contains("singbox")));
    Some((variant_rank, format_rank, arch_rank, core_name_rank, lower.len()))
}

fn pick_asset(release: &GhRelease, target: AssetTarget) -> Result<CoreUpdateInfo, String> {
    let asset = release
        .assets
        .iter()
        .filter_map(|asset| score_asset(&asset.name, target).map(|score| (score, asset)))
        .min_by_key(|(score, _)| *score)
        .map(|(_, asset)| asset)
        .ok_or_else(|| {
            format!(
                "{} 未提供适用于 {} 的资产",
                release.tag_name,
                target.label()
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

fn newest_release(releases: Vec<GhRelease>) -> Option<GhRelease> {
    releases.into_iter().filter(|r| !r.draft).rev().max_by_key(|r| {
        let parsed = parse_core_version(&r.tag_name);
        (
            parsed.as_ref().map(|v| (v.base, v.stage)),
            !r.prerelease,
            r.published_at.clone().unwrap_or_default(),
        )
    })
}

pub async fn check_core_update(
    repo: String,
    channel: String,
    github_token: Option<String>,
) -> Result<CoreUpdateInfo, String> {
    let repo = repo.trim().to_string();
    validate_repo(&repo)?;
    let target = platform::asset_target()?;
    let token = github_token.as_deref();

    let release = if channel == "testing" {
        let url = format!("https://api.github.com/repos/{}/releases?per_page=10", repo);
        let releases: Vec<GhRelease> = github_get_with_token(&url, token)
            .await?
            .json()
            .await
            .map_err(|e| format!("解析 GitHub API 响应失败: {}", e))?;
        newest_release(releases).ok_or("该仓库暂无发布版本")?
    } else {
        let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
        github_get_with_token(&url, token)
            .await?
            .json()
            .await
            .map_err(|e| format!("解析 GitHub API 响应失败: {}", e))?
    };

    pick_asset(&release, target)
}

#[cfg(test)]
pub(crate) fn panel_download_urls(
    mirror: &Option<String>,
    repo: &str,
    asset_url: &str,
    api_url: Option<&str>,
    prefer_api: bool,
) -> Vec<String> {
    let direct = apply_mirror(mirror, asset_url);
    let prefix = format!("{}repos/{}/releases/assets/", GITHUB_API, repo);
    let api = api_url.map(str::trim).filter(|url| {
        url.strip_prefix(&prefix)
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
    });
    match api {
        Some(api) if prefer_api => vec![api.to_string(), direct],
        Some(api) => vec![direct, api.to_string()],
        None => vec![direct],
    }
}

pub(crate) async fn download_asset(
    app: &UpdateContext,
    event: &str,
    url: &str,
    expected_size: u64,
    dest: &Path,
) -> Result<(), String> {
    download_asset_from(app, event, &[url.to_string()], None, expected_size, dest).await
}

pub(crate) async fn download_asset_from(
    app: &UpdateContext,
    event: &str,
    urls: &[String],
    token: Option<&str>,
    expected_size: u64,
    dest: &Path,
) -> Result<(), String> {
    // 下载不设总超时（大文件慢速网络下会中途截断），连接问题由系统层面报错
    let client =
        super::network::build_client(None).map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let mut first_error = None;
    for url in urls {
        match fetch_to_file(app, event, &client, url, token, expected_size, dest).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| "没有可用的下载地址".to_string()))
}

async fn fetch_to_file(
    app: &UpdateContext,
    event: &str,
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
    expected_size: u64,
    dest: &Path,
) -> Result<(), String> {
    let api = url.starts_with(GITHUB_API);
    let mut request = client.get(url).header("User-Agent", "singboard");
    if api {
        request = request.header("Accept", "application/octet-stream");
        if let Some(token) = github_token(token) {
            request = request.bearer_auth(token);
        }
    }
    let mut resp = request
        .send()
        .await
        .map_err(|e| format!("下载失败: {}", e))?;
    if api && resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(INVALID_TOKEN.into());
    }
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }

    let total = resp.content_length().unwrap_or(expected_size);
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("创建临时文件失败: {}", e))?;

    let max_size = if expected_size > 0 {
        expected_size
    } else {
        256 * 1024 * 1024
    };
    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();
    emit_progress(app, event, "download", 0, total);
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("下载中断: {}", e))? {
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > max_size {
            return Err(format!("下载内容超过预期大小上限（{} 字节）", max_size));
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CoreVersion {
    base: (u64, u64, u64),
    stage: (u8, u64),
    fork: String,
}

pub(crate) fn normalize_core_version(text: &str) -> &str {
    let line = text.lines().next().unwrap_or("").trim();
    let stripped = line
        .strip_prefix("sing-box version ")
        .or_else(|| line.strip_prefix("Sing-Box version "))
        .unwrap_or(line)
        .trim();
    stripped.trim_start_matches(['v', 'V'])
}

fn parse_core_version(text: &str) -> Option<CoreVersion> {
    let norm = normalize_core_version(text);
    let (base_part, suffix) = match norm.split_once('-') {
        Some((b, s)) => (b, Some(s)),
        None => (norm, None),
    };
    let mut parts = base_part.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = match parts.next() {
        Some(p) => p.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    let Some(suffix) = suffix else {
        return Some(CoreVersion {
            base: (major, minor, patch),
            stage: (4, 0),
            fork: String::new(),
        });
    };
    let lower = suffix.to_ascii_lowercase();
    let parse_stage = |prefix: &str, rank: u8| -> Option<((u8, u64), String)> {
        let rest = lower.strip_prefix(prefix)?;
        let rest = rest.strip_prefix('.').unwrap_or(rest);
        let digits_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits_len == 0 && !rest.is_empty() && !rest.starts_with('-') {
            return None;
        }
        let num = if digits_len > 0 {
            rest[..digits_len].parse().ok()?
        } else {
            0
        };
        let tail = rest[digits_len..].trim_start_matches(['-', '.']);
        Some(((rank, num), tail.to_string()))
    };
    let (stage, fork) = parse_stage("alpha", 1)
        .or_else(|| parse_stage("beta", 2))
        .or_else(|| parse_stage("rc", 3))
        .unwrap_or(((4, 0), lower));
    Some(CoreVersion {
        base: (major, minor, patch),
        stage,
        fork,
    })
}

pub(crate) fn compare_installed_core(installed: &str, latest: &str) -> Option<Ordering> {
    let latest_norm = normalize_core_version(latest);
    let installed_norm = normalize_core_version(installed);
    if latest_norm.is_empty() || installed_norm.is_empty() {
        return None;
    }
    if latest_norm.eq_ignore_ascii_case(installed_norm) {
        return Some(Ordering::Equal);
    }
    match (parse_core_version(latest_norm), parse_core_version(installed_norm)) {
        (Some(l), Some(i)) if l.fork == i.fork => Some((i.base, i.stage).cmp(&(l.base, l.stage))),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_raw = parts.next().unwrap_or("0");
    let digits: String = patch_raw.chars().take_while(char::is_ascii_digit).collect();
    let patch = digits.parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
pub(crate) fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => latest.trim().trim_start_matches(['v', 'V']) != current.trim(),
    }
}

#[cfg(test)]
fn is_release_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

#[cfg(test)]
pub fn is_release_page_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://github.com/") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        [owner, repo, "releases"] => is_release_segment(owner) && is_release_segment(repo),
        [owner, repo, "releases", "tag", tag] => {
            is_release_segment(owner) && is_release_segment(repo) && is_release_segment(tag)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_page_urls_are_limited_to_github_releases() {
        assert!(is_release_page_url("https://github.com/SagerNet/sing-box/releases/tag/v1.13.0-beta.2"));
        assert!(is_release_page_url("https://github.com/Yuu518/singboard/releases"));
        assert!(!is_release_page_url("http://github.com/SagerNet/sing-box/releases"));
        assert!(!is_release_page_url("https://github.com.evil.test/a/b/releases"));
        assert!(!is_release_page_url("https://github.com/a/b/releases/tag/v1&calc"));
        assert!(!is_release_page_url("https://github.com/a/b/releases/tag/.."));
        assert!(!is_release_page_url("https://github.com/a/b/issues"));
        assert!(!is_release_page_url("https://github.com/a/b/releases/tag/v1/extra"));
        assert!(!is_release_page_url("https://github.com/a/b/releases/tag/v 1"));
    }

    const ASSET: &str = "https://github.com/Yuu518/singboard/releases/download/v1.0.0/singboard.exe";
    const API: &str = "https://api.github.com/repos/Yuu518/singboard/releases/assets/726398165";
    const REPO: &str = "Yuu518/singboard";

    #[test]
    fn panel_downloads_try_the_release_link_before_the_api() {
        assert_eq!(
            panel_download_urls(&None, REPO, ASSET, Some(API), false),
            vec![ASSET, API]
        );
        assert_eq!(
            panel_download_urls(&Some("  ".into()), REPO, ASSET, Some(API), false),
            vec![ASSET, API]
        );
    }

    #[test]
    fn a_github_token_moves_the_api_first() {
        assert_eq!(
            panel_download_urls(&None, REPO, ASSET, Some(API), true),
            vec![API, ASSET]
        );
    }

    #[test]
    fn only_the_release_link_goes_through_the_mirror() {
        let mirror = Some("https://mirror.example.com/".to_string());
        let mirrored = format!("https://mirror.example.com/{ASSET}");
        assert_eq!(
            panel_download_urls(&mirror, REPO, ASSET, Some(API), false),
            vec![mirrored.clone(), API.to_string()]
        );
        assert_eq!(
            panel_download_urls(&mirror, REPO, ASSET, Some(API), true),
            vec![API.to_string(), mirrored]
        );
    }

    #[test]
    fn the_api_fallback_only_accepts_this_repos_asset_endpoint() {
        for bad in [
            "",
            "https://api.github.com/repos/Yuu518/singboard/releases/assets/",
            "https://api.github.com/repos/Yuu518/singboard/releases/assets/1/../x",
            "http://api.github.com/repos/Yuu518/singboard/releases/assets/1",
            "https://api.github.com.evil.test/repos/Yuu518/singboard/releases/assets/1",
            "https://api.github.com/repos/other/repo/releases/assets/1",
        ] {
            assert_eq!(
                panel_download_urls(&None, REPO, ASSET, Some(bad), true),
                vec![ASSET],
                "{bad}"
            );
        }
        assert_eq!(panel_download_urls(&None, REPO, ASSET, None, true), vec![ASSET]);
    }

    #[test]
    fn blank_github_tokens_are_ignored() {
        assert_eq!(github_token(None), None);
        assert_eq!(github_token(Some("   ")), None);
        assert_eq!(github_token(Some(" ghp_abc ")), Some("ghp_abc"));
    }

    fn release(tag: &str, prerelease: bool, draft: bool, published_at: &str) -> GhRelease {
        GhRelease {
            tag_name: tag.to_string(),
            prerelease,
            draft,
            published_at: Some(published_at.to_string()),
            assets: Vec::new(),
        }
    }

    fn release_with(names: &[&str]) -> GhRelease {
        GhRelease {
            tag_name: "v1.0.0".into(),
            prerelease: false,
            draft: false,
            published_at: None,
            assets: names
                .iter()
                .map(|name| GhAsset {
                    name: name.to_string(),
                    browser_download_url: format!("https://example.com/{name}"),
                    size: 1,
                    digest: None,
                })
                .collect(),
        }
    }

    #[test]
    fn testing_channel_prefers_the_highest_version_over_the_latest_published() {
        let releases = vec![
            release("v1.14.3-reF1nd", false, false, "2026-10-10T00:49:24Z"),
            release("v1.15.0-alpha.11-reF1nd", true, false, "2026-10-09T15:01:33Z"),
            release("v1.15.0-alpha.10-reF1nd", true, false, "2026-10-06T00:31:30Z"),
            release("v1.14.2-reF1nd", false, false, "2026-09-25T19:29:39Z"),
        ];
        assert_eq!(
            newest_release(releases).unwrap().tag_name,
            "v1.15.0-alpha.11-reF1nd"
        );
    }

    #[test]
    fn testing_channel_prefers_stable_over_prereleases_of_the_same_version() {
        let releases = vec![
            release("v1.15.0-rc.1", true, false, "2026-11-02T00:00:00Z"),
            release("v1.15.0", false, false, "2026-11-01T00:00:00Z"),
            release("v1.16.0-alpha.1", true, true, "2026-11-03T00:00:00Z"),
        ];
        assert_eq!(newest_release(releases).unwrap().tag_name, "v1.15.0");
    }

    #[test]
    fn unparsable_tags_fall_back_to_the_most_recent_release() {
        let releases = vec![
            release("nightly-b", true, false, "2026-10-02T00:00:00Z"),
            release("nightly-a", true, false, "2026-10-01T00:00:00Z"),
        ];
        assert_eq!(newest_release(releases).unwrap().tag_name, "nightly-b");
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
    fn picks_windows_assets_across_naming_variants_and_formats() {
        let win_amd64 = AssetTarget {
            os: TargetOs::Windows,
            arch: TargetArch::Amd64,
        };
        let win_arm64 = AssetTarget {
            os: TargetOs::Windows,
            arch: TargetArch::Arm64,
        };
        let win_x86 = AssetTarget {
            os: TargetOs::Windows,
            arch: TargetArch::X86,
        };

        let official = release_with(&[
            "sing-box-1.0.0-windows-amd64v3.zip",
            "sing-box-1.0.0-windows-amd64-legacy.zip",
            "sing-box-1.0.0-windows-amd64.zip",
            "sing-box-1.0.0-windows-arm64.zip",
        ]);
        assert_eq!(
            pick_asset(&official, win_amd64).unwrap().asset_name,
            "sing-box-1.0.0-windows-amd64.zip"
        );
        assert_eq!(
            pick_asset(&official, win_arm64).unwrap().asset_name,
            "sing-box-1.0.0-windows-arm64.zip"
        );
        assert_eq!(
            pick_asset(&official, win_x86).err().unwrap(),
            "v1.0.0 未提供适用于 windows-386 的资产"
        );

        let raw_exe_underscore = release_with(&[
            "sing-box_v1.15.0-alpha.13-xireiki.1_windows_amd64.exe",
            "sing-box_v1.15.0-alpha.13-xireiki.1_windows_amd64.exe.sha256",
        ]);
        assert_eq!(
            pick_asset(&raw_exe_underscore, win_amd64).unwrap().asset_name,
            "sing-box_v1.15.0-alpha.13-xireiki.1_windows_amd64.exe"
        );

        let zip_over_raw = release_with(&[
            "sing-box_v1.15.0_windows_amd64.exe",
            "sing-box_v1.15.0_windows_amd64.zip",
        ]);
        assert_eq!(
            pick_asset(&zip_over_raw, win_amd64).unwrap().asset_name,
            "sing-box_v1.15.0_windows_amd64.zip"
        );

        let alias_and_case = release_with(&[
            "Sing-Box_1.15.0_Windows_x86_64.zip",
            "Sing-Box_1.15.0_Windows_aarch64.exe",
        ]);
        assert_eq!(
            pick_asset(&alias_and_case, win_amd64).unwrap().asset_name,
            "Sing-Box_1.15.0_Windows_x86_64.zip"
        );
        assert_eq!(
            pick_asset(&alias_and_case, win_arm64).unwrap().asset_name,
            "Sing-Box_1.15.0_Windows_aarch64.exe"
        );

        let version_v3_not_microarch = release_with(&["sing-box-windows-amd64-v3.0.0.zip"]);
        assert_eq!(
            pick_asset(&version_v3_not_microarch, win_amd64)
                .unwrap()
                .asset_name,
            "sing-box-windows-amd64-v3.0.0.zip"
        );

        let rejected = release_with(&[
            "sing-box-1.0.0-windows-amd64-v3.zip",
            "sing-box-1.0.0-windows-amd64.zip.sha256",
            "sing-box-1.0.0-windows-amd64.msi",
            "sing-box-1.0.0-windows-amd64.tar.gz",
            "sing-box-1.0.0-linux-amd64.zip",
            "geoip-windows-amd64.zip",
        ]);
        assert_eq!(
            pick_asset(&rejected, win_amd64).err().unwrap(),
            "v1.0.0 未提供适用于 windows-amd64 的资产"
        );
    }

    #[test]
    fn picks_android_assets_by_go_abi_underscore_and_raw_binary() {
        let arm64 = AssetTarget {
            os: TargetOs::Android,
            arch: TargetArch::Arm64,
        };
        let arm = AssetTarget {
            os: TargetOs::Android,
            arch: TargetArch::Arm,
        };
        let amd64 = AssetTarget {
            os: TargetOs::Android,
            arch: TargetArch::Amd64,
        };

        let go_style = release_with(&[
            "sing-box-1.0.0-android-arm.tar.gz",
            "sing-box-1.0.0-android-arm64.tar.gz",
            "sing-box-1.0.0-android-amd64v3.tar.gz",
            "sing-box-1.0.0-android-amd64.tar.gz",
        ]);
        let abi_style = release_with(&[
            "sing-box-1.0.0-Yuu-android-armeabi-v7a.tar.gz",
            "sing-box-1.0.0-Yuu-android-arm64-v8a.tar.gz",
        ]);
        let underscore_and_raw = release_with(&[
            "sing-box_v1.15.0_android_arm64_v8a",
            "sing-box_v1.15.0_android_arm64.zip",
            "sing-box_v1.15.0_android_armv7.tgz",
            "sing-box_v1.15.0_android_arm64.sha256",
        ]);

        let pick = |release: &GhRelease, target: AssetTarget| {
            pick_asset(release, target).map(|info| info.asset_name)
        };
        assert_eq!(
            pick(&go_style, arm64).unwrap(),
            "sing-box-1.0.0-android-arm64.tar.gz"
        );
        assert_eq!(
            pick(&go_style, arm).unwrap(),
            "sing-box-1.0.0-android-arm.tar.gz"
        );
        assert_eq!(
            pick(&go_style, amd64).unwrap(),
            "sing-box-1.0.0-android-amd64.tar.gz"
        );
        assert_eq!(
            pick(&abi_style, arm64).unwrap(),
            "sing-box-1.0.0-Yuu-android-arm64-v8a.tar.gz"
        );
        assert_eq!(
            pick(&abi_style, arm).unwrap(),
            "sing-box-1.0.0-Yuu-android-armeabi-v7a.tar.gz"
        );
        assert_eq!(
            pick(&abi_style, amd64).unwrap_err(),
            "v1.0.0 未提供适用于 android-amd64 的资产"
        );
        assert_eq!(
            pick(&underscore_and_raw, arm64).unwrap(),
            "sing-box_v1.15.0_android_arm64_v8a"
        );
        assert_eq!(
            pick(&underscore_and_raw, arm).unwrap(),
            "sing-box_v1.15.0_android_armv7.tgz"
        );
    }

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
        assert!(is_newer("1.10.0", "1.9.9"));
    }

    #[test]
    fn same_or_older_is_not_newer() {
        assert!(!is_newer("3.2.4", "3.2.4"));
        assert!(!is_newer("v3.2.4", "3.2.4"));
        assert!(!is_newer("3.2.3", "3.2.4"));
        assert!(!is_newer("2.9.9", "3.0.0"));
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
    }

    #[test]
    fn unparsable_falls_back_to_string_inequality() {
        assert!(is_newer("nightly", "3.2.4"));
        assert!(!is_newer("nightly", "nightly"));
        assert!(!is_newer("vnightly", "nightly"));
    }

    #[test]
    fn core_version_comparison_handles_prereleases_and_forks() {
        let latest = "v1.15.0-alpha.13-Yuu";
        for (installed, expected) in [
            (
                "sing-box version 1.15.0-alpha.13-Yuu",
                Some(Ordering::Equal),
            ),
            ("1.15.0-alpha.13-Yuu", Some(Ordering::Equal)),
            ("1.15.0-alpha.9-Yuu", Some(Ordering::Less)),
            ("1.15.0-beta.1-Yuu", Some(Ordering::Greater)),
            ("1.15.0-alpha.13", None),
            ("", None),
        ] {
            assert_eq!(compare_installed_core(installed, latest), expected);
        }
        assert_eq!(
            compare_installed_core("1.15.0-rc.2-Yuu", "v1.15.0-Yuu"),
            Some(Ordering::Less)
        );
        assert_eq!(compare_installed_core(latest, "1.15.0-alpha.13"), None);
    }

    #[test]
    fn testing_channel_orders_numeric_prerelease_versions_over_publish_time() {
        let releases = vec![
            release("v1.15.0-alpha.9-Yuu", true, false, "2026-10-25T00:00:00Z"),
            release("v1.15.0-alpha.13-Yuu", true, false, "2026-10-24T00:00:00Z"),
        ];
        assert_eq!(
            newest_release(releases).unwrap().tag_name,
            "v1.15.0-alpha.13-Yuu"
        );
    }
}
