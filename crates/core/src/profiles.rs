use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;

pub fn validate_jsonc(text: &str) -> Result<(), String> {
    parse_config(text).map(|_| ())
}

pub(crate) fn parse_config(text: &str) -> Result<serde_json::Value, String> {
    let options = jsonc_parser::ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
        allow_bare_decimal_point_numbers: false,
        allow_non_finite_numbers: false,
        allow_extended_string_escapes: false,
    };
    let value: serde_json::Value =
        jsonc_parser::parse_to_serde_value(text.trim_start_matches('\u{feff}'), &options)
            .map_err(|e| format!("JSONC 语法错误：{e}"))?;
    if !value.is_object() {
        return Err("配置的最外层必须是 JSON 对象".into());
    }
    Ok(value)
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub url: String,
    pub hours: u32,
    pub updated: u64,
}

#[derive(Clone)]
pub struct Store(PathBuf);

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("无效路径")?).map_err(err)?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(err)?;
    file.persist(path).map_err(err)?;
    Ok(())
}

impl Profile {
    pub fn due(&self) -> bool {
        self.hours > 0 && now().saturating_sub(self.updated) >= u64::from(self.hours) * 3600
    }

    pub(crate) fn new(name: &str, url: &str, hours: u32) -> Result<Self, String> {
        let name = name.trim();
        let address = Url::parse(url.trim()).map_err(|_| "请输入有效的 HTTP(S) 订阅地址")?;
        if name.is_empty()
            || !matches!(address.scheme(), "http" | "https")
            || address.host().is_none()
            || hours > 576
        {
            return Err("请填写名称、HTTP(S) 地址和 0–576 小时的更新间隔".into());
        }
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            url: address.to_string(),
            hours,
            updated: 0,
        })
    }
}

fn is_local_or_private_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => {
            let lower = domain.to_ascii_lowercase();
            lower == "localhost" || lower.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(ip)) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
        }
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
        None => true,
    }
}

