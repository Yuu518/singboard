use crate::service::CoreState;
use gpui::{prelude::*, *};
use serde_json::Value;
use singboard_core::dashboard::{self, LEVELS, SORTS};
use singboard_core::{
    Feed, Subscription, Traffic,
    clash::{Clash, Command, Group},
};
use std::{collections::VecDeque, sync::Arc};

mod config;
mod connections;
mod dialog;
mod logs;
mod overview;
mod proxies;
mod rules;
mod settings;
mod scrolling;
mod tailscale;
mod toast;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Proxies,
    Connections,
    Logs,
    Rules,
    Tailscale,
    Config,
    Settings,
}

const OVERVIEW_SAMPLES: usize = 60;

pub const PAGES: [(Page, &str); 8] = [
    (Page::Overview, "概览"),
    (Page::Proxies, "代理"),
    (Page::Connections, "连接"),
    (Page::Logs, "日志"),
    (Page::Rules, "规则"),
    (Page::Tailscale, "Tailscale"),
    (Page::Config, "配置"),
    (Page::Settings, "设置"),
];

#[derive(Clone)]
enum Action {
    Refresh,
    ConnectionSort(usize),
    Detail(Value),
    Geo(String),
    Dismiss,
    RuleDetail(String),
    CloseRuleDetail,
    InspectRule(usize),
    CloseRuleInspect,
    Providers,
    Sort,
    History,
    Pause,
    Clear,
    UndoClear,
    Run(Command),
    Confirm(Command),
    Cancel,
}

#[derive(Clone)]
pub(super) struct LogRecord {
    pub seq: u64,
    pub time: String,
    pub level: String,
    pub payload: String,
    pub marker: bool,
}

fn normalize_core_level(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "warning" => "warn".into(),
        "panic" | "fatal" | "error" | "warn" | "info" | "debug" | "trace" => {
            raw.trim().to_ascii_lowercase()
        }
        _ => "info".into(),
    }
}

#[repr(C)]
struct WinSystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

unsafe extern "system" {
    fn GetLocalTime(lp: *mut WinSystemTime);
}

fn local_time_parts() -> WinSystemTime {
    let mut st = WinSystemTime {
        year: 0,
        month: 0,
        day_of_week: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        milliseconds: 0,
    };
    unsafe {
        GetLocalTime(&mut st);
    }
    st
}

fn local_hhmmss() -> String {
    let st = local_time_parts();
    format!("{:02}:{:02}:{:02}", st.hour, st.minute, st.second)
}

pub(super) fn now_iso() -> String {
    let st = local_time_parts();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        st.year, st.month, st.day, st.hour, st.minute, st.second
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ApiState {
    Connected,
    Disconnected,
    Waiting,
    Unreachable,
    AuthFailed,
}

impl ApiState {
    fn label(self) -> &'static str {
        match self {
            Self::Connected => "API 已连接",
            Self::Disconnected => "API 未连接",
            Self::Waiting => "等待 Clash API",
            Self::Unreachable => "API 端点不可达",
            Self::AuthFailed => "API 认证失败",
        }
    }
}

pub struct Workspace {
    pub custom_wallpaper: bool,
    pub glass_tint: f32,
    pub page: Page,
    pub traffic_status: &'static str,
    traffic_failures: u32,
    conn_failures: u32,
    has_traffic: bool,
    pub traffic_samples: VecDeque<Traffic>,
    pub local_state: CoreState,
    core_error: Option<String>,
    service_settled: bool,
    service_syncing: bool,
    traffic_reconnect: bool,
    service_checked: Option<std::time::Instant>,
    service_uptime: Option<(u64, std::time::Instant)>,
    uptime_tick: u64,
    api: Clash,
    api_enabled: bool,
    suspended: bool,
    initialized: bool,
    pending_update_check: bool,
    log_feed: Option<Subscription>,
    conn_feed: Option<Subscription>,
    groups: Vec<Group>,
    group_test_urls: std::collections::HashMap<String, String>,
    latency_test_url: String,
    expanded: std::collections::HashSet<String>,
    expanded_providers: std::collections::HashSet<String>,
    proxy_ui: proxies::ProxyUi,
    sort: bool,
    settings_layout: settings::SettingsLayout,
    segment_sliders: std::cell::RefCell<std::collections::HashMap<&'static str, settings::SegmentSlider>>,
    tailscale: tailscale::TailscaleState,
    filter: Option<Entity<crate::search::Search>>,
    local: Option<crate::service::Snapshot>,
    modes: Vec<String>,
    mode: String,
    endpoint: String,
    secret: String,
    undo: Option<(String, String, std::time::Instant)>,
    last_read: std::time::Instant,
    rules: Vec<Value>,
    rule_browser: rules::RuleBrowser,
    providers: Vec<(String, Value)>,
    updating_providers: std::collections::HashSet<String>,
    updating_all_providers: Option<(usize, usize)>,
    show_providers: bool,
    snapshot: Option<Arc<Value>>,
    closed: VecDeque<Value>,
    history: bool,
    logs: VecDeque<LogRecord>,
    log_seq: u64,
    expanded_logs: std::collections::HashSet<u64>,
    log_focus: Option<FocusHandle>,
    log_active_row: Option<u64>,
    pub(super) page_scroll: ScrollHandle,
    scroll_motion: Option<scrolling::SmoothScroll>,
    page_list_scroll: UniformListScrollHandle,
    conn_detail_scroll: ScrollHandle,
    log_list: ListState,
    log_list_len: usize,
    log_list_head: Option<u64>,
    log_list_filter: (String, usize, String, bool, u32),
    log_auto_scroll: bool,
    log_scroll_bound: bool,
    log_subscribed_level: String,
    log_gap_since: Option<String>,
    log_failures: usize,
    clear_gen: u64,
    cleared: VecDeque<LogRecord>,
    cleared_at: Option<std::time::Instant>,
    frozen: Option<VecDeque<LogRecord>>,
    frozen_connections: Option<(Option<Arc<Value>>, VecDeque<Value>)>,
    conn_ui: connections::ConnectionUi,
    paused_count: usize,
    log_level: usize,
    core_level: String,
    sort_key: usize,
    detail: Option<Value>,
    geo: Option<Result<Value, String>>,
    pending_restart: Option<String>,
    config_busy: Option<String>,
    last_profile_auto_check: Option<std::time::Instant>,
    auto_updating_profiles: bool,
    config_editor: Option<Entity<crate::editor::Editor>>,
    config_two_columns: bool,
    config_dialog: Option<config::ConfigDialog>,
    leaving_dialog: Option<(std::time::Instant, dialog::LeavingDialog)>,
    pub summary: Value,
    pub active_profile: String,
    active_profile_failed: bool,
    ipv6: bool,
    ipv6_nodes: std::collections::HashSet<String>,
    chart: bool,
    memory: Option<u64>,
    history_counts: VecDeque<u64>,
    history_memory: VecDeque<u64>,
    frozen_topology: Option<Value>,
    hovered_topology: Option<Value>,
    topology_pointer: Option<(f32, f32)>,
    topology_previous: Option<(Value, std::time::Instant)>,
    domestic_info: Option<Value>,
    domestic_busy: bool,
    domestic_checked: bool,
    domestic_updated_at: Option<u64>,
    domestic_gen: u64,
    abroad_info: Option<Value>,
    abroad_busy: bool,
    abroad_checked: bool,
    abroad_updated_at: Option<u64>,
    abroad_fingerprint: String,
    abroad_gen: u64,
    sites_info: Option<Value>,
    sites_busy: bool,
    sites_checked: bool,
    sites_updated_at: Option<u64>,
    sites_gen: u64,
    show_ip: bool,
    exits_due: Option<std::time::Instant>,
    sites_due: Option<std::time::Instant>,
    proxy_dirty: bool,
    proxy_due: Option<std::time::Instant>,
    network_cache_loaded_for: String,
    local_was_running: bool,
    log_status: &'static str,
    conn_status: &'static str,
    confirm: Option<Command>,
    page_dialog_focus: Option<(FocusHandle, Option<FocusHandle>)>,
    load_error: String,
    pub(crate) toasts: Option<Entity<toast::ToastHost>>,
    busy: bool,
    refreshing: bool,
    generation: u64,
}

impl Workspace {
    pub fn switch_endpoint(
        &mut self,
        base: &str,
        secret: &str,
        prefs: &singboard_core::settings::Settings,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let api = Clash::new(base, secret)?;
        let log_level = self.effective_log_level();
        let (log_feed, conn_feed) = if !self.suspended {
            (
                Some(Subscription::start_logs(base, secret, &log_level)?),
                Some(Subscription::start(base, secret, Feed::Connections)?),
            )
        } else {
            (None, None)
        };
        self.api = api;
        self.api_enabled = true;
        self.endpoint = base.into();
        self.secret = secret.into();
        self.log_feed = log_feed;
        self.conn_feed = conn_feed;
        self.log_subscribed_level = log_level;
        self.generation += 1;
        self.busy = false;
        self.refreshing = false;
        self.updating_providers.clear();
        self.updating_all_providers = None;
        self.proxy_ui.clear_testing();
        self.groups.clear();
        self.rules.clear();
        self.providers.clear();
        self.snapshot = None;
        self.summary = Value::Null;
        self.load_error.clear();
        self.traffic_status = "连接中";
        self.log_status = "连接中";
        self.conn_status = "连接中";
        self.traffic_failures = 0;
        self.conn_failures = 0;
        self.log_failures = 0;
        self.has_traffic = false;
        self.traffic_samples.clear();
        self.reset_network_state(false);
        self.preferences(prefs, cx);
        let page = self.page;
        self.open(page, cx);
        Ok(())
    }

    pub fn push_traffic(
        &mut self,
        status: &'static str,
        failures: u32,
        has_frame: bool,
        samples: VecDeque<Traffic>,
        cx: &mut Context<Self>,
    ) {
        if self.suspended {
            return;
        }
        let changed = status != self.traffic_status || failures != self.traffic_failures
            || has_frame != self.has_traffic || !samples.is_empty();
        self.traffic_status = status;
        self.traffic_failures = failures;
        self.has_traffic = has_frame;
        if has_frame && !samples.is_empty() {
            self.traffic_samples.extend(samples);
            while self.traffic_samples.len() > OVERVIEW_SAMPLES {
                self.traffic_samples.pop_front();
            }
        }
        if changed {
            cx.notify();
        }
    }

    pub fn preferences(
        &mut self,
        prefs: &singboard_core::settings::Settings,
        cx: &mut Context<Self>,
    ) {
        self.active_profile = prefs.profile_name().into();
        self.active_profile_failed = prefs
            .config_profiles
            .iter()
            .find(|p| p.id == prefs.active_config_profile_id)
            .is_some_and(|p| !p.last_fetch_error.trim().is_empty());
        self.ipv6 = prefs.ipv6_test_enabled;
        if self.latency_test_url != prefs.latency_test_url
            || self.group_test_urls != prefs.group_test_urls
        {
            self.latency_test_url = prefs.latency_test_url.clone();
            self.group_test_urls = prefs.group_test_urls.clone();
            if self.page == Page::Proxies {
                self.refresh(cx);
            }
        }
        cx.notify();
    }

    fn message(&mut self, text: String, error: bool, cx: &mut Context<Self>) {
        if text.is_empty() { return; }
        let host = self.toasts.get_or_insert_with(|| cx.new(|_| toast::ToastHost::default()));
        host.update(cx, |host,cx| host.push(text,error,cx));
        cx.notify();
    }

    pub fn take_traffic_reconnect(&mut self) -> bool {
        std::mem::take(&mut self.traffic_reconnect)
    }

