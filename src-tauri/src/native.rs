use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Runtime};
#[cfg(target_os = "android")]
use tauri::Manager;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DnsServers {
    #[serde(default)]
    pub servers: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkInfo {
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallArgs {
    path: String,
}

#[cfg(target_os = "android")]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("singboard-native")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                let handle = api.register_android_plugin("io.github.Yuu518.singboard", "SingboardPlugin")?;
                app.manage(Native(handle));
            }
            #[cfg(not(target_os = "android"))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}

#[cfg(target_os = "android")]
fn call<R: Runtime, T: serde::de::DeserializeOwned>(
    app: &AppHandle<R>,
    command: &str,
    payload: impl Serialize,
) -> Result<T, String> {
    let native = app
        .try_state::<Native<R>>()
        .ok_or("原生插件未初始化")?;
    native
        .0
        .run_mobile_plugin(command, payload)
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "android"))]
fn call<R: Runtime, T: serde::de::DeserializeOwned>(
    app: &AppHandle<R>,
    command: &str,
    payload: impl Serialize,
) -> Result<T, String> {
    let _ = (app, serde_json::to_value(payload));
    Err(format!("{command} 仅在 Android 上可用"))
}

pub fn dns_servers<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    call::<R, DnsServers>(app, "dnsServers", ())
        .map(|r| r.servers)
        .unwrap_or_default()
}

pub fn apk_info<R: Runtime>(app: &AppHandle<R>) -> Result<ApkInfo, String> {
    call(app, "apkInfo", ())
}

pub fn install_apk<R: Runtime>(app: &AppHandle<R>, path: String) -> Result<(), String> {
    call::<R, serde_json::Value>(app, "installApk", InstallArgs { path }).map(|_| ())
}

#[derive(Serialize)]
struct OpenUrlArgs {
    url: String,
}

pub fn open_url<R: Runtime>(app: &AppHandle<R>, url: String) -> Result<(), String> {
    call::<R, serde_json::Value>(app, "openUrl", OpenUrlArgs { url }).map(|_| ())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemBarsArgs {
    dark: bool,
    color: Option<String>,
}

pub fn set_system_bars<R: Runtime>(
    app: &AppHandle<R>,
    dark: bool,
    color: Option<String>,
) -> Result<(), String> {
    call::<R, serde_json::Value>(app, "setSystemBars", SystemBarsArgs { dark, color }).map(|_| ())
}

#[derive(Serialize, Deserialize, Default)]
pub struct SystemInsets {
    #[serde(default)]
    pub top: f64,
    #[serde(default)]
    pub right: f64,
    #[serde(default)]
    pub bottom: f64,
    #[serde(default)]
    pub left: f64,
}

pub fn system_insets<R: Runtime>(app: &AppHandle<R>) -> Result<SystemInsets, String> {
    call(app, "systemInsets", ())
}
