use crate::{
    commands::update,
    settings::Settings,
    tools::{Reply, Values, value},
};
use std::cmp::Ordering;
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
    _action: &str,
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
    handle_release(dir, repo, v, info, ticket).await
}

async fn handle_release(
    dir: &Path,
    repo: &str,
    v: &Values,
    info: update::CoreUpdateInfo,
    ticket: Option<&str>,
) -> Result<Reply, String> {
    let get = |key: &str| value(v, key);
    let target = PathBuf::from(get("core").trim());
    let installed = detect_installed_version(dir, &target, get("installed")).await;
    let order = installed
        .as_deref()
        .and_then(|cur| update::compare_installed_core(cur, &info.version));
    if order == Some(Ordering::Greater) {
        return Ok(Reply::text(format!(
            "本地核心版本高于上游版本（{}）",
            info.version
        )));
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
    if ticket.is_some_and(|t| t != selection) {
        return Err("版本或目标已变化，请重新检查并确认".into());
    }
    #[cfg(target_os = "android")]
    if ticket.is_some() || order == Some(Ordering::Equal) {
        if !Settings::load(dir)?.native_root_enabled {
            return Err("请先在本地核心设置中启用 Root 模式".into());
        }
        crate::root::set_enabled(true);
    }
    let context = update::UpdateContext {
        directory: dir.into(),
        progress: Box::new(|phase, n, total| {
            if let Ok(mut value) = PROGRESS.lock() {
                *value = Some((phase.into(), n, total));
            }
        }),
    };
    let mirror = (!get("mirror").is_empty()).then(|| get("mirror").trim().into());
    if ticket.is_none() {
        let detail = if order == Some(Ordering::Equal) {
            if update::core_matches_release(&context, &target, &info, &mirror).await? {
                return Ok(Reply::text(format!("已是最新版本（{}）", info.version)));
            }
            " · 同版本核心文件与上游不同，可重新安装"
        } else {
            ""
        };
        return Ok(Reply {
            text: format!(
                "{} · {:.1} MiB{detail}\n更新目标：{}\n核心正在运行时会重启服务。",
                info.version,
                info.asset_size as f64 / 1048576.,
                target.display()
            ),
            ticket: Some(selection),
            refresh: false,
        });
    }
    *PROGRESS.lock().unwrap() = None;
    let result =
        update::platform::perform_core_update(context, &info, &mirror, get("core").trim().into())
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[tokio::test]
    async fn same_version_core_check_offers_rebuilt_binary() {
        let dir = tempfile::tempdir().unwrap();
        let core = dir.path().join("sing-box.exe");
        let mut values = Values::from([
            ("core".into(), core.to_str().unwrap().into()),
            ("installed".into(), "sing-box version 1.14.4".into()),
        ]);
        let info = update::CoreUpdateInfo {
            version: "v1.14.4".into(),
            prerelease: false,
            published_at: String::new(),
            asset_name: "sing-box-win-x64.exe".into(),
            asset_url: "https://example.invalid/sing-box-win-x64.exe".into(),
            asset_size: 6,
            asset_digest: format!("sha256:{:x}", Sha256::digest(b"MZnew!")),
        };
        for (bytes, reinstall) in [(b"MZold!", true), (b"MZnew!", false)] {
            std::fs::write(&core, bytes).unwrap();
            let reply = handle_release(dir.path(), "example/core", &values, info.clone(), None)
                .await
                .unwrap();
            assert_eq!(reply.ticket.is_some(), reinstall, "{}", reply.text);
            if let Some(ticket) = reply.ticket {
                assert!(reply.text.contains("可重新安装"));
                let mut replaced = info.clone();
                replaced.asset_digest = format!("sha256:{:x}", Sha256::digest(b"MZnext"));
                let error =
                    handle_release(dir.path(), "example/core", &values, replaced, Some(&ticket))
                        .await
                        .err()
                        .unwrap();
                assert!(error.contains("重新检查"));
            }
        }
        std::fs::write(&core, b"MZold!").unwrap();
        values.insert("installed".into(), "1.14.5".into());
        let reply = handle_release(dir.path(), "example/core", &values, info.clone(), None)
            .await
            .unwrap();
        assert!(reply.ticket.is_none());
        values.insert("installed".into(), "1.14.3".into());
        let reply = handle_release(dir.path(), "example/core", &values, info, None)
            .await
            .unwrap();
        assert!(reply.ticket.is_some() && !reply.text.contains("重新安装"));
    }
}
