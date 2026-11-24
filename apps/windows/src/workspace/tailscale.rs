use super::*;
use crate::glass::{
    adaptive_pill_layer, adaptive_selection_chip, backdrop_heading, card_layer, glass_input,
};
use crate::theme::Palette;
use gpui_component::input::{InputEvent, InputState};
use singboard_core::service_api::{Command as ApiCommand, Feed as ApiFeed, ServiceApi};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum TailscaleTab {
    #[default]
    Devices,
    Taildrop,
    Ssh,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum PeerFilter {
    #[default]
    All,
    Online,
    Exit,
    Ssh,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StatusTone {
    Ok,
    Warn,
    Error,
    Muted,
}

#[derive(Clone, Default)]
pub(super) enum ServiceApiStatus {
    #[default]
    NotConfigured,
    Connecting,
    Connected {
        url: String,
        auto_discovered: bool,
    },
    SelectService(Vec<(String, String)>),
    Error,
}

#[derive(Clone)]
pub(super) struct PingSample {
    pub latency_ms: f64,
    pub is_direct: bool,
    pub endpoint: String,
    pub derp_region_id: i64,
    pub derp_region_code: String,
    pub peer_relay: String,
    pub error: String,
}

#[derive(Clone)]
pub(super) struct UploadProgress {
    pub target_name: String,
    pub file_count: usize,
    pub completed_files: usize,
    pub sent_bytes: i64,
    pub total_bytes: i64,
}

#[derive(Clone)]
pub(super) struct DownloadProgress {
    pub name: String,
    pub received_bytes: i64,
    pub total_bytes: i64,
}

pub(super) enum TailscaleDialog {
    Certificate {
        tag: String,
        domains: Vec<String>,
        selected_domain: String,
        min_validity_days: u32,
        loading: bool,
        cert_pem: String,
        key_pem: String,
        reveal_key: bool,
        error: String,
    },
}

pub(super) struct TailscaleState {
    api: Option<Arc<Mutex<ServiceApi>>>,
    generation: u64,
    settings_rev: u64,
    last_retry: Option<std::time::Instant>,
    pub(super) status: ServiceApiStatus,
    pub(super) endpoints: Vec<Value>,
    pub(super) selected_endpoint: Option<String>,
    pub(super) selected_service_tag: Option<String>,
    pub(super) tab: TailscaleTab,
    pub(super) filter: PeerFilter,
    pub(super) search_input: Option<Entity<InputState>>,
    pub(super) search_query: String,
    pub(super) selected_peer: Option<String>,
    pub(super) collapsed_groups: HashSet<String>,
    status_handle: Option<u64>,
    inbox_handle: Option<u64>,
    inbox_tag: Option<String>,
    pub(super) inbox: Option<Value>,
    ping_handle: Option<u64>,
    ping_peer_ip: Option<String>,
    pub(super) ping_samples: Vec<PingSample>,
    pub(super) upload_target: Option<String>,
    upload_opening: bool,
    upload_handle: Option<u64>,
    pub(super) upload_progress: Option<UploadProgress>,
    download_handle: Option<u64>,
    pub(super) download_progress: Option<DownloadProgress>,
    pub(super) ssh_peer: Option<String>,
    pub(super) ssh_user_input: Option<Entity<InputState>>,
    pub(super) ssh_cmd_input: Option<Entity<InputState>>,
    ssh_handle: Option<u64>,
    pub(super) ssh_ready: bool,
    pub(super) ssh_banner: String,
    pub(super) ssh_output: String,
    pub(super) ssh_status: String,
    pub(super) dialog: Option<TailscaleDialog>,
    draining: bool,
}

impl Default for TailscaleState {
    fn default() -> Self {
        Self {
            api: None,
            generation: 0,
            settings_rev: u64::MAX,
            last_retry: None,
            status: ServiceApiStatus::NotConfigured,
            endpoints: Vec::new(),
            selected_endpoint: None,
            selected_service_tag: None,
            tab: TailscaleTab::Devices,
            filter: PeerFilter::All,
            search_input: None,
            search_query: String::new(),
            selected_peer: None,
            collapsed_groups: HashSet::new(),
            status_handle: None,
            inbox_handle: None,
            inbox_tag: None,
            inbox: None,
            ping_handle: None,
            ping_peer_ip: None,
            ping_samples: Vec::new(),
            upload_target: None,
            upload_opening: false,
            upload_handle: None,
            upload_progress: None,
            download_handle: None,
            download_progress: None,
            ssh_peer: None,
            ssh_user_input: None,
            ssh_cmd_input: None,
            ssh_handle: None,
            ssh_ready: false,
            ssh_banner: String::new(),
            ssh_output: String::new(),
            ssh_status: String::new(),
            dialog: None,
            draining: false,
        }
    }
}

impl TailscaleState {
    pub(super) fn with_collapsed_groups(groups: impl IntoIterator<Item = String>) -> Self {
        Self {
            collapsed_groups: groups.into_iter().collect(),
            ..Default::default()
        }
    }

    fn ensure_api(&mut self) -> Option<Arc<Mutex<ServiceApi>>> {
        if self.api.is_none()
            && let Ok(dir) = crate::service::data_dir()
        {
            self.api = Some(Arc::new(Mutex::new(ServiceApi::new(dir))));
        }
        self.api.clone()
    }

    pub(super) fn has_endpoints(&self) -> bool {
        !self.endpoints.is_empty()
    }

    pub(super) fn status_chip(&self) -> (String, StatusTone) {
        match &self.status {
            ServiceApiStatus::NotConfigured => ("未配置".into(), StatusTone::Muted),
            ServiceApiStatus::Connecting => ("连接中".into(), StatusTone::Warn),
            ServiceApiStatus::Connected {
                url,
                auto_discovered,
                ..
            } => {
                let host = display_host(url);
                if *auto_discovered {
                    (format!("{host} (自动)"), StatusTone::Ok)
                } else {
                    (host, StatusTone::Ok)
                }
            }
            ServiceApiStatus::SelectService(list) => {
                (format!("发现 {} 个服务", list.len()), StatusTone::Warn)
            }
            ServiceApiStatus::Error => ("不可达".into(), StatusTone::Error),
        }
    }

    fn current_endpoint(&self) -> Option<&Value> {
        if let Some(tag) = &self.selected_endpoint
            && let Some(found) = self
                .endpoints
                .iter()
                .find(|ep| ep["endpointTag"].as_str() == Some(tag.as_str()))
        {
            return Some(found);
        }
        self.endpoints.first()
    }

    fn current_endpoint_tag(&self) -> Option<String> {
        self.current_endpoint()
            .and_then(|ep| ep["endpointTag"].as_str())
            .map(str::to_owned)
    }

    fn clear_streams(&mut self) {
        self.status_handle = None;
        self.inbox_handle = None;
        self.inbox_tag = None;
        self.inbox = None;
        self.ping_handle = None;
        self.ping_peer_ip = None;
        self.ping_samples.clear();
        self.upload_opening = false;
        self.upload_handle = None;
        self.upload_progress = None;
        self.download_handle = None;
        self.download_progress = None;
        self.ssh_handle = None;
        self.ssh_ready = false;
        self.ssh_banner.clear();
        self.ssh_status.clear();
    }
}

fn display_host(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str().map(|h| match u.port() {
                Some(p) => format!("{h}:{p}"),
                None => h.to_owned(),
            })
        })
        .unwrap_or_else(|| url.to_owned())
}

