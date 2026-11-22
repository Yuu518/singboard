use crate::{
    clash::{Clash, Command},
    commands::{ip_lookup, network, srs},
    settings::{Endpoint, Settings},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub type Values = BTreeMap<String, String>;

pub async fn dns_records(dir: &Path, domain: String, record: String, server: Option<String>, servers: &[String], geo_enabled: bool) -> Result<Value, String> {
    let domain = domain.trim();
    let mut answer = json!(network::lookup_dns(domain.to_owned(), record, server, servers).await?);
    if let Some(records) = answer["records"].as_array_mut() {
        for rec in records {
            if geo_enabled && let Some(ip) = rec["ip"].as_str() && public_ip(ip) {
                rec["geo"] = json!(geo(ip).await.unwrap_or_default());
            }
        }
    }
    answer["history"] = json!(Settings::dns_history(dir, domain, true)?);
    Ok(answer)
}

#[derive(Clone, Serialize)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
    pub choices: Vec<(String, String)>,
    pub secret: bool,
}
#[derive(Clone, Serialize)]
pub struct Form {
    pub title: &'static str,
    pub fields: Vec<Field>,
    pub actions: Vec<(&'static str, &'static str)>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Reply {
    pub text: String,
    pub refresh: bool,
    pub ticket: Option<String>,
}
impl Reply {
    pub fn text(value: impl Into<String>) -> Self {
        Self {
            text: value.into(),
            ..Default::default()
        }
    }
    pub fn saved(value: impl Into<String>) -> Self {
        Self {
            refresh: true,
            ..Self::text(value)
        }
    }
}
fn field(key: &'static str, label: &'static str, value: &str) -> Field {
    Field {
        key,
        label,
        value: value.into(),
        choices: Vec::new(),
        secret: false,
    }
}
fn choice(key: &'static str, label: &'static str, value: &str, options: &[(&str, &str)]) -> Field {
    let mut f = field(key, label, value);
    f.choices = options
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    if !f.choices.iter().any(|(k, _)| k == value) {
        f.value = f.choices.first().map(|p| p.0.clone()).unwrap_or_default();
    }
    f
}
fn toggle(key: &'static str, label: &'static str, yes: bool) -> Field {
    choice(
        key,
        label,
        if yes { "on" } else { "off" },
        &[("on", "开启"), ("off", "关闭")],
    )
}
pub fn forms(p: &Settings) -> Vec<Form> {
    let default = Endpoint {
        name: "默认 API".into(),
        url: "http://127.0.0.1:9090".into(),
        ..Default::default()
    };
    let active = p.active().unwrap_or(&default);
    let apis = p
        .clash_apis
        .iter()
        .map(|a| (a.id.as_str(), a.name.as_str()))
        .collect::<Vec<_>>();
    let mut secret = field("secret", "API 密钥", &active.secret);
    secret.secret = true;
    let mut token = field("token", "GitHub Token（可选）", &p.github_token);
    token.secret = true;
    let mut forms = vec![
        Form {
            title: "API 后端",
            fields: vec![
                field("id", "", &active.id),
                field("name", "后端名称", &active.name),
                field("url", "Clash API 地址", &active.url),
                secret.clone(),
            ],
            actions: vec![
                ("probe", "测试连接"),
                ("save_api", "保存并使用"),
                ("add_api", "另存为新后端"),
            ],
        },
        Form {
            title: "切换与移除后端",
            fields: vec![choice("id", "已保存的后端", &active.id, &apis)],
            actions: vec![("switch_api", "使用此后端"), ("delete_api", "移除此后端")],
        },
        Form {
            title: "DNS 查询",
            fields: vec![
                field("domain", "域名", ""),
                choice(
                    "record",
                    "记录类型",
                    "A",
                    &[("A", "A"), ("AAAA", "AAAA"), ("CNAME", "CNAME")],
                ),
                choice(
                    "resolver",
                    "解析器",
                    "system",
                    &[
                        ("system", "系统 DNS"),
                        ("1.1.1.1", "Cloudflare"),
                        ("8.8.8.8", "Google"),
                        ("223.5.5.5", "阿里 DNS"),
                        ("custom", "自定义"),
                    ],
                ),
                field("server", "自定义 DNS 地址或 IP:端口", ""),
                toggle("geo", "通过 ip.sb 查询 IP 归属地", false),
            ],
            actions: vec![("dns", "查询")],
        },
        Form {
            title: "网络测试",
            fields: vec![
                field("test_url", "默认测速地址", &p.latency_test_url),
                field("group", "代理组名称", ""),
                field("group_url", "此组测速地址（留空恢复默认）", ""),
                toggle("ipv6", "IPv6 可达性测试", p.ipv6_test_enabled),
            ],
            actions: vec![
                ("test_settings", "保存测速设置"),
                ("ping", "测试网址延迟"),
                ("site_tests", "测试常用站点"),
                ("exit_ip", "查询出口 IP"),
            ],
        },
        Form {
            title: "核心更新",
            fields: vec![
                choice(
                    "source",
                    "更新源",
                    &p.core_update_source,
                    &[
                        ("official", "SagerNet 官方"),
                        ("ref1nd", "reF1nd"),
                        ("custom", "自定义仓库"),
                    ],
                ),
                field("repo", "自定义 owner/repo", &p.core_update_custom_repo),
                choice(
                    "channel",
                    "通道",
                    &p.core_update_channel,
                    &[("stable", "稳定版"), ("testing", "测试版")],
                ),
                field("mirror", "下载镜像（可选）", &p.core_update_mirror),
                field("core", "核心文件路径", &p.singbox_path),
                token,
            ],
            actions: vec![
                ("save_token", "保存 GitHub Token"),
                ("core_check", "检查核心版本"),
                ("core_prepare", "准备更新"),
            ],
        },
        Form {
            title: "规则集内容与匹配",
            fields: vec![
                field("tag", "规则提供商名称", ""),
                field("query", "域名、IP 或关键词", ""),
                field("page", "结果页码（每页 200 条）", "1"),
                field("workdir", "核心工作目录", &p.working_dir),
                field("config", "配置文件", &p.config_path),
                field("core", "核心文件", &p.singbox_path),
            ],
            actions: vec![
                ("rules_list", "搜索规则内容"),
                ("rules_match", "检查域名或 IP"),
                ("rules_find", "查询全部提供商"),
                ("rules_usage", "查看规则引用"),
            ],
        },
    ];
    let mut pages = vec![
        ("/overview", "概览"),
        ("/proxies", "代理"),
        ("/connections", "连接"),
        ("/logs", "日志"),
        ("/rules", "规则"),
    ];
    if cfg!(windows) {
        pages.push(("/config", "配置"));
    }
    pages.push(("/settings", "设置"));
    let mut prefs = vec![
        choice("start", "起始页", &p.start_page, &pages),
        toggle(
            "autocheck",
            "启动时检查面板更新",
            p.panel_auto_check_update.unwrap_or(true),
        ),
        choice(
            "theme",
            "主题",
            p.theme.as_deref().unwrap_or("auto"),
            &[("auto", "跟随系统"), ("light", "浅色"), ("dark", "深色")],
        ),
    ];
    if cfg!(windows) {
        prefs.push(toggle(
            "tray",
            "关闭到托盘",
            p.close_to_tray.unwrap_or(false),
        ));
        prefs.push(toggle(
            "autolaunch",
            "开机自动启动面板",
            crate::settings::get_auto_launch(),
        ));
    } else {
        prefs.push(toggle("swipe", "左右滑动切换页面", p.page_swipe));
    }
    forms.push(Form {
        title: "启动与偏好",
        fields: prefs,
        actions: vec![("preferences", "保存偏好")],
    });
    #[cfg(target_os = "android")]
    forms.push(Form {
        title: "本地核心与开机启动",
        fields: vec![
            field("workdir", "sing-box 工作目录", &p.working_dir),
            field("core", "核心路径", &p.singbox_path),
            field("config", "配置文件路径", &p.config_path),
        ],
        actions: vec![
            ("root_enable", "启用 Root 模式"),
            ("root_disable", "关闭 Root 模式"),
            ("root_detect", "检测工作目录"),
            ("root_install", "登记服务"),
            ("root_status", "查询状态"),
            ("root_start", "启动核心"),
            ("root_restart", "重启核心"),
            ("root_stop", "停止核心"),
            ("root_boot", "启用开机启动"),
            ("root_unboot", "取消开机启动"),
            ("root_log", "服务日志"),
        ],
    });
    forms.push(Form {
        title: "首次配置",
        fields: vec![
            field("id", "", &active.id),
            field("name", "后端名称", &active.name),
            field("url", "Clash API 地址", &active.url),
            secret,
            field("workdir", "本地核心工作目录（可选）", &p.working_dir),
        ],
        actions: vec![
            ("probe", "测试连接"),
            ("setup", "完成配置"),
            ("skip_setup", "稍后配置"),
        ],
    });
    #[cfg(windows)]
    {
        let locals = p
            .config_profiles
            .iter()
            .filter(|v| v.kind == "local")
            .map(|v| (v.id.as_str(), v.name.as_str()))
            .collect::<Vec<_>>();
        forms.push(Form {
            title: "本地配置档案",
            fields: vec![
                choice("profile", "现有配置", &p.active_config_profile_id, &locals),
                field("name", "名称", ""),
                field("path", "本地 JSONC 文件路径", ""),
                field("core", "校验用核心", &p.singbox_path),
                field("workdir", "工作目录", &p.working_dir),
            ],
            actions: vec![
                ("local_add", "添加配置"),
                ("local_rename", "修改名称"),
                ("local_use", "校验并使用"),
                ("local_remove", "移除档案"),
            ],
        });
    }
    forms
}

pub fn value<'a>(values: &'a Values, key: &str) -> &'a str {
    values.get(key).map(String::as_str).unwrap_or("")
}
fn http_url(input: &str) -> Result<(), String> {
    let url = url::Url::parse(input.trim()).map_err(|_| "请输入完整的 HTTP(S) 地址")?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err("地址需使用 HTTP 或 HTTPS".into());
    }
    Ok(())
}
pub fn confirm(action: &str) -> bool {
    matches!(
        action,
        "delete_api"
            | "local_remove"
            | "local_use"
            | "root_install"
            | "root_stop"
            | "root_restart"
            | "root_boot"
            | "root_unboot"
    )
}
fn endpoint_url(values: &Values) -> Result<String, String> {
    if !values.contains_key("host") {
        return Ok(value(values, "url").trim().to_owned());
    }
    let host = value(values, "host").trim();
    if host.is_empty() {
        return Err("请填写主机".into());
    }
    let protocol = value(values, "protocol");
    if !matches!(protocol, "http" | "https") {
        return Err("协议须为 HTTP 或 HTTPS".into());
    }
    let pasted = host.contains("://");
    let address = if pasted {
        host.to_owned()
    } else {
        format!("{protocol}://{host}")
    };
    let mut url = reqwest::Url::parse(&address).map_err(|_| "主机地址无效")?;
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss") || url.host_str().is_none() {
        return Err("主机地址无效".into());
    }
    let port = value(values, "port").trim();
    if !pasted && !port.is_empty() {
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .ok_or("端口须为 1 到 65535")?;
        url.set_port(Some(port)).map_err(|_| "端口无效")?;
    }
    Ok(url.to_string())
}