    pub fn service_transition(&self) -> Option<&'static str> {
        match self.settings_layout.service_action {
            Some("restart") => Some("正在重启"),
            Some("start") => Some("正在启动"),
            Some("stop") => Some("正在停止"),
            _ if self.local_state == CoreState::Starting => Some("正在启动"),
            _ if self.local_state == CoreState::Stopping => Some("正在停止"),
            _ => None,
        }
    }

    pub fn core_status_label(&self) -> &'static str {
        self.service_transition().unwrap_or_else(|| self.local_state.label())
    }

    fn api_connected(&self) -> bool {
        self.api_enabled && [self.traffic_status, self.conn_status, self.log_status].contains(&"已连接")
    }

    fn api_state(&self) -> ApiState {
        if !self.api_enabled {
            return ApiState::Disconnected;
        }
        if self.api_connected() {
            return ApiState::Connected;
        }
        let local = self.local_endpoint();
        if local && self.service_transition().is_none()
            && (self.local_state.is_stopped() || self.local_state == CoreState::NotInstalled)
        {
            return ApiState::Disconnected;
        }
        let feeds = [self.traffic_status, self.conn_status, self.log_status];
        if feeds.iter().any(|status| status.contains("认证失败")) {
            ApiState::AuthFailed
        } else if !local && self.traffic_failures.max(self.conn_failures) >= 3 {
            ApiState::Unreachable
        } else {
            ApiState::Waiting
        }
    }

    pub fn api_status_label(&self) -> &'static str {
        self.api_state().label()
    }

    fn local_endpoint(&self) -> bool {
        reqwest::Url::parse(&self.endpoint).ok().is_some_and(|url| {
            url.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost") || host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
            })
        })
    }

    fn reconnect_feeds(&mut self, cx: &mut Context<Self>) {
        if !self.local_endpoint() {
            return;
        }
        self.traffic_failures = 0;
        self.conn_failures = 0;
        self.log_failures = 0;
        self.load_error.clear();
        if !self.api_enabled || self.suspended {
            return;
        }
        self.traffic_status = "连接中";
        self.conn_status = "连接中";
        self.log_status = "连接中";
        self.traffic_reconnect = true;
        self.log_feed = None;
        self.resubscribe_logs();
        self.conn_feed = Subscription::start(&self.endpoint, &self.secret, Feed::Connections).ok();
        cx.notify();
    }

    pub fn tray_service(&mut self, action: &'static str, cx: &mut Context<Self>) {
        if self.settings_layout.service_action.is_some()
            || !self.settings_layout.pending.insert("service-op")
        {
            return;
        }
        if matches!(action, "start" | "restart") {
            self.core_error = None;
        }
        self.settings_layout.service_action = Some(action);
        cx.notify();
        let task = singboard_core::runtime().spawn(crate::service::control(action));
        cx.spawn(async move |weak, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|v| v);
            match result {
                Ok(_) => {
                    let _ = weak.update(cx, |this, cx| {
                        this.settings_layout.service_action = None;
                        this.settings_layout.pending.remove("service-op");
                        if action != "stop" {
                            this.reconnect_feeds(cx);
                        }
                        this.refresh_local_state(cx);
                        this.refresh(cx);
                        cx.notify();
                    });
                }
                Err(error) => {
                    let _ = weak.update(cx, |this, cx| {
                        this.settings_layout.service_action = None;
                        this.settings_layout.pending.remove("service-op");
                        cx.notify();
                    });
                    if cx.update(|app| app.active_window().is_some()) {
                        let _ = weak.update(cx, |this,cx| this.message(error,true,cx));
                        return;
                    }
                    let msg = format!("{error}\0").encode_utf16().collect::<Vec<_>>();
                    let title = "Singboard\0".encode_utf16().collect::<Vec<_>>();
                    unsafe {
                        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                            std::ptr::null_mut(),
                            msg.as_ptr(),
                            title.as_ptr(),
                            0x10,
                        );
                    }
                }
            }
        })
        .detach();
    }

    pub fn sync_service(&mut self, cx: &mut Context<Self>) {
        self.service_syncing = true;
        let task = singboard_core::runtime().spawn(crate::service::sync());
        cx.spawn(async move |weak, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = weak.update(cx, |this, cx| {
                this.service_syncing = false;
                match result.as_deref() {
                    Ok("updated") => {
                        this.message("服务组件已自动更新".into(), false, cx);
                        this.reconnect_feeds(cx);
                        this.refresh_local_state(cx);
                        this.refresh(cx);
                    }
                    Ok("migrated") => {
                        this.message("旧版服务已自动迁移".into(), false, cx);
                        this.reconnect_feeds(cx);
                        this.refresh_local_state(cx);
                        this.refresh(cx);
                    }
                    Ok(_) => {}
                    Err(error) if error == crate::service::admin::CANCELLED => {}
                    Err(error) => this.message(format!("服务组件同步失败：{error}"), true, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn check_update(&mut self, cx: &mut Context<Self>) {
        self.check_panel_update(cx);
    }

    fn check_panel_update(&mut self, cx: &mut Context<Self>) {
        if self.settings_layout.panel_update.is_some()
            || !self.settings_layout.pending.insert("panel-update")
        {
            return;
        }
        let task = singboard_core::runtime().spawn(async move {
            let prefs = crate::maintenance::preferences()?;
            let mut info = singboard_core::update::check(&prefs.github_token).await?;
            info.mirror = prefs.core_update_mirror;
            info.compare_executable(&std::env::current_exe().map_err(|e| e.to_string())?)?;
            Ok::<_, String>(info)
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = weak.update(cx, |this, cx| {
                this.settings_layout.pending.remove("panel-update");
                let message = match result {
                    Ok(info) => {
                        let message = info.message.clone();
                        if info.available || info.reinstall {
                            this.settings_layout.panel_update = Some(settings::PanelUpdate::new(info, cx.focus_handle()));
                        }
                        message
                    }
                    Err(error) => error,
                };
                this.settings_layout.results.insert("panel-update", message);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn new(
        base: &str,
        secret: &str,
        prefs: &singboard_core::settings::Settings,
        connect: bool,
        active: bool,
    ) -> Result<Self, String> {
        let initial_status = if connect { "连接中" } else { "未连接" };
        let (log_feed, conn_feed) = if connect && active {
            (
                Some(Subscription::start_logs(base, secret, "info")?),
                Some(Subscription::start(base, secret, Feed::Connections)?),
            )
        } else {
            (None, None)
        };
        let log_list = ListState::new(0, ListAlignment::Top, px(300.))
            .with_uniform_item_height(px(26.));
        log_list.set_follow_mode(FollowMode::Tail);
        Ok(Self {
            custom_wallpaper: false,
            glass_tint: prefs.glass_tint.unwrap_or(0.5),
            traffic_status: initial_status,
            traffic_failures: 0,
            conn_failures: 0,
            has_traffic: false,
            traffic_samples: VecDeque::new(),
            last_read: std::time::Instant::now(),
            api: Clash::new(base, secret)?,
            api_enabled: connect,
            suspended: !active,
            initialized: active,
            pending_update_check: !active && prefs.panel_auto_check_update.unwrap_or(true),
            page: Page::Overview,
            log_feed,
            conn_feed,
            groups: Vec::new(),
            group_test_urls: prefs.group_test_urls.clone(),
            latency_test_url: prefs.latency_test_url.clone(),
            expanded: prefs.expanded_proxy_groups.iter().cloned().collect(),
            expanded_providers: Default::default(),
            proxy_ui: Default::default(),
            sort: prefs.proxy_sort_latency,
            settings_layout: Default::default(),
            tailscale: tailscale::TailscaleState::with_collapsed_groups(
                prefs.tailscale_collapsed_groups.iter().cloned(),
            ),
            filter: None,
            local: None,
            modes: Vec::new(),
            mode: String::new(),
            endpoint: base.into(),
            secret: secret.into(),
            undo: None,
            rules: Vec::new(),
            rule_browser: rules::RuleBrowser::default(),
            providers: Vec::new(),
            show_providers: false,
            snapshot: None,
            closed: VecDeque::new(),
            history: false,
            logs: VecDeque::new(),
            log_seq: 1,
            expanded_logs: Default::default(),
            log_focus: None,
            log_active_row: None,
            page_scroll: ScrollHandle::new(),
            scroll_motion: None,
            page_list_scroll: UniformListScrollHandle::new(),
            conn_detail_scroll: ScrollHandle::new(),
            log_list,
            log_list_len: 0,
            log_list_head: None,
            log_list_filter: (String::new(), 0, "info".into(), false, 26),
            log_auto_scroll: true,
            log_scroll_bound: false,
            log_subscribed_level: "info".into(),
            log_gap_since: None,
            log_failures: 0,
            clear_gen: 0,
            cleared: VecDeque::new(),
            cleared_at: None,
            frozen: None,
            frozen_connections: None,
            conn_ui: Default::default(),
            paused_count: 0,
            log_level: 0,
            core_level: "info".into(),
            sort_key: SORTS
                .iter()
                .position(|s| s.0 == prefs.connection_sort)
                .unwrap_or_default(),
            detail: None,
            geo: None,
            pending_restart: None,
            config_busy: None,
            last_profile_auto_check: None,
            auto_updating_profiles: false,
            config_editor: None,
            config_two_columns: false,
            config_dialog: None,
            leaving_dialog: None,
            summary: Value::Null,
            active_profile: prefs.profile_name().into(),
            active_profile_failed: prefs
                .config_profiles
                .iter()
                .find(|p| p.id == prefs.active_config_profile_id)
                .is_some_and(|p| !p.last_fetch_error.trim().is_empty()),
            ipv6: prefs.ipv6_test_enabled,
            ipv6_nodes: Default::default(),
            chart: true,
            memory: None,
            history_counts: Default::default(),
            history_memory: Default::default(),
            local_state: CoreState::Unknown,
            core_error: None,
            service_settled: false,
            service_syncing: false,
            traffic_reconnect: false,
            service_checked: None,
            service_uptime: None,
            uptime_tick: 0,
            frozen_topology: None,
            hovered_topology: None,
            topology_pointer: None,
            topology_previous: None,
            domestic_info: None,
            domestic_busy: false,
            domestic_checked: false,
            domestic_updated_at: None,
            domestic_gen: 0,
            abroad_info: None,
            abroad_busy: false,
            abroad_checked: false,
            abroad_updated_at: None,
            abroad_fingerprint: String::new(),
            abroad_gen: 0,
            sites_info: None,
            sites_busy: false,
            sites_checked: false,
            sites_updated_at: None,
            sites_gen: 0,
            show_ip: false,
            exits_due: None,
            sites_due: None,
            proxy_dirty: false,
            proxy_due: None,
            network_cache_loaded_for: String::new(),
            local_was_running: false,
            log_status: initial_status,
            conn_status: initial_status,
            confirm: None,
            page_dialog_focus: None,
            load_error: String::new(),
            toasts: None,
            busy: false,
            refreshing: false,
            updating_providers: std::collections::HashSet::new(),
            updating_all_providers: None,
            segment_sliders: std::cell::RefCell::new(std::collections::HashMap::new()),
            generation: 0,
        })
    }

    fn segment_slider(&self, key: &'static str) -> settings::SegmentSlider {
        self.segment_sliders
            .borrow_mut()
            .entry(key)
            .or_default()
            .clone()
    }

    pub fn uptime(&self) -> Option<String> {
        self.service_uptime.map(|(seconds, sampled)| {
            let total = seconds.saturating_add(sampled.elapsed().as_secs());
            format_uptime(total)
        })
    }

    fn apply_proxies(&mut self, data: Value) {
        match serde_json::from_value::<Vec<Group>>(data) {
            Ok(mut groups) => {
                for group in &mut groups {
                    let url = self
                        .group_test_urls
                        .get(&group.name)
                        .unwrap_or(&self.latency_test_url);
                    if !url.is_empty() {
                        group.test_url = url.clone();
                    }
                }
                self.groups = groups;
            }
            Err(_) => self.load_error = "代理列表格式无效".into(),
        }
    }

    pub fn prewarm(&mut self, start: Page) {
        self.page = start;
        let api = self.api.clone();
        let fetch_proxies = self.api_enabled && start == Page::Proxies;
        let budget = std::time::Duration::from_millis(180);
        let (status, proxies, providers) = singboard_core::runtime().block_on(async move {
            let status_fut = tokio::time::timeout(budget, crate::service::runtime_status());
            let proxies_fut = async {
                if !fetch_proxies {
                    return None;
                }
                tokio::time::timeout(budget, api.execute(Command::Proxies))
                    .await
                    .ok()
                    .and_then(Result::ok)
            };
            let providers_fut = async {
                if !fetch_proxies {
                    return None;
                }
                tokio::time::timeout(budget, api.execute(Command::ProxyProviders))
                    .await
                    .ok()
                    .and_then(Result::ok)
            };
            tokio::join!(status_fut, proxies_fut, providers_fut)
        });
        if let Ok(Ok((state, uptime, error))) = status {
            self.apply_core_status(state, uptime, error);
            self.service_checked = Some(std::time::Instant::now());
        }
        if let Some(data) = proxies {
            self.apply_proxies(data);
            self.last_read = std::time::Instant::now();
        }
        if let Some(data) = providers {
            self.providers = data["providers"]
                .as_object()
                .map(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
        }
    }

    fn apply_core_status(&mut self, state: CoreState, uptime: Option<u64>, error: Option<String>) {
        self.local_state = state;
        self.core_error = error;
        self.service_uptime = uptime.map(|seconds| (seconds, std::time::Instant::now()));
        self.uptime_tick = 0;
        self.service_settled = true;
    }

    pub fn refresh_local_state(&mut self, cx: &mut Context<Self>) {
        let started = std::time::Instant::now();
        self.service_checked = Some(started);
        let task = singboard_core::runtime().spawn(crate::service::runtime_status());
        cx.spawn(async move |weak, cx| {
            let status = task.await.ok().and_then(Result::ok);
            let _ = weak.update(cx, |this, cx| {
                if this.service_checked != Some(started) {
                    return;
                }
                let was_running = this.local_state == CoreState::Running;
                let (state, uptime, error) = status.unwrap_or_default();
                let now_running = state == CoreState::Running;
                this.apply_core_status(state, uptime, error);
                this.invalidate_if_core_restarted();
                if !was_running && now_running {
                    this.reconnect_feeds(cx);
                    this.refresh(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn suspend(&mut self) {
        if self.suspended {
            return;
        }
        self.suspended = true;
        self.log_feed = None;
        self.conn_feed = None;
        self.has_traffic = false;
        self.snapshot = None;
        self.memory = None;
        self.frozen_topology = None;
        self.hovered_topology = None;
        self.topology_pointer = None;
        self.topology_previous = None;
        self.exits_due = None;
        self.sites_due = None;
        self.scroll_motion = None;
        self.log_gap_since = None;
        if self.api_enabled {
            self.traffic_status = "连接中";
            self.log_status = "连接中";
            self.conn_status = "连接中";
        }
    }

    pub fn resume(&mut self, cx: &mut Context<Self>) {
        if !self.suspended {
            return;
        }
        self.suspended = false;
        if self.api_enabled {
            self.traffic_status = "连接中";
            self.log_status = "连接中";
            self.conn_status = "连接中";
            self.log_gap_since = None;
            self.resubscribe_logs();
            if self.conn_feed.is_none() {
                self.conn_feed =
                    Subscription::start(&self.endpoint, &self.secret, Feed::Connections).ok();
            }
        }
        if !self.initialized {
            self.initialized = true;
            let page = self.page;
            self.open(page, cx);
            self.sync_service(cx);
            if self.pending_update_check {
                self.pending_update_check = false;
                self.check_update(cx);
            }
            return;
        }
        self.refresh_local_state(cx);
        self.refresh(cx);
        cx.notify();
    }

    pub fn tick(&mut self, cx: &mut Context<Self>) {
        if self.suspended {
            return;
        }
        self.sync_tailscale(false, cx);
        self.drain_tailscale(cx);
        let svc_busy = self.service_syncing
            || self.settings_layout.service_action.is_some()
            || self.settings_layout.pending.contains("service-op")
            || self.local_state != CoreState::Running
            || (self.api_enabled
                && (self.traffic_status != "已连接" || self.conn_status != "已连接"));
        let poll_secs = if svc_busy { 1 } else { 2 };
        if self
            .service_checked
            .is_none_or(|t| t.elapsed().as_secs() >= poll_secs)
        {
            self.refresh_local_state(cx);
        }
        let mut notified = false;
        if let Some((_, sample)) = self.service_uptime {
            let elapsed = sample.elapsed().as_secs();
            if self.uptime_tick != elapsed {
                self.uptime_tick = elapsed;
                cx.notify();
                notified = true;
            }
        }
        if !notified
            && ((matches!(self.page, Page::Proxies | Page::Rules) && self.show_providers)
                || self.page == Page::Config)
        {
            cx.notify();
        }
        if self
            .undo
            .as_ref()
            .is_some_and(|v| v.2.elapsed().as_secs() >= 8)
        {
            self.undo = None;
            cx.notify();
        }
        if self
            .cleared_at
            .is_some_and(|t| t.elapsed().as_secs() >= 8)
        {
            self.cleared.clear();
            self.cleared_at = None;
            if self.page == Page::Logs {
                cx.notify();
            }
        }
        if !self.api_enabled {
            return;
        }
        let (Some(log_feed), Some(conn_feed)) = (&self.log_feed, &self.conn_feed) else {
            return;
        };
        let logs = log_feed.drain();
        let connections = conn_feed.drain();
        let next_failures = logs.failures as usize;
        let log_changed = !logs.logs.is_empty()
            || logs.status != self.log_status
            || next_failures != self.log_failures;
        let prev_log_status = self.log_status;
        let conn_changed = connections.memory.is_some()
            || (!connections.has_frame && self.snapshot.is_some())
            || (!connections.memory_open && self.memory.is_some())
            || connections.failures != self.conn_failures
            || connections.connections.is_some()
            || connections.status != self.conn_status;
        let reconnected = self.conn_status != "已连接" && connections.status == "已连接";
        self.log_status = logs.status;
        self.log_failures = next_failures;
        self.conn_status = connections.status;
        self.conn_failures = connections.failures;
        if prev_log_status == "已连接" && self.log_status != "已连接" {
            self.log_gap_since.get_or_insert_with(local_hhmmss);
        } else if self.log_status == "已连接" {
            if let Some(since) = self.log_gap_since.take()
                && !self.logs.is_empty()
            {
                let range = format!("{since}–{}", local_hhmmss());
                self.push_log_marker(format!("与核心的连接中断，期间的日志未收到（{range}）"));
            }
        }
        if self.frozen.is_some() {
            self.paused_count += logs.logs.len() + logs.dropped;
        }
        let stamp = if logs.logs.is_empty() {
            String::new()
        } else {
            local_hhmmss()
        };
        for line in logs.logs {
            let seq = self.log_seq;
            self.log_seq += 1;
            self.logs.push_back(LogRecord {
                seq,
                time: stamp.clone(),
                level: line.level,
                payload: line.payload,
                marker: false,
            });
        }
        while self.logs.len() > 5000 {
            if let Some(dropped) = self.logs.pop_front() {
                self.expanded_logs.remove(&dropped.seq);
            }
        }
        if let Some(memory) = connections.memory {
            self.memory = Some(memory);
            self.history_memory.push_front(memory);
            self.history_memory.truncate(OVERVIEW_SAMPLES);
        } else if !connections.memory_open {
            self.memory = None;
        }
        let closing_expired = self.conn_ui.expire_closing();
        if let Some(snapshot) = connections.connections {
            if self.page == Page::Overview && self.chart
                && let Some(last) = &self.snapshot
                && last["metrics"]["flow"] != snapshot["metrics"]["flow"]
            {
                self.topology_previous = Some((last["metrics"]["flow"].clone(), std::time::Instant::now()));
            }
            self.history_counts
                .push_front(snapshot["connections"].as_array().map_or(0, Vec::len) as u64);
            self.history_counts.truncate(OVERVIEW_SAMPLES);
            if self.frozen_connections.is_none() {
                self.conn_ui.prune_closing(&snapshot["connections"]);
            }
            self.snapshot = Some(snapshot);
        }
        if !connections.has_frame {
            self.snapshot = None;
            self.frozen_topology = None;
            self.hovered_topology = None;
            self.topology_pointer = None;
            self.topology_previous = None;
        }
        if self.local_state != CoreState::Running {
            let had_running = self.local_was_running;
            self.local_was_running = false;
            if had_running
                || self.domestic_checked
                || self.abroad_checked
                || self.sites_checked
                || self.exits_due.is_some()
                || self.sites_due.is_some()
                || self.proxy_due.is_some()
            {
                self.reset_network_state(had_running);
                cx.notify();
            }
        } else {
            self.local_was_running = true;
            if self.network_cache_loaded_for != self.endpoint {
                self.restore_network_cache();
                self.check_cached_proxy_fingerprint(cx);
                cx.notify();
            }
            if self.invalidate_if_core_restarted() {
                cx.notify();
            }
            if self.page == Page::Overview {
                let now = std::time::Instant::now();
                if !self.domestic_checked || !self.abroad_checked {
                    if self.exits_due.is_some_and(|due| now >= due) {
                        self.exits_due = None;
                        if !self.domestic_checked {
                            self.refresh_direct_exit(cx);
                        }
                        if !self.abroad_checked {
                            self.refresh_proxy_exit(false, cx);
                        }
                    } else if self.exits_due.is_none() {
                        self.exits_due = Some(now + std::time::Duration::from_secs(3));
                        cx.notify();
                    }
                }
                if !self.sites_checked {
                    if self.sites_due.is_some_and(|due| now >= due) {
                        self.refresh_sites(cx);
                    } else if self.sites_due.is_none() {
                        self.sites_due = Some(now + std::time::Duration::from_secs(3));
                        cx.notify();
                    }
                }
                if self.proxy_dirty && self.proxy_due.is_some_and(|due| now >= due) {
                    self.proxy_dirty = false;
                    self.proxy_due = None;
                    self.refresh_proxy_exit(true, cx);
                }
            }
        }
        for row in connections.closed.into_iter().rev() {
            self.closed.push_front(row);
        }
        self.closed.truncate(500);
        self.poll_due_remote_profiles(cx);
        if matches!(
            self.page,
            Page::Proxies | Page::Rules | Page::Overview | Page::Logs | Page::Settings
        ) && !self.refreshing
            && (reconnected || self.last_read.elapsed().as_secs() >= 30)
        {
            self.refresh(cx);
        }
        if (log_changed && self.page == Page::Logs)
            || ((conn_changed || closing_expired)
                && matches!(
                    self.page,
                    Page::Connections | Page::Overview | Page::Proxies
                ))
        {
            cx.notify();
        }
    }

    fn reset_network_state(&mut self, clear_disk: bool) {
        self.domestic_gen += 1;
        self.abroad_gen += 1;
        self.sites_gen += 1;
        self.domestic_info = None;
        self.domestic_busy = false;
        self.domestic_checked = false;
        self.domestic_updated_at = None;
        self.abroad_info = None;
        self.abroad_busy = false;
        self.abroad_checked = false;
        self.abroad_updated_at = None;
        self.abroad_fingerprint.clear();
        self.sites_info = None;
        self.sites_busy = false;
        self.sites_checked = false;
        self.sites_updated_at = None;
        self.exits_due = None;
        self.sites_due = None;
        self.proxy_dirty = false;
        self.proxy_due = None;
        self.network_cache_loaded_for.clear();
        if clear_disk && let Ok(dir) = crate::service::data_dir() {
            singboard_core::tools::clear_network_cache(&dir);
        }
    }

    fn proxy_fingerprint(&self) -> String {
        let mode = if !self.mode.trim().is_empty() {
            self.mode.trim().to_ascii_lowercase()
        } else {
            self.summary["config"]["mode"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        };
        let mut selections: Vec<(String, String)> = if !self.groups.is_empty() {
            self.groups
                .iter()
                .filter(|g| !g.now.is_empty())
                .map(|g| (g.name.clone(), g.now.clone()))
                .collect()
        } else if let Some(proxies) = self.summary["proxies"].as_object() {
            proxies
                .iter()
                .filter_map(|(k, v)| {
                    let now = v["now"].as_str().filter(|s| !s.is_empty())?;
                    Some((k.clone(), now.to_owned()))
                })
                .collect()
        } else {
            Vec::new()
        };
        if mode.is_empty() && selections.is_empty() {
            return String::new();
        }
        selections.sort_by(|a, b| a.0.cmp(&b.0));
        let joined = selections
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(";");
        format!("{mode}|{joined}")
    }

    fn core_started_at(&self) -> Option<u64> {
        let (seconds, sampled) = self.service_uptime?;
        let total = seconds.saturating_add(sampled.elapsed().as_secs());
        Some(singboard_core::tools::now_unix_secs().saturating_sub(total))
    }

    fn invalidate_if_core_restarted(&mut self) -> bool {
        let Some(started) = self.core_started_at() else {
            return false;
        };
        let stale = [
            self.domestic_updated_at,
            self.abroad_updated_at,
            self.sites_updated_at,
        ]
        .into_iter()
        .flatten()
        .any(|ts| ts.saturating_add(5) < started);
        if stale {
            self.reset_network_state(true);
            return true;
        }
        false
    }

    fn restore_network_cache(&mut self) {
        self.network_cache_loaded_for = self.endpoint.clone();
        let Ok(dir) = crate::service::data_dir() else {
            return;
        };
        let cached = singboard_core::tools::load_network_cache(
            &dir,
            &self.endpoint,
            self.core_started_at(),
        );
        if let Some(slot) = cached.domestic {
            self.domestic_info = Some(slot.data);
            self.domestic_updated_at = Some(slot.updated_at);
            self.domestic_checked = true;
        }
        if let Some(slot) = cached.abroad {
            self.abroad_info = Some(slot.data);
            self.abroad_updated_at = Some(slot.updated_at);
            self.abroad_fingerprint = slot.fingerprint;
            self.abroad_checked = true;
        }
        if let Some(slot) = cached.sites {
            self.sites_info = Some(slot.data);
            self.sites_updated_at = Some(slot.updated_at);
            self.sites_checked = true;
        }
    }

    fn check_cached_proxy_fingerprint(&mut self, cx: &mut Context<Self>) {
        if !self.abroad_checked || self.abroad_fingerprint.is_empty() || self.proxy_dirty {
            return;
        }
        let current = self.proxy_fingerprint();
        if !current.is_empty() && current != self.abroad_fingerprint {
            self.schedule_proxy_recheck(cx);
        }
    }

    fn schedule_proxy_recheck(&mut self, cx: &mut Context<Self>) {
        if self.local_state != CoreState::Running {
            return;
        }
        self.proxy_dirty = true;
        self.proxy_due = Some(std::time::Instant::now() + std::time::Duration::from_millis(1500));
        cx.notify();
    }

    fn refresh_exits(&mut self, cx: &mut Context<Self>) {
        self.exits_due = None;
        self.proxy_dirty = false;
        self.proxy_due = None;
        self.refresh_direct_exit(cx);
        self.refresh_proxy_exit(true, cx);
    }

    fn refresh_direct_exit(&mut self, cx: &mut Context<Self>) {
        if self.domestic_busy {
            return;
        }
        self.domestic_busy = true;
        self.domestic_checked = true;
        self.domestic_gen += 1;
        let generation = self.domestic_gen;
        let endpoint = self.endpoint.clone();
        let task = singboard_core::runtime().spawn(async move {
            let data = singboard_core::tools::direct_exit_info().await;
            let updated_at = crate::service::data_dir()
                .ok()
                .and_then(|dir| {
                    singboard_core::tools::save_network_slot(
                        &dir,
                        &endpoint,
                        "domestic",
                        data.clone(),
                        "",
                    )
                });
            (data, updated_at)
        });
        cx.spawn(async move |weak, cx| {
            let outcome = task.await;
            let _ = weak.update(cx, |this, cx| {
                if this.domestic_gen != generation {
                    return;
                }
                this.domestic_busy = false;
                if let Ok((data, updated_at)) = outcome {
                    this.domestic_info = Some(data);
                    if let Some(ts) = updated_at {
                        this.domestic_updated_at = Some(ts);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn refresh_proxy_exit(&mut self, reset_cooldown: bool, cx: &mut Context<Self>) {
        if self.abroad_busy {
            return;
        }
        self.abroad_busy = true;
        self.abroad_checked = true;
        self.proxy_dirty = false;
        self.proxy_due = None;
        self.abroad_gen += 1;
        let generation = self.abroad_gen;
        let endpoint = self.endpoint.clone();
        let fingerprint = self.proxy_fingerprint();
        let task = singboard_core::runtime().spawn(async move {
            let data = singboard_core::tools::proxy_exit_info(reset_cooldown).await;
            let updated_at = crate::service::data_dir()
                .ok()
                .and_then(|dir| {
                    singboard_core::tools::save_network_slot(
                        &dir,
                        &endpoint,
                        "abroad",
                        data.clone(),
                        &fingerprint,
                    )
                });
            (data, updated_at, fingerprint)
        });
        cx.spawn(async move |weak, cx| {
            let outcome = task.await;
            let _ = weak.update(cx, |this, cx| {
                if this.abroad_gen != generation {
                    return;
                }
                this.abroad_busy = false;
                if let Ok((data, updated_at, fp)) = outcome {
                    this.abroad_info = Some(data);
                    if let Some(ts) = updated_at {
                        this.abroad_updated_at = Some(ts);
                        this.abroad_fingerprint = fp;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn refresh_sites(&mut self, cx: &mut Context<Self>) {
        if self.sites_busy {
            return;
        }
        self.sites_busy = true;
        self.sites_checked = true;
        self.sites_due = None;
        self.sites_gen += 1;
        let generation = self.sites_gen;
        let endpoint = self.endpoint.clone();
        let task = singboard_core::runtime().spawn(async move {
            let data = singboard_core::tools::site_latencies_overview().await;
            let updated_at = crate::service::data_dir()
                .ok()
                .and_then(|dir| {
                    singboard_core::tools::save_network_slot(
                        &dir,
                        &endpoint,
                        "sites",
                        data.clone(),
                        "",
                    )
                });
            (data, updated_at)
        });
        cx.spawn(async move |weak, cx| {
            let outcome = task.await;
            let _ = weak.update(cx, |this, cx| {
                if this.sites_gen != generation {
                    return;
                }
                this.sites_busy = false;
                if let Ok((data, updated_at)) = outcome {
                    this.sites_info = Some(data);
                    if let Some(ts) = updated_at {
                        this.sites_updated_at = Some(ts);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn service_notice(&self, cx: &Context<Self>) -> Div {
        let palette = crate::theme::Palette::get(cx);
        let (stopped_detail, starting_detail) = match self.page {
            Page::Proxies => ("启动服务后即可查看代理组和节点延迟。", "核心启动后会自动加载代理组。"),
            Page::Connections => ("启动服务后即可查看实时连接。", "核心启动后会自动显示连接。"),
            Page::Rules => ("启动服务后即可查看路由规则和规则提供商。", "核心启动后会自动加载规则。"),
            _ => ("启动服务后即可查看实时日志。", "核心启动后会自动接收日志。"),
        };
        let local_stopped = self.local_state.is_stopped();
        let local_not_installed = self.local_state == CoreState::NotInstalled;
        let local_starting = self.local_state == CoreState::Starting;
        let local_stopping = self.local_state == CoreState::Stopping;
        let restarting = self.settings_layout.service_action == Some("restart");
        let (title, detail, action) = if self.service_syncing {
            (
                "正在更新服务组件…",
                starting_detail,
                None,
            )
        } else if restarting || local_starting {
            (
                if restarting { "正在重启 sing-box…" } else { "正在启动 sing-box…" },
                starting_detail,
                None,
            )
        } else if local_stopping {
            (
                "正在停止 sing-box…",
                "服务停止后可以在这里重新启动。",
                None,
            )
        } else if local_not_installed {
            (
                "尚未安装 singboard 服务",
                "在设置中安装服务后，即可启动 sing-box。",
                Some("settings"),
            )
        } else if local_stopped {
            (
                "sing-box 服务已停止",
                self.core_error.as_deref().unwrap_or(stopped_detail),
                Some("start"),
            )
        } else {
            ("无法获取服务状态", "可以在设置中查看服务状态和错误日志。", Some("settings"))
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .px_6()
            .when(action.is_none(), |el| {
                el.child(settings::loading_spinner(
                    "service-notice-spinner",
                    px(32.),
                    crate::glass::backdrop_muted(palette, cx),
                ))
            })
            .when(action.is_some(), |el| {
                el.child(
                    svg()
                        .path("power")
                        .size(px(64.))
                        .text_color(crate::glass::backdrop_muted(palette, cx)),
                )
            })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1()
                        .child(
                            crate::glass::backdrop_heading(title, palette, cx)
                                .text_size(crate::theme::text_px(18.))
                                .font_weight(FontWeight::MEDIUM),
                        )
                        .child(
                            crate::glass::backdrop_text(detail.to_owned(), true, palette, cx)
                                .text_sm(),
                        ),
                )
                .when_some(action, |el, kind| {
                    if kind == "start" {
                        el.child(
                            div()
                                .id("service-notice-start")
                                .cursor_pointer()
                                .h(crate::theme::text_px(32.))
                                .px_5()
                                .rounded(px(10.))
                                .bg(palette.primary_fill())
                                .flex()
                                .items_center()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(0xffffff))
                                .child("启动服务")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.tray_service("start", cx);
                                })),
                        )
                    } else {
                        el.child(
                            div()
                                .id("service-notice-settings")
                                .relative()
                                .overflow_hidden()
                                .cursor_pointer()
                                .h(crate::theme::text_px(32.))
                                .rounded(px(10.))
                                .bg(palette.fill())
                                .border_1()
                                .border_color(palette.control_rim())
                                .shadow_sm()
                                .child(crate::glass::control_layer(palette, px(9.), true))
                                .child(
                                    div()
                                        .relative()
                                        .h_full()
                                        .px_5()
                                        .hover(|s| s.bg(palette.fill_strong()))
                                        .flex()
                                        .items_center()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.accent())
                                        .child("前往设置"),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open(Page::Settings, cx);
                                })),
                        )
                    }
                })
    }

    pub fn header_scroll_progress(&self) -> f32 {
        match self.page {
            Page::Overview | Page::Settings | Page::Tailscale => {
                let scrolled = (-f32::from(self.page_scroll.offset().y)).max(0.);
                scrolling::smoothstep(12., 36., scrolled)
            }
            _ => 0.,
        }
    }

    pub fn open(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == Page::Overview && page != Page::Overview {
            self.hovered_topology = None;
            self.topology_pointer = None;
            self.topology_previous = None;
        }
        if self.page != page
            && let Some(filter) = &self.filter
        {
            filter.update(cx, |f, cx| f.clear(cx));
        }
        self.detail = None;
        self.config_dialog = None;
        self.leaving_dialog = None;
        self.rule_browser = rules::RuleBrowser::default();
        self.settings_layout.close_dialog();
        self.frozen_connections = None;
        self.conn_ui.reset();
        let page_changed = self.page != page;
        if page_changed {
            self.scroll_motion = None;
            self.proxy_ui.cancel_reveal();
            self.page_scroll = ScrollHandle::new();
            self.page_list_scroll = UniformListScrollHandle::new();
            self.conn_detail_scroll = ScrollHandle::new();
        }
        self.page = page;
        if page_changed {
            self.segment_sliders.borrow_mut().clear();
        }
        if page == Page::Overview
            && self.local_state == CoreState::Running
            && self.proxy_dirty
            && self.proxy_due.is_some_and(|due| std::time::Instant::now() >= due)
        {
            self.proxy_dirty = false;
            self.proxy_due = None;
            self.refresh_proxy_exit(true, cx);
        }
        if self.service_checked.is_none() {
            self.refresh_local_state(cx);
        }
        self.generation += 1;
        self.busy = false;
        self.refreshing = false;
        self.updating_providers.clear();
        self.updating_all_providers = None;
        self.proxy_ui.clear_testing();
        self.show_providers = false;
        if self.filter.is_none() {
            let search = cx.new(crate::search::Search::new);
            cx.observe(&search, |this, field, cx| {
                if this.page == Page::Proxies { this.proxy_ui.search_changed(field.read(cx).text.trim().to_lowercase()); }
                if this.page == Page::Rules { this.schedule_rule_search(cx); }
                cx.notify();
            }).detach();
            self.filter = Some(search);
            cx.observe_keystrokes(|this, event, window, cx| {
                let key = &event.keystroke;
                if matches!(this.page, Page::Connections | Page::Rules) && this.leaving_dialog.is_some() {
                    return;
                }
                if key.key == "escape" {
                    if this.page == Page::Rules {
                        if this.rule_browser.detail.is_some() {
                            this.apply(Action::CloseRuleDetail, cx);
                            return;
                        }
                        if this.rule_browser.inspect.is_some() {
                            this.apply(Action::CloseRuleInspect, cx);
                            return;
                        }
                    }
                    if this.page == Page::Connections {
                        if this.detail.is_some() {
                            this.apply(Action::Dismiss, cx);
                            return;
                        }
                        if this.confirm.is_some() {
                            this.apply(Action::Cancel, cx);
                            return;
                        }
                    }
                    if this.page == Page::Config && this.config_dialog.is_some() {
                        this.close_config_dialog(cx);
                        return;
                    }
                    if this.page == Page::Settings && this.settings_layout.dialog.is_some() {
                        if !matches!(
                            this.settings_layout.dialog,
                            Some(settings::SettingsDialog::ConfirmCoreUpdate { busy: true, .. })
                        ) {
                            this.close_settings_dialog(cx);
                        }
                        return;
                    }
                    if this.page == Page::Tailscale && this.tailscale.dialog.is_some() {
                        this.tailscale.dialog = None;
                        cx.notify();
                        return;
                    }
                }
                if this.page == Page::Connections
                    && key.key == "delete"
                    && !this.history
                    && let Some(id) = this.detail.as_ref().and_then(|r| r["id"].as_str())
                {
                    this.run(Command::Disconnect { id: id.into() }, cx);
                    this.apply(Action::Dismiss, cx);
                    cx.notify();
                }
                if matches!(
                    this.page,
                    Page::Proxies | Page::Rules | Page::Logs | Page::Connections
                ) && key.modifiers.control
                    && key.key == "f"
                {
                    if this.detail.is_some() || this.confirm.is_some() || this.rule_browser.inspect.is_some() { return; }
                    if let Some(dialog) = &this.rule_browser.detail {
                        dialog.search.update(cx, |field, cx| field.select_all(window, cx));
                    } else if let Some(filter) = &this.filter {
                        filter.update(cx, |field, cx| field.select_all(window, cx));
                    }
                    cx.notify();
                }
            })
            .detach();
        }
        if let Some(filter) = &self.filter {
            let hint = if page == Page::Rules {
                "搜索规则或出站，输入域名 / IP 时也会查找命中的规则提供商"
            } else if page == Page::Connections {
                "搜索域名、IP、端口、进程、规则或节点"
            } else if page == Page::Logs {
                "搜索日志内容"
            } else if page == Page::Proxies {
                "搜索节点或代理组"
            } else {
                "搜索（Ctrl+F）"
            };
            filter.update(cx, |f, cx| f.set_placeholder(hint, cx));
        }
        self.confirm = None;
        self.load_error.clear();
        if self.api_enabled && matches!(self.page, Page::Proxies | Page::Rules) {
            if page_changed {
                self.providers.clear();
            }
            let generation = self.generation;
            let api = self.api.clone();
            let cmd = if self.page == Page::Proxies {
                Command::ProxyProviders
            } else {
                Command::RuleProviders
            };
            let lookup = singboard_core::runtime().spawn(async move { api.execute(cmd).await });
            cx.spawn(async move |weak, cx| {
                if let Ok(Ok(data)) = lookup.await {
                    let _ = weak.update(cx, |this, cx| {
                        if generation != this.generation {
                            return;
                        }
                        this.providers = data["providers"]
                            .as_object()
                            .into_iter()
                            .flatten()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if this.page == Page::Rules { this.schedule_rule_search(cx); }
                        cx.notify();
                    });
                }
            })
            .detach();
        }
        if self.api_enabled && page == Page::Settings {
            let generation = self.generation;
            let api = self.api.clone();
            let job = singboard_core::runtime()
                .spawn(async move { api.execute(Command::Version).await });
            cx.spawn(async move |weak, cx| {
                if let Ok(Ok(data)) = job.await {
                    let _ = weak.update(cx, |this, cx| {
                        if this.generation == generation {
                            this.summary["version"] = data["version"].clone();
                            cx.notify();
                        }
                    });
                }
            })
            .detach();
        }
        self.refresh(cx);
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.last_read = std::time::Instant::now();
        if self.page == Page::Config {
            self.load_error.clear();
            self.local = None;
            match crate::service::snapshot() {
                Ok(snapshot) => self.local = snapshot,
                Err(error) => self.load_error = error,
            }
            return;
        }
        if !self.api_enabled {
            return;
        }
        let command = match (self.page, self.show_providers) {
            (Page::Proxies, false) => Command::Proxies,
            (Page::Proxies, true) => Command::ProxyProviders,
            (Page::Rules, false) => Command::Rules,
            (Page::Rules, true) => Command::RuleProviders,
            (Page::Settings | Page::Logs, _) => Command::Config,
            (Page::Overview, _) => Command::Summary,
            _ => return,
        };
        self.run(command, cx);
    }

    fn run(&mut self, command: Command, cx: &mut Context<Self>) {
        let is_read = matches!(
            command,
            Command::Proxies
                | Command::Rules
                | Command::ProxyProviders
                | Command::RuleProviders
                | Command::Config
                | Command::Summary
                | Command::Version
        );
        let is_exclusive_action = matches!(
            command,
            Command::GroupDelay { .. }
                | Command::UpdateProviders { .. }
                | Command::UpdateProxyProvider { .. }
                | Command::UpdateRuleProvider { .. }
                | Command::HealthCheck { .. }
        );
        if is_read {
            if self.refreshing {
                return;
            }
            self.refreshing = true;
        } else if is_exclusive_action {
            if self.busy {
                return;
            }
            self.busy = true;
        }
        if let Command::Delay { name, .. } = &command {
            if self.proxy_ui.is_node_testing(name) {
                return;
            }
            self.clear_node_delay(name);
            self.proxy_ui.mark_node_testing(name);
        }
        let mut rule_toggle_prev = None;
        if let Command::ToggleRule { uuid } = &command {
            if uuid.trim().is_empty() || self.rule_browser.toggling.contains(uuid) {
                return;
            }
            let prev = self.rule_disabled_by_uuid(uuid).unwrap_or(false);
            rule_toggle_prev = Some(prev);
            self.rule_browser.toggling.insert(uuid.clone());
            self.set_rule_disabled(uuid, !prev);
        }
        match &command {
            Command::Disconnect { id } => self.conn_ui.mark_closing(std::iter::once(id.clone())),
            Command::DisconnectMany { ids } => self.conn_ui.mark_closing(ids.iter().cloned()),
            Command::DisconnectAll => {
                let ids: Vec<String> = self
                    .shown_active()
                    .iter()
                    .filter_map(|v| v["id"].as_str().map(str::to_owned))
                    .collect();
                self.conn_ui.mark_closing(ids);
            }
            _ => {}
        }
        if is_read {
            self.load_error.clear();
        }
        let generation = self.generation;
        let api = self.api.clone();
        let request = command.clone();
        let task = singboard_core::runtime().spawn(async move { api.execute(request).await });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                if is_read {
                    this.refreshing = false;
                } else if is_exclusive_action {
                    this.busy = false;
                }
                if let Command::Delay { name, .. } = &command {
                    this.proxy_ui.unmark_node_testing(name);
                    this.release_proxy_order();
                }
                match result {
                    Err(error) => {
                        match &command {
                            Command::Proxies => this.groups.clear(),
                            Command::Rules => this.rules.clear(),
                            Command::ProxyProviders | Command::RuleProviders => {
                                this.providers.clear()
                            }
                            Command::Delay { name, .. } => {
                                this.sync_linked_delay(
                                    name,
                                    Some(0),
                                    this.ipv6.then_some(false),
                                );
                            }
                            Command::Disconnect { id } => this.conn_ui.unmark_closing(std::iter::once(id.as_str())),
                            Command::DisconnectMany { ids } => this.conn_ui.unmark_closing(ids.iter().map(String::as_str)),
                            Command::DisconnectAll => this.conn_ui.clear_closing(),
                            Command::ToggleRule { uuid } => {
                                this.rule_browser.toggling.remove(uuid);
                                if let Some(prev) = rule_toggle_prev {
                                    this.set_rule_disabled(uuid, prev);
                                }
                            }
                            _ => {}
                        }
                        if matches!(command, Command::Proxies | Command::Rules | Command::ProxyProviders | Command::RuleProviders | Command::Config | Command::Summary | Command::Version) {
                            this.load_error = error;
                        } else if !matches!(command, Command::Delay { .. }) {
                            this.message(error,true,cx);
                        }
                    }
                    Ok(data) => match command {
                        Command::Proxies => {
                            this.apply_proxies(data);
                            this.check_cached_proxy_fingerprint(cx);
                        }
                        Command::Rules => {
                            this.apply_rules(data["rules"].as_array().cloned().unwrap_or_default());
                        }
                        Command::ToggleRule { uuid } => {
                            this.rule_browser.toggling.remove(&uuid);
                            if this.rule_browser.toggling.is_empty() {
                                this.refresh(cx);
                            }
                        }
                        Command::ProxyProviders | Command::RuleProviders => {
                            this.providers = data["providers"]
                                .as_object()
                                .map(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                                .unwrap_or_default()
                        }
                        Command::Delay { name, .. } => {
                            this.sync_linked_delay(
                                &name,
                                data["delay"].as_u64(),
                                data["ipv6"].as_bool(),
                            );
                        }
                        Command::GroupDelay { .. } => {
                            let ipv6_map = data["ipv6"].as_object().cloned();
                            let latencies = data.get("latencies").unwrap_or(&data);
                            let updates: Vec<(String, u64, Option<bool>)> = latencies
                                .as_object()
                                .into_iter()
                                .flatten()
                                .filter_map(|(name, value)| {
                                    let delay = value.as_u64()?;
                                    let ipv6 = ipv6_map
                                        .as_ref()
                                        .and_then(|m| m.get(name))
                                        .map(|v| v.as_u64().unwrap_or(0) > 0);
                                    Some((name.clone(), delay, ipv6))
                                })
                                .collect();
                            for (name, delay, ipv6) in updates {
                                this.sync_linked_delay(&name, Some(delay), ipv6);
                            }
                        }
                        Command::Select { group, name } => {
                            if let Some(g) = this.groups.iter_mut().find(|g| g.name == group) {
                                let old = std::mem::replace(&mut g.now, name.clone());
                                this.undo = Some((group.clone(), old, std::time::Instant::now()));
                            }
                            this.sync_group_selection_delay(&group, &name);
                            this.schedule_proxy_recheck(cx);
                            this.refresh(cx);
                        }
                        Command::SetMode { mode } => {
                            this.mode = mode;
                            this.schedule_proxy_recheck(cx);
                            this.refresh(cx);
                        }
                        Command::Summary => {
                            if let Some(lvl) = data["config"]["log-level"].as_str() {
                                this.core_level = normalize_core_level(lvl);
                                if this.log_level == 0 {
                                    this.resubscribe_logs();
                                }
                            }
                            this.summary = data;
                            this.check_cached_proxy_fingerprint(cx);
                        }
                        Command::Config => {
                            this.core_level =
                                normalize_core_level(data["log-level"].as_str().unwrap_or("info"));
                            if this.log_level == 0 {
                                this.resubscribe_logs();
                            }
                            this.mode = data["mode"].as_str().unwrap_or_default().into();
                            this.modes = data["mode-list"]
                                .as_array()
                                .or_else(|| data["modes"].as_array())
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect();
                            this.check_cached_proxy_fingerprint(cx);
                        }
                        Command::Disconnect { .. }
                        | Command::DisconnectAll
                        | Command::DisconnectMany { .. } => this.message("已断开连接".into(),false,cx),
                        _ => this.refresh(cx),
                    },
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn apply_providers(&mut self, data: Value, proxies: bool, cx: &mut Context<Self>) {
        self.providers = data["providers"]
            .as_object()
            .map(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        if !proxies && self.page == Page::Rules {
            self.schedule_rule_search(cx);
        }
    }

    pub(super) fn update_provider(&mut self, proxies: bool, name: String, cx: &mut Context<Self>) {
        if self.updating_all_providers.is_some() || self.updating_providers.contains(&name) {
            return;
        }
        self.updating_providers.insert(name.clone());
        let generation = self.generation;
        let api = self.api.clone();
        let command = if proxies {
            Command::UpdateProxyProvider { name: name.clone() }
        } else {
            Command::UpdateRuleProvider { name: name.clone() }
        };
        let list_cmd = if proxies {
            Command::ProxyProviders
        } else {
            Command::RuleProviders
        };
        let task = singboard_core::runtime().spawn(async move {
            api.execute(command).await?;
            let list = api.execute(list_cmd).await.ok();
            Ok::<_, String>(list)
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.updating_providers.remove(&name);
                match result {
                    Ok(Some(data)) => this.apply_providers(data, proxies, cx),
                    Ok(None) => this.refresh(cx),
                    Err(error) => this.message(error, true, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn update_all_providers(
        &mut self,
        proxies: bool,
        names: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.updating_all_providers.is_some()
            || !self.updating_providers.is_empty()
            || names.is_empty()
        {
            return;
        }
        let total = names.len();
        self.updating_all_providers = Some((0, total));
        for name in &names {
            self.updating_providers.insert(name.clone());
        }
        let generation = self.generation;
        let api = self.api.clone();
        let list_cmd = if proxies {
            Command::ProxyProviders
        } else {
            Command::RuleProviders
        };
        cx.spawn(async move |weak, cx| {
            let mut failures = Vec::new();
            for name in names {
                let api_req = api.clone();
                let req_name = name.clone();
                let cmd = if proxies {
                    Command::UpdateProxyProvider { name: req_name }
                } else {
                    Command::UpdateRuleProvider { name: req_name }
                };
                let step = singboard_core::runtime()
                    .spawn(async move { api_req.execute(cmd).await })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                if let Err(e) = step {
                    failures.push(format!("{name} · {e}"));
                }
                let mut alive = false;
                let _ = weak.update(cx, |this, cx| {
                    if this.generation != generation {
                        return;
                    }
                    alive = true;
                    this.updating_providers.remove(&name);
                    if let Some((done, total)) = &mut this.updating_all_providers {
                        *done = (*done + 1).min(*total);
                    }
                    cx.notify();
                });
                if !alive {
                    return;
                }
            }
            let list = singboard_core::runtime()
                .spawn(async move { api.execute(list_cmd).await })
                .await
                .ok()
                .and_then(Result::ok);
            let _ = weak.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.updating_all_providers = None;
                this.updating_providers.clear();
                if let Some(data) = list {
                    this.apply_providers(data, proxies, cx);
                } else {
                    this.refresh(cx);
                }
                if !failures.is_empty() {
                    this.message(failures.join("\n"), true, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn set_node_delay(&mut self, name: &str, delay: Option<u64>) {
        for node in self
            .groups
            .iter_mut()
            .flat_map(|g| &mut g.nodes)
            .filter(|n| n.name == name)
        {
            node.delay = delay;
        }
        for (_, provider) in &mut self.providers {
            if let Some(list) = provider["proxies"].as_array_mut() {
                for node in list {
                    if node["name"].as_str() == Some(name) {
                        match delay {
                            Some(d) => {
                                if let Some(history) = node["history"].as_array_mut() {
                                    history.push(serde_json::json!({ "delay": d }));
                                } else {
                                    node["history"] = serde_json::json!([{ "delay": d }]);
                                }
                            }
                            None => {
                                node["history"] = serde_json::json!([]);
                            }
                        }
                    }
                }
            }
        }
    }

    fn clear_node_delay(&mut self, name: &str) {
        self.set_node_delay(name, None);
        self.ipv6_nodes.remove(name);
    }

    fn sync_linked_delay(&mut self, name: &str, delay: Option<u64>, ipv6: Option<bool>) {
        let targets = proxies::linked_proxy_names(&self.groups, name);
        for target in targets {
            if let Some(v) = ipv6 {
                if v {
                    self.ipv6_nodes.insert(target.clone());
                } else {
                    self.ipv6_nodes.remove(&target);
                }
            }
            self.set_node_delay(&target, delay);
        }
    }

    fn sync_group_selection_delay(&mut self, group_name: &str, selected_name: &str) {
        let delay = self
            .groups
            .iter()
            .find(|g| g.name == group_name)
            .and_then(|g| g.nodes.iter().find(|n| n.name == selected_name))
            .and_then(|n| n.delay);
        let ipv6 = self.ipv6.then(|| self.ipv6_nodes.contains(selected_name));
        let ancestors = proxies::ancestor_group_names(&self.groups, group_name);
        for target in ancestors {
            if let Some(v) = ipv6 {
                if v {
                    self.ipv6_nodes.insert(target.clone());
                } else {
                    self.ipv6_nodes.remove(&target);
                }
            }
            self.set_node_delay(&target, delay);
        }
    }

    fn apply_delay_result(&mut self, res: singboard_core::clash::DelayResult) {
        self.sync_linked_delay(&res.name, Some(res.delay), res.ipv6);
    }

    pub(super) fn test_node(
        &mut self,
        name: String,
        url: String,
        group: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.proxy_ui.is_node_testing(&name) {
            return;
        }
        if let Some(group) = &group {
            self.freeze_proxy_order(group);
        }
        self.clear_node_delay(&name);
        self.proxy_ui.mark_node_testing(&name);
        let generation = self.generation;
        let api = self.api.clone();
        let ipv6 = self.ipv6;
        let req_name = name.clone();
        let task = singboard_core::runtime().spawn(async move {
            api.execute(Command::Delay {
                name: req_name,
                url,
                ipv6,
            })
            .await
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.proxy_ui.unmark_node_testing(&name);
                match result {
                    Ok(data) => {
                        this.apply_delay_result(singboard_core::clash::DelayResult {
                            name,
                            delay: data["delay"].as_u64().unwrap_or(0),
                            ipv6: data["ipv6"].as_bool(),
                        });
                    }
                    Err(_) => {
                        this.sync_linked_delay(&name, Some(0), ipv6.then_some(false));
                    }
                }
                this.release_proxy_order();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn test_group(&mut self, group_name: String, cx: &mut Context<Self>) {
        if self.proxy_ui.testing_groups.contains_key(&group_name) {
            return;
        }
        let Some(group) = self.groups.iter().find(|g| g.name == group_name) else {
            return;
        };
        let url = group.test_url.clone();
        let mut seen = std::collections::HashSet::new();
        let nodes: Vec<String> = group
            .nodes
            .iter()
            .filter(|n| !n.kind.eq_ignore_ascii_case("direct"))
            .filter(|n| seen.insert(n.name.clone()))
            .map(|n| n.name.clone())
            .collect();
        if nodes.is_empty() {
            return;
        }
        self.test_nodes_batch(false, group_name, url, nodes, cx);
    }

    pub(super) fn test_provider(&mut self, provider_name: String, cx: &mut Context<Self>) {
        if self.proxy_ui.testing_providers.contains_key(&provider_name) {
            return;
        }
        let Some((_, item)) = self.providers.iter().find(|(k, _)| k == &provider_name) else {
            return;
        };
        let url = item["testUrl"]
            .as_str()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or(&self.latency_test_url)
            .to_owned();
        let mut seen = std::collections::HashSet::new();
        let nodes: Vec<String> = item["proxies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|n| !n["type"].as_str().unwrap_or("").eq_ignore_ascii_case("direct"))
            .filter_map(|n| n["name"].as_str())
            .filter(|name| !name.is_empty() && seen.insert((*name).to_owned()))
            .map(str::to_owned)
            .collect();
        if nodes.is_empty() {
            return;
        }
        self.test_nodes_batch(true, provider_name, url, nodes, cx);
    }

    fn test_nodes_batch(
        &mut self,
        is_provider: bool,
        target: String,
        url: String,
        nodes: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        const CONCURRENCY: usize = 8;
        if !is_provider {
            self.freeze_proxy_order(&target);
        }
        let total = nodes.len();
        if is_provider {
            self.proxy_ui.testing_providers.insert(target.clone(), (0, total));
        } else {
            self.proxy_ui.testing_groups.insert(target.clone(), (0, total));
        }
        for node in &nodes {
            self.clear_node_delay(node);
            self.proxy_ui.mark_node_testing(node);
        }
        let generation = self.generation;
        let mut rx = self.api.test_nodes(nodes, url, self.ipv6, CONCURRENCY);
        cx.spawn(async move |weak, cx| {
            while let Some(res) = rx.recv().await {
                let mut alive = false;
                let _ = weak.update(cx, |this, cx| {
                    if this.generation != generation {
                        return;
                    }
                    alive = true;
                    let name = res.name.clone();
                    this.apply_delay_result(res);
                    this.proxy_ui.unmark_node_testing(&name);
                    let map = if is_provider {
                        &mut this.proxy_ui.testing_providers
                    } else {
                        &mut this.proxy_ui.testing_groups
                    };
                    if let Some((done, _)) = map.get_mut(&target) {
                        *done += 1;
                    }
                    cx.notify();
                });
                if !alive {
                    break;
                }
            }
            let _ = weak.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                if is_provider {
                    this.proxy_ui.testing_providers.remove(&target);
                } else {
                    this.proxy_ui.testing_groups.remove(&target);
                    this.release_proxy_order();
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn apply(&mut self, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::Refresh => self.refresh(cx),
            Action::Providers => {
                self.generation += 1;
                self.busy = false;
                self.refreshing = false;
                self.updating_providers.clear();
                self.updating_all_providers = None;
                self.proxy_ui.clear_testing();
                self.show_providers = !self.show_providers;
                if self.page == Page::Rules {
                    if let Some(filter) = &self.filter {
                        filter.update(cx, |f,cx| {
                            f.clear(cx);
                            f.set_placeholder(if self.show_providers { "输入域名或 IP，查找命中的规则提供商" } else { "搜索规则或出站，输入域名 / IP 时也会查找命中的规则提供商" },cx);
                        });
                    }
                    self.schedule_rule_search(cx);
                }
                self.refresh(cx);
            }
            Action::Sort => {
                self.sort = !self.sort;
                self.proxy_ui.reset_order();
                if let Ok(dir) = crate::service::data_dir() {
                    let _ = crate::settings::Settings::edit(&dir,|p| {p.proxy_sort_latency=self.sort; Ok(())});
                }
            }
            Action::History => {
                self.history = !self.history;
                self.detail = None;
                self.confirm = None;
            }
            Action::Pause => {
                if self.page == Page::Connections {
                    self.frozen_connections = if self.frozen_connections.is_some() {
                        None
                    } else {
                        Some((self.snapshot.clone(), self.closed.clone()))
                    };
                } else {
                    self.frozen = if self.frozen.is_some() {
                        None
                    } else {
                        Some(self.logs.clone())
                    };
                    self.paused_count = 0;
                }
            }
            Action::ConnectionSort(index) => {
                self.sort_key = index;
                if let Ok(dir) = crate::service::data_dir() {
                    let key = SORTS[self.sort_key].0;
                    let _ = crate::settings::Settings::edit(&dir, |p| {
                        p.connection_sort = key.into();
                        Ok(())
                    });
                }
            }
            Action::Detail(row) => {
                self.leaving_dialog = None;
                self.cancel_scroll(scrolling::ScrollTarget::ConnDetail);
                self.conn_detail_scroll = ScrollHandle::new();
                let ip = row["metadata"]["destinationIP"].as_str().unwrap_or("").to_owned();
                self.detail = Some(row);
                self.geo = None;
                if singboard_core::tools::public_ip(&ip) {
                    self.apply(Action::Geo(ip), cx);
                }
            }
            Action::RuleDetail(tag) => {
                self.cancel_scroll(scrolling::ScrollTarget::RuleDetail);
                self.open_rule_detail(tag, cx);
            }
            Action::CloseRuleDetail => {
                self.cancel_scroll(scrolling::ScrollTarget::RuleDetail);
                if let Some(detail) = self.rule_browser.detail.take() {
                    self.leave_dialog(dialog::LeavingDialog::Rule(detail), cx);
                }
            }
            Action::InspectRule(index) => {
                self.open_rule_inspect(index);
            }
            Action::CloseRuleInspect => {
                if let Some(inspect) = self.rule_browser.inspect.take() {
                    self.leave_dialog(dialog::LeavingDialog::RuleInspect(inspect), cx);
                }
            }
            Action::Dismiss => {
                self.cancel_scroll(scrolling::ScrollTarget::ConnDetail);
                if let Some(detail) = self.detail.take() {
                    self.leave_dialog(dialog::LeavingDialog::Connection(detail), cx);
                }
            }
            Action::Geo(ip) => {
                let task = singboard_core::runtime()
                    .spawn(async move { singboard_core::tools::geo_details(&ip).await });
                let id = self.detail.as_ref().map(|r| r["id"].clone());
                self.geo = None;
                cx.spawn(async move |weak, cx| {
                    let result = task.await.map_err(|e| e.to_string()).and_then(|v| v);
                    let _ = weak.update(cx, |this, cx| {
                        if this.detail.as_ref().map(|r| &r["id"]) == id.as_ref() {
                            this.geo = Some(result);
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            Action::Clear => {
                self.frozen = None;
                self.paused_count = 0;
                self.expanded_logs.clear();
                let removed = std::mem::take(&mut self.logs);
                self.clear_gen += 1;
                if removed.iter().any(|l| !l.marker) {
                    self.cleared = removed;
                    self.cleared_at = Some(std::time::Instant::now());
                    let clear_id = self.clear_gen;
                    cx.spawn(async move |weak, cx| {
                        singboard_core::runtime()
                            .spawn(async { tokio::time::sleep(std::time::Duration::from_secs(8)).await })
                            .await
                            .ok();
                        let _ = weak.update(cx, |this, cx| {
                            if this.clear_gen == clear_id {
                                this.cleared.clear();
                                this.cleared_at = None;
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                } else {
                    self.cleared.clear();
                    self.cleared_at = None;
                }
            }
            Action::UndoClear => {
                self.clear_gen += 1;
                let mut restored = std::mem::take(&mut self.cleared);
                restored.append(&mut self.logs);
                while restored.len() > 5000 {
                    restored.pop_front();
                }
                self.logs = restored;
                self.cleared_at = None;
            }
            Action::Confirm(command) => {
                self.leaving_dialog = None;
                self.confirm = Some(command);
            }
            Action::Cancel => {
                if let Some(command) = self.confirm.take() {
                    self.leave_dialog(dialog::LeavingDialog::Disconnect(command), cx);
                }
            }
            Action::Run(command) => {
                if let Some(pending) = self.confirm.take() {
                    self.leave_dialog(dialog::LeavingDialog::Disconnect(pending), cx);
                }
                if matches!(command, Command::Disconnect { .. }) && self.detail.is_some() {
                    self.apply(Action::Dismiss, cx);
                }
                self.run(command, cx);
            }
        }
        cx.notify();
    }

    fn active(&self) -> &[Value] {
        self.snapshot
            .as_deref()
            .and_then(|v| v["connections"].as_array())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn metrics(&self) -> Value {
        self.snapshot
            .as_ref()
            .map(|s| s["metrics"].clone())
            .unwrap_or_default()
    }

    fn connection(&self, i: usize) -> &Value {
        if self.history {
            &self.shown_closed()[i]
        } else {
            &self.shown_active()[i]
        }
    }

    fn shown_snapshot(&self) -> Option<&Value> {
        match &self.frozen_connections {
            Some((snapshot, _)) => snapshot.as_deref(),
            None => self.snapshot.as_deref(),
        }
    }

    fn shown_active(&self) -> &[Value] {
        self.shown_snapshot()
            .and_then(|v| v["connections"].as_array())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn shown_closed(&self) -> &VecDeque<Value> {
        self.frozen_connections.as_ref().map_or(&self.closed, |s| &s.1)
    }

    pub(super) fn effective_log_level(&self) -> String {
        if self.log_level == 0 {
            self.core_level.clone()
        } else {
            LEVELS[self.log_level].to_owned()
        }
    }

    pub(super) fn push_log_marker(&mut self, payload: String) {
        let seq = self.log_seq;
        self.log_seq += 1;
        self.logs.push_back(LogRecord {
            seq,
            time: local_hhmmss(),
            level: "marker".into(),
            payload,
            marker: true,
        });
        while self.logs.len() > 5000 {
            if let Some(dropped) = self.logs.pop_front() {
                self.expanded_logs.remove(&dropped.seq);
            }
        }
    }

    fn resubscribe_logs(&mut self) {
        if !self.api_enabled || self.suspended {
            return;
        }
        let level = self.effective_log_level();
        if self.log_subscribed_level == level && self.log_feed.is_some() {
            return;
        }
        if let Ok(feed) = Subscription::start_logs(&self.endpoint, &self.secret, &level) {
            self.log_feed = Some(feed);
            self.log_subscribed_level = level;
            self.log_gap_since = None;
        }
    }

    pub(super) fn set_log_level(&mut self, next_index: usize, cx: &mut Context<Self>) {
        let prev_effective = self.effective_log_level();
        self.log_level = next_index.min(LEVELS.len() - 1);
        let next_effective = self.effective_log_level();
        if prev_effective != next_effective {
            if !self.logs.is_empty() {
                self.push_log_marker(format!(
                    "日志级别切换为 {next_effective}，之后的日志按新级别接收"
                ));
            }
            self.resubscribe_logs();
        }
        cx.notify();
    }

    fn indices(&self, query: &str) -> Vec<usize> {
        let level = if self.log_level == 0 {
            dashboard::level_rank(&self.core_level)
        } else {
            self.log_level
        };
        let mut rows: Vec<_> = (0..self.len())
            .filter(|&i| match self.page {
                Page::Logs => {
                    let entry = &self.frozen.as_ref().unwrap_or(&self.logs)[i];
                    if entry.marker {
                        query.is_empty()
                    } else {
                        dashboard::level_rank(&entry.level.to_lowercase()) <= level
                            && (query.is_empty() || entry.payload.to_lowercase().contains(query))
                    }
                }
                Page::Connections => {
                    connections::matches_filter(self.connection(i), query)
                }
                Page::Rules => {
                    if self.show_providers {
                        query.is_empty() || self.rule_browser.matched.contains(&self.providers[i].0)
                    } else {
                        self.rule_matches_query(i, query)
                    }
                }
                _ => true,
            })
            .collect();
        if self.page == Page::Logs {
            let source = self.frozen.as_ref().unwrap_or(&self.logs);
            if !rows.iter().any(|&i| !source[i].marker) {
                rows.clear();
            }
        }
        if self.page == Page::Connections {
            rows.sort_by(|a, b| {
                dashboard::compare(
                    self.connection(*a),
                    self.connection(*b),
                    SORTS[self.connection_sort()].0,
                    self.history,
                )
            });
        }
        rows
    }

    fn len(&self) -> usize {
        match self.page {
            Page::Proxies | Page::Rules if self.show_providers => self.providers.len(),
            Page::Proxies => self.groups.len(),
            Page::Rules => self.rules.len(),
            Page::Connections if self.history => self.shown_closed().len(),
            Page::Connections => self.shown_active().len(),
            Page::Logs => self.frozen.as_ref().unwrap_or(&self.logs).len(),
            Page::Overview | Page::Tailscale | Page::Config | Page::Settings => 0,
        }
    }

    pub fn close_editor(&mut self, cx: &mut Context<Self>) {
        self.config_editor = None;
        cx.notify();
    }

    fn edit(&mut self, path: std::path::PathBuf, name: String, remote: bool, window: &mut Window, cx: &mut Context<Self>) {
        let core = self
            .local
            .as_ref()
            .filter(|s| s.source_config == path)
            .map(|s| crate::editor::Core {
                executable: s.core.clone(),
                working_dir: s.working_dir.clone().into(),
            })
            .or_else(|| {
                let prefs = crate::maintenance::preferences().ok()?;
                let exe = std::path::PathBuf::from(prefs.singbox_path.trim());
                if !exe.is_file() {
                    return None;
                }
                let working_dir = if !prefs.working_dir.trim().is_empty() {
                    std::path::PathBuf::from(prefs.working_dir.trim())
                } else {
                    path.parent()?.to_path_buf()
                };
                Some(crate::editor::Core {
                    executable: exe,
                    working_dir,
                })
            });
        match crate::editor::inline(path, core, name, remote, cx.weak_entity(), window, cx) {
            Ok(editor) => {
                self.load_error.clear();
                self.config_editor = Some(editor);
            }
            Err(error) => self.message(error,true,cx),
        }
        cx.notify();
    }

    fn button(
        &self,
        id: impl Into<ElementId>,
        title: &'static str,
        action: Action,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let palette = crate::theme::Palette::get(cx);
        let keyboard_action = action.clone();
        let button = div()
            .id(id)
            .cursor_pointer()
            .rounded_full()
            .bg(palette.fill())
            .hover(|s| s.bg(palette.fill_strong()))
            .px_3()
            .py(px(5.))
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child(title)
            .on_click(cx.listener(move |this, _, _, cx| this.apply(action.clone(), cx)));
        if self.detail.is_some()
            || self.confirm.is_some()
            || self.rule_browser.detail.is_some()
            || self.rule_browser.inspect.is_some()
        {
            dialog::keyboard_action(button, keyboard_action, cx)
        } else {
            button
        }
    }
}

fn fmt_bytes(value: u64) -> String {
    if value < 1024 {
        format!("{value} B")
    } else if value < 1048576 {
        format!("{:.1} KiB", value as f64 / 1024.)
    } else if value < 1073741824 {
        format!("{:.1} MiB", value as f64 / 1048576.)
    } else {
        format!("{:.2} GiB", value as f64 / 1073741824.)
    }
}

fn format_uptime(total: u64) -> String {
    let days = total / 86400;
    let hours = total / 3600 % 24;
    let minutes = total / 60 % 60;
    let seconds = total % 60;
    if days > 0 {
        format!("{days}天 {hours}:{minutes:02}:{seconds:02}")
    } else if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn playback_button(
    id: &'static str,
    paused: bool,
    topology: bool,
    palette: crate::theme::Palette,
    cx: &App,
) -> Stateful<Div> {
    let (icon, label) = match (topology, paused) {
        (true, false) => ("topology-pause", "暂停拓扑更新"),
        (true, true) => ("topology-play", "继续拓扑更新"),
        (false, false) => ("pause", "暂停刷新"),
        (false, true) => ("play", "继续刷新"),
    };
    let ink = if paused {
        palette.accent()
    } else if topology {
        palette.muted()
    } else {
        crate::glass::backdrop_ink(palette, cx)
    };
    div()
        .id(id)
        .relative()
        .overflow_hidden()
        .role(Role::Button)
        .aria_label(label)
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(label).build(window, cx))
        .size(px(if topology { 28. } else { 32. }))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(paused, |d| {
            d.bg(palette.primary_soft())
                .border_1()
                .border_color(palette.selection_border().opacity(0.6))
                .child(crate::glass::control_layer(palette, px(999.), false))
        })
        .when(topology && !paused, |d| {
            d.bg(palette.fill())
                .border_1()
                .border_color(palette.control_rim())
                .child(crate::glass::control_layer(palette, px(14.), true))
        })
        .child(
            div()
                .relative()
                .size_full()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .when(paused, |d| d.bg(palette.primary_soft()))
                .hover(|s| s.bg(palette.fill_strong()))
                .child(
                    svg()
                        .path(icon)
                        .size(px(if topology { 14. } else { 16. }))
                        .text_color(ink),
                ),
        )
}

fn popover_card(palette: crate::theme::Palette, tint: f32) -> Div {
    div()
        .relative()
        .rounded(px(22.))
        .overflow_hidden()
        .bg(palette.popover(tint))
        .border_1()
        .border_color(palette.rim())
        .shadow(crate::glass::modal_shadows(palette.0))
        .child(crate::glass::card_layer(
            palette.popover(tint),
            px(21.),
            0.70,
        ))
        .child(crate::glass::grain_layer(px(21.)))
}

fn glass_card(palette: crate::theme::Palette, translucent: bool, tint: f32) -> Div {
    div()
        .relative()
        .rounded(px(18.))
        .overflow_hidden()
        .bg(palette.card_tinted(translucent, tint))
        .border_1()
        .border_color(palette.rim())
        .shadow(crate::glass::card_shadows(palette.0))
        .when(translucent, |el| {
            el.child(crate::glass::card_layer(
                palette.card_tinted(true, tint),
                px(17.),
                0.60,
            ))
        })
        .child(crate::glass::grain_layer(px(17.)))
}

fn pill_switch(
    id: impl Into<ElementId>,
    on: bool,
    palette: crate::theme::Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .w(px(40.))
        .h(px(22.))
        .p(px(2.))
        .rounded_full()
        .cursor_pointer()
        .bg(if on {
            palette.primary_fill()
        } else {
            palette.toggle_off()
        })
        .flex()
        .items_center()
        .when(on, |el| el.justify_end())
        .child(div().size(px(18.)).rounded_full().bg(rgb(0xffffff)).shadow_sm())
}

#[derive(IntoElement)]
pub(super) struct DropdownBox {
    id: &'static str,
    anchor: Anchor,
    palette: crate::theme::Palette,
    width: Option<Pixels>,
    min_width: Option<Pixels>,
    full_width: bool,
    padding_x: Pixels,
    gap: Pixels,
    label: SharedString,
    items: Vec<(SharedString, bool, std::rc::Rc<dyn Fn(&mut Window, &mut App)>)>,
}

impl DropdownBox {
    pub(super) fn new(
        id: &'static str,
        label: impl Into<SharedString>,
        palette: crate::theme::Palette,
    ) -> Self {
        Self {
            id,
            anchor: Anchor::TopLeft,
            palette,
            width: None,
            min_width: None,
            full_width: true,
            padding_x: px(12.),
            gap: px(0.),
            label: label.into(),
            items: Vec::new(),
        }
    }

    pub(super) fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub(super) fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self.full_width = false;
        self
    }

    pub(super) fn min_width(mut self, min_width: Pixels) -> Self {
        self.min_width = Some(min_width);
        self.full_width = false;
        self
    }

    pub(super) fn padding_x(mut self, padding_x: Pixels) -> Self {
        self.padding_x = padding_x;
        self
    }

    pub(super) fn gap(mut self, gap: Pixels) -> Self {
        self.gap = gap;
        self
    }

    pub(super) fn item(
        mut self,
        label: impl Into<SharedString>,
        checked: bool,
        on_pick: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.items
            .push((label.into(), checked, std::rc::Rc::new(on_pick)));
        self
    }
}

impl RenderOnce for DropdownBox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        use gpui_component::{
            ElementExt as _,
            button::{Button, ButtonVariants as _},
            menu::{DropdownMenu as _, PopupMenuItem},
        };

        let width_state = window.use_keyed_state(
            (ElementId::from(self.id), "dropdown-width"),
            cx,
            |_, _| std::rc::Rc::new(std::cell::Cell::new(px(0.))),
        );
        let width_cell = width_state.read(cx).clone();
        let measure_cell = width_cell.clone();
        let items = self.items;
        let anchor = self.anchor;

        div()
            .relative()
            .overflow_hidden()
            .when_some(self.width, |d, w| d.w(w))
            .when(self.full_width, |d| d.w_full())
            .when_some(self.min_width, |d, w| d.min_w(w))
            .h(crate::theme::text_px(32.))
            .rounded(px(10.))
            .bg(self.palette.fill())
            .border_1()
            .border_color(self.palette.control_rim())
            .on_prepaint(move |bounds, _, _| {
                measure_cell.set(bounds.size.width);
            })
            .child(crate::glass::control_layer(self.palette, px(9.), false))
            .child(
                Button::new(self.id)
                    .ghost()
                    .w_full()
                    .h(crate::theme::text_px(32.))
                    .px(self.padding_x)
                    .rounded(px(10.))
                    .text_color(self.palette.ink())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(self.gap)
                            .child(div().min_w_0().truncate().text_sm().child(self.label))
                            .child(
                                svg()
                                    .path("select-caret")
                                    .size(px(15.))
                                    .text_color(self.palette.muted()),
                            ),
                    )
                    .dropdown_menu_with_anchor(anchor, move |mut menu, window, _| {
                        let trigger_w = width_cell.get();
                        if trigger_w > px(0.) {
                            let default_max = rems(31.25).to_pixels(window.rem_size());
                            menu = menu.min_w(trigger_w).max_w(trigger_w.max(default_max));
                        }
                        for (label, checked, on_pick) in &items {
                            let on_pick = on_pick.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label.clone())
                                    .checked(*checked)
                                    .on_click(move |_, window, cx| on_pick(window, cx)),
                            );
                        }
                        menu
                    }),
            )
    }
}

pub(super) fn format_elapsed_relative(diff: u64) -> String {
    if diff < 60 {
        format!("{diff} 秒前")
    } else if diff < 3600 {
        format!("{} 分钟前", diff / 60)
    } else if diff < 86400 {
        format!("{} 小时前", diff / 3600)
    } else {
        format!("{} 天前", diff / 86400)
    }
}

pub(super) fn format_rfc3339_relative(iso: &str) -> String {
    let Some(ts) = singboard_core::tools::parse_rfc3339_unix_secs(iso) else {
        return String::new();
    };
    let now = singboard_core::tools::now_unix_secs();
    format_elapsed_relative(now.saturating_sub(ts as u64))
}

fn format_relative_age(updated_at: Option<u64>) -> String {
    let Some(ts) = updated_at else {
        return String::new();
    };
    let now = singboard_core::tools::now_unix_secs();
    let diff = now.saturating_sub(ts);
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

fn format_ip_row(data: Option<&Value>, busy: bool, revealed: bool) -> (String, String, bool) {
    let Some(row) = data else {
        return (
            if busy { "检测中…".into() } else { "未检测".into() },
            String::new(),
            false,
        );
    };
    match row["status"].as_str().unwrap_or("") {
        "done" => {
            let ip = row["ip"].as_str().unwrap_or("");
            let locs = row["location"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            let location = if revealed {
                locs.join(" ")
            } else {
                locs.first().copied().unwrap_or("位置未知").to_owned()
            };
            let shown_ip = if revealed {
                ip.to_owned()
            } else {
                singboard_core::tools::mask_ip(ip)
            };
            (
                if location.is_empty() {
                    "位置未知".into()
                } else {
                    location
                },
                shown_ip,
                false,
            )
        }
        "error" => (
            format!("检测失败：{}", row["error"].as_str().unwrap_or("未知错误")),
            String::new(),
            true,
        ),
        _ => ("未检测".into(), String::new(), false),
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.advance_scroll(window);
        let service_down = self.service_settled && self.local_state != CoreState::Running
            && self.local_endpoint() && !self.api_connected();
        let page = match self.page {
            Page::Proxies | Page::Connections | Page::Logs | Page::Rules if service_down => {
                self.service_notice(cx).pt(px(48.)).px(px(20.)).pb(px(16.))
            }
            Page::Overview => self.overview_page(window, cx).pt(px(48.)),
            Page::Proxies => self.proxy_page(window, cx).pt(px(48.)),
            Page::Connections => self.connections_page(cx).pt(px(48.)).px(px(20.)).pb(px(16.)),
            Page::Logs => self.logs_page(window, cx).pt(px(48.)).px(px(20.)).pb(px(16.)),
            Page::Rules => self.rules_page(cx).pt(px(48.)).px(px(20.)).pb(px(16.)),
            Page::Tailscale => self.tailscale_page(window, cx).pt(px(48.)),
            Page::Config => self.config_page(window, cx).pt(px(48.)),
            Page::Settings => self.settings_page(window, cx).pt(px(48.)),
        };
        div()
            .relative()
            .size_full()
            .on_scroll(cx.listener(|_, _, window, cx| {
                let entity = cx.weak_entity();
                window.on_next_frame(move |_, cx| { let _ = entity.update(cx, |_, cx| cx.notify()); });
            }))
            .child(page)
    }
}
