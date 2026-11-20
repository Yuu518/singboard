use crate::{
    clash::Clash,
    profiles::{Profile, Store, write_atomic},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::Path};

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub url: String,
    pub secret: String,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConfigProfile {
    pub id: String,
    pub name: String,
    pub source: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub auto_update_interval: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_updated: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_fetch_error: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub backup_updated_at: String,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub ipv6_test_enabled: bool,
    pub dns_history: Vec<String>,
    pub dns_geo_enabled: bool,
    pub page_swipe: bool,
    pub connection_sort: String,
    pub proxy_sort_latency: bool,
    pub expanded_proxy_groups: Vec<String>,
    pub native_root_enabled: bool,
    pub github_token: String,
    pub setup_completed: bool,
    pub core_update_source: String,
    pub core_update_custom_repo: String,
    pub core_update_channel: String,
    pub core_update_mirror: String,
    pub clash_apis: Vec<Endpoint>,
    pub active_clash_api_id: String,
    pub theme: Option<String>,
    pub start_page: String,
    pub glass_tint: Option<f32>,
    pub close_to_tray: Option<bool>,
    pub latency_test_url: String,
    pub group_test_urls: HashMap<String, String>,
    pub startup_delay_seconds: Option<u32>,
    pub singbox_path: String,
    pub working_dir: String,
    pub config_path: String,
    pub config_profiles: Vec<ConfigProfile>,
    pub active_config_profile_id: String,
    pub panel_auto_check_update: Option<bool>,
}

