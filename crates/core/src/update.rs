use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const REPO: &str = "Yuu518/singboard";
pub const CURRENT: &str = match option_env!("SINGBOARD_RELEASE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub fn staged_filename() -> &'static str {
    match std::env::consts::OS {
        "windows" => "singboard-update.exe",
        "android" => "singboard-update.apk",
        _ => "singboard-update",
    }
}

fn valid_remote_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | '?' | '#' | ' '))
}

fn name_tokens(stem: &str) -> Vec<&str> {
    stem.split(['-', '_', '.'])
        .filter(|s| !s.is_empty())
        .collect()
}

fn pick_asset(assets: Vec<Asset>, os: &str, arch: &str) -> Option<Asset> {
    let candidates: Vec<Asset> = assets
        .into_iter()
        .filter(|a| valid_remote_name(&a.name))
        .collect();
    let find_exact = |names: &[&str]| {
        names.iter().find_map(|expected| {
            candidates
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(expected))
                .cloned()
        })
    };
    match (os, arch) {
        ("windows", "x86_64") => {
            if let Some(a) = find_exact(&[
                "singboard-windows-x64.exe",
                "singboard-windows-amd64.exe",
                "singboard-native-windows-x64.exe",
                "singboard.exe",
            ]) {
                return Some(a);
            }
            let is_win_app = |a: &&Asset| {
                let lower = a.name.to_ascii_lowercase();
                if !lower.starts_with("singboard") || !lower.ends_with(".exe") {
                    return false;
                }
                let stem = &lower[..lower.len() - 4];
                !name_tokens(stem)
                    .iter()
                    .any(|t| matches!(*t, "service" | "updater" | "setup" | "installer"))
            };
            if let Some(a) = candidates.iter().filter(is_win_app).find(|a| {
                let lower = a.name.to_ascii_lowercase();
                let stem = &lower[..lower.len() - 4];
                lower.contains("x86_64")
                    || name_tokens(stem)
                        .iter()
                        .any(|t| matches!(*t, "x64" | "amd64"))
            }) {
                return Some(a.clone());
            }
            candidates
                .iter()
                .filter(is_win_app)
                .find(|a| {
                    let lower = a.name.to_ascii_lowercase();
                    let stem = &lower[..lower.len() - 4];
                    !name_tokens(stem).iter().any(|t| {
                        matches!(
                            *t,
                            "arm64" | "aarch64" | "arm" | "armv7" | "386" | "x86" | "i686" | "ia32"
                        )
                    })
                })
                .cloned()
        }
        ("windows", "aarch64") => {
            if let Some(a) = find_exact(&[
                "singboard-windows-arm64.exe",
                "singboard-native-windows-arm64.exe",
            ]) {
                return Some(a);
            }
            candidates
                .into_iter()
                .find(|a| {
                    let lower = a.name.to_ascii_lowercase();
                    if !lower.starts_with("singboard") || !lower.ends_with(".exe") {
                        return false;
                    }
                    let stem = &lower[..lower.len() - 4];
                    let tokens = name_tokens(stem);
                    !tokens
                        .iter()
                        .any(|t| matches!(*t, "service" | "updater" | "setup" | "installer"))
                        && tokens.iter().any(|t| matches!(*t, "arm64" | "aarch64"))
                })
        }
        ("android", "aarch64") => {
            if let Some(a) = find_exact(&[
                "singboard-arm64-v8a.apk",
                "singboard-native-arm64-v8a.apk",
                "singboard-arm64.apk",
            ]) {
                return Some(a);
            }
            let is_apk = |a: &&Asset| {
                let lower = a.name.to_ascii_lowercase();
                lower.starts_with("singboard") && lower.ends_with(".apk")
            };
            if let Some(a) = candidates.iter().filter(is_apk).find(|a| {
                let lower = a.name.to_ascii_lowercase();
                lower.contains("arm64") || lower.contains("aarch64")
            }) {
                return Some(a.clone());
            }
            candidates
                .iter()
                .filter(is_apk)
                .find(|a| {
                    let lower = a.name.to_ascii_lowercase();
                    lower.contains("universal")
                        || (!lower.contains("armeabi")
                            && !lower.contains("armv7")
                            && !lower.contains("x86")
                            && !lower.contains("amd64"))
                })
                .cloned()
        }
        ("android", "arm") => {
            if let Some(a) = find_exact(&[
                "singboard-armeabi-v7a.apk",
                "singboard-native-armeabi-v7a.apk",
                "singboard-armv7.apk",
            ]) {
                return Some(a);
            }
            let is_apk = |a: &&Asset| {
                let lower = a.name.to_ascii_lowercase();
                lower.starts_with("singboard") && lower.ends_with(".apk")
            };
            if let Some(a) = candidates.iter().filter(is_apk).find(|a| {
                let lower = a.name.to_ascii_lowercase();
                lower.contains("armeabi") || lower.contains("armv7")
            }) {
                return Some(a.clone());
            }
            candidates
                .iter()
                .filter(is_apk)
                .find(|a| {
                    let lower = a.name.to_ascii_lowercase();
                    lower.contains("universal")
                        || (!lower.contains("arm64")
                            && !lower.contains("aarch64")
                            && !lower.contains("x86")
                            && !lower.contains("amd64"))
                })
                .cloned()
        }
        _ => None,
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    published_at: String,
    assets: Vec<Asset>,
}

