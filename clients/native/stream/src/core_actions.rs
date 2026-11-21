use crate::{
    commands::update,
    settings::Settings,
    tools::{Reply, Values, value},
};
use std::path::{Path, PathBuf};

static PROGRESS: std::sync::Mutex<Option<(String, u64, u64)>> = std::sync::Mutex::new(None);
pub fn progress() -> String {
    PROGRESS
        .lock()
        .ok()
        .and_then(|p| p.clone())
        .map(|(phase, n, total)| {
            let label = match phase.as_str() {
                "download" => "正在下载",
                "extract" => "正在解压",
                "restart" => "正在重启",
                "replace" => "正在替换",
                s => s,
            };
            if total > 0 {
                format!(
                    "{label}   {:.1} / {:.1} MiB",
                    n as f64 / 1048576.,
                    total as f64 / 1048576.
                )
            } else {
                label.into()
            }
        })
        .unwrap_or_else(|| "正在准备核心更新…".into())
}
async fn detect_installed_version(dir: &Path, target: &Path, fallback: &str) -> Option<String> {
    #[cfg(windows)]
    let probed = {
        let _ = dir;
        if target.is_file() {
            update::platform::probe_core_version(target).await.ok()
        } else {
            None
        }
    };
    #[cfg(target_os = "android")]
    let probed = if !target.as_os_str().is_empty()
        && Settings::load(dir)
            .ok()
            .is_some_and(|prefs| prefs.native_root_enabled)
    {
        crate::root::set_enabled(true);
        update::platform::probe_core_version(target).await.ok()
    } else {
        None
    };
    #[cfg(not(any(windows, target_os = "android")))]
    let probed: Option<String> = {
        let _ = (dir, target);
        None
    };
    probed
        .filter(|v| !update::normalize_core_version(v).is_empty())
        .or_else(|| {
            let fallback = fallback.trim();
            (!fallback.is_empty()).then(|| fallback.to_owned())
        })
}

pub async fn run(
    dir: &Path,
    action: &str,
    v: &Values,
    ticket: Option<&str>,
) -> Result<Reply, String> {
    let get = |key: &str| value(v, key);
    let repo = match get("source") {
        "custom" => get("repo"),
        "ref1nd" => "reF1nd/sing-box-releases",
        _ => "SagerNet/sing-box",
    };
    let token = (!get("token").is_empty()).then(|| get("token").to_owned());
    let info = update::check_core_update(repo.into(), get("channel").into(), token).await?;
    Settings::edit(dir, |p| {
        p.core_update_source = get("source").into();
        p.core_update_custom_repo = get("repo").into();
        p.core_update_channel = get("channel").into();
        p.core_update_mirror = get("mirror").into();
        Ok(())
    })?;
    let summary = format!(
        "{} · {:.1} MiB",
        info.version,
        info.asset_size as f64 / 1048576.
    );
    let target = PathBuf::from(get("core").trim());
    let installed = detect_installed_version(dir, &target, get("installed")).await;
    let up_to_date = installed
        .as_deref()
        .is_some_and(|cur| update::is_core_up_to_date(&info.version, cur));
    if action == "core_check" {
        return Ok(Reply::text(if up_to_date {
            format!("已是最新版本（{}）", info.version)
        } else {
            format!("可更新至 {summary}")
        }));
    }
    if ticket.is_none() && up_to_date {
        return Ok(Reply::text(format!("已是最新版本（{}）", info.version)));
    }
    update::core_asset_hash(&info.asset_digest)?;
    if !target.is_absolute()
        || target.file_name().is_none()
        || get("core").chars().any(char::is_control)
    {
        return Err("请选择核心的绝对文件路径".into());
    }
    let selection = serde_json::to_string(&(
        repo,
        &info.version,
        &info.asset_digest,
        &target,
        get("mirror"),
    ))
    .map_err(|e| e.to_string())?;
    if ticket.is_none() {
        return Ok(Reply {
            text: format!(
                "{summary}\n更新目标：{}\n核心正在运行时会重启服务。",
                target.display()
            ),
            ticket: Some(selection),
            refresh: false,
        });
    }
    if ticket != Some(&selection) {
        return Err("版本或目标已变化，请重新检查并确认".into());
    }
    #[cfg(target_os = "android")]
    if !Settings::load(dir)?.native_root_enabled {
        return Err("请先在本地核心设置中启用 Root 模式".into());
    } else {
        crate::root::set_enabled(true);
    }
    *PROGRESS.lock().unwrap() = None;
    let context = update::UpdateContext {
        directory: dir.into(),
        progress: Box::new(|phase, n, total| {
            if let Ok(mut value) = PROGRESS.lock() {
                *value = Some((phase.into(), n, total));
            }
        }),
    };
    let mirror = (!get("mirror").is_empty()).then(|| get("mirror").trim().into());
    let result = update::platform::perform_core_update(
        context,
        info.asset_url,
        info.asset_size,
        info.asset_digest,
        mirror,
        get("core").trim().into(),
    )
    .await?;
    Ok(Reply::text(format!(
        "已更新：{}{}",
        result.version,
        if result.restarted {
            "，核心已重启"
        } else {
            ""
        }
    )))
}