pub async fn run(
    dir: &Path,
    action: &str,
    v: &Values,
    servers: &[String],
) -> Result<Reply, String> {
    let get = |key: &str| value(v, key);
    let flag = |key| get(key) == "on";
    match action {
        "probe" => {
            let version = Clash::new(&endpoint_url(v)?, get("secret"))?
                .execute(Command::Version)
                .await?;
            Ok(Reply::text(format!(
                "连接成功 · {}",
                version["version"].as_str().unwrap_or("Clash API")
            )))
        }
        "update_preferences" => {
            let source = get("source");
            let channel = get("channel");
            if !matches!(source, "official" | "ref1nd" | "custom")
                || !matches!(channel, "stable" | "testing")
            {
                return Err("更新源或版本通道无效".into());
            }
            let repo = get("repo").trim();
            if !repo.is_empty()
                && !repo.split_once('/').is_some_and(|(owner, name)| {
                    !owner.is_empty()
                        && !name.is_empty()
                        && !name.contains('/')
                        && repo.chars().all(|c| {
                            c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '/')
                        })
                })
            {
                return Err("GitHub 仓库请使用 owner/repo 格式".into());
            }
            let mirror = get("mirror").trim();
            if !mirror.is_empty() {
                http_url(mirror)?;
            }
            Settings::edit(dir, |p| {
                p.core_update_source = source.into();
                p.core_update_channel = channel.into();
                p.core_update_custom_repo = repo.into();
                p.core_update_mirror = mirror.into();
                Ok(())
            })?;
            Ok(Reply::saved("更新设置已保存"))
        }
        "save_token" => {
            Settings::edit(dir, |p| {
                p.github_token = get("token").trim().into();
                Ok(())
            })?;
            Ok(Reply::saved("更新凭据已保存"))
        }
        "skip_setup" => {
            Settings::edit(dir, |p| {
                p.setup_completed = true;
                Ok(())
            })?;
            Ok(Reply::saved("可在设置中继续配置"))
        }
        "connection_sort" => {
            let key = get("key");
            if !crate::dashboard::SORTS.iter().any(|s| s.0 == key) {
                return Err("未知排序方式".into());
            }
            Settings::edit(dir, |p| {
                p.connection_sort = key.into();
                Ok(())
            })?;
            Ok(Reply::saved("已保存排序"))
        }
        "setup" | "save_api" | "add_api" => {
            let url = endpoint_url(v)?;
            Clash::new(&url, get("secret"))?;
            if get("name").trim().is_empty() {
                return Err("请输入后端名称".into());
            }
            #[cfg(windows)]
            let runtime = if action == "setup" && !get("workdir").trim().is_empty() {
                let runtime = crate::commands::runtime_files::detect_runtime_files(Some(
                    get("workdir").into(),
                ))
                .await?;
                if runtime.singbox_path.is_none() {
                    return Err(
                        "未在该目录及子目录中检测到 sing-box 核心，请检查目录后重试。".into(),
                    );
                }
                if let (Some(core), Some(config)) = (&runtime.singbox_path, &runtime.config_path) {
                    crate::config_files::run_singbox_check(core, config, Some(&runtime.base_dir))
                        .await?;
                }
                Some(runtime)
            } else {
                None
            };
            Settings::edit(dir, |p| {
                if action == "setup" && !get("workdir").trim().is_empty() {
                    p.working_dir = get("workdir").trim().into();
                }
                #[cfg(windows)]
                if let Some(r) = runtime {
                    p.working_dir = r.base_dir;
                    if let Some(core) = r.singbox_path {
                        p.singbox_path = core;
                    }
                    if let Some(config) = r.config_path {
                        let entry = p
                            .config_profiles
                            .iter()
                            .find(|v| v.kind == "local" && v.source == config)
                            .map(|v| v.id.clone())
                            .unwrap_or_else(|| {
                                let id = uuid::Uuid::new_v4().to_string();
                                p.config_profiles.push(crate::settings::ConfigProfile {
                                    id: id.clone(),
                                    source: config.clone(),
                                    kind: "local".into(),
                                    name: "默认配置".into(),
                                    ..Default::default()
                                });
                                id
                            });
                        p.config_path = config;
                        p.active_config_profile_id = entry;
                    }
                }
                let id = if action == "add_api" || get("id").is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    get("id").to_owned()
                };
                let endpoint = Endpoint {
                    id: id.clone(),
                    name: get("name").trim().into(),
                    url: url.clone(),
                    secret: get("secret").into(),
                };
                if action != "add_api" && !get("id").is_empty() {
                    *p.clash_apis
                        .iter_mut()
                        .find(|a| a.id == id)
                        .ok_or("后端已被删除")? = endpoint;
                } else {
                    p.clash_apis.push(endpoint);
                }
                p.active_clash_api_id = id;
                p.setup_completed = true;
                Ok(())
            })?;
            Ok(Reply::saved("后端已保存"))
        }
        #[cfg(windows)]
        "local_add" | "local_rename" | "local_remove" => {
            let id = get("profile");
            let name = get("name").trim();
            if action != "local_remove" && name.is_empty() {
                return Err("请输入配置名称".into());
            }
            let path = get("path").trim();
            if action == "local_add" {
                if !Path::new(path).is_absolute() {
                    return Err("请输入配置文件的绝对路径".into());
                }
                crate::profiles::validate_jsonc(
                    &std::fs::read_to_string(path).map_err(|e| e.to_string())?,
                )?;
            }
            Settings::edit(dir, |p| {
                if action == "local_add" {
                    if p.config_profiles
                        .iter()
                        .any(|v| v.kind == "local" && v.source == path)
                    {
                        return Err("此配置已经登记".into());
                    }
                    p.config_profiles.push(crate::settings::ConfigProfile {
                        id: uuid::Uuid::new_v4().to_string(),
                        source: path.into(),
                        kind: "local".into(),
                        name: name.into(),
                        ..Default::default()
                    });
                    return Ok(());
                }
                let i = p
                    .config_profiles
                    .iter()
                    .position(|v| v.kind == "local" && v.id == id)
                    .ok_or("请选择本地配置")?;
                if action == "local_remove" {
                    if p.active_config_profile_id == id {
                        return Err("请先切换活动配置".into());
                    }
                    p.config_profiles.remove(i);
                } else {
                    p.config_profiles[i].name = name.into();
                }
                Ok(())
            })?;
            Ok(Reply::saved("本地配置列表已更新"))
        }
        "switch_api" => {
            Settings::select_endpoint(dir, get("id"))?;
            Ok(Reply::saved("已切换后端"))
        }
        "delete_api" => {
            Settings::edit(dir, |p| {
                let i = p
                    .clash_apis
                    .iter()
                    .position(|a| a.id == get("id"))
                    .ok_or("没有选择后端")?;
                if p.clash_apis.len() == 1 {
                    return Err("需要保留一个 API 后端".into());
                }
                p.clash_apis.remove(i);
                if p.active_clash_api_id == get("id") {
                    p.active_clash_api_id = p.clash_apis[0].id.clone();
                }
                Ok(())
            })?;
            Ok(Reply::saved("已移除后端"))
        }
        "preferences" => {
            if cfg!(windows) && v.contains_key("autolaunch") {
                crate::settings::set_auto_launch(flag("autolaunch"))?;
            }
            Settings::edit(dir, |p| {
                p.start_page = get("start").into();
                p.theme = Some(get("theme").into());
                p.panel_auto_check_update = Some(flag("autocheck"));
                if cfg!(windows) {
                    p.close_to_tray = Some(flag("tray"));
                } else {
                    p.page_swipe = flag("swipe");
                }
                Ok(())
            })?;
            Ok(Reply::saved("启动偏好已保存"))
        }
        "test_settings" => {
            for key in ["test_url", "group_url"] {
                if !get(key).is_empty() {
                    http_url(get(key))?;
                }
            }
            if get("group").trim().is_empty() && !get("group_url").is_empty() {
                return Err("请填写代理组名称".into());
            }
            Settings::edit(dir, |p| {
                p.latency_test_url = get("test_url").trim().into();
                p.ipv6_test_enabled = flag("ipv6");
                let group = get("group").trim();
                if !group.is_empty() {
                    if get("group_url").is_empty() {
                        p.group_test_urls.remove(group);
                    } else {
                        p.group_test_urls
                            .insert(group.into(), get("group_url").trim().into());
                    }
                }
                Ok(())
            })?;
            Ok(Reply::saved("测速设置已保存"))
        }
        "site_tests" => {
            let results = futures_util::future::join_all(OVERVIEW_SITES.into_iter().map(
                |(_, label, url)| async move {
                    let result = network::http_ping(url.into(), 3)
                        .await
                        .map(|n| format!("{n:.0} ms"))
                        .unwrap_or_else(|e| e);
                    format!("{label}  {result}")
                },
            ))
            .await;
            Ok(Reply::text(results.join("\n")))
        }
        "geo" => Ok(Reply::text(geo(get("ip")).await?)),
        "ping" => {
            let url = get("test_url");
            http_url(url)?;
            Ok(Reply::text(format!(
                "{:.0} ms",
                network::http_ping(url.into(), 3).await?
            )))
        }
        "exit_ip" => {
            let (direct, proxy, proxy_v4, proxy_v6) = tokio::join!(
                ip_lookup::fetch_direct_ip(),
                ip_lookup::fetch_exit_geo_with_fallback(),
                ip_lookup::fetch_exit_ip_v4(),
                ip_lookup::fetch_exit_ip_v6(),
            );
            let direct_text = direct.map(|d| d.exit_line()).unwrap_or_else(|e| e);
            let proxy_text = proxy.map(|g| g.exit_line()).unwrap_or_else(|e| e);
            let v4_text = proxy_v4
                .map(|ip| ip.to_string())
                .unwrap_or_else(|e| format!("不可用（{e}）"));
            let v6_text = proxy_v6
                .map(|ip| ip.to_string())
                .unwrap_or_else(|e| format!("不可用（{e}）"));
            Ok(Reply::text(format!(
                "直连出口（IPIP） · {}\n代理出口（ip.sb） · {}\n代理 IPv4（ip.sb） · {}\n代理 IPv6（ip.sb） · {}",
                direct_text,
                proxy_text,
                v4_text,
                v6_text,
            )))
        }
        "dns" => {
            let server = match get("resolver") {
                "system" => None,
                "custom" if get("server").trim().is_empty() => {
                    return Err("请输入自定义解析器地址".into());
                }
                "custom" => Some(get("server").into()),
                s => Some(s.into()),
            };
            let answer = json!(
                network::lookup_dns(get("domain").into(), get("record").into(), server, servers)
                    .await?
            );
            let mut lines = vec![];
            for rec in answer["records"].as_array().into_iter().flatten() {
                let mut line = format!(
                    "{}  {}  {}  TTL {}",
                    rec["name"].as_str().unwrap_or(""),
                    rec["record_type"].as_str().unwrap_or(""),
                    rec["value"].as_str().unwrap_or(""),
                    rec["ttl"]
                );
                if flag("geo")
                    && let Some(ip) = rec["ip"].as_str()
                    && public_ip(ip)
                {
                    line.push_str(&format!("  {}", geo(ip).await.unwrap_or_else(|e| e)));
                }
                lines.push(line);
            }
            Ok(Reply::text(if lines.is_empty() {
                "没有匹配记录".into()
            } else {
                lines.join("\n")
            }))
        }
        "rules_find" | "rules_usage" => {
            let prefs = Settings::load(dir)?;
            let endpoint = prefs.active();
            let api = Clash::new(
                endpoint.map_or("http://127.0.0.1:9090", |e| e.url.as_str()),
                endpoint.map_or("", |e| e.secret.as_str()),
            )?;
            if action == "rules_usage" {
                let tag = get("tag").trim();
                if tag.is_empty() {
                    return Err("请选择提供商".into());
                }
                let data = api.execute(Command::Rules).await?;
                let rows = data["rules"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter(|(_, r)| {
                        r["payload"]
                            .as_str()
                            .is_some_and(|p| p.split([',', ' ']).any(|v| v.trim() == tag))
                    })
                    .map(|(i, r)| format!("{}  {}  →  {}", i + 1, r["payload"], r["route"]))
                    .collect::<Vec<_>>();
                return Ok(Reply::text(if rows.is_empty() {
                    "没有规则引用此提供商".into()
                } else {
                    rows.join("\n")
                }));
            }
            let query = get("query").trim().to_owned();
            if query.is_empty() {
                return Err("请输入域名或 IP".into());
            }
            let providers = api.execute(Command::RuleProviders).await?;
            let tags = providers["providers"]
                .as_object()
                .ok_or("核心返回的提供商无效")?
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            let work = get("workdir").to_owned();
            let path = get("config").to_owned();
            let core = get("core").to_owned();
            let text = tokio::task::spawn_blocking(move || {
                let mut found = Vec::new();
                let mut errors = Vec::new();
                for tag in tags {
                    match srs::srs_match_provider(
                        work.clone(),
                        path.clone(),
                        core.clone(),
                        tag.clone(),
                        query.clone(),
                    ) {
                        Ok(true) => found.push(tag),
                        Ok(false) => {}
                        Err(e) => errors.push(format!("{tag} · {e}")),
                    }
                }
                let mut text = format!("匹配 {} 个提供商\n{}", found.len(), found.join("\n"));
                if !errors.is_empty() {
                    text.push_str(&format!("\n读取失败\n{}", errors.join("\n")));
                }
                text
            })
            .await
            .map_err(|e| e.to_string())?;
            Ok(Reply::text(text))
        }
        "rules_list" | "rules_match" => {
            if get("tag").trim().is_empty() {
                return Err("请填写规则提供商名称".into());
            }
            let args = (
                get("workdir").to_owned(),
                get("config").to_owned(),
                get("core").to_owned(),
                get("tag").to_owned(),
            );
            let q = get("query").trim().to_lowercase();
            let page = get("page").parse::<usize>().unwrap_or(1).max(1);
            let matches = action == "rules_match";
            let text = tokio::task::spawn_blocking(move || {
                let (dir, config, core, tag) = args;
                if matches {
                    if q.is_empty() {
                        return Err("请填写域名或 IP".into());
                    }
                    return srs::srs_match_provider(dir, config, core, tag, q)
                        .map(|b| if b { "匹配" } else { "未匹配" }.to_owned());
                }
                let entries = srs::srs_list_provider(dir, config, core, tag)?;
                let mut rows = entries.iter().filter(|r| {
                    r.value.to_lowercase().contains(&q) || r.rule_type.to_lowercase().contains(&q)
                });
                let count = rows.clone().count();
                let mut shown = Vec::new();
                for r in rows
                    .by_ref()
                    .skip(page.saturating_sub(1).saturating_mul(200))
                    .take(200)
                {
                    shown.push(format!("{}  {}", r.rule_type, r.value));
                }
                Ok(format!(
                    "共 {count} 条，第 {page} 页，显示 {} 条\n{}",
                    shown.len(),
                    shown.join("\n")
                ))
            })
            .await
            .map_err(|e| e.to_string())??;
            Ok(Reply::text(text))
        }
        _ => Err("未知面板操作".into()),
    }
}
pub async fn readiness(prefs: &Settings) -> String {
    let api = prefs.active();
    let Ok(api) = Clash::new(
        api.map_or("http://127.0.0.1:9090", |e| e.url.as_str()),
        api.map_or("", |e| e.secret.as_str()),
    ) else {
        return "核心已启动，API 地址无效".into();
    };
    for attempt in 0..3 {
        if let Ok(Ok(info)) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            api.execute(Command::Version),
        )
        .await
        {
            return format!(
                "核心已启动，API 已连接 · {}",
                info["version"].as_str().unwrap_or("")
            );
        }
        if attempt < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        }
    }
    "核心启动操作已完成，当前 API 暂未就绪。请检查端点和密钥。".into()
}
pub const OVERVIEW_SITES: [(&str, &str, &str); 5] = [
    ("wechat", "微信", "https://res.wx.qq.com/favicon.ico"),
    ("bilibili", "Bilibili", "https://i0.hdslb.com/favicon.ico"),
    ("github", "GitHub", "https://github.com/favicon.ico"),
    ("cloudflare", "Cloudflare", "https://www.cloudflare.com/favicon.ico"),
    ("youtube", "YouTube", "https://yt3.ggpht.com/favicon.ico"),
];

