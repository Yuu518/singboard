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

pub fn asset_name() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("android", "arm") => "singboard-native-armeabi-v7a.apk",
        ("android", "aarch64") => "singboard-native-arm64-v8a.apk",
        ("windows", "x86_64") => "singboard-native-windows-x64.exe",
        _ => "",
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    published_at: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
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
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .https_only(true)
        .user_agent("Singboard-Native")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())
}

fn select(release: Release, name: &str, current: &str) -> Result<Check, String> {
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
        digest: String::new(),
        size: 0,
    };
    let Some(asset) = release.assets.into_iter().find(|a| a.name == name) else {
        info.message = format!("v{} 尚未发布 {}", info.version, name);
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
    info.available = latest > installed;
    info.message = if info.available {
        format!("可更新至 v{}", info.version)
    } else {
        format!("当前已是 v{current} 或更新版本")
    };
    Ok(info)
}

pub async fn check(token: &str) -> Result<Check, String> {
    let request = client()?.get(format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    ));
    let request = if token.is_empty() {
        request
    } else {
        request.bearer_auth(token)
    };
    let r = request
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    select(
        r.json().await.map_err(|e| e.to_string())?,
        asset_name(),
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
    let target = dir.join(asset_name());
    if target.is_file() && hash(&target)? == info.digest {
        return Ok((target, info.digest));
    }
    let http = client()?;
    let asset_url = format!(
        "https://github.com/{REPO}/releases/download/{}/{}",
        info.tag,
        asset_name()
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
                let target = dir.path().join(asset_name());
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
                name: "package".into(),
                size: 8,
                digest: Some(format!("sha256:{}", hash(&path).unwrap())),
            }],
        };
        let mut info = select(release, "package", CURRENT).unwrap();
        assert!(!info.available && !info.reinstall);
        info.compare_executable(&path).unwrap();
        assert!(!info.reinstall);
        fs::write(&path, b"modified").unwrap();
        info.compare_executable(&path).unwrap();
        assert!(info.reinstall && !info.available);
    }
    #[test]
    fn requires_native_asset_version_and_verified_bytes() {
        let release = |name: &str| Release {
            tag_name: "v4.10.0".into(),
            published_at: String::new(),
            assets: vec![Asset {
                name: name.into(),
                id: 1,
                size: 128,
                digest: Some(format!("sha256:{}", "a".repeat(64))),
            }],
        };
        let name = "singboard-native-windows-x64.exe";
        assert!(
            !select(release("singboard.exe"), name, "4.9.2")
                .unwrap()
                .available
        );
        assert!(!select(release(name), name, "4.11.0").unwrap().available);
        let update = select(release(name), name, "4.9.2").unwrap();
        assert!(update.available && verified(128, &"a".repeat(64), &update));
        assert!(
            !verified(127, &update.digest, &update) && !verified(128, &"0".repeat(64), &update)
        );
    }
}
