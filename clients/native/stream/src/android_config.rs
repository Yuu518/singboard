use crate::{
    profiles::{self, Store},
    root,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Command {
    Tools,
    CoreProgress,
    NetworkOverview,
    NetworkCacheLoad {
        endpoint: String,
        #[serde(default)]
        core_started_at: Option<u64>,
    },
    NetworkCacheClear,
    NetworkDirect {
        #[serde(default)]
        endpoint: String,
    },
    NetworkProxy {
        #[serde(default)]
        endpoint: String,
        #[serde(default)]
        fingerprint: String,
        #[serde(default)]
        reset_cooldown: bool,
    },
    NetworkSites {
        #[serde(default)]
        endpoint: String,
    },
    RootServiceStatus,
    RootServiceSync,
    ConnectionGeo { ip: String },
    RuleEntries { tag: String },
    RuleMatches { tags: Vec<String>, query: String },
    SetGlassTint {
        tint: f32,
    },
    SetFontScale {
        scale: f32,
    },
    Format {
        text: String,
    },
    Modules {
        text: String,
    },
    Tool {
        action: String,
        values: crate::tools::Values,
        #[serde(default)]
        servers: Vec<String>,
        ticket: Option<String>,
    },
    Preferences,
    SelectEndpoint {
        id: String,
    },
    CheckUpdate { installed_apk: Option<std::path::PathBuf> },
    SetProxySort { enabled: bool },
    SetProxyGroupOpen { group: String, open: bool },
    UpdateProgress,
    CancelUpdate { ticket: String },
    DnsLookup { domain: String, record: String, server: Option<String>, servers: Vec<String>, geo: bool },
    RemoveDnsHistory { domain: String },
    SetDnsGeo { enabled: bool },
    DownloadUpdate {
        version: String,
        #[serde(default)]
        ticket: String,
    },
    List,
    Add {
        name: String,
        url: String,
        hours: u32,
    },
    Update {
        id: String,
        name: String,
        url: String,
        hours: u32,
    },
    Refresh {
        id: String,
    },
    Due,
    Remove {
        id: String,
    },
    Open {
        id: String,
    },
    Backup {
        id: String,
    },
    Save {
        id: String,
        expected: String,
        text: String,
    },
    Validate {
        text: String,
    },
    RootOpen,
    RootBackup {
        path: String,
    },
    RootCheck {
        path: String,
        text: String,
    },
    RootSave {
        path: String,
        expected: String,
        text: String,
    },
}

struct Runtime {
    config: String,
    core: String,
    dir: String,
}

async fn root_action(
    dir: &Path,
    action: &str,
    v: &crate::tools::Values,
) -> Result<crate::tools::Reply, String> {
    use crate::{
        commands::{config, service},
        settings::Settings,
        tools::{Reply, value},
    };
    if action == "root_disable" {
        Settings::edit(dir, |p| {
            p.native_root_enabled = false;
            Ok(())
        })?;
        root::set_enabled(false);
        return Ok(Reply::saved("已切换到远程模式"));
    }
    if action == "root_enable" {
        if !root::probe() {
            return Err("无法取得 Root 权限".into());
        }
        Settings::edit(dir, |p| {
            p.native_root_enabled = true;
            Ok(())
        })?;
        root::set_enabled(true);
        return Ok(Reply::saved("Root 模式已启用"));
    }
    if !Settings::load(dir)?.native_root_enabled {
        return Err("请先启用 Root 模式".into());
    }
    root::set_enabled(true);
    let path = |key| {
        let s = value(v, key).trim();
        if !Path::new(s).is_absolute() || s.chars().any(char::is_control) {
            Err("请输入有效的绝对路径")
        } else {
            Ok(s.to_string())
        }
    };
    match action {
        "root_detect" => {
            let files = config::detect_runtime_files(Some(path("workdir")?)).await?;
            if !files.found {
                return Err("所选目录中没有找到核心或配置".into());
            }
            let core_path = files
                .singbox_path
                .clone()
                .unwrap_or_else(|| files.core_install_path.clone());
            let config_path = files.config_path.clone().unwrap_or_default();
            Settings::edit(dir, |p| {
                p.working_dir = files.base_dir.clone();
                p.singbox_path = core_path.clone();
                p.config_path = config_path.clone();
                Ok(())
            })?;
            let install_note = if let (Some(detected_core), Some(detected_config)) =
                (files.singbox_path.clone(), files.config_path.clone())
            {
                match service::service_install(
                    detected_core,
                    detected_config,
                    files.base_dir.clone(),
                )
                .await
                {
                    Ok(()) => "\n服务脚本已同步",
                    Err(_) => "",
                }
            } else {
                ""
            };
            Ok(Reply::saved(format!(
                "工作目录：{}\n核心：{}\n配置：{}{}",
                files.base_dir, core_path, config_path, install_note
            )))
        }
        "root_install" => {
            let work = path("workdir")?;
            let core = path("core")?;
            let config_path = path("config")?;
            config::validate_config(core.clone(), config_path.clone(), Some(work.clone())).await?;
            service::service_install(core.clone(), config_path.clone(), work.clone()).await?;
            Settings::edit(dir, |p| {
                p.working_dir = work;
                p.config_path = config_path;
                p.singbox_path = core;
                Ok(())
            })?;
            Ok(Reply::saved("本地服务已登记"))
        }
        "root_status" => {
            let s = json!(service::service_status().await?);
            Ok(Reply::text(format!(
                "{} · PID {}\n运行 {} 秒 · CPU {}% · 内存 {} MiB\n开机启动：{}",
                s["state"].as_str().unwrap_or("unknown"),
                s["pid"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "—".into()),
                s["uptimeSeconds"].as_u64().unwrap_or_default(),
                s["cpuPercent"]
                    .as_f64()
                    .map(|v| format!("{v:.1}"))
                    .unwrap_or_else(|| "—".into()),
                s["memoryBytes"].as_u64().unwrap_or(0) / 1048576,
                if service::service_boot_hook_exists().await {
                    "开启"
                } else {
                    "关闭"
                }
            )))
        }
        "root_start" => {
            let prefs = Settings::load(dir)?;
            let work = prefs.working_dir.trim();
            let core = prefs.singbox_path.trim();
            let cfg = prefs.config_path.trim();
            if !work.is_empty() && !core.is_empty() && !cfg.is_empty() {
                let needs_install = match tokio::task::spawn_blocking(runtime_files).await {
                    Ok(Ok(rt)) => rt.dir != work || rt.core != core || rt.config != cfg,
                    _ => true,
                };
                if needs_install {
                    config::validate_config(core.into(), cfg.into(), Some(work.into())).await?;
                    service::service_install(core.into(), cfg.into(), work.into()).await?;
                }
            }
            service::service_start().await?;
            Ok(Reply::text(
                crate::tools::readiness(&Settings::load(dir)?).await,
            ))
        }
        "root_stop" => {
            service::service_stop().await?;
            Ok(Reply::text("核心已停止"))
        }
        "root_restart" => {
            service::service_restart().await?;
            Ok(Reply::text(
                crate::tools::readiness(&Settings::load(dir)?).await,
            ))
        }
        "root_boot_status" => Ok(Reply::text(if service::service_boot_hook_exists().await {
            "on"
        } else {
            "off"
        })),
        "root_boot" => {
            service::service_create_boot_hook().await?;
            Ok(Reply::text("已启用开机启动"))
        }
        "root_unboot" => {
            service::service_delete_boot_hook().await?;
            Ok(Reply::text("已取消开机启动"))
        }
        "root_log" => {
            let log = service::service_error_log().await?;
            Ok(Reply::text(if log.is_empty() {
                "暂无服务日志".into()
            } else {
                log
            }))
        }
        _ => Err("未知服务操作".into()),
    }
}

fn runtime_files() -> Result<Runtime, String> {
    root::set_enabled(true);
    let home = root::SERVICE_HOME;
    let env = root::run(&format!(
        "test -f {home}/singboard.sh || {{ echo '未安装 Singboard 服务'; exit 1; }}\nif test -f {home}/service.env; then cat {home}/service.env; fi"
    ))?;
    let mut values = std::collections::HashMap::new();
    for line in env.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !matches!(key, "CONFIG" | "CORE" | "WORKDIR") {
            continue;
        }
        let value = value
            .strip_prefix('\'')
            .and_then(|v| v.strip_suffix('\''))
            .ok_or("服务路径格式错误")?
            .replace("'\\''", "'");
        if !Path::new(&value).is_absolute() || value.contains(['\n', '\0']) {
            return Err("服务路径无效".into());
        }
        values.insert(key, value);
    }
    let dir = values.remove("WORKDIR").unwrap_or_else(|| home.to_string());
    Ok(Runtime {
        config: values
            .remove("CONFIG")
            .unwrap_or_else(|| format!("{dir}/config.json")),
        core: values
            .remove("CORE")
            .unwrap_or_else(|| format!("{dir}/sing-box")),
        dir,
    })
}