static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
impl Settings {
    pub fn revision() -> u64 {
        REVISION.load(std::sync::atomic::Ordering::Acquire)
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 16 * 1024 * 1024 {
            return Err("设置备份超过 16 MiB".into());
        }
        let raw: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|_| "设置文件不是有效的 JSON")?;
        let raw = if raw.get("format").is_some() {
            if raw["format"] != "singboard-settings" || raw["version"] != 1 {
                return Err("设置备份格式或版本不受支持".into());
            }
            &raw["config"]
        } else {
            &raw
        };
        let supported = [
            "clashApis",
            "clashApiUrl",
            "theme",
            "startPage",
            "glassTint",
            "glassMode",
            "startupDelaySeconds",
            "configProfiles",
            "configPath",
            "panelAutoCheckUpdate",
            "latencyTestUrl",
            "closeToTray",
            "workingDir",
            "singboxPath",
            "groupTestUrls",
        ];
        if !supported.iter().any(|k| raw.get(k).is_some()) {
            return Err("文件中没有 Singboard 旧版设置".into());
        }
        let mut data: Self = serde_json::from_value(raw.clone())
            .map_err(|e| format!("旧版设置字段格式错误：{e}"))?;
        if data.clash_apis.is_empty()
            && let Some(url) = raw["clashApiUrl"].as_str()
        {
            data.clash_apis.push(Endpoint {
                id: "default".into(),
                name: "默认 API".into(),
                url: url.into(),
                secret: raw["clashApiSecret"].as_str().unwrap_or("").into(),
            });
        }
        data.native_root_enabled = false;
        data.github_token.clear();
        let mut ids = std::collections::HashSet::new();
        for api in &mut data.clash_apis {
            if api.id.is_empty() || !ids.insert(api.id.clone()) {
                api.id = uuid::Uuid::new_v4().to_string();
                ids.insert(api.id.clone());
            }
            if api.name.trim().is_empty() {
                api.name = "API".into();
            }
            if api.url.trim().is_empty() {
                api.url = "http://127.0.0.1:9090".into();
            }
            Clash::new(&api.url, &api.secret).map_err(|_| "旧版设置包含无效的 Clash API 地址")?;
        }
        if let Some(api) = data.active() {
            data.active_clash_api_id = api.id.clone();
        }
        let active = match raw["activeConfigProfileId"].as_str() {
            Some("") => None,
            Some(id) => data
                .config_profiles
                .iter()
                .find(|p| p.id == id)
                .or(data.config_profiles.first()),
            None => data.config_profiles.first(),
        };
        if let Some(p) = active {
            data.active_config_profile_id = p.id.clone();
            if p.kind == "local" && data.config_path.is_empty() {
                data.config_path = p.source.clone();
            }
        }
        if data
            .theme
            .as_deref()
            .is_some_and(|s| !matches!(s, "auto" | "light" | "dark"))
        {
            return Err("主题设置无效".into());
        }
        if data.glass_tint.is_none() {
            data.glass_tint = raw["glassMode"]
                .as_str()
                .map(|m| if m == "clear" { 0. } else { 1. });
        }
        data.glass_tint = data.glass_tint.map(|n| n.clamp(0., 1.));
        data.startup_delay_seconds = data.startup_delay_seconds.map(|n| n.min(3600));
        for p in data.config_profiles.iter().filter(|p| p.kind == "remote") {
            Profile::new(&p.name, &p.source, p.auto_update_interval)?;
        }
        Ok(data)
    }

    pub fn profile_name(&self) -> &str {
        self.config_profiles
            .iter()
            .find(|p| p.id == self.active_config_profile_id)
            .map(|p| p.name.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("—")
    }
    pub fn active(&self) -> Option<&Endpoint> {
        self.clash_apis
            .iter()
            .find(|p| p.id == self.active_clash_api_id)
            .or(self.clash_apis.first())
    }
    pub fn preview(&self) -> String {
        let remote = self
            .config_profiles
            .iter()
            .filter(|p| p.kind == "remote")
            .count();
        format!(
            "{} 个 API 后端，{} 个远程订阅。导入会保存后端密钥并替换原生偏好，已有订阅按地址保留。",
            self.clash_apis.len(),
            remote
        )
    }
    pub fn load(dir: &Path) -> Result<Self, String> {
        match fs::read(dir.join("native-settings.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn import(&self, dir: &Path, store: &Store) -> Result<Value, String> {
        let _lock = Self::lock(dir)?;
        let mut added = 0;
        for p in self.config_profiles.iter().filter(|p| p.kind == "remote") {
            if store.register(&p.name, &p.source, p.auto_update_interval)? {
                added += 1
            }
        }
        let mut imported = self.clone();
        imported.github_token = Self::load(dir)?.github_token;
        imported.save(dir)?;
        Ok(json!({"added":added}))
    }
    fn lock(dir: &Path) -> Result<fs::File, String> {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("native-settings.lock"))
            .map_err(|e| e.to_string())?;
        file.lock().map_err(|e| e.to_string())?;
        Ok(file)
    }
    fn save(&self, dir: &Path) -> Result<(), String> {
        write_atomic(
            &dir.join("native-settings.json"),
            &serde_json::to_vec(self).map_err(|e| e.to_string())?,
        )?;
        REVISION.fetch_add(1, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    pub fn edit(
        dir: &Path,
        change: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        let _lock = Self::lock(dir)?;
        let mut data = Self::load(dir)?;
        change(&mut data)?;
        data.save(dir)
    }
    pub fn select_endpoint(dir: &Path, id: &str) -> Result<(), String> {
        Self::edit(dir, |s| {
            if !s.clash_apis.iter().any(|api| api.id == id) {
                return Err("API 后端不存在，请重新打开设置".into());
            }
            s.active_clash_api_id = id.into();
            Ok(())
        })
    }

    pub fn dns_history(dir: &Path, domain: &str, add: bool) -> Result<Vec<String>, String> {
        let mut history = Vec::new();
        Self::edit(dir, |s| {
            s.dns_history.retain(|item| item != domain);
            if add {
                s.dns_history.insert(0, domain.to_owned());
            }
            s.dns_history.truncate(3);
            history = s.dns_history.clone();
            Ok(())
        })?;
        Ok(history)
    }
    pub fn set_glass_tint(dir: &Path, tint: f32) -> Result<(), String> {
        Self::edit(dir, |s| {
            s.glass_tint = Some(tint.clamp(0., 1.));
            Ok(())
        })
    }
}

pub fn wallpaper_path(dir: &Path) -> std::path::PathBuf {
    dir.join("wallpaper.png")
}

#[cfg(windows)]
mod auto_launch {
    use std::env;
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    const AUTO_LAUNCH_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const AUTO_LAUNCH_NAME: &str = "singboard";
    const AUTO_LAUNCH_APPROVED_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    fn command() -> Result<String, String> {
        let exe = env::current_exe().map_err(|e| format!("获取程序路径失败: {e}"))?;
        Ok(format!("\"{}\" --hidden", exe.display()))
    }

    fn registered() -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(AUTO_LAUNCH_KEY)
            .and_then(|key| key.get_value::<String, _>(AUTO_LAUNCH_NAME))
            .ok()
    }

    fn disabled_by_system() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(AUTO_LAUNCH_APPROVED_KEY)
            .and_then(|key| key.get_raw_value(AUTO_LAUNCH_NAME))
            .is_ok_and(|value| value.bytes.first().is_some_and(|flag| flag & 1 == 1))
    }

    fn clear_approval() {
        if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(AUTO_LAUNCH_APPROVED_KEY)
        {
            let _ = key.delete_value(AUTO_LAUNCH_NAME);
        }
    }

    fn write_command(cmd: &str) -> Result<(), String> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(AUTO_LAUNCH_KEY)
            .map_err(|e| format!("打开注册表失败: {e}"))?;
        key.set_value(AUTO_LAUNCH_NAME, &cmd)
            .map_err(|e| format!("写入注册表失败: {e}"))
    }

    pub fn get() -> bool {
        let (Some(reg), Ok(expected)) = (registered(), command()) else {
            return false;
        };
        reg.eq_ignore_ascii_case(&expected) && !disabled_by_system()
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        if enabled {
            write_command(&command()?)?;
        } else {
            let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey(AUTO_LAUNCH_KEY)
                .map_err(|e| format!("打开注册表失败: {e}"))?;
            if let Err(e) = key.delete_value(AUTO_LAUNCH_NAME)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                return Err(format!("删除注册表值失败: {e}"));
            }
        }
        clear_approval();
        Ok(())
    }

    pub fn repair() {
        if cfg!(debug_assertions) {
            return;
        }
        let (Some(reg), Ok(expected)) = (registered(), command()) else {
            return;
        };
        if !reg.eq_ignore_ascii_case(&expected) {
            let _ = write_command(&expected);
        }
    }
}

