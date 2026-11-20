use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

#[derive(Clone)]
pub struct Clash {
    base: Url,
    secret: String,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    Proxies,
    Select {
        group: String,
        name: String,
    },
    Delay {
        name: String,
        url: String,
        #[serde(default)]
        ipv6: bool,
    },
    GroupDelay {
        name: String,
        url: String,
        #[serde(default)]
        ipv6: bool,
    },
    Rules,
    Disconnect {
        id: String,
    },
    DisconnectAll,
    DisconnectMany {
        ids: Vec<String>,
    },
    UpdateProviders {
        proxies: bool,
        names: Vec<String>,
    },
    Summary,
    RawProxies,
    RawRules,
    ProxyProviders,
    RuleProviders,
    UpdateProxyProvider {
        name: String,
    },
    UpdateRuleProvider {
        name: String,
    },
    HealthCheck {
        name: String,
    },
    Version,
    Config,
    SetMode {
        mode: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub kind: String,
    pub delay: Option<u64>,
    #[serde(default)]
    pub udp: bool,
    #[serde(default)]
    pub xudp: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub kind: String,
    pub now: String,
    pub nodes: Vec<Node>,
    pub test_url: String,
}

fn groups(response: Value) -> Result<Value, String> {
    let proxies = response["proxies"]
        .as_object()
        .ok_or("核心返回的代理列表无效")?;
    let mut groups = Vec::new();
    for (name, proxy) in proxies {
        if name == "GLOBAL" {
            continue;
        }
        let Some(members) = proxy["all"].as_array() else {
            continue;
        };
        let nodes = members
            .iter()
            .filter_map(Value::as_str)
            .map(|name| {
                let entry = proxies.get(name);
                let delay = entry
                    .and_then(|v| v["history"].as_array())
                    .and_then(|items| items.last())
                    .and_then(|item| item["delay"].as_u64());
                Node {
                    name: name.into(),
                    kind: entry
                        .and_then(|p| p["type"].as_str())
                        .unwrap_or_default()
                        .into(),
                    delay,
                    udp: entry.and_then(|p| p["udp"].as_bool()).unwrap_or(false),
                    xudp: entry.and_then(|p| p["xudp"].as_bool()).unwrap_or(false),
                }
            })
            .collect();
        groups.push(Group {
            name: name.clone(),
            kind: proxy["type"].as_str().unwrap_or_default().into(),
            now: proxy["now"].as_str().unwrap_or_default().into(),
            test_url: proxy["testUrl"]
                .as_str()
                .filter(|url| !url.is_empty())
                .unwrap_or("https://www.gstatic.com/generate_204")
                .into(),
            nodes,
        });
    }
    if let Some(order) = proxies.get("GLOBAL").and_then(|p| p["all"].as_array()) {
        let positions: std::collections::HashMap<_, _> = order
            .iter()
            .enumerate()
            .filter_map(|(i, name)| name.as_str().map(|s| (s, i)))
            .collect();
        groups.sort_by_key(|group| {
            positions
                .get(group.name.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
    }
    Ok(json!(groups))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_order_and_history_follow_the_core() {
        let result = groups(json!({"proxies":{
            "A":{"all":["direct"],"type":"Selector","now":"direct"},
            "B":{"all":["direct"],"type":"URLTest","now":"direct"},
            "direct":{"type":"Direct","history":[{"delay":100},{"delay":27}]},
            "GLOBAL":{"all":["B","A"],"type":"Selector"}
        }}))
        .unwrap();
        let rows: Vec<Group> = serde_json::from_value(result).unwrap();
        assert_eq!(
            rows.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            ["B", "A"]
        );
        assert_eq!(
            (rows[1].nodes[0].kind.as_str(), rows[1].nodes[0].delay),
            ("Direct", Some(27))
        );
    }

    #[test]
    fn rest_and_stream_preserve_the_endpoint_prefix() {
        let api = Clash::new("wss://example.org/clash/panel/", "a&b").unwrap();
        assert_eq!(
            api.url(&["proxies", "a/b?#"]).unwrap().as_str(),
            "https://example.org/clash/panel/proxies/a%2Fb%3F%23"
        );
        assert_eq!(
            api.socket("logs").unwrap().as_str(),
            "wss://example.org/clash/panel/logs?token=a%26b"
        );
        assert!(api.url(&["proxies", ".."]).is_err());
    }
}

impl Clash {
    pub fn new(base: &str, secret: &str) -> Result<Self, String> {
        let mut base = Url::parse(base.trim()).map_err(|_| "端点地址无效")?;
        let scheme = match base.scheme() {
            "https" | "wss" => "https",
            "http" | "ws" => "http",
            _ => return Err("端点需要 HTTP 或 HTTPS 地址".into()),
        };
        if base.host_str().is_none() || !base.username().is_empty() || base.password().is_some() {
            return Err("端点地址无效".into());
        }
        base.set_scheme(scheme).map_err(|_| "端点协议无效")?;
        base.set_query(None);
        base.set_fragment(None);
        Ok(Self {
            base,
            secret: secret.into(),
        })
    }

    fn url(&self, parts: &[&str]) -> Result<Url, String> {
        let mut url = self.base.clone();
        if parts.iter().any(|p| matches!(*p, "" | "." | "..")) {
            return Err("资源名称无效".into());
        }
        url.path_segments_mut()
            .map_err(|_| "端点路径无效")?
            .pop_if_empty()
            .extend(parts);
        Ok(url)
    }

    pub(crate) fn socket(&self, name: &str) -> Result<Url, String> {
        let mut url = self.url(&[name])?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).map_err(|_| "端点协议无效")?;
        if !self.secret.is_empty() {
            url.query_pairs_mut().append_pair("token", &self.secret);
        }
        Ok(url)
    }

    pub async fn execute(&self, command: Command) -> Result<Value, String> {
        use Command::*;
        const V6: &str = "https://ipv6.google.com/generate_204";
        match &command {
            Delay {
                name,
                url,
                ipv6: true,
            } => {
                let (data, v6) = tokio::join!(
                    Box::pin(self.execute(Delay {
                        name: name.clone(),
                        url: url.clone(),
                        ipv6: false
                    })),
                    Box::pin(self.execute(Delay {
                        name: name.clone(),
                        url: V6.into(),
                        ipv6: false
                    }))
                );
                let mut data = data?;
                data["ipv6"] = v6
                    .ok()
                    .and_then(|d| d["delay"].as_u64())
                    .is_some_and(|d| d > 0)
                    .into();
                return Ok(data);
            }
            GroupDelay {
                name,
                url,
                ipv6: true,
            } => {
                let (data, v6) = tokio::join!(
                    Box::pin(self.execute(GroupDelay {
                        name: name.clone(),
                        url: url.clone(),
                        ipv6: false
                    })),
                    Box::pin(self.execute(GroupDelay {
                        name: name.clone(),
                        url: V6.into(),
                        ipv6: false
                    }))
                );
                return Ok(json!({"latencies":data?,"ipv6":v6.unwrap_or_default()}));
            }
            _ => {}
        }
        let (method, path) = match &command {
            Summary => {
                let (version, config, proxies) = tokio::join!(
                    Box::pin(self.execute(Version)),
                    Box::pin(self.execute(Config)),
                    Box::pin(self.execute(RawProxies))
                );
                let config = config?;
                let proxies_val = proxies.unwrap_or_default();
                let mode = config["mode"].as_str().unwrap_or("");
                return Ok(json!({
                    "version": version.unwrap_or_default()["version"],
                    "config": config,
                    "route": crate::dashboard::route_summary(&proxies_val),
                    "exit": crate::dashboard::resolve_exit(&proxies_val, mode),
                }));
            }
            UpdateProviders { proxies, names } => {
                let mut failures = Vec::new();
                for name in names {
                    let op = if *proxies {
                        UpdateProxyProvider { name: name.clone() }
                    } else {
                        UpdateRuleProvider { name: name.clone() }
                    };
                    if let Err(e) = Box::pin(self.execute(op)).await {
                        failures.push(format!("{name} · {e}"))
                    }
                }
                return if failures.is_empty() {
                    Ok(Value::Null)
                } else {
                    Err(failures.join("\n"))
                };
            }
            DisconnectMany { ids } => {
                let mut errors = Vec::new();
                for id in ids {
                    if let Err(e) = Box::pin(self.execute(Disconnect { id: id.clone() })).await {
                        errors.push(e)
                    }
                }
                return if errors.is_empty() {
                    Ok(Value::Null)
                } else {
                    Err(errors.join("\n"))
                };
            }
            RawProxies | Proxies => (Method::GET, vec!["proxies"]),
            Select { group, .. } => (Method::PUT, vec!["proxies", group.as_str()]),
            Delay { name, .. } => (Method::GET, vec!["proxies", name, "delay"]),
            GroupDelay { name, .. } => (Method::GET, vec!["group", name, "delay"]),
            Rules => {
                let (rules, proxies) = tokio::join!(
                    Box::pin(self.execute(RawRules)),
                    Box::pin(self.execute(RawProxies))
                );
                let mut data = rules?;
                let proxies = proxies.unwrap_or_default();
                if let Some(rows) = data["rules"].as_array_mut() {
                    for row in rows {
                        let raw_proxy = row["proxy"].as_str().unwrap_or("");
                        let target = raw_proxy
                            .strip_prefix("route(")
                            .and_then(|s| s.strip_suffix(')'))
                            .and_then(|s| s.split(',').next())
                            .map(str::trim)
                            .unwrap_or(raw_proxy);
                        let chain_vec =
                            crate::dashboard::chain_list(&proxies["proxies"], target);
                        let route = chain_vec.join(" → ");
                        row["chain"] = json!(chain_vec);
                        row["route"] = route.into();
                        if row["type"].as_str().is_some_and(|s| {
                            matches!(s.to_ascii_lowercase().as_str(), "match" | "final")
                        }) {
                            row["condition"] = "最终回退".into();
                        }
                    }
                }
                return Ok(data);
            }
            RawRules => (Method::GET, vec!["rules"]),
            Disconnect { id } => (Method::DELETE, vec!["connections", id]),
            DisconnectAll => (Method::DELETE, vec!["connections"]),
            ProxyProviders => (Method::GET, vec!["providers", "proxies"]),
            RuleProviders => (Method::GET, vec!["providers", "rules"]),
            UpdateProxyProvider { name } => (Method::PUT, vec!["providers", "proxies", name]),
            UpdateRuleProvider { name } => (Method::PUT, vec!["providers", "rules", name]),
            HealthCheck { name } => (
                Method::GET,
                vec!["providers", "proxies", name, "healthcheck"],
            ),
            Version => (Method::GET, vec!["version"]),
            Config => (Method::GET, vec!["configs"]),
            SetMode { .. } => (Method::PATCH, vec!["configs"]),
        };
        static CLIENT: OnceLock<Client> = OnceLock::new();
        let client = CLIENT.get_or_init(|| {
            Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("初始化 HTTP 客户端")
        });
        let mut request = client.request(method, self.url(&path)?);
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        let seconds = match &command {
            UpdateRuleProvider { .. } | UpdateProxyProvider { .. } => 120,
            HealthCheck { .. } => 30,
            _ => 15,
        };
        request = match &command {
            Select { name, .. } => request.json(&json!({"name":name})),
            SetMode { mode } => request.json(&json!({"mode":mode})),
            Delay { url, .. } | GroupDelay { url, .. } => {
                request.query(&[("url", url.as_str()), ("timeout", "5000")])
            }
            _ => request,
        };
        let response = request
            .timeout(Duration::from_secs(seconds))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "核心响应超时"
                } else {
                    "无法连接 Clash API"
                }
            })?;
        let status = response.status();
        if matches!(status.as_u16(), 401 | 403) {
            return Err("认证失败，请检查密钥".into());
        }
        let bytes = response.bytes().await.map_err(|_| "读取核心响应失败")?;
        if !status.is_success() {
            let data = serde_json::from_slice::<Value>(&bytes).unwrap_or_default();
            return Err(format!(
                "HTTP {} · {}",
                status.as_u16(),
                data["message"].as_str().unwrap_or("核心拒绝了请求")
            ));
        }
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| "核心返回的数据无效")?
        };
        if matches!(command, Proxies) {
            groups(value)
        } else {
            Ok(value)
        }
    }
}
