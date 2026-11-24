mod com;
mod popup;
mod registry;

pub use com::ResidentHost;
pub use registry::{DELEGATE_FLAG, SEND_FLAG, is_registered, repair, set_registered};

use serde_json::Value;
use singboard_core::service_api::{Command as ApiCommand, Feed as ApiFeed, ServiceApi};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use windows_sys::Win32::Foundation::POINT;

#[derive(Clone)]
pub struct ServiceCandidate {
    pub service_tag: String,
    pub url: String,
}

#[derive(Clone)]
pub struct PeerItem {
    pub stable_id: String,
    pub display_name: String,
    pub os: String,
    pub primary_ip: String,
    pub online: bool,
    pub is_last: bool,
}

#[derive(Clone)]
pub struct EndpointPeers {
    pub endpoint_tag: String,
    pub peers: Vec<PeerItem>,
}

pub struct LoadResult {
    pub services: Vec<ServiceCandidate>,
    pub selected_service: Option<String>,
    pub endpoints: Vec<EndpointPeers>,
    pub error: String,
}

pub fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0))
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else if b >= 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn peer_display_name(peer: &Value) -> String {
    let host = peer["hostName"].as_str().unwrap_or_default().trim();
    let host_ok = !host.is_empty() && host != "device-of-shared-to-user";
    let dns = peer["dnsName"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('.');
    if let Some(label) = dns.split('.').next().map(str::trim)
        && !label.is_empty()
        && label != "device-of-shared-to-user"
    {
        if host_ok && host.eq_ignore_ascii_case(label) {
            return host.to_owned();
        }
        return label.to_owned();
    }
    if host_ok {
        return host.to_owned();
    }
    if host == "device-of-shared-to-user" {
        return "共享设备".into();
    }
    peer["stableId"].as_str().unwrap_or("未命名").to_owned()
}

fn parse_endpoints(snap: &Value, last_peer: Option<&str>) -> Vec<EndpointPeers> {
    let mut result = Vec::new();
    let Some(eps) = snap["endpoints"].as_array() else {
        return result;
    };
    for ep in eps {
        if ep["canShareFiles"].as_bool() != Some(true) {
            continue;
        }
        let network = ep["networkName"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();
        let mut groups: Vec<&Value> = ep["userGroups"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        groups.sort_by_key(|g| {
            let login = g["loginName"].as_str().unwrap_or_default().to_lowercase();
            if !network.is_empty() && login == network {
                0
            } else {
                let has_own = g["peers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|p| p["shareeNode"].as_bool() != Some(true));
                if has_own { 1 } else { 2 }
            }
        });

        let mut online = Vec::new();
        let mut offline = Vec::new();
        for g in groups {
            let Some(raw_peers) = g["peers"].as_array() else {
                continue;
            };
            for p in raw_peers {
                if p["canReceiveFiles"].as_bool() != Some(true) {
                    continue;
                }
                let stable_id = p["stableId"].as_str().unwrap_or_default().trim();
                if stable_id.is_empty() {
                    continue;
                }
                let primary_ip = p["tailscaleIps"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let is_online = p["online"].as_bool() == Some(true);
                let item = PeerItem {
                    stable_id: stable_id.to_owned(),
                    display_name: peer_display_name(p),
                    os: p["os"].as_str().unwrap_or_default().to_owned(),
                    primary_ip,
                    online: is_online,
                    is_last: last_peer == Some(stable_id),
                };
                if is_online {
                    online.push(item);
                } else {
                    offline.push(item);
                }
            }
        }
        if let Some(pos) = online.iter().position(|p| p.is_last) {
            let item = online.remove(pos);
            online.insert(0, item);
        }
        let mut peers = online;
        peers.extend(offline);
        result.push(EndpointPeers {
            endpoint_tag: ep["endpointTag"].as_str().unwrap_or_default().to_owned(),
            peers,
        });
    }
    result
}

pub fn load_tailscale_targets(dir: PathBuf, requested_service: Option<String>) -> LoadResult {
    let last_peer = registry::last_peer_id();
    singboard_core::runtime().block_on(async move {
        let mut api = ServiceApi::new(dir);
        let discovered = match api.execute(ApiCommand::Discover).await {
            Ok(v) => v
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|item| ServiceCandidate {
                            service_tag: item["serviceTag"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            url: item["url"].as_str().unwrap_or_default().to_owned(),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            Err(e) => {
                return LoadResult {
                    services: Vec::new(),
                    selected_service: None,
                    endpoints: Vec::new(),
                    error: e.message,
                };
            }
        };
        if discovered.is_empty() {
            return LoadResult {
                services: Vec::new(),
                selected_service: None,
                endpoints: Vec::new(),
                error: "尚未配置 Tailscale Service API".into(),
            };
        }
        let chosen = match requested_service {
            Some(ref tag) if discovered.iter().any(|s| s.service_tag == *tag) => Some(tag.clone()),
            _ if discovered.len() == 1 => Some(discovered[0].service_tag.clone()),
            _ => None,
        };
        if chosen.is_none() {
            return LoadResult {
                services: discovered,
                selected_service: None,
                endpoints: Vec::new(),
                error: String::new(),
            };
        }
        if let Err(e) = api
            .execute(ApiCommand::Connect {
                service_tag: chosen.clone().filter(|s| !s.is_empty()),
            })
            .await
        {
            return LoadResult {
                services: discovered,
                selected_service: chosen,
                endpoints: Vec::new(),
                error: e.message,
            };
        }
        match api.execute(ApiCommand::Status).await {
            Ok(snap) => LoadResult {
                services: discovered,
                selected_service: chosen,
                endpoints: parse_endpoints(&snap, last_peer.as_deref()),
                error: String::new(),
            },
            Err(e) => LoadResult {
                services: discovered,
                selected_service: chosen,
                endpoints: Vec::new(),
                error: e.message,
            },
        }
    })
}

fn upload_files(
    dir: PathBuf,
    target: &popup::SelectedTarget,
    paths: Vec<PathBuf>,
) -> Result<(), String> {
    let service_tag = target.service_tag.clone().filter(|s| !s.is_empty());
    let endpoint_tag = Some(target.endpoint_tag.clone()).filter(|s| !s.is_empty());
    let peer_stable_id = target.peer.stable_id.clone();
    singboard_core::runtime().block_on(async move {
        let mut api = ServiceApi::new(dir);
        api.execute(ApiCommand::Connect { service_tag })
            .await
            .map_err(|e| e.message)?;
        let opened = api
            .execute(ApiCommand::Open {
                feed: ApiFeed::Upload {
                    tag: endpoint_tag,
                    peer_stable_id,
                    paths,
                },
            })
            .await
            .map_err(|e| e.message)?;
        let handle = opened
            .as_u64()
            .ok_or_else(|| "无法创建 Taildrop 上传会话".to_owned())?;
        let outcome = loop {
            let batch = api
                .execute(ApiCommand::Drain { handle })
                .await
                .map_err(|e| e.message)?;
            if let Some(events) = batch["events"].as_array()
                && let Some(err) = events
                    .iter()
                    .find_map(|ev| ev["error"]["message"].as_str())
            {
                break Err(err.to_owned());
            }
            if batch["finished"].as_bool() == Some(true) {
                break Ok(());
            }
            tokio::time::sleep(Duration::from_millis(140)).await;
        };
        let _ = api.execute(ApiCommand::Close { handle }).await;
        let _ = api.execute(ApiCommand::Disconnect).await;
        outcome
    })
}

fn run_with_paths(paths: Vec<PathBuf>, anchor: Option<POINT>) -> Result<(), String> {
    let files: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_file()).collect();
    if files.is_empty() {
        return Ok(());
    }
    let dir = crate::service::data_dir()?;
    let Some(target) = popup::pick_target(dir.clone(), &files, anchor)? else {
        return Ok(());
    };
    registry::save_last_peer_id(&target.peer.stable_id);

    let count = files.len();
    let peer_name = target.peer.display_name.clone();
    let mut notifier = popup::BalloonNotifier::new();

    match upload_files(dir, &target, files) {
        Ok(()) => {
            notifier.notify(
                "Taildrop 发送成功",
                &format!("已发送 {count} 个文件到 {peer_name}"),
                false,
            );
            notifier.flush_and_close(1200);
        }
        Err(err) => {
            notifier.notify("Taildrop 发送失败", &err, true);
            notifier.flush_and_close(2000);
        }
    }
    Ok(())
}

pub fn start_host() -> Option<ResidentHost> {
    com::ResidentHost::start(|payload| {
        std::thread::spawn(move || {
            registry::repair();
            let _ = run_with_paths(payload.paths, payload.position);
        });
    })
}

pub fn run_delegate() -> Result<(), String> {
    registry::repair();
    let Some(payload) = com::receive_delegate_payload()? else {
        return Ok(());
    };
    run_with_paths(payload.paths, payload.position)
}

pub fn run_send(args: &[OsString]) -> Result<(), String> {
    let paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    run_with_paths(paths, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_own_online_peers_and_promotes_last_peer() {
        let snap = json!({
            "endpoints": [{
                "endpointTag": "default-TS",
                "networkName": "me@example.com",
                "canShareFiles": true,
                "userGroups": [
                    {
                        "loginName": "other@example.com",
                        "peers": [{
                            "stableId": "shared-1",
                            "hostName": "device-of-shared-to-user",
                            "dnsName": "friend-pc.ts.net.",
                            "canReceiveFiles": true,
                            "shareeNode": true,
                            "online": true,
                            "os": "linux",
                            "tailscaleIps": ["100.64.0.9"]
                        }]
                    },
                    {
                        "loginName": "me@example.com",
                        "peers": [
                            {
                                "stableId": "offline-1",
                                "hostName": "old-laptop",
                                "canReceiveFiles": true,
                                "online": false,
                                "os": "windows",
                                "tailscaleIps": ["100.64.0.2"]
                            },
                            {
                                "stableId": "online-a",
                                "hostName": "yuu-hk",
                                "canReceiveFiles": true,
                                "online": true,
                                "os": "linux",
                                "tailscaleIps": ["100.64.0.3"]
                            },
                            {
                                "stableId": "online-b",
                                "hostName": "yuu-mi17",
                                "canReceiveFiles": true,
                                "online": true,
                                "os": "android",
                                "tailscaleIps": ["100.64.0.4"]
                            }
                        ]
                    }
                ]
            }]
        });

        let eps = parse_endpoints(&snap, Some("online-b"));
        assert_eq!(eps.len(), 1);
        let ids: Vec<&str> = eps[0].peers.iter().map(|p| p.stable_id.as_str()).collect();
        assert_eq!(ids, vec!["online-b", "online-a", "shared-1", "offline-1"]);
        assert!(eps[0].peers[0].is_last);
        assert_eq!(eps[0].peers[2].display_name, "friend-pc");
    }

    #[test]
    fn peer_display_name_prefers_dns_label_and_preserves_matching_host_case() {
        assert_eq!(
            peer_display_name(&json!({
                "hostName": "DESKTOP-ABC",
                "dnsName": "yuu-desktop.tailnet.ts.net."
            })),
            "yuu-desktop"
        );
        assert_eq!(
            peer_display_name(&json!({
                "hostName": "JP-Oracle-Yuu",
                "dnsName": "jp-oracle-yuu.tailnet.ts.net."
            })),
            "JP-Oracle-Yuu"
        );
        assert_eq!(
            peer_display_name(&json!({
                "hostName": "local-box"
            })),
            "local-box"
        );
        assert_eq!(
            peer_display_name(&json!({
                "hostName": "device-of-shared-to-user"
            })),
            "共享设备"
        );
    }
}