pub async fn download(url: &str) -> Result<String, String> {
    let initial = Url::parse(url).map_err(|_| "请输入有效的 HTTP(S) 订阅地址")?;
    let initial_local = is_local_or_private_host(&initial);
    let mut builder = reqwest::Client::builder()
        .user_agent("sing-box")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("重定向次数过多");
            }
            let next = attempt.url();
            if !matches!(next.scheme(), "http" | "https") {
                return attempt.error("不支持的重定向协议");
            }
            if !initial_local && is_local_or_private_host(next) {
                return attempt.error("拒绝重定向到本机或内网地址");
            }
            attempt.follow()
        }));
    builder = match crate::commands::network::get_system_proxy()
        .and_then(|proxy| reqwest::Proxy::all(&proxy).ok())
    {
        Some(proxy) => builder.proxy(proxy),
        None => builder.no_proxy(),
    };
    let client = builder.build().map_err(err)?;
    let mut response = client
        .get(initial)
        .send()
        .await
        .map_err(|e| err(e.without_url()))?
        .error_for_status()
        .map_err(|e| err(e.without_url()))?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| err(e.without_url()))? {
        if body.len() + chunk.len() > 16 * 1024 * 1024 {
            return Err("订阅超过 16 MiB".into());
        }
        body.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(body).map_err(|_| "配置需要 UTF-8 编码")?;
    validate_jsonc(&text)?;
    Ok(text)
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self(dir)
    }

    fn lock(&self) -> Result<fs::File, String> {
        fs::create_dir_all(&self.0).map_err(err)?;
        let guard = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.0.join("profiles.lock"))
            .map_err(err)?;
        guard.lock().map_err(err)?;
        Ok(guard)
    }

    fn directory(&self, id: &str) -> Result<PathBuf, String> {
        let id = uuid::Uuid::parse_str(id).map_err(|_| "订阅标识无效")?;
        Ok(self.0.join(id.to_string()))
    }

    pub fn path(&self, id: &str) -> Result<PathBuf, String> {
        Ok(self.directory(id)?.join("config.json"))
    }

    fn entries(&self) -> Result<Vec<Profile>, String> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.0).map_err(err)? {
            let entry = entry.map_err(err)?;
            let name = entry.file_name();
            if uuid::Uuid::parse_str(&name.to_string_lossy()).is_err() {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path().join("profile.json")) else {
                continue;
            };
            let Ok(profile) = serde_json::from_slice::<Profile>(&bytes) else {
                continue;
            };
            if profile.id != name.to_string_lossy() {
                continue;
            }
            out.push(profile);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    pub fn list(&self) -> Result<Vec<Profile>, String> {
        let _guard = self.lock()?;
        self.entries()
    }

    pub fn get(&self, id: &str) -> Result<(Profile, String), String> {
        let _guard = self.lock()?;
        let path = self.directory(id)?;
        let profile = serde_json::from_slice(&fs::read(path.join("profile.json")).map_err(err)?)
            .map_err(err)?;
        Ok((
            profile,
            fs::read_to_string(path.join("config.json")).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "该订阅尚未下载，请先更新".into()
                } else {
                    err(e)
                }
            })?,
        ))
    }

    pub async fn add<F: Future<Output = Result<(), String>> + Send>(
        &self,
        name: &str,
        url: &str,
        hours: u32,
        check: impl FnOnce(String) -> F + Send,
    ) -> Result<Profile, String> {
        let mut profile = Profile::new(name, url, hours)?;
        let text = download(&profile.url).await?;
        check(text.clone()).await?;
        let _guard = self.lock()?;
        if self.entries()?.iter().any(|p| p.url == profile.url) {
            return Err("已存在相同地址的订阅".into());
        }
        profile.updated = now();
        self.insert(&profile, Some(&text))?;
        Ok(profile)
    }

    pub fn register(&self, name: &str, url: &str, hours: u32) -> Result<bool, String> {
        let profile = Profile::new(name, url, hours)?;
        let _guard = self.lock()?;
        if self.entries()?.iter().any(|p| p.url == profile.url) {
            return Ok(false);
        }
        self.insert(&profile, None)?;
        Ok(true)
    }

    fn insert(&self, profile: &Profile, text: Option<&str>) -> Result<(), String> {
        let temp = tempfile::tempdir_in(&self.0).map_err(err)?;
        if let Some(text) = text {
            fs::write(temp.path().join("config.json"), text).map_err(err)?;
        }
        fs::write(
            temp.path().join("profile.json"),
            serde_json::to_vec(&profile).map_err(err)?,
        )
        .map_err(err)?;
        fs::rename(temp.path(), self.directory(&profile.id)?).map_err(err)?;
        Ok(())
    }

    pub async fn update<F: Future<Output = Result<(), String>> + Send>(
        &self,
        id: &str,
        settings: Option<(&str, &str, u32)>,
        check: impl FnOnce(String) -> F + Send,
    ) -> Result<bool, String> {
        let (old, original) = {
            let _guard = self.lock()?;
            let old = self
                .entries()?
                .into_iter()
                .find(|p| p.id == id)
                .ok_or("订阅不存在")?;
            let original = match fs::read_to_string(self.path(id)?) {
                Ok(text) => Some(text),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(err(e)),
            };
            (old, original)
        };
        let mut next = match settings {
            Some((name, url, hours)) => Profile {
                id: old.id.clone(),
                ..Profile::new(name, url, hours)?
            },
            None => old.clone(),
        };
        let incoming = if settings.is_some() && next.url == old.url {
            None
        } else {
            let text = download(&next.url).await?;
            check(text.clone()).await?;
            Some(text)
        };
        let _guard = self.lock()?;
        let entries = self.entries()?;
        if !entries.iter().any(|p| p == &old)
            || entries.iter().any(|p| p.id != id && p.url == next.url)
        {
            return Err("订阅已变更、已删除或地址重复，请刷新后重试".into());
        }
        let changed = incoming
            .as_ref()
            .is_some_and(|text| Some(text) != original.as_ref());
        next.updated = if let Some(text) = incoming {
            let path = self.path(id)?;
            if let Some(original) = original {
                replace_document(&path, &original, &text)?;
            } else {
                if path.exists() {
                    return Err("订阅已变更，请刷新后重试".into());
                }
                write_atomic(&path, text.as_bytes())?;
            }
            now()
        } else {
            old.updated
        };
        write_atomic(
            &self.directory(id)?.join("profile.json"),
            &serde_json::to_vec(&next).map_err(err)?,
        )?;
        Ok(changed)
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let _guard = self.lock()?;
        let dir = self.directory(id)?;
        let trash = tempfile::tempdir_in(&self.0).map_err(err)?;
        fs::rename(dir, trash.path().join("removed")).map_err(err)?;
        trash.close().map_err(err)
    }
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".bak");
    name.into()
}