fn runtime_files_with_fallback(prefs: &crate::settings::Settings) -> Result<Runtime, String> {
    if let Ok(rt) = runtime_files() {
        return Ok(rt);
    }
    let dir = prefs.working_dir.trim();
    let core = prefs.singbox_path.trim();
    let config = prefs.config_path.trim();
    if !dir.is_empty()
        && !core.is_empty()
        && !config.is_empty()
        && Path::new(dir).is_absolute()
        && Path::new(core).is_absolute()
        && Path::new(config).is_absolute()
    {
        return Ok(Runtime {
            config: config.to_string(),
            core: core.to_string(),
            dir: dir.to_string(),
        });
    }
    Err("未安装 Singboard 服务，请先在设置中识别工作目录".into())
}

fn root_document(backup: bool, expected: Option<&str>) -> Result<Value, String> {
    let runtime = runtime_files()?;
    if expected.is_some_and(|p| p != runtime.config) {
        return Err("服务配置路径已变化，请重新打开".into());
    }
    let path = Path::new(&runtime.config);
    let previous = profiles::backup_path(path);
    let content = root::fs::read_to_string(if backup { &previous } else { path })?;
    Ok(json!({"path":runtime.config,"text":content,"backup":root::fs::is_file(&previous)}))
}

fn check_root_with_runtime(
    runtime: &Runtime,
    path: &str,
    text: &str,
    expected: Option<&str>,
) -> Result<Value, String> {
    profiles::validate_jsonc(text)?;
    if runtime.config != path {
        return Err("服务配置路径已变化，请重新打开".into());
    }
    let target = root::quote(path);
    let dir = root::quote(&runtime.dir);
    let executable = root::quote(&runtime.core);
    let parent = root::quote(
        Path::new(path)
            .parent()
            .and_then(Path::to_str)
            .ok_or("无效的配置目录")?,
    );
    let encoded = STANDARD.encode(text);
    let encoded = encoded
        .as_bytes()
        .chunks(76)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let mut script = format!(
        "set -e\numask 077\ncd {dir}\ntmp=$(mktemp {parent}/.singboard-edit.XXXXXX)\nold=''\ntrap 'rm -f \"$tmp\"; if [ -n \"$old\" ]; then rm -f \"$old\"; fi' EXIT\nbase64 -d > \"$tmp\" <<'SINGBOARD_CONFIG'\n{}\nSINGBOARD_CONFIG\ntimeout 60 {executable} check -c \"$tmp\" --disable-color\n",
        encoded
    );
    if let Some(expected) = expected {
        let digest = format!("{:x}", Sha256::digest(expected.as_bytes()));
        let backup = root::quote(&format!("{path}.bak"));
        script.push_str(&format!("actual=$(sha256sum {target}); [ \"${{actual%% *}}\" = {digest} ] || {{ echo '配置已被其他程序修改'; exit 1; }}\ncmp -s \"$tmp\" {target} && exit 0\nold=$(mktemp {parent}/.singboard-backup.XXXXXX)\ncp -p {target} \"$old\" 2>/dev/null || cp {target} \"$old\""));
        script.push_str(&format!("\nchmod --reference={target} \"$tmp\" 2>/dev/null || chmod \"$(stat -c %a {target})\" \"$tmp\" 2>/dev/null || true\nchown \"$(stat -c %u:%g {target})\" \"$tmp\" 2>/dev/null || true\nmv -f \"$old\" {backup}\nmv -f \"$tmp\" {target}\n"));
    }
    root::run(&script)?;
    if expected.is_some() {
        root::fs::forget_shared(Path::new(path));
    }
    Ok(json!(if expected.is_some() {
        "已保存，重启核心后生效"
    } else {
        "sing-box 校验通过"
    }))
}