pub fn mask_ip(ip: &str) -> String {
    ip.chars()
        .map(|c| if c.is_ascii_hexdigit() { '•' } else { c })
        .collect()
}

pub use crate::commands::ip_lookup::parse_ipipnet;

pub async fn network_overview() -> Value {
    let domestic_fut = async {
        match ip_lookup::fetch_direct_ip().await {
            Ok(info) => json!({"status":"done","ip":info.ip,"location":info.location,"error":""}),
            Err(e) => json!({"status":"error","ip":"","location":[],"error":e}),
        }
    };
    let abroad_fut = async {
        match ip_lookup::fetch_exit_geo_with_fallback().await {
            Ok(geo) => {
                let location = geo.overview_location();
                json!({"status":"done","ip":geo.ip,"location":location,"error":""})
            }
            Err(e) => json!({"status":"error","ip":"","location":[],"error":e}),
        }
    };
    let sites_fut = futures_util::future::join_all(OVERVIEW_SITES.into_iter().map(
        |(key, label, url)| async move {
            match network::http_ping(url.into(), 3).await {
                Ok(ms) => json!({"key":key,"label":label,"status":"done","ms":ms.round().max(1.) as u64,"error":""}),
                Err(e) => json!({"key":key,"label":label,"status":"error","ms":0,"error":e}),
            }
        },
    ));
    let (domestic, abroad, sites) = tokio::join!(domestic_fut, abroad_fut, sites_fut);
    json!({"domestic": domestic, "abroad": abroad, "sites": sites})
}