#[cfg(windows)]
pub fn replace_document(path: &Path, expected: &str, text: &str) -> Result<(), String> {
    validate_jsonc(text)?;
    crate::config_files::replace_checked(path, text.as_bytes(), Some(expected.as_bytes()))
        .map(|_| ())
}

#[cfg(not(windows))]
pub fn replace_document(path: &Path, expected: &str, text: &str) -> Result<(), String> {
    validate_jsonc(text)?;
    let saved = fs::read_to_string(path).map_err(err)?;
    if saved != expected {
        return Err("磁盘文件已更改，请重新打开后再保存".into());
    }
    if saved == text {
        return Ok(());
    }
    write_atomic(&backup_path(path), saved.as_bytes())?;
    write_atomic(path, text.as_bytes())
}
pub fn save_profile(store: &Store, id: &str, expected: &str, text: &str) -> Result<(), String> {
    let _guard = store.lock()?;
    replace_document(&store.path(id)?, expected, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    fn server(payloads: Vec<&'static str>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for body in payloads {
                let (mut conn, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let n = conn.read(&mut request).unwrap();
                assert!(
                    String::from_utf8_lossy(&request[..n])
                        .to_lowercase()
                        .contains("user-agent: sing-box")
                );
                write!(
                    conn,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        format!("http://{address}/profile")
    }
    #[test]
    fn imported_subscription_fetches_its_first_copy() {
        crate::runtime().block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::new(dir.path().into());
            let url = server(vec!["{}"]);
            assert!(store.register("导入", &url, 0).unwrap());
            let id = store.list().unwrap().remove(0).id;
            assert!(store.get(&id).is_err());
            assert!(store.update(&id, None, |_| async { Ok(()) }).await.unwrap());
            assert!(!store.register("重复", &url, 0).unwrap());
            assert_eq!(store.get(&id).unwrap().1, "{}");
        });
    }
    #[test]
    fn last_good_config_survives_failed_refresh_and_stale_edits() {
        crate::runtime().block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::new(dir.path().into());
            let url = server(vec!["{/*one*/}", "bad", "{\"log\":{}}"]);
            let profile = store
                .add("测试", &url, 24, |_| async { Ok(()) })
                .await
                .unwrap();
            assert!(
                !store
                    .update(&profile.id, Some(("手动", &url, 0)), |_| async {
                        Err("未更改地址".into())
                    })
                    .await
                    .unwrap()
            );
            assert_eq!(store.get(&profile.id).unwrap().0.hours, 0);
            assert!(
                store
                    .update(&profile.id, None, |_| async { Ok(()) })
                    .await
                    .is_err()
            );
            assert_eq!(store.get(&profile.id).unwrap().1, "{/*one*/}");
            assert!(
                store
                    .update(&profile.id, None, |_| async { Ok(()) })
                    .await
                    .unwrap()
            );
            assert_eq!(
                fs::read_to_string(backup_path(&store.path(&profile.id).unwrap())).unwrap(),
                "{/*one*/}"
            );
            assert!(save_profile(&store, &profile.id, "{/*one*/}", "{}").is_err());
            assert!(store.path("../x").is_err());
            store.remove(&profile.id).unwrap();
            assert!(store.list().unwrap().is_empty());
        });
    }
}
