use crate::profiles::write_atomic;
use serde::{Deserialize, Serialize};
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
    pub font_scale: Option<f32>,
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
    pub fn load(dir: &Path) -> Result<Self, String> {
        match fs::read(dir.join("native-settings.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
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
    pub fn set_font_scale(dir: &Path, scale: f32) -> Result<(), String> {
        Self::edit(dir, |s| {
            s.font_scale = Some(scale.clamp(0.9, 1.4));
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

    pub(super) fn is_disabled_approval(bytes: &[u8]) -> bool {
        bytes
            .first()
            .is_some_and(|&flag| flag != 0x02 && flag != 0x00)
    }

    fn disabled_by_system() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(AUTO_LAUNCH_APPROVED_KEY)
            .and_then(|key| key.get_raw_value(AUTO_LAUNCH_NAME))
            .is_ok_and(|value| is_disabled_approval(&value.bytes))
    }

    fn write_approval(enabled: bool) {
        if let Ok((key, _)) =
            RegKey::predef(HKEY_CURRENT_USER).create_subkey(AUTO_LAUNCH_APPROVED_KEY)
        {
            if enabled {
                let value = winreg::RegValue {
                    bytes: vec![0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    vtype: winreg::enums::RegType::REG_BINARY,
                };
                if key.set_raw_value(AUTO_LAUNCH_NAME, &value).is_err() {
                    let _ = key.delete_value(AUTO_LAUNCH_NAME);
                }
            } else {
                let _ = key.delete_value(AUTO_LAUNCH_NAME);
            }
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
        write_approval(enabled);
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
    fn selects_existing_endpoint_and_rejects_missing() {
        let dir = tempfile::tempdir().unwrap();
        Settings::edit(dir.path(), |s| {
            s.clash_apis = vec![
                Endpoint {
                    id: "a".into(),
                    name: "A".into(),
                    url: "http://localhost:1".into(),
                    secret: String::new(),
                },
                Endpoint {
                    id: "b".into(),
                    name: "B".into(),
                    url: "https://example.test".into(),
                    secret: String::new(),
                },
            ];
            s.active_clash_api_id = "b".into();
            Ok(())
        })
        .unwrap();
        assert_eq!(
            Settings::load(dir.path()).unwrap().active().unwrap().id,
            "b"
        );
        Settings::select_endpoint(dir.path(), "a").unwrap();
        assert!(Settings::select_endpoint(dir.path(), "missing").is_err());
        assert_eq!(
            Settings::load(dir.path()).unwrap().active().unwrap().id,
            "a"
        );
    }

    #[cfg(windows)]
    #[test]
    fn recognizes_odd_and_even_startup_approved_disable_flags() {
        assert!(!auto_launch::is_disabled_approval(&[]));
        assert!(!auto_launch::is_disabled_approval(&[0x00, 0, 0, 0]));
        assert!(!auto_launch::is_disabled_approval(&[0x02, 0, 0, 0]));
        assert!(auto_launch::is_disabled_approval(&[0x03, 0, 0, 0]));
        assert!(auto_launch::is_disabled_approval(&[0x06, 0, 0, 0]));
        assert!(auto_launch::is_disabled_approval(&[0x07, 0, 0, 0]));
    }
}