#[derive(Clone, Deserialize)]
struct Asset {
    id: u64,
    name: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Check {
    pub version: String,
    pub published_at: String,
    pub message: String,
    pub available: bool,
    pub reinstall: bool,
    pub mirror: String,
    digest: String,
    pub size: u64,
    #[serde(skip)]
    tag: String,
    #[serde(skip)]
    asset_id: u64,
    #[serde(skip)]
    asset_name: String,
}

fn client() -> Result<reqwest::Client, String> {
    let builder = reqwest::Client::builder()
        .https_only(true)
        .user_agent("Singboard-Native")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180));
    #[cfg(windows)]
    let builder = match crate::commands::network::get_system_proxy()
        .and_then(|url| reqwest::Proxy::all(&url).ok())
    {
        Some(proxy) => builder.proxy(proxy),
        None => builder.no_proxy(),
    };
    builder.build().map_err(|e| e.to_string())
}

fn describe_request_error(err: reqwest::Error) -> String {
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

fn select(release: Release, os: &str, arch: &str, current: &str) -> Result<Check, String> {
    let version = release.tag_name.trim_start_matches('v');
    let latest = Version::parse(version).map_err(|_| "发行标签不是有效版本号")?;
    let installed = Version::parse(current).map_err(|e| e.to_string())?;
    let mut info = Check {
        version: version.into(),
        published_at: release.published_at,
        message: String::new(),
        available: false,
        reinstall: false,
        mirror: String::new(),
        tag: release.tag_name,
        asset_id: 0,
        asset_name: String::new(),
        digest: String::new(),
        size: 0,
    };
    let Some(asset) = pick_asset(release.assets, os, arch) else {
        info.message = format!("v{} 尚未发布安装包", info.version);
        return Ok(info);
    };
    let digest = asset
        .digest
        .as_deref()
        .and_then(|v| v.strip_prefix("sha256:"))
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("发布包缺少有效的 SHA-256")?;
    if asset.size == 0 || asset.size > 256 * 1024 * 1024 {
        return Err("发布包大小无效".into());
    }
    info.digest = digest.to_ascii_lowercase();
    info.size = asset.size;
    info.asset_id = asset.id;
    info.asset_name = asset.name;
    info.available = latest > installed;
    info.message = if info.available {
        format!("可更新至 v{}", info.version)
    } else {
        "已是最新版本".into()
    };
    Ok(info)
}

pub async fn check(token: &str) -> Result<Check, String> {
    let token = token.trim();
    if !token.is_empty() && (!token.is_ascii() || token.chars().any(char::is_control)) {
        return Err("GitHub Token 包含无效字符，请重新填写".into());
    }
    let http = client()?;
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut attempt = 0u8;
    let r = loop {
        let request = http
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        let request = if token.is_empty() {
            request
        } else {
            request.bearer_auth(token)
        };
        match request.send().await {
            Ok(resp) => break resp,
            Err(e)
                if attempt == 0
                    && !e.is_builder()
                    && (e.is_connect() || e.is_timeout() || e.is_request()) =>
            {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => return Err(describe_request_error(e)),
        }
    };
    let status = r.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("GitHub Token 无效或已过期 (401)".into());
    }
    if status == reqwest::StatusCode::FORBIDDEN {
        let body = r.text().await.unwrap_or_default();
        if body.contains("rate limit") {
            return Err("GitHub API 限流，请稍后再试".into());
        }
        return Err("GitHub API 拒绝访问 (403)".into());
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("仓库不存在或没有发布版本".into());
    }
    if !status.is_success() {
        return Err(format!("GitHub API 返回错误: {status}"));
    }
    select(
        r.json().await.map_err(|e| e.to_string())?,
        std::env::consts::OS,
        std::env::consts::ARCH,
        CURRENT,
    )
}

impl Check {
    pub fn compare_executable(&mut self, path: &Path) -> Result<(), String> {
        if self.version == CURRENT && !self.digest.is_empty() && self.digest != hash(path)? {
            self.reinstall = true;
            self.message = "本地面板和官方发布的文件不同".into();
        }
        Ok(())
    }
}

pub fn hash(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub async fn download(dir: &Path, version: &str, token: &str) -> Result<(PathBuf, String), String> {
    download_with_progress(dir, version, token, "", |_| {}).await
}

pub async fn download_with_progress(
    dir: &Path,
    version: &str,
    token: &str,
    mirror: &str,
    progress: impl Fn(u64) + Sync,
) -> Result<(PathBuf, String), String> {
    let token = token.trim();
    let info = check(token).await?;
    if info.digest.is_empty()
        || (!info.available && info.version != CURRENT)
        || info.version != version
    {
        return Err("版本信息已变化，请重新检查更新".into());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dir = dir.join(&info.digest);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(staged_filename());
    if target.is_file() && hash(&target)? == info.digest {
        return Ok((target, info.digest));
    }
    let http = client()?;
    let asset_url = format!(
        "https://github.com/{REPO}/releases/download/{}/{}",
        info.tag, info.asset_name
    );
    let direct = http.get(&asset_url);
    let api = http
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/assets/{}",
            info.asset_id
        ))
        .header(reqwest::header::ACCEPT, "application/octet-stream");
    let mut requests = Vec::new();
    let mirror = mirror.trim().trim_end_matches('/');
    if !mirror.is_empty() {
        requests.push(http.get(format!("{mirror}/{asset_url}")));
    }
    if token.is_empty() {
        requests.extend([direct, api]);
    } else {
        requests.extend([api.bearer_auth(token), direct]);
    }
    let mut errors = Vec::new();
    for request in requests {
        progress(0);
        match fetch_verified(request, &dir, &info, &progress).await {
            Ok(file) => {
                file.persist(&target).map_err(|e| e.to_string())?;
                return Ok((target, info.digest));
            }
            Err(error) => errors.push(error),
        }
    }
    Err(errors.join("\n"))
}

async fn fetch_verified(
    request: reqwest::RequestBuilder,
    dir: &Path,
    info: &Check,
    progress: &impl Fn(u64),
) -> Result<tempfile::NamedTempFile, String> {
    let mut response = request
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut size = 0;
    while let Some(bytes) = response.chunk().await.map_err(|e| e.to_string())? {
        size += bytes.len() as u64;
        if size > info.size {
            return Err("下载内容超出发布包大小".into());
        }
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        digest.update(bytes);
        progress(size);
    }
    if !verified(size, &format!("{:x}", digest.finalize()), info) {
        return Err("更新文件长度或 SHA-256 校验失败".into());
    }
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    Ok(file)
}

fn verified(size: u64, digest: &str, info: &Check) -> bool {
    size == info.size && digest == info.digest
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn split_download(
        expected: &[u8],
    ) -> (
        tempfile::TempDir,
        tokio::task::JoinHandle<Result<tempfile::NamedTempFile, String>>,
        tokio::sync::mpsc::UnboundedReceiver<u64>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (resume, continued) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc")
                .await
                .unwrap();
            if continued.await.is_ok() {
                let _ = socket.write_all(b"def").await;
            }
        });
        let info = Check {
            version: CURRENT.into(),
            published_at: String::new(),
            available: true,
            reinstall: false,
            message: String::new(),
            mirror: String::new(),
            digest: format!("{:x}", Sha256::digest(expected)),
            size: 6,
            tag: String::new(),
            asset_id: 0,
            asset_name: String::new(),
        };
        let dir = tempfile::tempdir().unwrap();
        let stage = dir.path().to_owned();
        let request = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/asset"));
        let (progress, received) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            fetch_verified(request, &stage, &info, &|bytes| {
                let _ = progress.send(bytes);
            })
            .await
        });
        (dir, task, received, resume)
    }

    #[tokio::test]
    async fn staged_download_requires_a_complete_verified_stream() {
        for (expected, valid) in [(b"abcdef", true), (b"abcdeg", false)] {
            let (dir, task, mut progress, resume) = split_download(expected).await;
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), progress.recv())
                    .await
                    .unwrap(),
                Some(3)
            );
            resume.send(()).unwrap();
            let result = task.await.unwrap();
            assert_eq!(progress.recv().await, Some(6));
            if valid {
                let target = dir.path().join(staged_filename());
                result.unwrap().persist(&target).unwrap();
                assert_eq!(fs::read(target).unwrap(), expected);
            } else {
                assert!(result.unwrap_err().contains("SHA-256"));
                assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
            }
        }
    }

    #[tokio::test]
    async fn cancelling_download_removes_the_partial_file() {
        let (dir, task, mut progress, resume) = split_download(b"abcdef").await;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), progress.recv())
                .await
                .unwrap(),
            Some(3)
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        drop(resume);
    }

    #[test]
    fn same_version_reinstall_requires_a_manual_digest_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installed");
        fs::write(&path, b"official").unwrap();
        let release = Release {
            tag_name: format!("v{CURRENT}"),
            published_at: String::new(),
            assets: vec![Asset {
                id: 1,
                name: "singboard-windows-x64.exe".into(),
                size: 8,
                digest: Some(format!("sha256:{}", hash(&path).unwrap())),
            }],
        };
        let mut info = select(release, "windows", "x86_64", CURRENT).unwrap();
        assert!(!info.available && !info.reinstall);
        info.compare_executable(&path).unwrap();
        assert!(!info.reinstall);
        fs::write(&path, b"modified").unwrap();
        info.compare_executable(&path).unwrap();
        assert!(info.reinstall && !info.available);
    }
    #[test]
    fn selects_platform_assets_flexibly_and_rejects_mismatches() {
        let make = |names: &[&str]| Release {
            tag_name: "v5.1.0".into(),
            published_at: String::new(),
            assets: names
                .iter()
                .enumerate()
                .map(|(idx, name)| Asset {
                    name: (*name).into(),
                    id: idx as u64 + 1,
                    size: 128,
                    digest: Some(format!("sha256:{}", "a".repeat(64))),
                })
                .collect(),
        };

        let win_missing = select(
            make(&["singboard-service.exe", "singboard-windows-arm64.exe"]),
            "windows",
            "x86_64",
            "5.0.0",
        )
        .unwrap();
        assert!(!win_missing.available && win_missing.message == "v5.1.0 尚未发布安装包");

        let win_renamed = select(
            make(&["singboard-service.exe", "singboard-5.1.0-win-amd64.exe"]),
            "windows",
            "x86_64",
            "5.0.0",
        )
        .unwrap();
        assert!(win_renamed.available && win_renamed.asset_name == "singboard-5.1.0-win-amd64.exe");

        let win_plain = select(make(&["singboard.exe"]), "windows", "x86_64", "5.0.0").unwrap();
        assert!(win_plain.available && win_plain.asset_name == "singboard.exe");

        let android_arm64 = select(
            make(&["singboard-armeabi-v7a.apk", "singboard-5.1.0-arm64-v8a.apk"]),
            "android",
            "aarch64",
            "5.0.0",
        )
        .unwrap();
        assert!(
            android_arm64.available && android_arm64.asset_name == "singboard-5.1.0-arm64-v8a.apk"
        );

        let android_universal = select(
            make(&["singboard-universal.apk"]),
            "android",
            "arm",
            "5.0.0",
        )
        .unwrap();
        assert!(
            android_universal.available
                && android_universal.asset_name == "singboard-universal.apk"
        );

        let latest = select(
            make(&["singboard-windows-x64.exe"]),
            "windows",
            "x86_64",
            "5.2.0",
        )
        .unwrap();
        assert!(!latest.available && latest.message == "已是最新版本");

        let update = select(
            make(&["singboard-windows-x64.exe"]),
            "windows",
            "x86_64",
            "5.0.0",
        )
        .unwrap();
        assert!(
            update.available
                && update.message == "可更新至 v5.1.0"
                && verified(128, &"a".repeat(64), &update)
        );
        assert!(
            !verified(127, &update.digest, &update) && !verified(128, &"0".repeat(64), &update)
        );
    }
}