pub fn public_ip(ip: &str) -> bool {
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(addr)) => {
            let [a, b, _, _] = addr.octets();
            !matches!((a, b), (0 | 10 | 127 | 224..=255, _) | (100, 64..=127) | (169, 254)
                | (172, 16..=31) | (192, 168) | (198, 18..=19))
        }
        Ok(std::net::IpAddr::V6(addr)) => addr.to_ipv4_mapped().map_or_else(
            || !addr.is_unspecified() && !addr.is_loopback() && addr.octets()[0] < 0xfc,
            |v| public_ip(&v.to_string()),
        ),
        Err(_) => false,
    }
}

pub async fn geo_details(ip: &str) -> Result<Value, String> {
    let geo = ip_lookup::lookup_geo(ip).await?;
    serde_json::to_value(geo).map_err(|e| e.to_string())
}

pub async fn geo(ip: &str) -> Result<String, String> {
    Ok(ip_lookup::lookup_geo(ip).await?.summary())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skipped_setup_stays_completed_after_reopening_preferences() {
        crate::runtime().block_on(async {
            let dir = tempfile::tempdir().unwrap();
            run(dir.path(), "skip_setup", &Values::new(), &[]).await.unwrap();
            assert!(Settings::load(dir.path()).unwrap().setup_completed);
        });
    }
    fn values(input: &[(&str, &str)]) -> Values {
        input
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    #[test]
    fn endpoint_fields_accept_ipv6_and_pasted_urls() {
        let mut v = values(&[
            ("protocol", "https"),
            ("host", "[::1]/api"),
            ("port", "9443"),
        ]);
        assert_eq!(endpoint_url(&v).unwrap(), "https://[::1]:9443/api");
        v.insert("port".into(), "65536".into());
        assert!(endpoint_url(&v).is_err());
        v.insert("host".into(), "http://localhost:9090".into());
        assert_eq!(endpoint_url(&v).unwrap(), "http://localhost:9090/");
    }

    #[test]
    fn update_preferences_preserve_the_existing_configuration_on_error() {
        crate::runtime().block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let mut v = values(&[
                ("source", "custom"),
                ("repo", "example/core"),
                ("channel", "stable"),
                ("mirror", "https://example.org"),
            ]);
            run(dir.path(), "update_preferences", &v, &[])
                .await
                .unwrap();
            v.insert("channel".into(), "testing".into());
            v.insert("mirror".into(), "invalid".into());
            assert!(
                run(dir.path(), "update_preferences", &v, &[])
                    .await
                    .is_err()
            );
            let p = Settings::load(dir.path()).unwrap();
            assert_eq!(p.core_update_source, "custom");
            assert_eq!(p.core_update_custom_repo, "example/core");
            assert_eq!(p.core_update_channel, "stable");
            assert_eq!(p.core_update_mirror, "https://example.org");
        });
    }

    #[test]
    fn endpoint_edits_follow_the_selected_backend_and_keep_the_last_one() {
        crate::runtime().block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let api = values(&[
                ("name", "路由器"),
                ("url", "http://127.0.0.1:9090"),
                ("secret", "old"),
            ]);
            run(dir.path(), "save_api", &api, &[]).await.unwrap();
            let old = Settings::load(dir.path()).unwrap().active_clash_api_id;
            let mut second = api.clone();
            second.insert("secret".into(), "new".into());
            run(dir.path(), "add_api", &second, &[]).await.unwrap();
            assert_eq!(
                Settings::load(dir.path()).unwrap().active().unwrap().secret,
                "new"
            );
            let values = values(&[("id", &old)]);
            run(dir.path(), "switch_api", &values, &[]).await.unwrap();
            assert_eq!(
                Settings::load(dir.path()).unwrap().active().unwrap().secret,
                "old"
            );
            run(dir.path(), "delete_api", &values, &[]).await.unwrap();
            let id = Settings::load(dir.path()).unwrap().active_clash_api_id;
            assert!(
                run(
                    dir.path(),
                    "delete_api",
                    &BTreeMap::from([("id".into(), id)]),
                    &[]
                )
                .await
                .is_err()
            );
        })
    }
    #[test]
    fn query_a_record_through_a_custom_local_resolver() {
        crate::runtime().block_on(async {
            let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let server = socket.local_addr().unwrap().to_string();
            let responder = tokio::spawn(async move {
                let mut bytes = [0u8; 512];
                let (n, peer) = socket.recv_from(&mut bytes).await.unwrap();
                let end = 12 + bytes[12..n].iter().position(|b| *b == 0).unwrap() + 5;
                let mut answer = bytes[..end].to_vec();
                answer[2..12].copy_from_slice(&[0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
                answer
                    .extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 192, 0, 2, 42]);
                socket.send_to(&answer, peer).await.unwrap();
            });
            let input = values(&[
                ("domain", "sample.test"),
                ("record", "A"),
                ("resolver", "custom"),
                ("server", &server),
            ]);
            let dir = tempfile::tempdir().unwrap();
            let reply = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                run(dir.path(), "dns", &input, &[]),
            )
            .await
            .unwrap()
            .unwrap();
            responder.await.unwrap();
            assert!(reply.text.contains("192.0.2.42") && reply.text.contains("TTL 60"));
        })
    }
    #[test]
    fn parses_ipipnet_and_masks_ips() {
        let (ip, loc) = parse_ipipnet("当前 IP：1.2.3.4  来自于：中国 广东 深圳  电信");
        assert_eq!(ip, "1.2.3.4");
        assert_eq!(loc, vec!["中国", "广东", "深圳", "电信"]);
        assert_eq!(mask_ip("1.2.3.4"), "•.•.•.•");
        assert_eq!(mask_ip("2001:db8::1"), "••••:•••::•");
    }
}
