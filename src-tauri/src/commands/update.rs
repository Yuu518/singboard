use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

#[cfg_attr(windows, path = "update/windows.rs")]
#[cfg_attr(target_os = "android", path = "update/android.rs")]
pub mod platform;

/// 防止并发执行更新
pub(crate) static UPDATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) const CORE_PROGRESS_EVENT: &str = "core-update-progress";

/// 一致性校验阶段解压结果的清单，供随后的安装复用（同一资产不下载两次）
pub(crate) const STAGED_MANIFEST: &str = "staged.json";

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
    pub(crate) version: String,
    pub(crate) restarted: bool,
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
pub(crate) struct StagedCore {
    pub(crate) asset_url: String,
    pub(crate) asset_size: u64,
    pub(crate) asset_digest: String,
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

pub(crate) fn core_asset_hash(asset_digest: &str) -> Result<&str, String> {
    asset_digest
        .trim()
        .strip_prefix("sha256:")
        .map(str::trim)
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| "上游未提供有效的 SHA-256 校验信息，已中止核心更新".to_string())
}

pub(crate) fn verify_archive(archive: &Path, asset_digest: &str) -> Result<Vec<u8>, String> {
    let expected = core_asset_hash(asset_digest)?;
    let bytes = std::fs::read(archive).map_err(|e| format!("打开压缩包失败: {}", e))?;
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

fn asset_label(suffix: &str) -> &str {
    let suffix = suffix.trim_start_matches('-');
    suffix
        .strip_suffix(".tar.gz")
        .or_else(|| suffix.strip_suffix(".zip"))
        .unwrap_or(suffix)
}

fn pick_asset(release: &GhRelease, suffixes: &[String]) -> Result<CoreUpdateInfo, String> {
    let asset = suffixes
        .iter()
        .find_map(|suffix| release.assets.iter().find(|a| a.name.ends_with(suffix.as_str())))
        .ok_or_else(|| {
            let labels: Vec<&str> = suffixes.iter().map(|s| asset_label(s)).collect();
            format!(
                "{} 未提供适用于 {} 的资产",
                release.tag_name,
                labels.join(" / ")
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
        (
            parse_version(&r.tag_name),
            !r.prerelease,
            r.published_at.clone().unwrap_or_default(),
        )
    })
}

#[tauri::command]
pub async fn check_core_update(repo: String, channel: String) -> Result<CoreUpdateInfo, String> {
    let repo = repo.trim().to_string();
    validate_repo(&repo)?;
    let suffixes = platform::asset_suffixes()?;

    let release = if channel == "testing" {
        let url = format!("https://api.github.com/repos/{}/releases?per_page=10", repo);
        let releases: Vec<GhRelease> = github_get(&url)
            .await?
            .json()
            .await
            .map_err(|e| format!("解析 GitHub API 响应失败: {}", e))?;
        newest_release(releases).ok_or("该仓库暂无发布版本")?
    } else {
        let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
        github_get(&url)
            .await?
            .json()
            .await
            .map_err(|e| format!("解析 GitHub API 响应失败: {}", e))?
    };

    pick_asset(&release, &suffixes)
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

pub(crate) fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => latest.trim().trim_start_matches(['v', 'V']) != current.trim(),
    }
}

pub(crate) fn digest_hash(asset_digest: &str) -> Option<&str> {
    asset_digest
        .trim()
        .strip_prefix("sha256:")
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    let content = std::fs::read(path).map_err(|e| format!("读取文件失败: {}", e))?;
    Ok(format!("{:x}", Sha256::digest(&content)))
}

pub(crate) fn verify_digest(
    file: &Path,
    asset_digest: &str,
    mirror: &Option<String>,
) -> Result<(), String> {
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

    let actual = sha256_file(file)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("文件校验失败，已中止更新".into())
    }
}

fn is_release_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

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

#[tauri::command]
pub fn open_release_page(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if !is_release_page_url(&url) {
        return Err("只能打开 GitHub 发布页".into());
    }
    #[cfg(target_os = "android")]
    return crate::native::open_url(&app, url);
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        std::process::Command::new("explorer")
            .arg(&url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("无法打开浏览器: {e}"))
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

    fn suffixes(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
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
    fn picks_windows_asset_by_exact_suffix() {
        let release = release_with(&[
            "sing-box-1.0.0-windows-amd64v3.zip",
            "sing-box-1.0.0-windows-amd64.zip",
            "sing-box-1.0.0-windows-arm64.zip",
        ]);
        let picked = pick_asset(&release, &suffixes(&["windows-amd64.zip"])).unwrap();
        assert_eq!(picked.asset_name, "sing-box-1.0.0-windows-amd64.zip");
        let error = pick_asset(&release, &suffixes(&["windows-386.zip"])).err().unwrap();
        assert_eq!(error, "v1.0.0 未提供适用于 windows-386 的资产");
    }

    #[test]
    fn picks_android_asset_by_go_or_abi_arch_name() {
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
        let arm64 = suffixes(&["-android-arm64.tar.gz", "-android-arm64-v8a.tar.gz"]);
        let arm = suffixes(&[
            "-android-arm.tar.gz",
            "-android-armv7.tar.gz",
            "-android-armeabi-v7a.tar.gz",
        ]);
        let amd64 = suffixes(&["-android-amd64.tar.gz", "-android-x86_64.tar.gz"]);

        let pick = |release: &GhRelease, suffixes: &[String]| {
            pick_asset(release, suffixes).map(|info| info.asset_name)
        };
        assert_eq!(pick(&go_style, &arm64).unwrap(), "sing-box-1.0.0-android-arm64.tar.gz");
        assert_eq!(pick(&go_style, &arm).unwrap(), "sing-box-1.0.0-android-arm.tar.gz");
        assert_eq!(pick(&go_style, &amd64).unwrap(), "sing-box-1.0.0-android-amd64.tar.gz");
        assert_eq!(
            pick(&abi_style, &arm64).unwrap(),
            "sing-box-1.0.0-Yuu-android-arm64-v8a.tar.gz"
        );
        assert_eq!(
            pick(&abi_style, &arm).unwrap(),
            "sing-box-1.0.0-Yuu-android-armeabi-v7a.tar.gz"
        );
        assert_eq!(
            pick(&abi_style, &amd64).unwrap_err(),
            "v1.0.0 未提供适用于 android-amd64 / android-x86_64 的资产"
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
}