fn format_bytes(bytes: i64) -> String {
    if bytes <= 0 {
        return "0 B".into();
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut n = bytes as f64;
    let mut idx = 0;
    while n >= 1024. && idx + 1 < units.len() {
        n /= 1024.;
        idx += 1;
    }
    if idx == 0 {
        format!("{bytes} B")
    } else {
        format!("{n:.1} {}", units[idx])
    }
}

fn format_relative_ts(unix_secs: i64) -> String {
    if unix_secs <= 0 {
        return "—".into();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let diff = now - unix_secs;
    if diff < 0 {
        let days = (-diff) / 86400;
        if days > 0 {
            return format!("{days} 天后过期");
        }
        let hours = (-diff) / 3600;
        return format!("{} 小时后过期", hours.max(1));
    }
    if diff < 60 {
        "刚刚".into()
    } else if diff < 3600 {
        format!("{} 分钟前", diff / 60)
    } else if diff < 86400 {
        format!("{} 小时前", diff / 3600)
    } else {
        format!("{} 天前", diff / 86400)
    }
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(input: &str) -> Vec<u8> {
    let val = |b: u8| -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            break;
        }
        let Some(a) = val(chunk[0]) else { break };
        let Some(b) = val(chunk[1]) else { break };
        out.push((a << 2) | (b >> 4));
        if chunk.len() > 2 && chunk[2] != b'=' {
            if let Some(c) = val(chunk[2]) {
                out.push((b << 4) | (c >> 2));
                if chunk.len() > 3 && chunk[3] != b'='
                    && let Some(d) = val(chunk[3])
                {
                    out.push((c << 6) | d);
                }
            }
        }
    }
    out
}

fn strip_ansi(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for ch in chars.by_ref() {
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            } else if matches!(chars.peek(), Some(']')) {
                chars.next();
                while let Some(ch) = chars.next() {
                    if ch == '\x07' {
                        break;
                    }
                    if ch == '\x1b' && matches!(chars.peek(), Some('\\')) {
                        chars.next();
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

fn sorted_user_groups(endpoint: &Value) -> Vec<&Value> {
    let network = endpoint["networkName"].as_str().unwrap_or_default();
    let mut groups: Vec<&Value> = endpoint["userGroups"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    groups.sort_by_key(|g| {
        let login = g["loginName"].as_str().unwrap_or_default();
        let is_owner = !network.is_empty() && login.eq_ignore_ascii_case(network);
        let has_own_peer = g["peers"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|p| p["shareeNode"] != true);
        if is_owner {
            0u8
        } else if has_own_peer {
            1u8
        } else {
            2u8
        }
    });
    groups
}

fn all_peers(endpoint: &Value) -> Vec<&Value> {
    sorted_user_groups(endpoint)
        .into_iter()
        .flat_map(|g| g["peers"].as_array().into_iter().flatten())
        .collect()
}

fn peer_display_name(peer: &Value) -> String {
    let host = peer["hostName"].as_str().unwrap_or_default().trim();
    let host_ok = !host.is_empty() && host != "device-of-shared-to-user";
    if let Some(label) = peer["dnsName"]
        .as_str()
        .map(|s| s.trim().trim_end_matches('.'))
        .and_then(|s| s.split('.').next())
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "device-of-shared-to-user")
    {
        if host_ok && host.eq_ignore_ascii_case(label) {
            return host.to_owned();
        }
        return label.to_owned();
    }
    if host_ok {
        host.to_owned()
    } else if host == "device-of-shared-to-user" {
        "共享设备".into()
    } else {
        "未命名".into()
    }
}

fn peer_primary_ip(peer: &Value) -> String {
    peer["tailscaleIps"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn can_receive_taildrop(peer: &Value) -> bool {
    peer["canReceiveFiles"] == true && peer["stableId"].as_str().is_some_and(|id| !id.is_empty())
}

fn peer_has_ssh(peer: &Value) -> bool {
    peer["sshHostKeys"]
        .as_array()
        .is_some_and(|keys| !keys.is_empty())
}

fn ping_sparkline(samples: &[PingSample], color: Rgba) -> Div {
    let vals: Vec<f64> = samples
        .iter()
        .filter(|s| s.error.is_empty() && s.latency_ms > 0.)
        .map(|s| s.latency_ms)
        .collect();
    div().w_full().h(px(44.)).child(canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if vals.len() < 2 {
                return;
            }
            let max_v = vals.iter().copied().fold(25.0_f64, f64::max) * 1.15;
            let count = vals.len();
            let mut pts = Vec::with_capacity(count);
            for (i, v) in vals.iter().enumerate() {
                let t = i as f32 / (count - 1) as f32;
                let x = bounds.origin.x + bounds.size.width * t;
                let y = bounds.origin.y + bounds.size.height - px(4.)
                    - (bounds.size.height - px(10.)) * ((*v / max_v).clamp(0., 1.) as f32);
                pts.push(point(x, y));
            }
            let mut stroke = PathBuilder::stroke(px(2.));
            stroke.move_to(pts[0]);
            for p in &pts[1..] {
                stroke.line_to(*p);
            }
            if let Ok(path) = stroke.build() {
                window.paint_path(path, color);
            }
        },
    ))
}

impl Workspace {
    pub(crate) fn has_tailscale_endpoints(&self) -> bool {
        self.tailscale.has_endpoints()
    }

    pub(super) fn tailscale_status_chip(&self) -> (String, StatusTone) {
        self.tailscale.status_chip()
    }

    pub(super) fn sync_tailscale(&mut self, force: bool, cx: &mut Context<Self>) {
        let rev = singboard_core::settings::Settings::revision();
        let retry_due = matches!(self.tailscale.status, ServiceApiStatus::Error)
            && (!self.local_endpoint() || self.local_state == CoreState::Running)
            && self
                .tailscale
                .last_retry
                .is_none_or(|t| t.elapsed().as_secs() >= 8);
        if !force && self.tailscale.settings_rev == rev && !retry_due {
            return;
        }
        self.tailscale.settings_rev = rev;
        self.tailscale.last_retry = Some(std::time::Instant::now());
        self.connect_tailscale(self.tailscale.selected_service_tag.clone(), cx);
    }

    pub(super) fn connect_tailscale(
        &mut self,
        service_tag: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(api) = self.tailscale.ensure_api() else {
            self.tailscale.status = ServiceApiStatus::NotConfigured;
            self.tailscale.endpoints.clear();
            return;
        };
        self.tailscale.generation = self.tailscale.generation.wrapping_add(1);
        let epoch = self.tailscale.generation;
        self.tailscale.clear_streams();
        self.tailscale.selected_service_tag = service_tag.clone();
        self.tailscale.status = ServiceApiStatus::Connecting;

        let task = singboard_core::runtime().spawn(async move {
            let mut guard = api.lock().await;
            let _ = guard.execute(ApiCommand::Disconnect).await;
            let candidates = guard
                .execute(ApiCommand::Discover)
                .await
                .map_err(|e| (e.code, e.message))?;
            let list: Vec<(String, String)> = candidates
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| {
                    Some((
                        c["serviceTag"].as_str().unwrap_or_default().to_owned(),
                        c["url"].as_str()?.to_owned(),
                    ))
                })
                .collect();
            if list.is_empty() {
                return Ok::<_, (&'static str, String)>((None, Vec::new(), None, None));
            }
            if service_tag.is_none() && list.len() > 1 {
                return Ok((None, list, None, None));
            }
            let chosen = if let Some(tag) = &service_tag {
                list.iter()
                    .find(|(t, _)| t == tag)
                    .or_else(|| list.first())
                    .cloned()
            } else {
                list.first().cloned()
            };
            let Some((tag, url)) = chosen else {
                return Ok((None, Vec::new(), None, None));
            };
            let auto_discovered = !tag.is_empty();
            let _ = guard
                .execute(ApiCommand::Connect {
                    service_tag: if tag.is_empty() { None } else { Some(tag) },
                })
                .await
                .map_err(|e| (e.code, e.message))?;
            let initial_status = guard.execute(ApiCommand::Status).await.ok();
            let handle = guard
                .execute(ApiCommand::Open {
                    feed: ApiFeed::Status,
                })
                .await
                .ok()
                .and_then(|v| v.as_u64());
            Ok((Some((url, auto_discovered)), list, initial_status, handle))
        });

        cx.spawn(async move |weak, cx| {
            let res = task.await.unwrap_or_else(|e| Err(("internal", e.to_string())));
            let _ = weak.update(cx, |this, cx| {
                if this.tailscale.generation != epoch {
                    return;
                }
                match res {
                    Ok((Some((url, auto_discovered)), _, initial, handle)) => {
                        this.tailscale.status = ServiceApiStatus::Connected {
                            url,
                            auto_discovered,
                        };
                        this.tailscale.status_handle = handle;
                        if let Some(snap) = initial {
                            this.apply_tailscale_status(snap);
                        }
                    }
                    Ok((None, list, _, _)) if list.len() > 1 => {
                        this.tailscale.status = ServiceApiStatus::SelectService(list);
                        this.tailscale.endpoints.clear();
                    }
                    Ok((None, _, _, _)) => {
                        this.tailscale.status = ServiceApiStatus::NotConfigured;
                        this.tailscale.endpoints.clear();
                    }
                    Err((code, _)) => {
                        this.tailscale.status = if code == "notConfigured" {
                            ServiceApiStatus::NotConfigured
                        } else {
                            ServiceApiStatus::Error
                        };
                        this.tailscale.endpoints.clear();
                    }
                }
                if this.page == Page::Tailscale && !this.has_tailscale_endpoints() {
                    this.page = Page::Overview;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_tailscale_status(&mut self, value: Value) {
        let eps = value["endpoints"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        self.tailscale.endpoints = eps;
        self.refresh_tailscale_selection();
    }

    fn refresh_tailscale_selection(&mut self) {
        if let Some(ep) = self.tailscale.current_endpoint().cloned() {
            let tag = ep["endpointTag"].as_str().unwrap_or_default().to_owned();
            self.tailscale.selected_endpoint = Some(tag);
            let peers = all_peers(&ep);
            let valid_sel = self.tailscale.selected_peer.as_deref().is_some_and(|id| {
                peers.iter().any(|p| p["stableId"].as_str() == Some(id))
            });
            if !valid_sel {
                self.tailscale.selected_peer = peers
                    .first()
                    .and_then(|p| p["stableId"].as_str())
                    .map(str::to_owned);
            }
            let valid_upload = self.tailscale.upload_target.as_deref().is_some_and(|id| {
                peers
                    .iter()
                    .any(|p| p["stableId"].as_str() == Some(id) && can_receive_taildrop(p))
            });
            if !valid_upload {
                self.tailscale.upload_target = peers
                    .iter()
                    .find(|p| can_receive_taildrop(p))
                    .and_then(|p| p["stableId"].as_str())
                    .map(str::to_owned);
            }
            let valid_ssh = self.tailscale.ssh_peer.as_deref().is_some_and(|id| {
                peers
                    .iter()
                    .any(|p| p["stableId"].as_str() == Some(id) && peer_has_ssh(p))
            });
            if !valid_ssh {
                self.tailscale.ssh_peer = peers
                    .iter()
                    .find(|p| peer_has_ssh(p) && p["online"] == true)
                    .or_else(|| peers.iter().find(|p| peer_has_ssh(p)))
                    .and_then(|p| p["stableId"].as_str())
                    .map(str::to_owned);
            }
        } else {
            self.tailscale.selected_endpoint = None;
            self.tailscale.selected_peer = None;
            if self.page == Page::Tailscale {
                self.page = Page::Overview;
            }
        }
    }

    pub(super) fn drain_tailscale(&mut self, cx: &mut Context<Self>) {
        if self.tailscale.draining {
            return;
        }
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let handles: Vec<(&'static str, u64)> = [
            ("status", self.tailscale.status_handle),
            ("inbox", self.tailscale.inbox_handle),
            ("ping", self.tailscale.ping_handle),
            ("upload", self.tailscale.upload_handle),
            ("download", self.tailscale.download_handle),
            ("ssh", self.tailscale.ssh_handle),
        ]
        .into_iter()
        .filter_map(|(k, h)| h.map(|id| (k, id)))
        .collect();
        if handles.is_empty() {
            return;
        }
        self.tailscale.draining = true;
        let epoch = self.tailscale.generation;
        let task = singboard_core::runtime().spawn(async move {
            let Ok(mut guard) = api.try_lock() else {
                return Vec::new();
            };
            let mut results = Vec::new();
            for (kind, handle) in handles {
                let drained = match guard.execute(ApiCommand::Drain { handle }).await {
                    Ok(value) => value,
                    Err(error) if kind == "upload" => {
                        serde_json::json!({"finished": true, "events": [{"error": error}]})
                    }
                    Err(_) => continue,
                };
                if kind == "upload" && (drained["finished"] == true || drained["events"]
                    .as_array().is_some_and(|events| events.iter().any(|ev| ev.get("error").is_some()))) {
                    let _ = guard.execute(ApiCommand::Close { handle }).await;
                }
                results.push((kind, handle, drained));
            }
            results
        });

        cx.spawn(async move |weak, cx| {
            let batch = task.await.unwrap_or_default();
            let _ = weak.update(cx, |this, cx| {
                this.tailscale.draining = false;
                if this.tailscale.generation != epoch {
                    return;
                }
                let mut changed = false;
                for (kind, handle, drained) in batch {
                    let events = drained["events"].as_array().cloned().unwrap_or_default();
                    let finished = drained["finished"].as_bool().unwrap_or(false);
                    if !events.is_empty() || finished {
                        changed = true;
                    }
                    match kind {
                        "status" if this.tailscale.status_handle == Some(handle) => {
                            for ev in events {
                                if let Some(data) = ev.get("data") {
                                    this.apply_tailscale_status(data.clone());
                                }
                            }
                            if finished {
                                this.tailscale.status_handle = None;
                            }
                        }
                        "inbox" if this.tailscale.inbox_handle == Some(handle) => {
                            for ev in events {
                                if let Some(data) = ev.get("data") {
                                    this.tailscale.inbox = Some(data.clone());
                                }
                            }
                            if finished {
                                this.tailscale.inbox_handle = None;
                            }
                        }
                        "ping" if this.tailscale.ping_handle == Some(handle) => {
                            for ev in events {
                                if let Some(data) = ev.get("data") {
                                    this.tailscale.ping_samples.push(PingSample {
                                        latency_ms: data["latencyMs"].as_f64().unwrap_or(0.),
                                        is_direct: data["isDirect"].as_bool().unwrap_or(false),
                                        endpoint: data["endpoint"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .to_owned(),
                                        derp_region_id: data["derpRegionId"]
                                            .as_i64()
                                            .unwrap_or(0),
                                        derp_region_code: data["derpRegionCode"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .to_owned(),
                                        peer_relay: data["peerRelay"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .to_owned(),
                                        error: data["error"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .to_owned(),
                                    });
                                    if this.tailscale.ping_samples.len() > 24 {
                                        this.tailscale.ping_samples.remove(0);
                                    }
                                } else if let Some(err) = ev["error"]["message"].as_str() {
                                    this.tailscale.ping_samples.push(PingSample {
                                        latency_ms: 0.,
                                        is_direct: false,
                                        endpoint: String::new(),
                                        derp_region_id: 0,
                                        derp_region_code: String::new(),
                                        peer_relay: String::new(),
                                        error: err.to_owned(),
                                    });
                                }
                            }
                            if finished {
                                this.tailscale.ping_handle = None;
                            }
                        }
                        "upload" if this.tailscale.upload_handle == Some(handle) => {
                            let mut err_msg = None;
                            for ev in events {
                                if let Some(bytes) = ev["totalSentBytes"].as_i64()
                                    && let Some(prog) = &mut this.tailscale.upload_progress
                                {
                                    prog.sent_bytes = bytes;
                                    prog.completed_files = ev["completedFiles"]
                                        .as_u64()
                                        .map(|n| n as usize)
                                        .unwrap_or(prog.completed_files);
                                }
                                if let Some(err) = ev["error"]["message"].as_str() {
                                    err_msg = Some(err.to_owned());
                                }
                            }
                            if let Some(err) = err_msg {
                                this.tailscale.upload_handle = None;
                                this.tailscale.upload_progress = None;
                                this.message(format!("发送文件失败：{err}"), true, cx);
                            } else if finished {
                                let name = this
                                    .tailscale
                                    .upload_progress
                                    .as_ref()
                                    .map(|p| p.target_name.clone())
                                    .unwrap_or_default();
                                this.tailscale.upload_handle = None;
                                this.tailscale.upload_progress = None;
                                this.message(format!("已通过 Taildrop 发送到 {name}"), false, cx);
                            }
                        }
                        "download" if this.tailscale.download_handle == Some(handle) => {
                            let mut err_msg = None;
                            let mut completed = false;
                            for ev in events {
                                if let Some(data) = ev.get("data")
                                    && let Some(prog) = &mut this.tailscale.download_progress
                                {
                                    prog.received_bytes = data["receivedBytes"]
                                        .as_i64()
                                        .unwrap_or(prog.received_bytes);
                                    prog.total_bytes = data["size"].as_i64().unwrap_or(prog.total_bytes);
                                    if data["completed"] == true {
                                        completed = true;
                                    }
                                }
                                if let Some(err) = ev["error"]["message"].as_str() {
                                    err_msg = Some(err.to_owned());
                                }
                            }
                            if let Some(err) = err_msg {
                                this.tailscale.download_handle = None;
                                this.tailscale.download_progress = None;
                                this.message(format!("下载失败：{err}"), true, cx);
                            } else if completed || finished {
                                let name = this
                                    .tailscale
                                    .download_progress
                                    .as_ref()
                                    .map(|p| p.name.clone())
                                    .unwrap_or_default();
                                this.tailscale.download_handle = None;
                                this.tailscale.download_progress = None;
                                this.message(format!("已另存副本：{name}"), false, cx);
                            }
                        }
                        "ssh" if this.tailscale.ssh_handle == Some(handle) => {
                            for ev in events {
                                if let Some(b64) = ev["output"].as_str() {
                                    let decoded = base64_decode(b64);
                                    let text = String::from_utf8_lossy(&decoded);
                                    let clean = strip_ansi(&text);
                                    this.tailscale.ssh_output.push_str(&clean);
                                    if this.tailscale.ssh_output.len() > 64 * 1024 {
                                        let cut = this.tailscale.ssh_output.len() - 48 * 1024;
                                        let boundary = this
                                            .tailscale
                                            .ssh_output
                                            .ceil_char_boundary(cut);
                                        this.tailscale.ssh_output.drain(..boundary);
                                    }
                                }
                                if let Some(banner) = ev["authBanner"].as_str() {
                                    this.tailscale.ssh_banner = banner.to_owned();
                                }
                                if ev["ready"] == true {
                                    this.tailscale.ssh_ready = true;
                                    this.tailscale.ssh_status = "已连接 · xterm-256color".into();
                                }
                                if let Some(exit) = ev.get("exit") {
                                    let code = exit["exitCode"].as_i64().unwrap_or(0);
                                    this.tailscale.ssh_status = format!("会话已退出 ({code})");
                                    this.tailscale.ssh_ready = false;
                                }
                                if let Some(err) = ev["error"]["message"].as_str() {
                                    this.tailscale.ssh_status = format!("SSH 错误：{err}");
                                    this.tailscale.ssh_ready = false;
                                }
                            }
                            if finished {
                                this.tailscale.ssh_handle = None;
                                this.tailscale.ssh_ready = false;
                            }
                        }
                        _ => {}
                    }
                }
                if changed {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn close_stream(&self, handle: u64) {
        if let Some(api) = self.tailscale.api.clone() {
            singboard_core::runtime().spawn(async move {
                let mut guard = api.lock().await;
                let _ = guard.execute(ApiCommand::Close { handle }).await;
            });
        }
    }

    fn ensure_taildrop_inbox(&mut self, cx: &mut Context<Self>) {
        let Some(tag) = self.tailscale.current_endpoint_tag() else {
            return;
        };
        if self.tailscale.inbox_handle.is_some()
            && self.tailscale.inbox_tag.as_deref() == Some(tag.as_str())
        {
            return;
        }
        if let Some(old) = self.tailscale.inbox_handle.take() {
            self.close_stream(old);
        }
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        self.tailscale.inbox_tag = Some(tag.clone());
        let epoch = self.tailscale.generation;
        let mark_read = self
            .tailscale
            .current_endpoint()
            .and_then(|e| e["unreadFileCount"].as_i64())
            .unwrap_or(0)
            > 0;
        let task = singboard_core::runtime().spawn(async move {
            let mut guard = api.lock().await;
            if mark_read {
                let _ = guard
                    .execute(ApiCommand::MarkInboxRead {
                        tag: Some(tag.clone()),
                    })
                    .await;
            }
            guard
                .execute(ApiCommand::Open {
                    feed: ApiFeed::Inbox { tag: Some(tag) },
                })
                .await
                .ok()
                .and_then(|v| v.as_u64())
        });
        cx.spawn(async move |weak, cx| {
            let handle = task.await.ok().flatten();
            let _ = weak.update(cx, |this, cx| {
                if this.tailscale.generation == epoch {
                    this.tailscale.inbox_handle = handle;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn set_tailscale_exit_node(&mut self, stable_id: String, cx: &mut Context<Self>) {
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let tag = self.tailscale.current_endpoint_tag();
        let clearing = stable_id.is_empty();
        let task = singboard_core::runtime().spawn(async move {
            let mut guard = api.lock().await;
            guard
                .execute(ApiCommand::SetExitNode { tag, stable_id })
                .await
                .map_err(|e| e.message)
        });
        cx.spawn(async move |weak, cx| {
            let res = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                match res {
                    Ok(_) => this.message(
                        if clearing {
                            "已停用出口节点".into()
                        } else {
                            "已切换出口节点".into()
                        },
                        false,
                        cx,
                    ),
                    Err(e) => this.message(format!("设置出口节点失败：{e}"), true, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_or_stop_ping(&mut self, peer_ip: String, cx: &mut Context<Self>) {
        if let Some(old) = self.tailscale.ping_handle.take() {
            self.close_stream(old);
            if self.tailscale.ping_peer_ip.as_deref() == Some(peer_ip.as_str()) {
                self.tailscale.ping_peer_ip = None;
                cx.notify();
                return;
            }
        }
        let Ok(parsed_ip) = peer_ip.parse() else {
            self.message("设备没有有效的 Tailscale IP".into(), true, cx);
            return;
        };
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let tag = self.tailscale.current_endpoint_tag();
        let epoch = self.tailscale.generation;
        self.tailscale.ping_samples.clear();
        self.tailscale.ping_peer_ip = Some(peer_ip);
        let task = singboard_core::runtime().spawn(async move {
            let mut guard = api.lock().await;
            guard
                .execute(ApiCommand::Open {
                    feed: ApiFeed::Ping {
                        tag,
                        peer_ip: parsed_ip,
                    },
                })
                .await
                .map_err(|e| e.message)
        });
        cx.spawn(async move |weak, cx| {
            let res = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                if this.tailscale.generation != epoch {
                    return;
                }
                match res {
                    Ok(v) => this.tailscale.ping_handle = v.as_u64(),
                    Err(e) => {
                        this.tailscale.ping_peer_ip = None;
                        this.message(format!("启动 Ping 失败：{e}"), true, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn resolve_upload_target(&self) -> Option<(String, String)> {
        let ep = self.tailscale.current_endpoint()?;
        if ep["canShareFiles"] != true {
            return None;
        }
        let target_id = self.tailscale.upload_target.clone().or_else(|| {
            all_peers(ep)
                .into_iter()
                .find(|p| can_receive_taildrop(p))
                .and_then(|p| p["stableId"].as_str().map(str::to_owned))
        })?;
        let target_name = all_peers(ep)
            .into_iter()
            .find(|p| p["stableId"].as_str() == Some(target_id.as_str()))
            .map(peer_display_name)
            .unwrap_or_else(|| "目标设备".into());
        Some((target_id, target_name))
    }

    fn launch_taildrop_upload(
        &mut self,
        peer_stable_id: String,
        target_name: String,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let tag = self.tailscale.current_endpoint_tag();
        let epoch = self.tailscale.generation;
        let total_bytes: i64 = paths
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len() as i64))
            .sum();
        let file_count = paths.len();
        self.tailscale.upload_opening = true;
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let upload_api = api.clone();
            let res = singboard_core::runtime()
                .spawn(async move {
                    let mut guard = upload_api.lock().await;
                    guard
                        .execute(ApiCommand::Open {
                            feed: ApiFeed::Upload {
                                tag,
                                peer_stable_id,
                                paths,
                            },
                        })
                        .await
                        .map_err(|e| e.message)
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            let opened = res.as_ref().ok().and_then(Value::as_u64);
            let attached = weak
                .update(cx, |this, cx| {
                    if this.tailscale.generation != epoch {
                        return false;
                    }
                    this.tailscale.upload_opening = false;
                    match res {
                        Ok(_) => {
                            this.tailscale.upload_handle = opened;
                            this.tailscale.upload_progress = Some(UploadProgress {
                                target_name,
                                file_count,
                                completed_files: 0,
                                sent_bytes: 0,
                                total_bytes,
                            });
                        }
                        Err(e) => this.message(format!("无法开始发送：{e}"), true, cx),
                    }
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !attached && let Some(handle) = opened {
                singboard_core::runtime().spawn(async move {
                    let _ = api.lock().await.execute(ApiCommand::Close { handle }).await;
                });
            }
        })
        .detach();
    }

    fn upload_taildrop_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.tailscale.upload_opening
            || self.tailscale.upload_handle.is_some()
            || matches!(self.tailscale.status, ServiceApiStatus::Connecting)
        {
            return;
        }
        let files: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_file()).collect();
        if files.is_empty() {
            self.message("暂不支持直接发送文件夹，请拖入文件".into(), true, cx);
            return;
        }
        let Some((peer_stable_id, target_name)) = self.resolve_upload_target() else {
            self.message("请先选择可接收文件的设备".into(), true, cx);
            return;
        };
        self.launch_taildrop_upload(peer_stable_id, target_name, files, cx);
    }

    fn pick_and_upload_taildrop(&mut self, cx: &mut Context<Self>) {
        if self.tailscale.upload_opening
            || self.tailscale.upload_handle.is_some()
            || matches!(self.tailscale.status, ServiceApiStatus::Connecting)
        {
            return;
        }
        let Some((peer_stable_id, target_name)) = self.resolve_upload_target() else {
            self.message("请先选择接收设备".into(), true, cx);
            return;
        };
        let epoch = self.tailscale.generation;
        self.tailscale.upload_opening = true;
        cx.notify();
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择要通过 Taildrop 发送的文件".into()),
        });
        cx.spawn(async move |weak, cx| {
            let paths = picker
                .await
                .ok()
                .and_then(|result| result.ok())
                .flatten()
                .unwrap_or_default();
            let _ = weak.update(cx, |this, cx| {
                if this.tailscale.generation != epoch {
                    return;
                }
                if paths.is_empty() {
                    this.tailscale.upload_opening = false;
                    cx.notify();
                    return;
                }
                this.launch_taildrop_upload(peer_stable_id, target_name, paths, cx);
            });
        })
        .detach();
    }

    fn download_taildrop_file(
        &mut self,
        name: String,
        size: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let tag = self.tailscale.current_endpoint_tag();
        let epoch = self.tailscale.generation;
        let default_dir = std::env::var_os("USERPROFILE")
            .map(|p| PathBuf::from(p).join("Downloads"))
            .unwrap_or_else(|| PathBuf::from("."));
        let save = cx.prompt_for_new_path(&default_dir, Some(&name));
        let handle = window.window_handle();
        cx.spawn(async move |weak, cx| {
            let Ok(Ok(Some(destination))) = save.await else {
                return;
            };
            let file_name = name.clone();
            let res = singboard_core::runtime()
                .spawn(async move {
                    let mut guard = api.lock().await;
                    guard
                        .execute(ApiCommand::Open {
                            feed: ApiFeed::Download {
                                tag,
                                name: file_name,
                                destination,
                            },
                        })
                        .await
                        .map_err(|e| e.message)
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            let _ = handle.update(cx, |_, _, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if this.tailscale.generation != epoch {
                        return;
                    }
                    match res {
                        Ok(v) => {
                            this.tailscale.download_handle = v.as_u64();
                            this.tailscale.download_progress = Some(DownloadProgress {
                                name,
                                received_bytes: 0,
                                total_bytes: size,
                            });
                        }
                        Err(e) => this.message(format!("无法下载文件：{e}"), true, cx),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn delete_taildrop_file(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let tag = self.tailscale.current_endpoint_tag();
        let task = singboard_core::runtime().spawn(async move {
            let mut guard = api.lock().await;
            guard
                .execute(ApiCommand::DeleteFile { tag, name })
                .await
                .map_err(|e| e.message)
        });
        cx.spawn(async move |weak, cx| {
            let res = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                match res {
                    Ok(_) => this.message("已从 Taildrop 接收目录删除文件".into(), false, cx),
                    Err(e) => this.message(format!("删除失败：{e}"), true, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn cancel_taildrop_receiving(
        &mut self,
        sender_id: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let tag = self.tailscale.current_endpoint_tag();
        let task = singboard_core::runtime().spawn(async move {
            let mut guard = api.lock().await;
            guard
                .execute(ApiCommand::CancelReceiving {
                    tag,
                    sender_id,
                    name,
                })
                .await
                .map_err(|e| e.message)
        });
        cx.spawn(async move |weak, cx| {
            let res = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                match res {
                    Ok(_) => this.message("已取消接收文件".into(), false, cx),
                    Err(e) => this.message(format!("取消接收失败：{e}"), true, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn connect_or_disconnect_ssh(&mut self, cx: &mut Context<Self>) {
        if let Some(old) = self.tailscale.ssh_handle.take() {
            self.close_stream(old);
            self.tailscale.ssh_ready = false;
            self.tailscale.ssh_status = "已断开".into();
            cx.notify();
            return;
        }
        let Some(peer_stable_id) = self.tailscale.ssh_peer.clone() else {
            self.message("请选择支持 SSH 的设备".into(), true, cx);
            return;
        };
        let username = self
            .tailscale
            .ssh_user_input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "root".into());
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let tag = self.tailscale.current_endpoint_tag();
        let epoch = self.tailscale.generation;
        self.tailscale.ssh_banner.clear();
        self.tailscale.ssh_output.clear();
        self.tailscale.ssh_ready = false;
        self.tailscale.ssh_status = "正在连接 SSH…".into();
        let task = singboard_core::runtime().spawn(async move {
            let mut guard = api.lock().await;
            guard
                .execute(ApiCommand::Open {
                    feed: ApiFeed::Ssh {
                        tag,
                        peer_stable_id,
                        username,
                        columns: 108,
                        rows: 28,
                    },
                })
                .await
                .map_err(|e| e.message)
        });
        cx.spawn(async move |weak, cx| {
            let res = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                if this.tailscale.generation != epoch {
                    return;
                }
                match res {
                    Ok(v) => this.tailscale.ssh_handle = v.as_u64(),
                    Err(e) => {
                        this.tailscale.ssh_status = format!("连接失败：{e}");
                        this.message(format!("SSH 连接失败：{e}"), true, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn send_ssh_bytes(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let Some(handle) = self.tailscale.ssh_handle else {
            return;
        };
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let data = base64_encode(&bytes);
        let task = singboard_core::runtime().spawn(async move {
            let mut guard = api.lock().await;
            guard
                .execute(ApiCommand::SshInput { handle, data })
                .await
                .map_err(|e| e.message)
        });
        cx.spawn(async move |weak, cx| {
            if let Ok(Err(e)) = task.await {
                let _ = weak.update(cx, |this, cx| {
                    this.message(format!("SSH 输入失败：{e}"), true, cx);
                });
            }
        })
        .detach();
    }

    fn fetch_tailscale_certificate(&mut self, cx: &mut Context<Self>) {
        let Some(TailscaleDialog::Certificate {
            tag,
            selected_domain,
            min_validity_days,
            loading,
            error,
            ..
        }) = &mut self.tailscale.dialog
        else {
            return;
        };
        if *loading {
            return;
        }
        *loading = true;
        error.clear();
        let tag = tag.clone();
        let domain = selected_domain.clone();
        let min_validity_seconds = (*min_validity_days).saturating_mul(86400);
        let Some(api) = self.tailscale.api.clone() else {
            return;
        };
        let task = singboard_core::runtime().spawn(async move {
            let mut guard = api.lock().await;
            guard
                .execute(ApiCommand::Certificate {
                    tag: Some(tag),
                    domain,
                    min_validity_seconds,
                })
                .await
                .map_err(|e| e.message)
        });
        cx.spawn(async move |weak, cx| {
            let res = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                if let Some(TailscaleDialog::Certificate {
                    loading,
                    cert_pem,
                    key_pem,
                    error,
                    ..
                }) = &mut this.tailscale.dialog
                {
                    *loading = false;
                    match res {
                        Ok(val) => {
                            *cert_pem = val["certificatePem"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned();
                            *key_pem = val["privateKeyPem"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned();
                        }
                        Err(e) => *error = e,
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_tailscale_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = Palette::get(cx);
        let tint = self.glass_tint;
        let sel_chip = adaptive_selection_chip(p, cx);
        let dialog = self.tailscale.dialog.as_ref()?;
        let scrim = if p.0 {
            rgba(0x0000007a)
        } else {
            rgba(0x00000044)
        };

        let body: Div = match dialog {
            TailscaleDialog::Certificate {
                domains,
                selected_domain,
                min_validity_days,
                loading,
                cert_pem,
                key_pem,
                reveal_key,
                error,
                ..
            } => {
                let domains = domains.clone();
                let selected_domain = selected_domain.clone();
                let min_validity_days = *min_validity_days;
                let loading = *loading;
                let cert_pem = cert_pem.clone();
                let key_pem = key_pem.clone();
                let reveal_key = *reveal_key;
                let error = error.clone();
                let cert_copy = cert_pem.clone();
                let key_copy = key_pem.clone();
                let domain_pills = domains.into_iter().enumerate().map(|(idx, d)| {
                    let selected = d == selected_domain;
                    let d_click = d.clone();
                    div()
                        .id(("ts-cert-domain", idx))
                        .cursor_pointer()
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .border_1()
                        .border_color(if selected { sel_chip.border } else { p.line() })
                        .text_xs()
                        .font_family("Consolas")
                        .bg(if selected { sel_chip.tint } else { p.fill() })
                        .text_color(if selected { sel_chip.ink } else { p.ink() })
                        .child(d)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(TailscaleDialog::Certificate {
                                selected_domain, ..
                            }) = &mut this.tailscale.dialog
                            {
                                *selected_domain = d_click.clone();
                                cx.notify();
                            }
                        }))
                });
                let validity_pills = [(0u32, "默认"), (7, "7 天"), (30, "30 天")]
                    .into_iter()
                    .map(|(days, label)| {
                        let selected = days == min_validity_days;
                        div()
                            .id(("ts-cert-days", days as usize))
                            .cursor_pointer()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .border_1()
                            .border_color(if selected { sel_chip.border } else { p.line() })
                            .text_xs()
                            .bg(if selected { sel_chip.tint } else { p.fill() })
                            .text_color(if selected { sel_chip.ink } else { p.muted() })
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(TailscaleDialog::Certificate {
                                    min_validity_days, ..
                                }) = &mut this.tailscale.dialog
                                {
                                    *min_validity_days = days;
                                    cx.notify();
                                }
                            }))
                    });
                popover_card(p, tint)
                    .w(px(500.))
                    .max_w_full()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .text_color(p.ink())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Tailscale TLS 证书"),
                            )
                            .child(
                                div()
                                    .id("ts-cert-close")
                                    .cursor_pointer()
                                    .px_3()
                                    .py_1()
                                    .rounded_full()
                                    .bg(p.fill())
                                    .text_xs()
                                    .child("关闭")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.tailscale.dialog = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(div().text_xs().text_color(p.muted()).child("证书域名"))
                            .child(div().flex().flex_wrap().gap_1p5().children(domain_pills)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(div().text_xs().text_color(p.muted()).child("最短剩余有效期"))
                            .child(div().flex().gap_1p5().children(validity_pills)),
                    )
                    .when(!error.is_empty(), |d| {
                        d.child(div().text_xs().text_color(p.danger()).child(error))
                    })
                    .when(!cert_pem.is_empty(), |d| {
                        let preview_cert = cert_pem.lines().take(2).collect::<Vec<_>>().join(" ");
                        let preview_key = if reveal_key {
                            key_pem.lines().take(2).collect::<Vec<_>>().join(" ")
                        } else {
                            "••••••••••••••••••••••••••••••••••••".into()
                        };
                        d.child(
                            div()
                                .rounded(px(14.))
                                .bg(p.tile())
                                .border_1()
                                .border_color(p.line())
                                .p_3()
                                .flex()
                                .flex_col()
                                .gap_1p5()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child("Certificate PEM"),
                                        )
                                        .child(
                                            div()
                                                .id("ts-copy-cert")
                                                .cursor_pointer()
                                                .px_2p5()
                                                .py(px(2.))
                                                .rounded_full()
                                                .bg(p.primary_soft())
                                                .text_color(p.accent())
                                                .text_xs()
                                                .child("复制")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(cert_copy.clone()),
                                                    );
                                                    this.message("已复制证书 PEM".into(), false, cx);
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .font_family("Consolas")
                                        .text_xs()
                                        .text_color(p.muted())
                                        .truncate()
                                        .child(preview_cert),
                                ),
                        )
                        .child(
                            div()
                                .rounded(px(14.))
                                .bg(p.tile())
                                .border_1()
                                .border_color(p.line())
                                .p_3()
                                .flex()
                                .flex_col()
                                .gap_1p5()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child("Private Key PEM"),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .gap_1p5()
                                                .child(
                                                    div()
                                                        .id("ts-reveal-key")
                                                        .cursor_pointer()
                                                        .px_2p5()
                                                        .py(px(2.))
                                                        .rounded_full()
                                                        .bg(p.fill())
                                                        .text_xs()
                                                        .child(if reveal_key { "隐藏" } else { "显示" })
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            if let Some(TailscaleDialog::Certificate {
                                                                reveal_key,
                                                                ..
                                                            }) = &mut this.tailscale.dialog
                                                            {
                                                                *reveal_key = !*reveal_key;
                                                                cx.notify();
                                                            }
                                                        })),
                                                )
                                                .child(
                                                    div()
                                                        .id("ts-copy-key")
                                                        .cursor_pointer()
                                                        .px_2p5()
                                                        .py(px(2.))
                                                        .rounded_full()
                                                        .bg(p.primary_soft())
                                                        .text_color(p.accent())
                                                        .text_xs()
                                                        .child("复制")
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            cx.write_to_clipboard(
                                                                ClipboardItem::new_string(key_copy.clone()),
                                                            );
                                                            this.message("已复制私钥 PEM".into(), false, cx);
                                                        })),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .font_family("Consolas")
                                        .text_xs()
                                        .text_color(p.muted())
                                        .truncate()
                                        .child(preview_key),
                                ),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .pt_1()
                            .child(
                                div()
                                    .id("ts-fetch-cert")
                                    .cursor_pointer()
                                    .px_4()
                                    .py_1p5()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(sel_chip.border)
                                    .bg(sel_chip.tint)
                                    .text_color(sel_chip.ink)
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(if loading { "正在获取证书…" } else { "获取证书" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.fetch_tailscale_certificate(cx);
                                    })),
                            ),
                    )
            }
        };

        Some(
            div()
                .id("ts-dialog-scrim")
                .occlude()
                .absolute()
                .inset_0()
                .bg(scrim)
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(body)
                .into_any_element(),
        )
    }

    pub(super) fn tailscale_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = Palette::get(cx);
        let tint = self.glass_tint;
        let narrow = window.viewport_size().width < px(860.);

        if self.tailscale.search_input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索设备、IP、系统…"));
            cx.subscribe(&input, |this: &mut Self, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.tailscale.search_query = input.read(cx).value().trim().to_lowercase();
                    cx.notify();
                }
            })
            .detach();
            self.tailscale.search_input = Some(input);
        }
        if self.tailscale.ssh_user_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("root")
                    .default_value("root")
            });
            self.tailscale.ssh_user_input = Some(input);
        }
        if self.tailscale.ssh_cmd_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder("输入命令后按回车或点击发送…")
            });
            cx.subscribe_in(&input, window, |this: &mut Self, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let cmd = input.read(cx).value().to_string();
                    this.send_ssh_bytes(format!("{cmd}\n").into_bytes(), cx);
                    input.update(cx, |i, cx| i.set_value(String::new(), window, cx));
                }
            })
            .detach();
            self.tailscale.ssh_cmd_input = Some(input);
        }

        if self.tailscale.tab == TailscaleTab::Taildrop {
            self.ensure_taildrop_inbox(cx);
        }

        let card = || {
            glass_card(p, true, tint)
                .w_full()
                .flex()
                .flex_col()
                .flex_none()
        };
        let pill_wash = p.card_tinted(true, tint);
        let glass_pill = |id: ElementId| {
            div()
                .id(id)
                .relative()
                .overflow_hidden()
                .rounded_full()
                .bg(pill_wash)
                .border_1()
                .border_color(p.rim())
                .child(card_layer(pill_wash, px(999.), 0.68))
        };

        let Some(ep) = self.tailscale.current_endpoint().cloned() else {
            let empty_body = div()
                .w_full()
                .flex()
                .flex_col()
                .gap_4()
                .pb_6()
                .child(
                    backdrop_heading("Tailscale", p, cx)
                        .text_size(crate::theme::text_px(26.))
                        .line_height(crate::theme::text_px(32.))
                        .font_weight(FontWeight::BOLD),
                )
                .child(card().p_5().child(
                    div()
                        .relative()
                        .text_sm()
                        .text_color(p.muted())
                        .child("未检测到已配置的 Tailscale 端点。"),
                ));
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.scroll_pane("tailscale-scroll", empty_body, cx));
        };

        let tag = ep["endpointTag"].as_str().unwrap_or_default().to_owned();
        let backend_state = ep["backendState"].as_str().unwrap_or_default().to_owned();
        let state_text = ep["stateText"].as_str().unwrap_or_default().to_owned();
        let auth_url = ep["authUrl"].as_str().unwrap_or_default().to_owned();
        let network_name = ep["networkName"].as_str().unwrap_or_default().to_owned();
        let magic_suffix = ep["magicDnsSuffix"].as_str().unwrap_or_default().to_owned();
        let self_peer = ep["self"].clone();
        let exit_node = ep["exitNode"].clone();
        let can_share_files = ep["canShareFiles"].as_bool().unwrap_or(false);
        let cert_domains: Vec<String> = ep["certDomains"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();

        let endpoint_tags: Vec<String> = self
            .tailscale
            .endpoints
            .iter()
            .filter_map(|e| e["endpointTag"].as_str().map(str::to_owned))
            .collect();

        let sel_chip = adaptive_selection_chip(p, cx);
        let header = div()
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        backdrop_heading("Tailscale", p, cx)
                            .text_size(crate::theme::text_px(26.))
                            .line_height(crate::theme::text_px(32.))
                            .font_weight(FontWeight::BOLD),
                    )
                    .when(endpoint_tags.len() > 1, |el| {
                        el.child(div().flex().gap_1p5().children(endpoint_tags.into_iter().enumerate().map(|(i, t)| {
                            let active = t == tag;
                            let t_click = t.clone();
                            div()
                                .id(("ts-ep", i))
                                .relative()
                                .overflow_hidden()
                                .cursor_pointer()
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .shadow_sm()
                                .child(adaptive_pill_layer(
                                    p,
                                    px(999.),
                                    active.then_some(sel_chip.tint),
                                    if active { sel_chip.border } else { p.control_rim() },
                                ))
                                .child(
                                    div()
                                        .relative()
                                        .text_xs()
                                        .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                                        .text_color(if active { sel_chip.ink } else { p.ink() })
                                        .child(t),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.tailscale.selected_endpoint = Some(t_click.clone());
                                    this.refresh_tailscale_selection();
                                    this.tailscale.inbox_tag = None;
                                    this.tailscale.inbox = None;
                                    cx.notify();
                                }))
                        })))
                    }),
            )
            .when(!cert_domains.is_empty(), |el| {
                let tag_c = tag.clone();
                let domains_c = cert_domains.clone();
                el.child(
                    glass_pill("ts-open-cert".into())
                        .cursor_pointer()
                        .px_3()
                        .py_1p5()
                        .child(
                            div()
                                .relative()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.ink())
                                .child("TLS 证书"),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let first = domains_c.first().cloned().unwrap_or_default();
                            this.tailscale.dialog = Some(TailscaleDialog::Certificate {
                                tag: tag_c.clone(),
                                domains: domains_c.clone(),
                                selected_domain: first,
                                min_validity_days: 0,
                                loading: false,
                                cert_pem: String::new(),
                                key_pem: String::new(),
                                reveal_key: false,
                                error: String::new(),
                            });
                            cx.notify();
                        })),
                )
            });

        let needs_login = backend_state == "NeedsLogin" || !auth_url.is_empty();
        if needs_login {
            let auth_open = auth_url.clone();
            let auth_copy = auth_url.clone();
            let login_card = card().p_5().child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(8.)).rounded_full().bg(p.warning_dot()))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!(
                                        "等待节点授权{}",
                                        if state_text.is_empty() {
                                            String::new()
                                        } else {
                                            format!(" · {state_text}")
                                        }
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted())
                            .child(format!("端点 {tag} 需要完成 Tailscale 登录授权。授权完成后界面将自动刷新。")),
                    )
                    .when(!auth_url.is_empty(), |el| {
                        el.child(
                            div()
                                .rounded(px(12.))
                                .bg(p.fill())
                                .px_3()
                                .py_2()
                                .font_family("Consolas")
                                .text_xs()
                                .text_color(p.ink())
                                .truncate()
                                .child(auth_url.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .id("ts-open-auth")
                                        .cursor_pointer()
                                        .px_4()
                                        .py_1p5()
                                        .rounded_full()
                                        .bg(p.primary_fill())
                                        .text_color(rgb(0xffffff))
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("在浏览器中打开")
                                        .on_click(move |_, _, cx| {
                                            cx.open_url(&auth_open);
                                        }),
                                )
                                .child(
                                    div()
                                        .id("ts-copy-auth")
                                        .cursor_pointer()
                                        .px_3p5()
                                        .py_1p5()
                                        .rounded_full()
                                        .bg(p.fill())
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child("复制授权链接")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                auth_copy.clone(),
                                            ));
                                            this.message("已复制授权链接".into(), false, cx);
                                        })),
                                ),
                        )
                    }),
            );
            let login_body = div()
                .w_full()
                .flex()
                .flex_col()
                .gap_4()
                .pb_6()
                .text_color(p.ink())
                .child(header)
                .child(login_card);
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.scroll_pane("tailscale-scroll", login_body, cx));
        }

        let self_name = peer_display_name(&self_peer);
        let self_host = if self_name == "未命名" {
            "本机".to_owned()
        } else {
            self_name
        };
        let self_ip = peer_primary_ip(&self_peer);
        let self_dns = self_peer["dnsName"]
            .as_str()
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_owned();
        let active_exit_id = exit_node["stableId"].as_str().unwrap_or_default().to_owned();
        let active_exit_name = peer_display_name(&exit_node);

        let peers_all = all_peers(&ep);
        let online_count = peers_all.iter().filter(|p| p["online"] == true).count();
        let total_count = peers_all.len();
        let exit_options: Vec<(String, String)> = peers_all
            .iter()
            .filter(|peer| peer["exitNodeOption"] == true)
            .filter_map(|peer| {
                let id = peer["stableId"].as_str()?.to_owned();
                let name = peer_display_name(peer);
                let label = if peer["online"] == true {
                    name
                } else {
                    format!("{name} (离线)")
                };
                Some((id, label))
            })
            .collect();
        let has_exit_choices = !exit_options.is_empty() || !active_exit_id.is_empty();
        let mut exit_picker_options = Vec::with_capacity(exit_options.len() + 2);
        exit_picker_options.push((String::new(), "不使用出口节点".into()));
        for (id, label) in &exit_options {
            exit_picker_options.push((id.clone(), label.clone()));
        }
        if !active_exit_id.is_empty() && !exit_options.iter().any(|(id, _)| id == &active_exit_id) {
            exit_picker_options.push((active_exit_id.clone(), active_exit_name.clone()));
        }

        let hero_card = card().p_4().child(
            div()
                .relative()
                .flex()
                .when(narrow, |d| d.flex_col().gap_3())
                .when(!narrow, |d| d.items_center().justify_between().gap_4())
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .size(px(10.))
                                        .rounded_full()
                                        .bg(if backend_state == "Running" {
                                            p.success_dot()
                                        } else {
                                            p.warning_dot()
                                        }),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(self_host),
                                )
                                .child(
                                    div()
                                        .relative()
                                        .overflow_hidden()
                                        .px_2p5()
                                        .py(px(3.))
                                        .rounded_full()
                                        .child(adaptive_pill_layer(
                                            p,
                                            px(999.),
                                            Some(p.success_soft()),
                                            p.success_dot().opacity(0.45),
                                        ))
                                        .child(
                                            div()
                                                .relative()
                                                .text_color(if p.0 {
                                                    rgb(0x6ee787)
                                                } else {
                                                    rgb(0x116329)
                                                })
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(if network_name.is_empty() {
                                                    if state_text.is_empty() {
                                                        backend_state.clone()
                                                    } else {
                                                        state_text.clone()
                                                    }
                                                } else {
                                                    network_name.clone()
                                                }),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_1p5()
                                .when(!self_ip.is_empty(), |el| {
                                    let ip_copy = self_ip.clone();
                                    el.child(
                                        div()
                                            .id("ts-copy-self-ip")
                                            .relative()
                                            .overflow_hidden()
                                            .cursor_pointer()
                                            .px_2p5()
                                            .py(px(3.))
                                            .rounded_full()
                                            .child(adaptive_pill_layer(
                                                p,
                                                px(999.),
                                                None,
                                                p.control_rim(),
                                            ))
                                            .child(
                                                div()
                                                    .relative()
                                                    .font_family("Consolas")
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(p.ink())
                                                    .child(self_ip),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    ip_copy.clone(),
                                                ));
                                                this.message("已复制本机 Tailscale IP".into(), false, cx);
                                            })),
                                    )
                                })
                                .when(!self_dns.is_empty() || !magic_suffix.is_empty(), |el| {
                                    let dns_val = if !self_dns.is_empty() {
                                        self_dns
                                    } else {
                                        magic_suffix
                                    };
                                    let dns_copy = dns_val.clone();
                                    el.child(
                                        div()
                                            .id("ts-copy-self-dns")
                                            .relative()
                                            .overflow_hidden()
                                            .cursor_pointer()
                                            .px_2p5()
                                            .py(px(3.))
                                            .rounded_full()
                                            .child(adaptive_pill_layer(
                                                p,
                                                px(999.),
                                                None,
                                                p.control_rim(),
                                            ))
                                            .child(
                                                div()
                                                    .relative()
                                                    .font_family("Consolas")
                                                    .text_xs()
                                                    .text_color(p.ink())
                                                    .child(dns_val),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    dns_copy.clone(),
                                                ));
                                                this.message("已复制 MagicDNS 域名".into(), false, cx);
                                            })),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .relative()
                        .rounded(px(16.))
                        .bg(p.tile())
                        .border_1()
                        .border_color(p.line())
                        .px_3p5()
                        .py_2p5()
                        .when(!narrow, |d| d.min_w(px(270.)))
                        .child(
                            div()
                                .relative()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .flex_none()
                                        .when(!active_exit_id.is_empty(), |el| {
                                            el.child(
                                                div()
                                                    .size(px(7.))
                                                    .rounded_full()
                                                    .bg(p.success_dot()),
                                            )
                                        })
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(p.muted())
                                                .child("出口节点"),
                                        ),
                                )
                                .when(!has_exit_choices, |el| {
                                    el.child(
                                        div()
                                            .text_xs()
                                            .text_color(p.muted())
                                            .child("无可用出口节点"),
                                    )
                                })
                                .when(has_exit_choices, |el| {
                                    el.child(
                                        div()
                                            .w(px(180.))
                                            .flex_none()
                                            .child(super::settings::picker(
                                                "ts-exit-node",
                                                active_exit_id.clone(),
                                                exit_picker_options,
                                                |this, id, cx| {
                                                    this.set_tailscale_exit_node(id, cx);
                                                },
                                                cx,
                                            )),
                                    )
                                }),
                        ),
                ),
        );

        let segmented = {
            let thumb_bg = if p.0 {
                rgba(0xffffff26)
            } else {
                rgba(0xffffff8c)
            };
            let thumb_border = if p.0 {
                rgba(0xffffff33)
            } else {
                rgba(0xffffffb8)
            };
            let tab_btn = |id: &'static str,
                           tab: TailscaleTab,
                           label: String,
                           highlight_badge: Option<String>| {
                let active = self.tailscale.tab == tab;
                div()
                    .id(id)
                    .cursor_pointer()
                    .px_3p5()
                    .py_1p5()
                    .rounded_full()
                    .text_xs()
                    .font_weight(if active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .border_1()
                    .border_color(if active {
                        thumb_border
                    } else {
                        transparent_black().into()
                    })
                    .bg(if active {
                        thumb_bg
                    } else {
                        transparent_black().into()
                    })
                    .when(active, |el| el.shadow_sm())
                    .text_color(if active { p.ink() } else { p.ink().opacity(0.76) })
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(label)
                    .when_some(highlight_badge, |el, badge| {
                        el.child(
                            div()
                                .px_1p5()
                                .rounded_full()
                                .bg(p.primary_fill())
                                .text_color(rgb(0xffffff))
                                .text_xs()
                                .child(badge),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tailscale.tab = tab;
                        if tab == TailscaleTab::Taildrop {
                            this.ensure_taildrop_inbox(cx);
                        }
                        cx.notify();
                    }))
            };
            div()
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .relative()
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .p_1()
                        .rounded_full()
                        .border_1()
                        .border_color(p.control_rim())
                        .shadow_sm()
                        .child(crate::glass::control_layer(p, px(999.), true))
                        .child(
                            div()
                                .relative()
                                .flex()
                                .items_center()
                                .child(tab_btn(
                                    "ts-tab-devices",
                                    TailscaleTab::Devices,
                                    format!("设备 ({online_count}/{total_count})"),
                                    None,
                                ))
                                .child(tab_btn(
                                    "ts-tab-taildrop",
                                    TailscaleTab::Taildrop,
                                    "Taildrop".into(),
                                    None,
                                ))
                                .child(tab_btn(
                                    "ts-tab-ssh",
                                    TailscaleTab::Ssh,
                                    "SSH 终端".into(),
                                    self.tailscale.ssh_ready.then(|| "1".into()),
                                )),
                        ),
                )
        };

        let body = match self.tailscale.tab {
            TailscaleTab::Devices => self.render_tailscale_devices(&ep, &active_exit_id, narrow, p, tint, cx),
            TailscaleTab::Taildrop => self.render_tailscale_taildrop(&ep, can_share_files, narrow, p, tint, cx),
            TailscaleTab::Ssh => self.render_tailscale_ssh(&ep, p, tint, cx),
        };

        let page_content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .pb_6()
            .text_color(p.ink())
            .child(header)
            .child(hero_card)
            .child(segmented)
            .child(body);

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.scroll_pane("tailscale-scroll", page_content, cx))
    }

    fn render_tailscale_devices(
        &self,
        ep: &Value,
        active_exit_id: &str,
        narrow: bool,
        p: Palette,
        tint: f32,
        cx: &Context<Self>,
    ) -> Div {
        let card = || {
            glass_card(p, true, tint)
                .w_full()
                .flex()
                .flex_col()
                .flex_none()
        };
        let pill_wash = p.card_tinted(true, tint);

        let query = self.tailscale.search_query.as_str();
        let filter = self.tailscale.filter;
        let sel_chip = adaptive_selection_chip(p, cx);
        let filter_pills = [
            (PeerFilter::All, "全部"),
            (PeerFilter::Online, "在线"),
            (PeerFilter::Exit, "出口"),
            (PeerFilter::Ssh, "SSH"),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (f, label))| {
            let selected = f == filter;
            div()
                .id(("ts-filter", i))
                .relative()
                .overflow_hidden()
                .cursor_pointer()
                .px_3()
                .py_1p5()
                .rounded_full()
                .shadow_sm()
                .child(adaptive_pill_layer(
                    p,
                    px(999.),
                    selected.then_some(sel_chip.tint),
                    if selected { sel_chip.border } else { p.control_rim() },
                ))
                .child(
                    div()
                        .relative()
                        .text_xs()
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if selected { sel_chip.ink } else { p.ink() })
                        .child(label),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tailscale.filter = f;
                    cx.notify();
                }))
        });

        let search_bar = div()
            .w_full()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .when_some(self.tailscale.search_input.as_ref(), |el, input| {
                el.child(
                    div()
                        .relative()
                        .overflow_hidden()
                        .flex_1()
                        .min_w(px(180.))
                        .rounded(px(12.))
                        .bg(pill_wash)
                        .border_1()
                        .border_color(p.rim())
                        .child(card_layer(pill_wash, px(11.), 0.68))
                        .child(div().relative().child(glass_input("ts-search", input, false, p))),
                )
            })
            .child(div().flex().items_center().gap_1p5().children(filter_pills));

        let mut group_blocks = Vec::new();
        for (gi, group) in sorted_user_groups(ep).into_iter().enumerate() {
            let display_name = group["displayName"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| group["loginName"].as_str())
                .unwrap_or("设备组")
                .to_owned();
            let login_name = group["loginName"].as_str().unwrap_or_default().to_owned();
            let matching_peers: Vec<&Value> = group["peers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|peer| {
                    let online = peer["online"].as_bool().unwrap_or(false);
                    if filter == PeerFilter::Online && !online {
                        return false;
                    }
                    if filter == PeerFilter::Exit && peer["exitNodeOption"] != true {
                        return false;
                    }
                    if filter == PeerFilter::Ssh && !peer_has_ssh(peer) {
                        return false;
                    }
                    if !query.is_empty() {
                        let host = peer_display_name(peer).to_lowercase();
                        let os = peer["os"].as_str().unwrap_or_default().to_lowercase();
                        let ip = peer_primary_ip(peer).to_lowercase();
                        if !host.contains(query) && !os.contains(query) && !ip.contains(query) {
                            return false;
                        }
                    }
                    true
                })
                .collect();
            if matching_peers.is_empty() {
                continue;
            }
            let online_in_group = matching_peers
                .iter()
                .filter(|peer| peer["online"] == true)
                .count();
            let group_key = if !login_name.is_empty() {
                login_name.clone()
            } else {
                display_name.clone()
            };
            let collapsed =
                query.is_empty() && self.tailscale.collapsed_groups.contains(&group_key);
            let mut rows = Vec::new();
            if !collapsed {
                for (pi, peer) in matching_peers.iter().enumerate() {
                    let stable_id = peer["stableId"].as_str().unwrap_or_default().to_owned();
                    let host = peer_display_name(peer);
                    let os = peer["os"].as_str().unwrap_or_default().to_owned();
                    let ip = peer_primary_ip(peer);
                    let online = peer["online"].as_bool().unwrap_or(false);
                    let is_exit = !stable_id.is_empty() && stable_id == active_exit_id;
                    let sharee = peer["shareeNode"].as_bool().unwrap_or(false);
                    let expired = peer["expired"].as_bool().unwrap_or(false);
                    let rx = peer["rxBytes"].as_i64().unwrap_or(0);
                    let tx = peer["txBytes"].as_i64().unwrap_or(0);
                    let last_seen = peer["lastSeen"].as_i64().unwrap_or(0);
                    let selected =
                        self.tailscale.selected_peer.as_deref() == Some(stable_id.as_str());
                    let click_id = stable_id.clone();

                    rows.push(
                        div()
                            .id(("ts-peer", gi * 1000 + pi))
                            .relative()
                            .overflow_hidden()
                            .cursor_pointer()
                            .rounded(px(14.))
                            .px_3()
                            .py_2()
                            .when(selected, |el| {
                                el.child(adaptive_pill_layer(
                                    p,
                                    px(14.),
                                    Some(sel_chip.tint),
                                    sel_chip.border,
                                ))
                            })
                            .when(!selected, |el| el.hover(|s| s.bg(p.hover())))
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap_2p5()
                                            .child(
                                                div()
                                                    .size(px(8.))
                                                    .rounded_full()
                                                    .bg(if online { p.success_dot() } else { p.muted() }),
                                            )
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(2.))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_wrap()
                                                            .items_center()
                                                            .gap_1p5()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .font_weight(FontWeight::SEMIBOLD)
                                                                    .child(host),
                                                            )
                                                            .when(!os.is_empty(), |el| {
                                                                el.child(
                                                                    div()
                                                                        .px_1p5()
                                                                        .rounded(px(6.))
                                                                        .bg(p.fill())
                                                                        .text_color(p.muted())
                                                                        .text_xs()
                                                                        .child(os),
                                                                )
                                                            })
                                                            .when(is_exit, |el| {
                                                                el.child(
                                                                    div()
                                                                        .px_1p5()
                                                                        .rounded_full()
                                                                        .bg(p.primary_soft())
                                                                        .text_color(p.accent())
                                                                        .text_xs()
                                                                        .child("出口中"),
                                                                )
                                                            })
                                                            .when(sharee, |el| {
                                                                el.child(
                                                                    div()
                                                                        .px_1p5()
                                                                        .rounded_full()
                                                                        .bg(p.udp_soft())
                                                                        .text_color(p.udp())
                                                                        .text_xs()
                                                                        .child("共享"),
                                                                )
                                                            })
                                                            .when(expired, |el| {
                                                                el.child(
                                                                    div()
                                                                        .px_1p5()
                                                                        .rounded_full()
                                                                        .bg(p.danger_soft())
                                                                        .text_color(p.danger())
                                                                        .text_xs()
                                                                        .child("已过期"),
                                                                )
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .text_xs()
                                                            .text_color(p.muted())
                                                            .child(div().font_family("Consolas").child(ip))
                                                            .when(!online, |el| {
                                                                el.child(format_relative_ts(last_seen))
                                                            })
                                                            .when(online && (rx > 0 || tx > 0), |el| {
                                                                el.child(format!(
                                                                    "↓ {} · ↑ {}",
                                                                    format_bytes(rx),
                                                                    format_bytes(tx)
                                                                ))
                                                            }),
                                                    ),
                                            ),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.tailscale.selected_peer.as_deref() != Some(click_id.as_str()) {
                                    if let Some(old) = this.tailscale.ping_handle.take() {
                                        this.close_stream(old);
                                    }
                                    this.tailscale.ping_peer_ip = None;
                                    this.tailscale.ping_samples.clear();
                                    this.tailscale.selected_peer = Some(click_id.clone());
                                }
                                cx.notify();
                            })),
                    );
                }
            }
            let toggle_key = group_key.clone();
            let chev_angle = if collapsed { 0.0 } else { std::f32::consts::FRAC_PI_2 };
            group_blocks.push(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .id(("ts-group-toggle", gi))
                            .cursor_pointer()
                            .rounded(px(10.))
                            .px_2p5()
                            .py_1p5()
                            .hover(|s| s.bg(p.hover()))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        svg()
                                            .path("chevron")
                                            .size(px(12.))
                                            .text_color(p.muted())
                                            .with_transformation(Transformation::rotate(radians(
                                                chev_angle,
                                            ))),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(display_name),
                                    )
                                    .when(!login_name.is_empty(), |el| {
                                        el.child(div().text_xs().text_color(p.muted()).child(login_name))
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted())
                                    .child(format!("{online_in_group}/{} 在线", matching_peers.len())),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.tailscale.collapsed_groups.remove(&toggle_key) {
                                    this.tailscale.collapsed_groups.insert(toggle_key.clone());
                                }
                                let mut saved: Vec<String> =
                                    this.tailscale.collapsed_groups.iter().cloned().collect();
                                saved.sort();
                                singboard_core::runtime().spawn(async move {
                                    if let Ok(dir) = crate::service::data_dir() {
                                        let _ = singboard_core::settings::Settings::edit(&dir, |p| {
                                            p.tailscale_collapsed_groups = saved;
                                            Ok(())
                                        });
                                    }
                                });
                                cx.notify();
                            })),
                    )
                    .children(rows),
            );
        }

        let list_card = card().p_2p5().child(
            div()
                .relative()
                .flex()
                .flex_col()
                .gap_3()
                .when(group_blocks.is_empty(), |el| {
                    el.child(
                        div()
                            .py_8()
                            .flex()
                            .justify_center()
                            .text_xs()
                            .text_color(p.muted())
                            .child("没有匹配条件的 Tailnet 设备"),
                    )
                })
                .children(group_blocks),
        );

        let selected_peer = self.tailscale.selected_peer.as_deref().and_then(|id| {
            all_peers(ep)
                .into_iter()
                .find(|peer| peer["stableId"].as_str() == Some(id))
        });

        let inspector = if let Some(peer) = selected_peer {
            let stable_id = peer["stableId"].as_str().unwrap_or_default().to_owned();
            let host = peer_display_name(peer);
            let dns = peer["dnsName"]
                .as_str()
                .unwrap_or_default()
                .trim_end_matches('.')
                .to_owned();
            let os = peer["os"].as_str().unwrap_or_default().to_owned();
            let ips: Vec<String> = peer["tailscaleIps"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            let primary_ip = ips.first().cloned().unwrap_or_default();
            let online = peer["online"].as_bool().unwrap_or(false);
            let exit_option = peer["exitNodeOption"].as_bool().unwrap_or(false);
            let is_exit = !stable_id.is_empty() && stable_id == active_exit_id;
            let has_ssh = peer_has_ssh(peer);
            let can_recv = peer["canReceiveFiles"].as_bool().unwrap_or(false);
            let rx = peer["rxBytes"].as_i64().unwrap_or(0);
            let tx = peer["txBytes"].as_i64().unwrap_or(0);
            let key_expiry = peer["keyExpiry"].as_i64().unwrap_or(0);
            let expired = peer["expired"].as_bool().unwrap_or(false);
            let ssh_key_count = peer["sshHostKeys"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);

            let pinging = self.tailscale.ping_handle.is_some()
                && self.tailscale.ping_peer_ip.as_deref() == Some(primary_ip.as_str());
            let last_ping = self.tailscale.ping_samples.last().cloned();
            let ping_color = match last_ping.as_ref() {
                Some(s) if s.is_direct => p.success_dot(),
                Some(s) if !s.peer_relay.is_empty() => p.udp(),
                Some(_) => p.warning_dot(),
                None => p.accent(),
            };

            let exit_click_id = if is_exit {
                String::new()
            } else {
                stable_id.clone()
            };
            let ssh_click_id = stable_id.clone();
            let send_click_id = stable_id.clone();
            let ping_click_ip = primary_ip.clone();

            card().p_4().child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .size(px(9.))
                                            .rounded_full()
                                            .bg(if online { p.success_dot() } else { p.muted() }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(host),
                                    )
                                    .when(!os.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .px_2()
                                                .py(px(1.))
                                                .rounded(px(6.))
                                                .bg(p.fill())
                                                .text_xs()
                                                .text_color(p.muted())
                                                .child(os),
                                        )
                                    }),
                            )
                            .when(!dns.is_empty(), |el| {
                                el.child(
                                    div()
                                        .font_family("Consolas")
                                        .text_xs()
                                        .text_color(p.muted())
                                        .truncate()
                                        .child(dns),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1p5()
                            .when(exit_option, |el| {
                                el.child(
                                    div()
                                        .id("ts-peer-exit")
                                        .cursor_pointer()
                                        .px_3()
                                        .py_1()
                                        .rounded_full()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .bg(if is_exit {
                                            p.danger_soft()
                                        } else {
                                            p.primary_fill()
                                        })
                                        .text_color(if is_exit {
                                            p.danger()
                                        } else {
                                            rgb(0xffffff)
                                        })
                                        .child(if is_exit { "停用出口" } else { "设为出口" })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_tailscale_exit_node(exit_click_id.clone(), cx);
                                        })),
                                )
                            })
                            .when(has_ssh && online, |el| {
                                el.child(
                                    div()
                                        .id("ts-peer-ssh")
                                        .cursor_pointer()
                                        .px_3()
                                        .py_1()
                                        .rounded_full()
                                        .bg(p.fill())
                                        .hover(|s| s.bg(p.fill_strong()))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child("SSH 终端")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.tailscale.ssh_peer = Some(ssh_click_id.clone());
                                            this.tailscale.tab = TailscaleTab::Ssh;
                                            cx.notify();
                                        })),
                                )
                            })
                            .when(can_recv, |el| {
                                el.child(
                                    div()
                                        .id("ts-peer-send")
                                        .cursor_pointer()
                                        .px_3()
                                        .py_1()
                                        .rounded_full()
                                        .bg(p.fill())
                                        .hover(|s| s.bg(p.fill_strong()))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child("发送文件")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.tailscale.upload_target = Some(send_click_id.clone());
                                            this.tailscale.tab = TailscaleTab::Taildrop;
                                            this.ensure_taildrop_inbox(cx);
                                            cx.notify();
                                        })),
                                )
                            }),
                    )
                    .child(
                        div()
                            .rounded(px(14.))
                            .bg(p.tile())
                            .border_1()
                            .border_color(p.line())
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("实时链路 Ping"),
                                    )
                                    .when(!primary_ip.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .id("ts-toggle-ping")
                                                .cursor_pointer()
                                                .px_2p5()
                                                .py(px(3.))
                                                .rounded_full()
                                                .bg(if pinging {
                                                    p.primary_soft()
                                                } else {
                                                    p.fill()
                                                })
                                                .text_color(if pinging {
                                                    p.accent()
                                                } else {
                                                    p.ink()
                                                })
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(if pinging { "停止探测" } else { "开始 Ping" })
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.start_or_stop_ping(
                                                        ping_click_ip.clone(),
                                                        cx,
                                                    );
                                                })),
                                        )
                                    }),
                            )
                            .when_some(last_ping, |el, sample| {
                                let (badge_bg, badge_fg, badge_text, path_text) =
                                    if !sample.error.is_empty() {
                                        (
                                            p.danger_soft(),
                                            p.danger(),
                                            "探测失败".into(),
                                            sample.error.clone(),
                                        )
                                    } else if sample.is_direct {
                                        (
                                            p.success_soft(),
                                            p.success_ink(),
                                            "直连 Direct".into(),
                                            format!("对端 {}", sample.endpoint),
                                        )
                                    } else if !sample.peer_relay.is_empty() {
                                        (
                                            p.udp_soft(),
                                            p.udp(),
                                            "Peer Relay".into(),
                                            format!("中继 {}", sample.peer_relay),
                                        )
                                    } else {
                                        (
                                            p.warning_soft(),
                                            p.warning_ink(),
                                            format!(
                                                "DERP · {} #{}",
                                                sample.derp_region_code, sample.derp_region_id
                                            ),
                                            "经由 DERP 中继转发".into(),
                                        )
                                    };
                                el.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .px_2()
                                                .py(px(2.))
                                                .rounded_full()
                                                .bg(badge_bg)
                                                .text_color(badge_fg)
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(badge_text),
                                        )
                                        .when(sample.error.is_empty(), |r| {
                                            r.child(
                                                div()
                                                    .font_family("Consolas")
                                                    .text_sm()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(format!("{:.1} ms", sample.latency_ms)),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .font_family("Consolas")
                                        .text_xs()
                                        .text_color(p.muted())
                                        .truncate()
                                        .child(path_text),
                                )
                                .child(ping_sparkline(&self.tailscale.ping_samples, ping_color))
                            })
                            .when(self.tailscale.ping_samples.is_empty(), |el| {
                                el.child(div().text_xs().text_color(p.muted()).child(if pinging {
                                    "正在等待首个探测响应…"
                                } else {
                                    "点击「开始 Ping」实时检测直连或 DERP 链路。"
                                }))
                            }),
                    )
                    .child(
                        div()
                            .pt_2()
                            .border_t_1()
                            .border_color(p.line())
                            .flex()
                            .flex_col()
                            .gap_2()
                            .text_xs()
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap_2()
                                    .child(div().text_color(p.muted()).child("Tailscale IP"))
                                    .child(div().flex().flex_col().items_end().gap_1().children(
                                        ips.into_iter().enumerate().map(|(i, ip)| {
                                            let ip_copy = ip.clone();
                                            div()
                                                .id(("ts-peer-ip", i))
                                                .cursor_pointer()
                                                .font_family("Consolas")
                                                .child(ip)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(ip_copy.clone()),
                                                    );
                                                    this.message("已复制设备 IP".into(), false, cx);
                                                }))
                                        }),
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .child(div().text_color(p.muted()).child("累计流量"))
                                    .child(div().font_family("Consolas").child(format!(
                                        "↓ {} · ↑ {}",
                                        format_bytes(rx),
                                        format_bytes(tx)
                                    ))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .child(div().text_color(p.muted()).child("密钥有效期"))
                                    .child(
                                        div()
                                            .text_color(if expired { p.danger() } else { p.ink() })
                                            .child(if expired {
                                                "已过期".into()
                                            } else if key_expiry <= 0 {
                                                "永不过期".into()
                                            } else {
                                                format_relative_ts(key_expiry)
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .child(div().text_color(p.muted()).child("SSH 主机密钥"))
                                    .child(if ssh_key_count > 0 {
                                        format!("{ssh_key_count} 组已公布")
                                    } else {
                                        "未启用".into()
                                    }),
                            ),
                    ),
            )
        } else {
            card().p_5().child(
                div()
                    .relative()
                    .text_xs()
                    .text_color(p.muted())
                    .child("选择左侧设备查看详情与链路 Ping 诊断。"),
            )
        };

        div()
            .w_full()
            .flex_none()
            .flex()
            .when(narrow, |d| d.flex_col().gap_4())
            .when(!narrow, |d| d.items_start().gap_4())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(search_bar)
                    .child(list_card),
            )
            .child(div().when(!narrow, |d| d.w(px(330.)).flex_none()).child(inspector))
    }

    fn render_tailscale_taildrop(
        &self,
        ep: &Value,
        can_share_files: bool,
        narrow: bool,
        p: Palette,
        tint: f32,
        cx: &Context<Self>,
    ) -> Div {
        let card = || {
            glass_card(p, true, tint)
                .w_full()
                .flex()
                .flex_col()
                .flex_none()
        };

        let targets: Vec<(String, String, bool)> = all_peers(ep)
            .into_iter()
            .filter(|peer| can_receive_taildrop(peer))
            .filter_map(|peer| {
                Some((
                    peer["stableId"].as_str()?.to_owned(),
                    peer_display_name(peer),
                    peer["online"].as_bool().unwrap_or(false),
                ))
            })
            .collect();

        let selected_target_name = targets
            .iter()
            .find(|(id, _, _)| self.tailscale.upload_target.as_deref() == Some(id.as_str()))
            .or_else(|| targets.first())
            .map(|(_, host, _)| host.clone());
        let sel_chip = adaptive_selection_chip(p, cx);
        let no_targets = targets.is_empty();
        let upload_busy =
            self.tailscale.upload_opening || self.tailscale.upload_handle.is_some();
        let upload_disabled = !can_share_files || no_targets || upload_busy;
        let drag_tint = sel_chip.tint;
        let drag_border = sel_chip.border;

        let send_card = card()
            .p_4()
            .when(!upload_disabled, |el| {
                el.on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                    this.upload_taildrop_paths(paths.paths().to_vec(), cx);
                }))
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("发送文件 (Taildrop)"),
                    )
                    .when(!can_share_files, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(p.muted())
                                .child("当前 Tailnet 未启用 Taildrop 文件传输。"),
                        )
                    })
                    .when(can_share_files, |el| {
                        el.child(div().text_xs().text_color(p.muted()).child("选择接收设备"))
                            .when(no_targets, |el| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .text_color(p.muted())
                                        .child("当前 Tailnet 中没有可接收文件的设备。"),
                                )
                            })
                            .when(!no_targets, |el| {
                                el.child(div().flex().flex_wrap().gap_1p5().children(
                                    targets.into_iter().enumerate().map(|(i, (id, host, online))| {
                                        let selected =
                                            self.tailscale.upload_target.as_deref() == Some(id.as_str());
                                        div()
                                            .id(("ts-upload-target", i))
                                            .relative()
                                            .overflow_hidden()
                                            .cursor_pointer()
                                            .px_3()
                                            .py_1()
                                            .rounded_full()
                                            .shadow_sm()
                                            .child(adaptive_pill_layer(
                                                p,
                                                px(999.),
                                                selected.then_some(sel_chip.tint),
                                                if selected { sel_chip.border } else { p.control_rim() },
                                            ))
                                            .child(
                                                div()
                                                    .relative()
                                                    .text_xs()
                                                    .font_weight(if selected {
                                                        FontWeight::SEMIBOLD
                                                    } else {
                                                        FontWeight::MEDIUM
                                                    })
                                                    .text_color(if selected {
                                                        sel_chip.ink
                                                    } else {
                                                        p.ink()
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .child(
                                                        div()
                                                            .size(px(7.))
                                                            .rounded_full()
                                                            .bg(if online {
                                                                p.success_dot()
                                                            } else {
                                                                p.muted()
                                                            }),
                                                    )
                                                    .child(host),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.tailscale.upload_target = Some(id.clone());
                                                cx.notify();
                                            }))
                                    }),
                                ))
                            })
                            .when_some(self.tailscale.upload_progress.as_ref(), |el, prog| {
                                let pct = if prog.total_bytes > 0 {
                                    (prog.sent_bytes as f32 / prog.total_bytes as f32).clamp(0., 1.)
                                } else {
                                    0.
                                };
                                el.child(
                                    div()
                                        .rounded(px(14.))
                                        .bg(p.tile())
                                        .p_3()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .child(div().text_xs().child(format!(
                                            "正在发送到 {} · {}/{} ({}/{})",
                                            prog.target_name,
                                            prog.completed_files,
                                            prog.file_count,
                                            format_bytes(prog.sent_bytes),
                                            format_bytes(prog.total_bytes)
                                        )))
                                        .child(
                                            div().w_full().h(px(6.)).rounded_full().bg(p.fill()).child(
                                                div()
                                                    .h_full()
                                                    .w(relative(pct))
                                                    .rounded_full()
                                                    .bg(p.primary_fill()),
                                            ),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .id("ts-pick-upload")
                                    .relative()
                                    .overflow_hidden()
                                    .rounded(px(16.))
                                    .px_4()
                                    .py_5()
                                    .bg(p.tile())
                                    .border_1()
                                    .border_dashed()
                                    .border_color(p.control_rim())
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_1p5()
                                    .when(upload_disabled, |el| el.opacity(0.48))
                                    .when(!upload_disabled, |el| {
                                        el.cursor_pointer()
                                            .hover(|s| s.bg(p.hover()))
                                            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                                                style.bg(drag_tint).border_color(drag_border)
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.pick_and_upload_taildrop(cx)
                                            }))
                                    })
                                    .child(
                                        div()
                                            .relative()
                                            .overflow_hidden()
                                            .size(px(34.))
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .shadow_sm()
                                            .child(adaptive_pill_layer(
                                                p,
                                                px(999.),
                                                None,
                                                p.control_rim(),
                                            ))
                                            .child(
                                                svg()
                                                    .path("upload")
                                                    .size(px(16.))
                                                    .text_color(p.ink()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.ink())
                                            .child(if upload_busy {
                                                "正在发送文件…"
                                            } else if no_targets {
                                                "暂无可接收设备"
                                            } else {
                                                "拖拽文件到此处，或点击选择"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(p.muted())
                                            .child(match selected_target_name {
                                                Some(name) if !upload_busy => {
                                                    format!("支持多选 · 将发送到 {name}")
                                                }
                                                _ => "支持多选或从资源管理器直接拖入".into(),
                                            }),
                                    ),
                            )
                    }),
            );

        let receiving = self
            .tailscale
            .inbox
            .as_ref()
            .and_then(|v| v["receiving"].as_array())
            .cloned()
            .unwrap_or_default();
        let files = self
            .tailscale
            .inbox
            .as_ref()
            .and_then(|v| v["files"].as_array())
            .cloned()
            .unwrap_or_default();

        let inbox_empty = files.is_empty() && receiving.is_empty();
        let dl_state = self.tailscale.download_progress.as_ref().map(|prog| {
            let pct = if prog.total_bytes > 0 && prog.received_bytes > 0 {
                Some(
                    ((prog.received_bytes as f32 / prog.total_bytes as f32).clamp(0., 1.) * 100.)
                        .round() as u32,
                )
            } else {
                None
            };
            (prog.name.clone(), pct)
        });
        let inbox_card = card().p_4().child(
            div()
                .relative()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("收件箱"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted())
                                .child(if self.tailscale.inbox.is_some() {
                                    format!("已接收 {} 个文件", files.len())
                                } else {
                                    "等待读取收件箱".into()
                                }),
                        ),
                )
                .when(!files.is_empty(), |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(p.muted())
                            .child("文件已保存在此端点的 Taildrop 接收目录。"),
                    )
                })
                .children(receiving.into_iter().enumerate().map(|(i, item)| {
                    let name = item["name"].as_str().unwrap_or_default().to_owned();
                    let sender_id = item["senderId"].as_str().unwrap_or_default().to_owned();
                    let sender = item["senderName"].as_str().unwrap_or("未知设备").to_owned();
                    let size = item["size"].as_i64().unwrap_or(0);
                    let recv = item["receivedBytes"].as_i64().unwrap_or(0);
                    let pct = if size > 0 {
                        (recv as f32 / size as f32).clamp(0., 1.)
                    } else {
                        0.
                    };
                    let cancel_name = name.clone();
                    div()
                        .rounded(px(14.))
                        .bg(p.tile())
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .truncate()
                                                .child(name),
                                        )
                                        .child(div().text_xs().text_color(p.muted()).child(format!(
                                            "正在接收 · 来自 {sender} · {} / {}",
                                            format_bytes(recv),
                                            format_bytes(size)
                                        ))),
                                )
                                .child(
                                    div()
                                        .id(("ts-cancel-recv", i))
                                        .cursor_pointer()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_full()
                                        .bg(p.danger_soft())
                                        .text_color(p.danger())
                                        .text_xs()
                                        .child("取消")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.cancel_taildrop_receiving(
                                                sender_id.clone(),
                                                cancel_name.clone(),
                                                cx,
                                            );
                                        })),
                                ),
                        )
                        .child(
                            div().w_full().h(px(6.)).rounded_full().bg(p.fill()).child(
                                div()
                                    .h_full()
                                    .w(relative(pct))
                                    .rounded_full()
                                    .bg(p.success_dot()),
                            ),
                        )
                }))
                .when(self.tailscale.inbox.is_some() && inbox_empty, |el| {
                    el.child(
                        div()
                            .py_6()
                            .flex()
                            .justify_center()
                            .text_xs()
                            .text_color(p.muted())
                            .child("收件箱为空"),
                    )
                })
                .children(files.into_iter().enumerate().map(|(i, file)| {
                    let name = file["name"].as_str().unwrap_or_default().to_owned();
                    let size = file["size"].as_i64().unwrap_or(0);
                    let sender = file["senderName"].as_str().unwrap_or_default().to_owned();
                    let modified = file["modifiedAt"].as_i64().unwrap_or(0);
                    let is_saving_this = dl_state.as_ref().is_some_and(|(n, _)| n == &name);
                    let saving_busy = dl_state.is_some();
                    let save_label = match (&dl_state, is_saving_this) {
                        (Some((_, Some(pct))), true) => format!("另存中 {pct}%"),
                        (Some((_, None)), true) => "另存中…".into(),
                        _ => "另存为".into(),
                    };
                    let dl_name = name.clone();
                    let del_name = name.clone();
                    div()
                        .pt_2p5()
                        .border_t_1()
                        .border_color(p.line())
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .truncate()
                                        .child(name),
                                )
                                .child(div().text_xs().text_color(p.muted()).child(format!(
                                    "{} · {} · {}",
                                    if sender.is_empty() { "Taildrop" } else { &sender },
                                    format_bytes(size),
                                    format_relative_ts(modified)
                                ))),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .id(("ts-dl-file", i))
                                        .when(!saving_busy, |d| d.cursor_pointer())
                                        .when(saving_busy && !is_saving_this, |d| d.opacity(0.45))
                                        .px_2p5()
                                        .py_1()
                                        .rounded_full()
                                        .bg(if is_saving_this {
                                            sel_chip.tint
                                        } else {
                                            p.primary_soft()
                                        })
                                        .text_color(if is_saving_this {
                                            sel_chip.ink
                                        } else {
                                            p.accent()
                                        })
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(save_label)
                                        .when(!saving_busy, |d| {
                                            d.on_click(cx.listener(move |this, _, window, cx| {
                                                this.download_taildrop_file(
                                                    dl_name.clone(),
                                                    size,
                                                    window,
                                                    cx,
                                                );
                                            }))
                                        }),
                                )
                                .child(
                                    div()
                                        .id(("ts-del-file", i))
                                        .cursor_pointer()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_full()
                                        .bg(p.fill())
                                        .text_color(p.danger())
                                        .text_xs()
                                        .child("删除原文件")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.delete_taildrop_file(del_name.clone(), cx);
                                        })),
                                ),
                        )
                })),
        );

        div()
            .w_full()
            .flex_none()
            .flex()
            .when(narrow, |d| d.flex_col().gap_4())
            .when(!narrow, |d| d.items_start().gap_4())
            .child(div().when(!narrow, |d| d.w(px(320.)).flex_none()).child(send_card))
            .child(div().flex_1().min_w_0().child(inbox_card))
    }

    fn render_tailscale_ssh(
        &self,
        ep: &Value,
        p: Palette,
        tint: f32,
        cx: &Context<Self>,
    ) -> Div {
        let card = || {
            glass_card(p, true, tint)
                .w_full()
                .flex()
                .flex_col()
                .flex_none()
        };

        let ssh_peers: Vec<(String, String, bool)> = all_peers(ep)
            .into_iter()
            .filter(|peer| peer_has_ssh(peer))
            .filter_map(|peer| {
                Some((
                    peer["stableId"].as_str()?.to_owned(),
                    peer_display_name(peer),
                    peer["online"].as_bool().unwrap_or(false),
                ))
            })
            .collect();

        let connected = self.tailscale.ssh_handle.is_some();
        let has_ssh_peers = !ssh_peers.is_empty();
        let can_connect = connected || has_ssh_peers;
        let sel_chip = adaptive_selection_chip(p, cx);
        let top_bar = card().p_3().child(
            div()
                .relative()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_1p5()
                        .when(!has_ssh_peers, |el| {
                            el.child(
                                div()
                                    .relative()
                                    .overflow_hidden()
                                    .px_2p5()
                                    .py_1()
                                    .rounded_full()
                                    .child(adaptive_pill_layer(p, px(999.), None, p.control_rim()))
                                    .child(
                                        div()
                                            .relative()
                                            .text_xs()
                                            .text_color(p.muted())
                                            .child("暂无支持 Tailscale SSH 的设备"),
                                    ),
                            )
                        })
                        .children(ssh_peers.into_iter().enumerate().map(|(i, (id, host, online))| {
                            let selected = self.tailscale.ssh_peer.as_deref() == Some(id.as_str());
                            div()
                                .id(("ts-ssh-peer", i))
                                .relative()
                                .overflow_hidden()
                                .cursor_pointer()
                                .px_2p5()
                                .py_1()
                                .rounded_full()
                                .shadow_sm()
                                .child(adaptive_pill_layer(
                                    p,
                                    px(999.),
                                    selected.then_some(sel_chip.tint),
                                    if selected { sel_chip.border } else { p.control_rim() },
                                ))
                                .child(
                                    div()
                                        .relative()
                                        .text_xs()
                                        .font_weight(if selected {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::MEDIUM
                                        })
                                        .text_color(if selected { sel_chip.ink } else { p.ink() })
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(
                                            div()
                                                .size(px(7.))
                                                .rounded_full()
                                                .bg(if online { p.success_dot() } else { p.muted() }),
                                        )
                                        .child(host),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.tailscale.ssh_peer = Some(id.clone());
                                    cx.notify();
                                }))
                        }))
                        .when_some(self.tailscale.ssh_user_input.as_ref(), |el, input| {
                            el.child(div().w(px(110.)).child(glass_input("ts-ssh-user", input, true, p)))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(!self.tailscale.ssh_status.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted())
                                    .child(self.tailscale.ssh_status.clone()),
                            )
                        })
                        .child(
                            div()
                                .id("ts-ssh-connect")
                                .px_3p5()
                                .py_1p5()
                                .rounded_full()
                                .bg(if connected {
                                    p.danger_soft()
                                } else {
                                    p.primary_fill()
                                })
                                .text_color(if connected { p.danger() } else { rgb(0xffffff) })
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(if connected { "断开会话" } else { "连接 SSH" })
                                .when(!can_connect, |el| el.opacity(0.45))
                                .when(can_connect, |el| {
                                    el.cursor_pointer().on_click(cx.listener(|this, _, _, cx| {
                                        this.connect_or_disconnect_ssh(cx);
                                    }))
                                }),
                        ),
                ),
        );

        let terminal_text = if self.tailscale.ssh_output.is_empty() {
            if connected {
                "等待终端输出…".to_owned()
            } else if has_ssh_peers {
                "选择支持 Tailscale SSH 的设备后点击「连接 SSH」。".to_owned()
            } else {
                "当前 Tailnet 中没有启用 Tailscale SSH 的设备。".to_owned()
            }
        } else {
            self.tailscale.ssh_output.clone()
        };

        let term_box = card().p_3().gap_2p5().child(
            div()
                .relative()
                .flex()
                .flex_col()
                .gap_2p5()
                .child(
                    div()
                        .rounded(px(12.))
                        .overflow_hidden()
                        .bg(rgba(0x0f121ae8))
                        .border_1()
                        .border_color(rgba(0xffffff22))
                        .flex()
                        .flex_col()
                        .when(!self.tailscale.ssh_banner.is_empty(), |el| {
                            el.child(
                                div()
                                    .px_3p5()
                                    .py_2()
                                    .bg(rgba(0x0a84ff24))
                                    .border_b_1()
                                    .border_color(rgba(0xffffff18))
                                    .text_xs()
                                    .text_color(rgb(0x8cc4ff))
                                    .child(self.tailscale.ssh_banner.clone()),
                            )
                        })
                        .child(
                            div()
                                .id("ts-ssh-out")
                                .h(px(300.))
                                .overflow_y_scroll()
                                .p_3p5()
                                .font_family("Consolas")
                                .text_xs()
                                .text_color(rgb(0xe6edf3))
                                .child(terminal_text),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when_some(self.tailscale.ssh_cmd_input.as_ref(), |el, input| {
                            el.child(div().flex_1().min_w_0().child(glass_input("ts-ssh-cmd", input, true, p)))
                        })
                        .child(
                            div()
                                .id("ts-ssh-send")
                                .px_3()
                                .py_1p5()
                                .rounded_full()
                                .bg(p.primary_fill())
                                .text_color(rgb(0xffffff))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .child("发送")
                                .when(!connected, |el| el.opacity(0.45))
                                .when(connected, |el| {
                                    el.cursor_pointer().on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(input) = this.tailscale.ssh_cmd_input.clone() {
                                            let cmd = input.read(cx).value().to_string();
                                            this.send_ssh_bytes(format!("{cmd}\n").into_bytes(), cx);
                                            input.update(cx, |i, cx| {
                                                i.set_value(String::new(), window, cx)
                                            });
                                        }
                                    }))
                                }),
                        )
                        .child(
                            div()
                                .id("ts-ssh-ctrlc")
                                .px_2p5()
                                .py_1p5()
                                .rounded_full()
                                .bg(p.fill())
                                .text_color(p.ink())
                                .text_xs()
                                .font_family("Consolas")
                                .child("Ctrl+C")
                                .when(!connected, |el| el.opacity(0.45))
                                .when(connected, |el| {
                                    el.cursor_pointer()
                                        .hover(|s| s.bg(p.fill_strong()))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.send_ssh_bytes(vec![0x03], cx);
                                        }))
                                }),
                        )
                        .child(
                            div()
                                .id("ts-ssh-clear")
                                .cursor_pointer()
                                .px_2p5()
                                .py_1p5()
                                .rounded_full()
                                .bg(p.fill())
                                .hover(|s| s.bg(p.fill_strong()))
                                .text_color(p.ink())
                                .text_xs()
                                .child("清屏")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.tailscale.ssh_output.clear();
                                    cx.notify();
                                })),
                        ),
                ),
        );

        div().w_full().flex_none().flex().flex_col().gap_3().child(top_bar).child(term_box)
    }
}
