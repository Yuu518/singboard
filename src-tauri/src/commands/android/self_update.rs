use std::path::Path;

use serde::Serialize;
use tauri::Manager;

use super::update::{
    GhRelease, UPDATE_LOCK, digest_hash, download_asset_from, emit_progress,
    github_get_with_token, is_newer, panel_download_urls, sha256_file, verify_digest,
};

const PANEL_REPO: &str = "Yuu518/singboard";
const PROGRESS_EVENT: &str = "panel-update-progress";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelUpdateInfo {
    current_version: String,
    latest_version: String,
    has_update: bool,
    out_of_sync: bool,
    published_at: String,
    asset_url: String,
    asset_api_url: String,
    asset_size: u64,
    asset_digest: String,
}

fn abi() -> Result<&'static str, String> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("arm64-v8a"),
        "arm" => Ok("armeabi-v7a"),
        "x86_64" => Ok("x86_64"),
        "x86" => Ok("x86"),
        other => Err(format!("不支持的 CPU 架构: {}", other)),
    }
}

fn asset_candidates() -> Result<[String; 2], String> {
    Ok([
        format!("singboard-{}.apk", abi()?),
        "singboard-universal.apk".to_string(),
    ])
}

fn installed_out_of_sync(app: &tauri::AppHandle, asset_digest: &str) -> bool {
    let Some(expected) = digest_hash(asset_digest) else {
        return false;
    };
    let Ok(info) = crate::native::apk_info(app) else {
        return false;
    };
    match sha256_file(Path::new(&info.path)) {
        Ok(actual) => !actual.eq_ignore_ascii_case(expected),
        Err(_) => false,
    }
}

#[tauri::command]
pub async fn check_panel_update(
    app: tauri::AppHandle,
    github_token: Option<String>,
) -> Result<PanelUpdateInfo, String> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        PANEL_REPO
    );
    let release: GhRelease = github_get_with_token(&url, github_token.as_deref())
        .await?
        .json()
        .await
        .map_err(|e| format!("解析 GitHub API 响应失败: {}", e))?;

    let candidates = asset_candidates()?;
    let asset = candidates
        .iter()
        .find_map(|name| {
            release
                .assets
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| format!("该版本未提供 {} 安装包", candidates[0]))?;

    let current_version = app.package_info().version.to_string();
    let latest_version = release.tag_name.trim_start_matches(['v', 'V']).to_string();
    let asset_digest = asset.digest.clone().unwrap_or_default();
    let has_update = is_newer(&latest_version, &current_version);

    let out_of_sync = if has_update {
        false
    } else {
        let digest = asset_digest.clone();
        let app = app.clone();
        tokio::task::spawn_blocking(move || installed_out_of_sync(&app, &digest))
            .await
            .unwrap_or(false)
    };

    Ok(PanelUpdateInfo {
        has_update,
        out_of_sync,
        current_version,
        latest_version,
        published_at: release.published_at.unwrap_or_default(),
        asset_url: asset.browser_download_url.clone(),
        asset_api_url: asset.url.clone(),
        asset_size: asset.size,
        asset_digest,
    })
}

#[tauri::command]
pub async fn perform_panel_update(
    app: tauri::AppHandle,
    asset_url: String,
    asset_api_url: Option<String>,
    asset_size: u64,
    asset_digest: String,
    mirror: Option<String>,
    github_token: Option<String>,
) -> Result<(), String> {
    let _guard = UPDATE_LOCK
        .try_lock()
        .map_err(|_| "更新正在进行中".to_string())?;

    let staging = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("获取缓存目录失败: {e}"))?
        .join("panel-update");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("创建临时目录失败: {}", e))?;
    let cleanup = |msg: String| {
        let _ = std::fs::remove_dir_all(&staging);
        msg
    };

    let apk = staging.join("singboard.apk");
    let token = github_token.as_deref();
    let urls = panel_download_urls(
        &mirror,
        PANEL_REPO,
        &asset_url,
        asset_api_url.as_deref(),
        super::update::github_token(token).is_some(),
    );
    download_asset_from(&app, PROGRESS_EVENT, &urls, token, asset_size, &apk)
        .await
        .map_err(cleanup)?;

    emit_progress(&app, PROGRESS_EVENT, "verify", 0, 0);
    {
        let apk = apk.clone();
        tokio::task::spawn_blocking(move || verify_digest(&apk, &asset_digest, &mirror))
            .await
            .map_err(|e| format!("任务执行失败: {}", e))
            .and_then(|r| r)
            .map_err(cleanup)?;
    }

    emit_progress(&app, PROGRESS_EVENT, "replace", 0, 0);
    crate::native::install_apk(&app, apk.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn cleanup_panel_update(app: tauri::AppHandle) {
    if let Ok(dir) = app.path().app_cache_dir() {
        let _ = std::fs::remove_dir_all(dir.join("panel-update"));
    }
}