fn check_root(path: &str, text: &str, expected: Option<&str>) -> Result<Value, String> {
    let runtime = runtime_files()?;
    check_root_with_runtime(&runtime, path, text, expected)
}

static UPDATE_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn update_cancel() -> &'static tokio::sync::watch::Sender<String> {
    static CANCEL: std::sync::OnceLock<tokio::sync::watch::Sender<String>> = std::sync::OnceLock::new();
    CANCEL.get_or_init(|| tokio::sync::watch::channel(String::new()).0)
}

pub async fn request(dir: PathBuf, input: &str) -> Result<Value, String> {
    let store = Store::new(dir.join("subscriptions"));
    let op: Command = serde_json::from_str(input).map_err(|e| e.to_string())?;
    let prefs = crate::settings::Settings::load(&dir)?;
    if matches!(
        op,
        Command::RootOpen
            | Command::RootServiceStatus
            | Command::RootServiceSync
            | Command::RuleEntries { .. }
            | Command::RuleMatches { .. }
            | Command::RootCheck { .. }
            | Command::RootSave { .. }
            | Command::RootBackup { .. }
    ) && !prefs.native_root_enabled
    {
        return Err("请先在设置中启用 Root 模式".into());
    }
    let check_with_core = prefs.native_root_enabled
        && !prefs.config_path.is_empty()
        && !prefs.singbox_path.is_empty();
    let fallback_prefs = prefs.clone();
    let syntax = move |text: String| {
        let fallback_prefs = fallback_prefs.clone();
        async move {
            if check_with_core {
                tokio::task::spawn_blocking(move || {
                    let runtime = runtime_files_with_fallback(&fallback_prefs)?;
                    check_root_with_runtime(&runtime, &runtime.config, &text, None).map(|_| ())
                })
                .await
                .map_err(|e| e.to_string())??;
            }
            Ok::<(), String>(())
        }
    };
    let reading_backup = matches!(op, Command::Backup { .. });
    Ok(match op {
        Command::Format{text}=>json!(crate::editor::format(&text)?),
        Command::Modules{text}=>json!(crate::editor::modules(&text)?.into_iter().map(|(name,range)|json!({"name":name,"start":text[..range.start].encode_utf16().count(),"end":text[..range.end].encode_utf16().count()})).collect::<Vec<_>>()),
        Command::CoreProgress=>json!(crate::core_actions::progress()),
        Command::NetworkOverview=>crate::tools::network_overview().await,
        Command::NetworkCacheLoad { endpoint, core_started_at } => {
            json!(crate::tools::load_network_cache(&dir, &endpoint, core_started_at))
        }
        Command::NetworkCacheClear => {
            crate::tools::clear_network_cache(&dir);
            json!(true)
        }
        Command::NetworkDirect { endpoint } => {
            let data = crate::tools::direct_exit_info().await;
            let updated_at = crate::tools::save_network_slot(&dir, &endpoint, "domestic", data.clone(), "");
            json!({"data": data, "updatedAt": updated_at})
        }
        Command::NetworkProxy { endpoint, fingerprint, reset_cooldown } => {
            let data = crate::tools::proxy_exit_info(reset_cooldown).await;
            let updated_at = crate::tools::save_network_slot(&dir, &endpoint, "abroad", data.clone(), &fingerprint);
            json!({"data": data, "updatedAt": updated_at})
        }
        Command::NetworkSites { endpoint } => {
            let data = crate::tools::site_latencies_overview().await;
            let updated_at = crate::tools::save_network_slot(&dir, &endpoint, "sites", data.clone(), "");
            json!({"data": data, "updatedAt": updated_at})
        }
        Command::RootServiceStatus=>{
            root::set_enabled(true);
            json!(crate::commands::service::service_status().await?)
        }
        Command::RootServiceSync=>{
            root::set_enabled(true);
            json!(crate::commands::service::service_component_sync().await?)
        }
        Command::ConnectionGeo { ip } => {
            if crate::tools::public_ip(&ip) { crate::tools::geo_details(&ip).await? } else { Value::Null }
        }
        Command::RuleEntries { tag } => {
            let rule_prefs = prefs.clone();
            tokio::task::spawn_blocking(move || {
                let runtime = runtime_files_with_fallback(&rule_prefs)?;
                crate::commands::srs::srs_list_provider(runtime.dir, runtime.config, runtime.core, tag)
            }).await.map_err(|e| e.to_string()).and_then(|r| r).map(|rows| json!(rows))?
        }
        Command::RuleMatches { tags, query } => {
            let rule_prefs = prefs.clone();
            tokio::task::spawn_blocking(move || {
                let runtime = runtime_files_with_fallback(&rule_prefs)?;
                let mut hits = Vec::new();
                let mut errors = Vec::new();
                for tag in tags {
                    match crate::commands::srs::srs_match_provider(runtime.dir.clone(), runtime.config.clone(), runtime.core.clone(), tag.clone(), query.clone()) {
                        Ok(true) => hits.push(tag),
                        Ok(false) => {}
                        Err(e) => errors.push(format!("{tag} · {e}")),
                    }
                }
                Ok::<_, String>(json!({"hits": hits, "errors": errors}))
            }).await.map_err(|e| e.to_string())??
        }
        Command::SetGlassTint{tint}=>{
            crate::settings::Settings::set_glass_tint(&dir,tint)?;
            Value::Null
        }
        Command::SetFontScale{scale}=>{
            crate::settings::Settings::set_font_scale(&dir,scale)?;
            Value::Null
        }
        Command::Tools=>json!(crate::tools::forms(&crate::settings::Settings::load(&dir)?)),
        Command::Tool{action,values,servers,ticket}=>{
            let reply=if action.starts_with("core_"){crate::core_actions::run(&dir,&action,&values,ticket.as_deref()).await?}
                else if action.starts_with("root_"){root_action(&dir,&action,&values).await?}
                else{
                    if action.starts_with("rules_"){root::set_enabled(prefs.native_root_enabled);}
                    crate::tools::run(&dir,&action,&values,&servers).await?
                };
            json!(reply)
        }
        Command::Preferences => json!(crate::settings::Settings::load(&dir)?),
        Command::SelectEndpoint { id } => {
            crate::settings::Settings::select_endpoint(&dir, &id)?;
            Value::Null
        }
        Command::SetProxySort { enabled } => {
            crate::settings::Settings::edit(&dir, |s| { s.proxy_sort_latency = enabled; Ok(()) })?;
            Value::Null
        }
        Command::SetProxyGroupOpen { group, open } => {
            crate::settings::Settings::edit(&dir, |s| {
                s.expanded_proxy_groups.retain(|existing| existing != &group);
                if open { s.expanded_proxy_groups.push(group); }
                Ok(())
            })?;
            Value::Null
        }
        Command::CheckUpdate { installed_apk } => {
            let mut info = crate::update::check(&prefs.github_token).await?;
            info.mirror = prefs.core_update_mirror;
            if let Some(path) = installed_apk { info.compare_executable(&path)?; }
            json!(info)
        }
        Command::UpdateProgress => json!(UPDATE_BYTES.load(std::sync::atomic::Ordering::Relaxed)),
        Command::CancelUpdate { ticket } => {
            update_cancel().send_replace(ticket);
            Value::Null
        }
        Command::DnsLookup { domain, record, server, servers, geo } => crate::tools::dns_records(&dir, domain, record, server, &servers, geo).await?,
        Command::RemoveDnsHistory { domain } => json!(crate::settings::Settings::dns_history(&dir, &domain, false)?),
        Command::SetDnsGeo { enabled } => {
            crate::settings::Settings::edit(&dir, |s| { s.dns_geo_enabled = enabled; Ok(()) })?;
            Value::Null
        }
        Command::DownloadUpdate { version, ticket } => {
            UPDATE_BYTES.store(0, std::sync::atomic::Ordering::Relaxed);
            let path = dir.join("updates");
            let mut cancel = update_cancel().subscribe();
            let download = crate::update::download_with_progress(&path, &version, &prefs.github_token, &prefs.core_update_mirror, |bytes| {
                UPDATE_BYTES.store(bytes, std::sync::atomic::Ordering::Relaxed);
            });
            let (path, _) = tokio::select! {
                biased;
                _ = cancel.wait_for(|value| !ticket.is_empty() && value == &ticket) => return Err("已取消下载".into()),
                result = download => result?,
            };
            json!(path)
        }
        Command::List => json!(store.list()?),
        Command::Add { name, url, hours } => json!(store.add(&name, &url, hours, syntax).await?),
        Command::Update {
            id,
            name,
            url,
            hours,
        } => json!(
            store
                .update(&id, Some((&name, &url, hours)), syntax)
                .await?
        ),
        Command::Refresh { id } => json!(store.update(&id, None, syntax).await?),
        Command::Due => {
            let mut errors = vec![];
            for p in store.list()?.into_iter().filter(|p| p.due()) {
                if let Err(e) = store.update(&p.id, None, &syntax).await {
                    errors.push(format!("{} · {e}", p.name));
                }
            }
            json!(errors)
        }
        Command::Remove { id } => {
            store.remove(&id)?;
            Value::Null
        }
        Command::Open { id } | Command::Backup { id } => {
            let path = store.path(&id)?;
            let backup = profiles::backup_path(&path);
            let file = if reading_backup { &backup } else { &path };
            json!({"path":path,"text":fs::read_to_string(file).map_err(|e|e.to_string())?,"backup":backup.is_file()})
        }
        Command::Save { id, expected, text } => {
            syntax(text.clone()).await?;
            profiles::save_profile(&store, &id, &expected, &text)?;
            json!("已保存")
        }
        Command::Validate { text } => {
            profiles::validate_jsonc(&text)?;
            json!("JSONC 语法通过")
        }
        Command::RootOpen => root_document(false, None)?,
        Command::RootBackup { path } => root_document(true, Some(&path))?,
        Command::RootCheck { path, text } => check_root(&path, &text, None)?,
        Command::RootSave {
            path,
            expected,
            text,
        } => check_root(&path, &text, Some(&expected))?,
    })
}