#[cfg(windows)]
pub fn get_auto_launch() -> bool {
    auto_launch::get()
}

#[cfg(not(windows))]
pub fn get_auto_launch() -> bool {
    false
}

#[cfg(windows)]
pub fn set_auto_launch(enabled: bool) -> Result<(), String> {
    auto_launch::set(enabled)
}

#[cfg(not(windows))]
pub fn set_auto_launch(_enabled: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub fn repair_auto_launch() {
    auto_launch::repair();
}

#[cfg(not(windows))]
pub fn repair_auto_launch() {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_both_generations_and_rejects_core_configs() {
        let s=Settings::parse(r#"{"clashApiUrl":"http://localhost:8080","clashApiSecret":"test","glassMode":"clear"}"#).unwrap();
        assert_eq!(s.active().unwrap().secret, "test");
        assert_eq!(s.glass_tint, Some(0.));
        assert!(Settings::parse(r#"{"outbounds":[]}"#).is_err());
        assert!(
            Settings::parse(r#"{"format":"singboard-settings","version":2,"config":{}}"#).is_err()
        );
    }
    #[test]
    fn importing_twice_preserves_subscriptions_and_selected_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("subscriptions"));
        let data=Settings::parse(r#"{"format":"singboard-settings","version":1,"config":{"activeClashApiId":"b","clashApis":[{"id":"a","url":"http://localhost:1"},{"id":"b","url":"https://example.test"}],"configProfiles":[{"type":"remote","name":"test","source":"https://example.test/profile"}]}}"#).unwrap();
        assert_eq!(data.import(dir.path(), &store).unwrap()["added"], 1);
        assert_eq!(data.import(dir.path(), &store).unwrap()["added"], 0);
        assert_eq!(
            Settings::load(dir.path()).unwrap().active().unwrap().id,
            "b"
        );
        assert!(!store.path(&store.list().unwrap()[0].id).unwrap().exists());
        Settings::select_endpoint(dir.path(), "a").unwrap();
        assert!(Settings::select_endpoint(dir.path(), "missing").is_err());
        assert_eq!(
            Settings::load(dir.path()).unwrap().active().unwrap().id,
            "a"
        );
    }
    #[test]
    fn honors_the_selected_local_configuration() {
        let data=Settings::parse(r#"{"configProfiles":[{"id":"a","type":"local","source":"a.json"},{"id":"b","type":"local","source":"b.json"}],"activeConfigProfileId":"b"}"#).unwrap();
        assert_eq!(data.config_path, "b.json");
        let empty=Settings::parse(r#"{"configProfiles":[{"id":"a","type":"local","source":"a.json"}],"activeConfigProfileId":""}"#).unwrap();
        assert!(empty.config_path.is_empty());
    }
}
