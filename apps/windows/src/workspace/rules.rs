use super::*;
use gpui_component::scroll::ScrollableElement;
use singboard_core::commands::srs::{RuleEntry, srs_list_provider, srs_match_provider};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct RuleBrowser {
    pub(super) matched: HashSet<String>,
    criteria: Option<(String, Vec<String>, bool)>,
    status: String,
    search_task: Option<Task<()>>,
    pub(super) detail: Option<RuleDetail>,
}

pub(super) struct RuleDetail {
    tag: String,
    pub(super) search: Entity<crate::search::Search>,
    entries: Vec<RuleEntry>,
    type_counts: Vec<(String, usize)>,
    active_type: Option<String>,
    shown: Vec<usize>,
    query: String,
    status: String,
    match_status: String,
    pub(super) scroll: UniformListScrollHandle,
    task: Option<Task<()>>,
}

fn rule_paths() -> Result<(String, String, String), String> {
    let prefs = crate::maintenance::preferences()?;
    let config = crate::service::snapshot()?.map(|s| s.config.to_string_lossy().into_owned())
        .unwrap_or(prefs.config_path);
    Ok((prefs.working_dir, config, prefs.singbox_path))
}

fn lookup_query(q: &str) -> bool {
    let (host, prefix) = q.split_once('/').map_or((q, None), |(h, p)| (h, Some(p)));
    if let Ok(addr) = host.parse::<std::net::IpAddr>() {
        return prefix.is_none_or(|p| {
            p.parse::<u8>()
                .is_ok_and(|n| n <= if addr.is_ipv4() { 32 } else { 128 })
        });
    }
    let name = q.trim_end_matches('.');
    name.rsplit_once('.').is_some_and(|(_, t)| {
        t.len() >= 2 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }) && name.split('.').all(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    })
}

#[derive(Clone)]
enum ConditionToken {
    Word(String),
    Text(String),
    Mono(String),
    RuleSet { short: String, full: String },
}

#[derive(Clone)]
enum RuleAction {
    Route {
        outbound: String,
        final_node: Option<String>,
    },
    Reject(&'static str),
    Stop(&'static str),
    Pass(&'static str),
}

fn field_label(key: &str) -> Option<&'static str> {
    Some(match key {
        "domain" => "域名",
        "domain_suffix" => "域名后缀",
        "domain_keyword" => "域名关键词",
        "domain_regex" => "域名正则",
        "ip_cidr" => "IP 段",
        "source_ip_cidr" => "来源 IP 段",
        "default_interface_address" => "默认网卡地址",
        "port" => "端口",
        "source_port" => "来源端口",
        "port_range" => "端口范围",
        "source_port_range" => "来源端口范围",
        "protocol" => "协议",
        "network" => "网络",
        "process_name" => "进程",
        "process_path" => "进程路径",
        "package_name" => "应用",
        "inbound" => "入站",
        "clash_mode" => "模式",
        "network_type" => "网络类型",
        "wifi_ssid" => "Wi-Fi",
        "wifi_bssid" => "Wi-Fi BSSID",
        "query_type" => "DNS 查询类型",
        "process_path_regex" => "进程路径正则",
        "package_name_regex" => "应用正则",
        "network_is_expensive" => "计费网络",
        "network_is_constrained" => "受限网络",
        "user" => "用户",
        _ => return None,
    })
}

fn rule_type_order(rule_type: &str) -> u8 {
    match rule_type {
        "domain_suffix" => 0,
        "domain" => 1,
        "domain_keyword" => 2,
        "domain_regex" => 3,
        "ip_cidr" => 4,
        "source_ip_cidr" => 5,
        _ => 6,
    }
}

fn rule_type_badge_colors(rule_type: &str, p: crate::theme::Palette) -> (Rgba, Rgba) {
    match rule_type {
        "domain_suffix" => (p.primary_soft(), p.accent()),
        "domain" => (p.success_soft(), p.success_ink()),
        "domain_keyword" => (p.warning_soft(), p.warning_ink()),
        "domain_regex" => (p.tcp_soft(), p.tcp()),
        "ip_cidr" | "source_ip_cidr" => (p.udp_soft(), p.udp()),
        _ => (p.fill(), p.muted()),
    }
}

fn sort_rule_entries(entries: &mut [RuleEntry]) {
    entries.sort_by(|a, b| {
        rule_type_order(&a.rule_type)
            .cmp(&rule_type_order(&b.rule_type))
            .then_with(|| a.rule_type.cmp(&b.rule_type))
            .then_with(|| {
                if matches!(a.rule_type.as_str(), "ip_cidr" | "source_ip_cidr") {
                    std::cmp::Ordering::Equal
                } else {
                    a.value
                        .to_ascii_lowercase()
                        .cmp(&b.value.to_ascii_lowercase())
                        .then_with(|| a.value.cmp(&b.value))
                }
            })
    });
}

fn collect_type_counts(entries: &[RuleEntry]) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for entry in entries {
        if let Some(last) = counts.last_mut().filter(|(t, _)| t == &entry.rule_type) {
            last.1 += 1;
        } else {
            counts.push((entry.rule_type.clone(), 1));
        }
    }
    counts
}

fn rebuild_rule_detail_shown(d: &mut RuleDetail) {
    let q = &d.query;
    d.shown = d
        .entries
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            if let Some(active) = &d.active_type
                && &r.rule_type != active
            {
                return false;
            }
            if q.is_empty() {
                return true;
            }
            r.value.to_lowercase().contains(q)
                || r.rule_type.to_lowercase().contains(q)
                || field_label(&r.rule_type).is_some_and(|label| label.to_lowercase().contains(q))
                || (r.rule_type == "domain_suffix"
                    && q.ends_with(&format!(".{}", r.value.trim_start_matches('.'))))
        })
        .map(|(i, _)| i)
        .collect();
}

fn is_mono_field(key: &str) -> bool {
    matches!(
        key,
        "ip_cidr"
            | "source_ip_cidr"
            | "default_interface_address"
            | "port"
            | "source_port"
            | "port_range"
            | "source_port_range"
            | "domain"
            | "domain_suffix"
            | "domain_regex"
            | "process_path"
    )
}

fn is_upper_field(key: &str) -> bool {
    matches!(key, "protocol" | "network")
}

fn parse_condition_items(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let key_start = i;
        while i < bytes.len() && (bytes[i].is_ascii_lowercase() || bytes[i] == b'_') {
            i += 1;
        }
        if i == key_start || i >= bytes.len() || bytes[i] != b'=' {
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            continue;
        }
        let key = body[key_start..i].to_owned();
        i += 1;
        let val_start = i;
        if i < bytes.len() && bytes[i] == b'[' {
            while i < bytes.len() && bytes[i] != b']' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
        } else {
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        }
        let val = body[val_start..i].to_owned();
        if !val.is_empty() {
            out.push((key, val));
        }
    }
    out
}

fn item_tokens(key: &str, raw: &str) -> Vec<ConditionToken> {
    let list: Vec<&str> = if raw.starts_with('[') && raw.ends_with(']') {
        raw[1..raw.len() - 1].split_whitespace().collect()
    } else {
        vec![raw]
    };
    if key == "rule_set" {
        return list
            .into_iter()
            .map(|full| {
                let short = full.rsplit('/').next().unwrap_or(full).to_owned();
                ConditionToken::RuleSet {
                    short,
                    full: full.to_owned(),
                }
            })
            .collect();
    }
    if key == "ip_is_private" {
        return vec![ConditionToken::Text(
            if raw == "true" {
                "私有 IP"
            } else {
                "非私有 IP"
            }
            .into(),
        )];
    }
    if key == "ip_version" {
        return vec![ConditionToken::Text(format!("IPv{raw}"))];
    }
    let Some(label) = field_label(key) else {
        return vec![ConditionToken::Mono(format!("{key}={raw}"))];
    };
    let mut tokens = vec![ConditionToken::Word(label.into())];
    for (idx, val) in list.into_iter().enumerate() {
        if idx > 0 {
            tokens.push(ConditionToken::Word("、".into()));
        }
        let formatted = if is_upper_field(key) {
            val.to_uppercase()
        } else {
            val.to_owned()
        };
        if is_mono_field(key) {
            tokens.push(ConditionToken::Mono(formatted));
        } else {
            tokens.push(ConditionToken::Text(formatted));
        }
    }
    tokens
}

fn term_tokens(term: &str) -> Vec<ConditionToken> {
    let mut body = term.trim();
    let mut tokens = Vec::new();
    if let Some(inner) = body.strip_prefix("!(").and_then(|s| s.strip_suffix(')')) {
        tokens.push(ConditionToken::Word("不满足".into()));
        body = inner;
    } else if let Some(inner) = body.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        body = inner;
    }
    let items = parse_condition_items(body);
    if items.is_empty() {
        tokens.push(ConditionToken::Mono(body.to_owned()));
        return tokens;
    }
    for (i, (k, v)) in items.into_iter().enumerate() {
        if i > 0 {
            tokens.push(ConditionToken::Word("，".into()));
        }
        tokens.extend(item_tokens(&k, &v));
    }
    tokens
}

fn tokenize_condition(rule: &Value) -> Vec<ConditionToken> {
    if rule["condition"].as_str() == Some("最终回退") {
        return vec![ConditionToken::Word("以上都没有命中".into())];
    }
    let payload = rule["payload"].as_str().unwrap_or("").trim();
    if payload.is_empty() {
        return vec![ConditionToken::Word("所有流量".into())];
    }
    let mut tokens = Vec::new();
    let mut rest = payload;
    while !rest.is_empty() {
        let and_pos = rest.find(" && ");
        let or_pos = rest.find(" || ");
        match (and_pos, or_pos) {
            (Some(a), Some(o)) if a < o => {
                tokens.extend(term_tokens(&rest[..a]));
                tokens.push(ConditionToken::Word("且".into()));
                rest = &rest[a + 4..];
            }
            (Some(a), None) => {
                tokens.extend(term_tokens(&rest[..a]));
                tokens.push(ConditionToken::Word("且".into()));
                rest = &rest[a + 4..];
            }
            (_, Some(o)) => {
                tokens.extend(term_tokens(&rest[..o]));
                tokens.push(ConditionToken::Word("或".into()));
                rest = &rest[o + 4..];
            }
            (None, None) => {
                tokens.extend(term_tokens(rest));
                break;
            }
        }
    }
    tokens
}

pub(super) fn rule_set_tags_from_payload(payload: &str) -> Vec<String> {
    payload
        .split("rule_set=")
        .skip(1)
        .flat_map(|tail| {
            let tags = if let Some(rest) = tail.strip_prefix('[') {
                rest.split(']').next().unwrap_or("")
            } else {
                tail.split(|c: char| c.is_whitespace() || matches!(c, ']' | ')'))
                    .next()
                    .unwrap_or("")
            };
            tags.split_whitespace().map(str::to_owned)
        })
        .collect()
}

fn parse_rule_action(rule: &Value) -> RuleAction {
    let proxy = rule["proxy"].as_str().unwrap_or("");
    if proxy == "reject" {
        return RuleAction::Reject("拒绝");
    }
    if proxy == "reject(drop)" {
        return RuleAction::Reject("丢弃");
    }
    if proxy.starts_with("reject") {
        return RuleAction::Reject("拒绝");
    }
    if proxy == "hijack-dns" {
        return RuleAction::Stop("劫持 DNS");
    }
    if proxy.starts_with("sniff") {
        return RuleAction::Pass("嗅探");
    }
    if proxy == "resolve(match_only)" {
        return RuleAction::Pass("解析 · 仅匹配");
    }
    if proxy.starts_with("resolve") {
        return RuleAction::Pass("解析");
    }
    if proxy.starts_with("route-options") {
        return RuleAction::Pass("路由选项");
    }
    let outbound = proxy
        .strip_prefix("route(")
        .and_then(|s| s.strip_suffix(')'))
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .unwrap_or(proxy)
        .to_owned();
    let chain: Vec<&str> = rule["chain"]
        .as_array()
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let final_node = if chain.len() > 1 {
        chain.last().map(|s| (*s).to_owned())
    } else {
        None
    };
    RuleAction::Route {
        outbound,
        final_node,
    }
}

fn vehicle_label(vehicle: &str) -> String {
    match vehicle.to_lowercase().as_str() {
        "remote" | "http" => "远程".into(),
        "local" | "file" => "本地".into(),
        "inline" => "内联".into(),
        other => other.to_owned(),
    }
}

fn format_label(behavior: &str) -> String {
    match behavior.to_lowercase().as_str() {
        "binary" => "二进制".into(),
        "source" => "源文件".into(),
        other => other.to_owned(),
    }
}

fn provider_name_parts(name: &str) -> (&str, &str) {
    match name.rfind('/') {
        Some(idx) => (&name[..=idx], &name[idx + 1..]),
        None => ("", name),
    }
}

fn format_count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

impl Workspace {
    pub(super) fn schedule_rule_search(&mut self, cx: &mut Context<Self>) {
        let q = self
            .filter
            .as_ref()
            .map(|f| f.read(cx).text.trim().to_lowercase())
            .unwrap_or_default();
        let tags = self
            .providers
            .iter()
            .map(|p| p.0.clone())
            .collect::<Vec<_>>();
        let criteria = (q.clone(), tags.clone(), self.show_providers);
        if self.rule_browser.criteria.as_ref() == Some(&criteria) {
            return;
        }
        self.rule_browser.criteria = Some(criteria);
        self.rule_browser.search_task = None;
        self.rule_browser.status.clear();
        self.rule_browser.matched.clear();
        if q.is_empty() || tags.is_empty() || (!self.show_providers && !lookup_query(&q)) {
            return;
        }
        self.rule_browser.status = "正在查找命中的规则提供商…".into();
        self.rule_browser.search_task = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(400))
                .await;
            let result = singboard_core::runtime()
                .spawn_blocking(move || {
                    let (dir, path, core) = rule_paths()?;
                    let mut hits = HashSet::new();
                    let mut errors = Vec::new();
                    for tag in tags {
                        match srs_match_provider(
                            dir.clone(),
                            path.clone(),
                            core.clone(),
                            tag.clone(),
                            q.clone(),
                        ) {
                            Ok(true) => {
                                hits.insert(tag);
                            }
                            Ok(false) => {}
                            Err(e) => errors.push(format!("{tag} · {e}")),
                        }
                    }
                    Ok::<_, String>((hits, errors.join("；")))
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| v);
            let _ = weak.update(cx, |this, cx| {
                match result {
                    Ok((hits, errors)) => {
                        this.rule_browser.matched = hits;
                        this.rule_browser.status = errors;
                    }
                    Err(e) => this.rule_browser.status = e,
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn open_rule_detail(&mut self, tag: String, cx: &mut Context<Self>) {
        if !self.providers.iter().any(|p| p.0 == tag) {
            return;
        }
        self.leaving_dialog = None;
        let search = cx.new(crate::search::Search::new);
        search.update(cx, |s, cx| s.set_placeholder("筛选条目或检查域名 / IP", cx));
        cx.observe(&search, |this, field, cx| {
            let q = field.read(cx).text.trim().to_lowercase();
            if let Some(d) = &this.rule_browser.detail
                && d.search == field
                && d.query != q
            {
                this.filter_rule_detail(q, cx);
            }
        })
        .detach();
        let identity = search.entity_id();
        self.rule_browser.detail = Some(RuleDetail {
            tag: tag.clone(),
            search,
            entries: Vec::new(),
            type_counts: Vec::new(),
            active_type: None,
            shown: Vec::new(),
            query: String::new(),
            status: "正在读取…".into(),
            match_status: String::new(),
            scroll: UniformListScrollHandle::new(),
            task: None,
        });
        cx.spawn(async move |weak, cx| {
            let result = singboard_core::runtime()
                .spawn_blocking(move || {
                    let (dir, config, core) = rule_paths()?;
                    srs_list_provider(dir, config, core, tag)
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| v);
            let _ = weak.update(cx, |this, cx| {
                if let Some(d) = &mut this.rule_browser.detail
                    && d.search.entity_id() == identity
                {
                    match result {
                        Ok(mut entries) => {
                            sort_rule_entries(&mut entries);
                            d.type_counts = collect_type_counts(&entries);
                            d.active_type = None;
                            d.entries = entries;
                            d.status.clear();
                            let q = d.query.clone();
                            this.filter_rule_detail(q, cx);
                        }
                        Err(e) => d.status = e,
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn select_rule_detail_type(&mut self, rule_type: Option<String>, cx: &mut Context<Self>) {
        let Some(d) = &mut self.rule_browser.detail else {
            return;
        };
        if d.active_type == rule_type {
            return;
        }
        d.active_type = rule_type;
        rebuild_rule_detail_shown(d);
        d.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.cancel_scroll(scrolling::ScrollTarget::RuleDetail);
        cx.notify();
    }

    fn filter_rule_detail(&mut self, q: String, cx: &mut Context<Self>) {
        let Some(d) = &mut self.rule_browser.detail else {
            return;
        };
        d.query = q.clone();
        d.task = None;
        rebuild_rule_detail_shown(d);
        d.scroll.scroll_to_item(0, ScrollStrategy::Top);
        d.match_status.clear();
        if !q.is_empty() {
            d.match_status = "正在匹配…".into();
            let tag = d.tag.clone();
            d.task = Some(cx.spawn(async move |weak, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let result = singboard_core::runtime()
                    .spawn_blocking(move || {
                        let (dir, config, core) = rule_paths()?;
                        srs_match_provider(dir, config, core, tag, q)
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|v| v);
                let _ = weak.update(cx, |this, cx| {
                    if let Some(d) = &mut this.rule_browser.detail {
                        d.match_status = match result {
                            Ok(true) => "命中".into(),
                            Ok(false) => "未命中".into(),
                            Err(e) => e,
                        };
                        cx.notify();
                    }
                });
            }));
        }
        self.cancel_scroll(scrolling::ScrollTarget::RuleDetail);
        cx.notify();
    }

    pub(super) fn rules_page(&self, cx: &Context<Self>) -> Div {
        let palette = crate::theme::Palette::get(cx);
        let translucent = true;
        let tint = self.glass_tint;
        let query = self
            .filter
            .as_ref()
            .map(|f| f.read(cx).text.trim().to_lowercase())
            .unwrap_or_default();
        let rows = self.indices(&query);
        let count = rows.len();

        let mut tabs = div()
            .relative()
            .overflow_hidden()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded_full()
            .border_1()
            .border_color(palette.control_rim())
            .child(crate::glass::control_layer(palette, px(999.), true))
            .child(
                div()
                    .id("rules-tab-rules")
                    .relative()
                    .h(crate::theme::text_px(24.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .px_3()
                    .rounded_full()
                    .text_xs()
                    .when(!self.show_providers, |el| {
                        el.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(self.show_providers, |el| {
                        el.text_color(palette.ink().opacity(0.72))
                    })
                    .child(format!("规则 ({})", self.rules.len()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.show_providers {
                            this.apply(Action::Providers, cx);
                        }
                    })),
            );
        if !self.providers.is_empty() || self.show_providers {
            tabs = tabs.child(
                div()
                    .id("rules-tab-providers")
                    .relative()
                    .h(crate::theme::text_px(24.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .px_3()
                    .rounded_full()
                    .text_xs()
                    .when(self.show_providers, |el| {
                        el.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(!self.show_providers, |el| {
                        el.text_color(palette.ink().opacity(0.72))
                    })
                    .child(format!("规则提供商 ({})", self.providers.len()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.show_providers {
                            this.apply(Action::Providers, cx);
                        }
                    })),
            );
        }

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
                        crate::glass::backdrop_heading("规则", palette, cx)
                            .text_size(crate::theme::text_px(26.))
                            .line_height(crate::theme::text_px(32.))
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(tabs)
                    .when(self.show_providers, |el| {
                        let disabled = self.busy || self.providers.is_empty();
                        let names: Vec<String> =
                            self.providers.iter().map(|p| p.0.clone()).collect();
                        el.child(
                            div()
                                .id("rules-update-all")
                                .role(Role::Button)
                                .aria_label("更新全部规则提供商")
                                .tooltip(|window, cx| {
                                    gpui_component::tooltip::Tooltip::new("更新全部规则提供商")
                                        .build(window, cx)
                                })
                                .size(px(32.))
                                .flex_none()
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(!disabled, |d| {
                                    d.cursor_pointer()
                                        .hover(|s| s.bg(palette.fill_strong()))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.apply(
                                                Action::Run(Command::UpdateProviders {
                                                    proxies: false,
                                                    names: names.clone(),
                                                }),
                                                cx,
                                            );
                                        }))
                                })
                                .when(disabled, |d| d.opacity(0.45))
                                .child(
                                    svg()
                                        .path("refresh")
                                        .size(px(16.))
                                        .text_color(crate::glass::backdrop_ink(palette, cx)),
                                ),
                        )
                    }),
            );
        let mut provider_usage: std::collections::HashMap<String, Vec<(usize, String)>> =
            std::collections::HashMap::new();
        for (idx, r) in self.rules.iter().enumerate() {
            let payload = r["payload"].as_str().unwrap_or("");
            let tags = rule_set_tags_from_payload(payload);
            if tags.is_empty() {
                continue;
            }
            let target = match parse_rule_action(r) {
                RuleAction::Route { outbound, .. } => outbound,
                RuleAction::Reject(l) | RuleAction::Stop(l) | RuleAction::Pass(l) => l.into(),
            };
            let mut seen = std::collections::HashSet::new();
            for tag in tags {
                if seen.insert(tag.clone()) {
                    provider_usage
                        .entry(tag)
                        .or_default()
                        .push((idx + 1, target.clone()));
                }
            }
        }

        let show_providers = self.show_providers;
        let list_card = glass_card(palette, translucent, tint)
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .p(px(6.))
            .child(if count == 0 {
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(palette.muted())
                    .child(if self.busy {
                        "正在读取…"
                    } else if show_providers {
                        "当前配置里没有规则提供商"
                    } else if query.is_empty() {
                        "当前配置里没有路由规则"
                    } else {
                        "没有找到匹配的规则"
                    })
                    .into_any_element()
            } else if show_providers {
                let usage_map = std::sync::Arc::new(provider_usage);
                div()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .top(self.overscroll_offset(scrolling::ScrollTarget::PageList))
                            .size_full()
                            .child(
                                uniform_list(
                    "rule-providers-list",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        let p = crate::theme::Palette::get(cx);
                        range
                            .filter_map(|idx| rows.get(idx).copied())
                            .filter(|&i| i < this.providers.len())
                            .map(|i| {
                                let (name, item) = &this.providers[i];
                                let rule_count = item["ruleCount"].as_u64().unwrap_or(0);
                                let updated = item["updatedAt"]
                                    .as_str()
                                    .unwrap_or("")
                                    .split('T')
                                    .next()
                                    .unwrap_or("");
                                let vehicle = item["vehicleType"].as_str().unwrap_or("");
                                let behavior = item["behavior"].as_str().unwrap_or("");
                                let is_inline = vehicle.eq_ignore_ascii_case("inline");
                                let (prefix, base) = provider_name_parts(name);
                                let mut meta_parts = Vec::new();
                                let v_lbl = vehicle_label(vehicle);
                                if !v_lbl.is_empty() {
                                    meta_parts.push(v_lbl);
                                }
                                let f_lbl = format_label(behavior);
                                if !f_lbl.is_empty() {
                                    meta_parts.push(f_lbl);
                                }
                                if !updated.is_empty() {
                                    meta_parts.push(format!("{updated} 更新"));
                                }
                                let usage_str = match usage_map.get(name) {
                                    None => "未被路由规则引用".into(),
                                    Some(u) if u.len() == 1 => {
                                        format!("第 {} 条 → {}", u[0].0, u[0].1)
                                    }
                                    Some(u) => format!(
                                        "第 {} 条",
                                        u.iter()
                                            .map(|x| x.0.to_string())
                                            .collect::<Vec<_>>()
                                            .join("、")
                                    ),
                                };
                                meta_parts.push(usage_str);
                                let icon_glyph = if vehicle.eq_ignore_ascii_case("remote")
                                    || vehicle.eq_ignore_ascii_case("http")
                                {
                                    "remote"
                                } else if is_inline {
                                    "inline"
                                } else {
                                    "file"
                                };
                                let inspect_name = name.clone();
                                let update_name = name.clone();

                                let rp_row_h =
                                    (36. * crate::theme::effective_text_scale(cx) + 18.).ceil();
                                let mut line = div()
                                    .id(("rp-line", i))
                                    .w_full()
                                    .h(px(rp_row_h))
                                    .px(px(10.))
                                    .rounded(px(12.))
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .hover(|s| s.bg(p.hover()))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.apply(Action::RuleDetail(inspect_name.clone()), cx);
                                    }))
                                    .child(
                                        div()
                                            .size(px(30.))
                                            .flex_none()
                                            .rounded(px(9.))
                                            .bg(p.fill())
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .text_color(p.muted())
                                            .child(
                                                svg()
                                                    .path(icon_glyph)
                                                    .size(px(16.))
                                                    .text_color(p.muted()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(1.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_baseline()
                                                    .min_w_0()
                                                    .text_size(crate::theme::text_px(13.))
                                                    .when(!prefix.is_empty(), |el| {
                                                        el.child(
                                                            div()
                                                                .text_color(p.muted())
                                                                .child(prefix.to_owned()),
                                                        )
                                                    })
                                                    .child(
                                                        div()
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .truncate()
                                                            .child(crate::emoji::text(base.to_owned())),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(p.muted())
                                                    .truncate()
                                                    .child(meta_parts.join(" · ")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .flex()
                                            .items_baseline()
                                            .gap(px(3.))
                                            .child(
                                                div()
                                                    .text_size(crate::theme::text_px(13.))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(format_count(rule_count)),
                                            )
                                            .child(
                                                div().text_xs().text_color(p.muted()).child("条"),
                                            ),
                                    );
                                if !is_inline {
                                    line = line.child(
                                        div()
                                            .id(("rp-update", i))
                                            .size(px(28.))
                                            .flex_none()
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .hover(|s| s.bg(p.fill()))
                                            .text_xs()
                                            .text_color(p.muted())
                                            .child(
                                                svg()
                                                    .path("refresh")
                                                    .size(px(14.))
                                                    .text_color(p.muted()),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.apply(
                                                    Action::Run(Command::UpdateRuleProvider {
                                                        name: update_name.clone(),
                                                    }),
                                                    cx,
                                                );
                                            })),
                                    );
                                }
                                line
                            })
                            .collect()
                    }),
                )
                .track_scroll(&self.page_list_scroll)
                .size_full(),
                    ),
                )
                    .child(self.smooth_scroll_capture(scrolling::ScrollTarget::PageList, cx))
                    .into_any_element()
            } else {
                let total_rows = count;
                div()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .top(self.overscroll_offset(scrolling::ScrollTarget::PageList))
                            .size_full()
                            .child(
                                uniform_list(
                    "rules-flow-list",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        let p = crate::theme::Palette::get(cx);
                        range
                            .filter_map(|pos| rows.get(pos).copied().map(|i| (pos, i)))
                            .filter(|&(_, i)| i < this.rules.len())
                            .map(|(pos, i)| {
                                let r = &this.rules[i];
                                let tokens = tokenize_condition(r);
                                let action = parse_rule_action(r);
                                let is_first = pos == 0;
                                let is_last = pos + 1 == total_rows;
                                let is_final = r["condition"].as_str() == Some("最终回退");

                                let rail_dot = match &action {
                                    RuleAction::Reject(_) => div()
                                        .size(px(15.))
                                        .rounded_full()
                                        .bg(p.danger_soft())
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div().size(px(9.)).rounded_full().bg(p.danger_fill()),
                                        )
                                        .into_any_element(),
                                    RuleAction::Pass(_) => div()
                                        .size(px(9.))
                                        .rounded_full()
                                        .bg(p.card(false))
                                        .border_2()
                                        .border_color(p.muted())
                                        .into_any_element(),
                                    _ if is_final => div()
                                        .size(px(11.))
                                        .rounded(px(4.))
                                        .bg(p.ink())
                                        .into_any_element(),
                                    _ => div()
                                        .size(px(9.))
                                        .rounded_full()
                                        .bg(p.ink().opacity(0.44))
                                        .into_any_element(),
                                };

                                let rule_row_h =
                                    (26. * crate::theme::effective_text_scale(cx) + 16.).ceil();
                                let rail = div()
                                    .w(px(14.))
                                    .h(px(rule_row_h))
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .w(px(1.5))
                                            .when(!is_first, |el| el.bg(p.ink().opacity(0.20))),
                                    )
                                    .child(rail_dot)
                                    .child(
                                        div()
                                            .flex_1()
                                            .w(px(1.5))
                                            .when(!is_last, |el| el.bg(p.ink().opacity(0.20))),
                                    );

                                let mut cond_row = div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_size(crate::theme::text_px(13.));
                                for (ti, tok) in tokens.into_iter().enumerate() {
                                    match tok {
                                        ConditionToken::Word(t) => {
                                            cond_row = cond_row.child(
                                                div().flex_none().text_color(p.muted()).child(t),
                                            );
                                        }
                                        ConditionToken::Text(t) => {
                                            cond_row = cond_row.child(
                                                div().flex_none().text_color(p.ink()).child(t),
                                            );
                                        }
                                        ConditionToken::Mono(t) => {
                                            cond_row = cond_row.child(
                                                div()
                                                    .font_family("Consolas")
                                                    .text_xs()
                                                    .truncate()
                                                    .child(t),
                                            );
                                        }
                                        ConditionToken::RuleSet { short, full } => {
                                            cond_row = cond_row.child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "rset-{i}-{ti}"
                                                    )))
                                                    .flex_none()
                                                    .cursor_pointer()
                                                    .h(crate::theme::text_px(22.))
                                                    .px(px(9.))
                                                    .rounded_full()
                                                    .bg(
                                                        if this.rule_browser.matched.contains(&full)
                                                        {
                                                            p.primary_soft()
                                                        } else {
                                                            p.fill()
                                                        },
                                                    )
                                                    .hover(|s| s.bg(p.fill_strong()))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(4.))
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(p.ink())
                                                    .child(
                                                        div()
                                                            .text_size(crate::theme::text_px(10.))
                                                            .text_color(p.muted())
                                                            .child(
                                                                svg()
                                                                    .path("layers")
                                                                    .size(px(12.))
                                                                    .text_color(p.muted()),
                                                            ),
                                                    )
                                                    .child(short)
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            cx.stop_propagation();
                                                            this.apply(
                                                                Action::RuleDetail(full.clone()),
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            );
                                        }
                                    }
                                }

                                let right_el = match action {
                                    RuleAction::Route {
                                        outbound,
                                        final_node,
                                    } => {
                                        let reject_final = final_node
                                            .as_deref()
                                            .is_some_and(|n| n.to_lowercase().contains("reject"));
                                        div()
                                            .flex_none()
                                            .max_w(px(340.))
                                            .flex()
                                            .items_baseline()
                                            .gap(px(5.))
                                            .text_size(crate::theme::text_px(13.))
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(p.ink())
                                                    .child(crate::emoji::text(outbound)),
                                            )
                                            .when_some(final_node, |el, node| {
                                                el.child(
                                                    div()
                                                        .truncate()
                                                        .text_color(if reject_final {
                                                            p.danger()
                                                        } else {
                                                            p.muted()
                                                        })
                                                        .child(crate::emoji::text(format!("› {node}"))),
                                                )
                                            })
                                            .into_any_element()
                                    }
                                    RuleAction::Reject(label) => div()
                                        .flex_none()
                                        .h(crate::theme::text_px(24.))
                                        .px(px(10.))
                                        .rounded_full()
                                        .bg(p.danger_soft())
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.danger())
                                        .child(
                                            svg()
                                                .path("reject")
                                                .size(px(13.))
                                                .text_color(p.danger()),
                                        )
                                        .child(label)
                                        .into_any_element(),
                                    RuleAction::Stop(label) => div()
                                        .flex_none()
                                        .h(crate::theme::text_px(24.))
                                        .px(px(10.))
                                        .rounded_full()
                                        .bg(p.fill())
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.muted())
                                        .child(
                                            svg()
                                                .path("hijack")
                                                .size(px(13.))
                                                .text_color(p.muted()),
                                        )
                                        .child(label)
                                        .into_any_element(),
                                    RuleAction::Pass(label) => div()
                                        .flex_none()
                                        .h(crate::theme::text_px(24.))
                                        .px(px(10.))
                                        .rounded_full()
                                        .bg(p.hover())
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.muted())
                                        .child(
                                            svg()
                                                .path(match label {
                                                    "嗅探" => "sniff",
                                                    "路由选项" => "tune",
                                                    _ => "resolve",
                                                })
                                                .size(px(13.))
                                                .text_color(p.muted()),
                                        )
                                        .child(label)
                                        .into_any_element(),
                                };

                                let links =
                                    rule_set_tags_from_payload(r["payload"].as_str().unwrap_or(""))
                                        .into_iter()
                                        .filter(|tag| this.providers.iter().any(|p| &p.0 == tag))
                                        .collect::<HashSet<_>>();
                                div()
                                    .id(("rule-entry", i))
                                    .when(links.len() == 1, |el| {
                                        let tag = links.into_iter().next().unwrap();
                                        el.cursor_pointer().on_click(cx.listener(
                                            move |this, _, _, cx| {
                                                this.apply(Action::RuleDetail(tag.clone()), cx)
                                            },
                                        ))
                                    })
                                    .w_full()
                                    .h(px(rule_row_h))
                                    .pl(px(10.))
                                    .pr(px(12.))
                                    .rounded(px(12.))
                                    .when(is_final, |el| el.bg(p.fill()))
                                    .hover(|s| s.bg(p.hover()))
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(rail)
                                    .child(
                                        div()
                                            .w(px(24.))
                                            .flex_none()
                                            .flex()
                                            .justify_end()
                                            .text_size(crate::theme::text_px(11.))
                                            .text_color(p.muted())
                                            .child(if is_final {
                                                String::new()
                                            } else {
                                                format!("{}", i + 1)
                                            }),
                                    )
                                    .child(cond_row)
                                    .child(right_el)
                            })
                            .collect()
                    }),
                )
                .track_scroll(&self.page_list_scroll)
                .size_full(),
                    ),
                )
                    .child(self.smooth_scroll_capture(scrolling::ScrollTarget::PageList, cx))
                    .into_any_element()
            })
            .when(count > 0, |card| card.vertical_scrollbar(&self.page_list_scroll));

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(header)
            .child(div().w_full().children(self.filter.clone()))
            .when(!self.rule_browser.status.is_empty(), |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(palette.muted())
                        .truncate()
                        .child(self.rule_browser.status.clone()),
                )
            })
            .when(!self.load_error.is_empty(), |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(palette.danger())
                        .child(self.load_error.clone()),
                )
            })
            .child(list_card)
    }

    pub(crate) fn rule_overlay(&self, window: &Window, cx: &Context<Self>) -> Option<Div> {
        let (detail, leaving) = match (&self.rule_browser.detail, &self.leaving_dialog) {
            (Some(detail), _) => (detail, false),
            (None, Some((_, dialog::LeavingDialog::Rule(detail)))) => (detail, true),
            _ => return None,
        };
        let tint = self.glass_tint;
        let palette = crate::theme::Palette::get(cx);
        let uses = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                rule_set_tags_from_payload(r["payload"].as_str().unwrap_or(""))
                    .contains(&detail.tag)
            })
            .map(|(i, r)| {
                (
                    i + 1,
                    match parse_rule_action(r) {
                        RuleAction::Route { outbound, .. } => outbound,
                        RuleAction::Reject(s) | RuleAction::Stop(s) | RuleAction::Pass(s) => {
                            s.to_owned()
                        }
                    },
                )
            })
            .collect::<Vec<_>>();
        let detail_uses = match uses.as_slice() {
            [] => None,
            [(i, target)] => Some(format!("用于第 {i} 条规则 → {target}")),
            other => Some(format!(
                "用于第 {} 条规则",
                other
                    .iter()
                    .map(|u| u.0.to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            )),
        };
        Some(div().absolute().inset_0().size_full().when_some(
            Some(detail),
            |el, d| {
                let metadata = self
                    .providers
                    .iter()
                    .find(|(name, _)| name == &d.tag)
                    .map(|p| &p.1);
                let vehicle = metadata
                    .and_then(|p| p["vehicleType"].as_str())
                    .unwrap_or("");
                let icon = match vehicle.to_lowercase().as_str() {
                    "remote" | "http" => "remote",
                    "inline" => "inline",
                    _ => "file",
                };
                let meta = metadata
                    .map(|p| {
                        let updated = p["updatedAt"]
                            .as_str()
                            .and_then(|v| v.split('T').next())
                            .unwrap_or("");
                        [
                            vehicle_label(vehicle),
                            format_label(p["behavior"].as_str().unwrap_or("")),
                            format!("{} 条", p["ruleCount"].as_u64().unwrap_or(0)),
                            if updated.is_empty() {
                                String::new()
                            } else {
                                format!("{updated} 更新")
                            },
                        ]
                        .into_iter()
                        .filter(|v| !v.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ")
                    })
                    .unwrap_or_default();
                let mut card = popover_card(palette, tint)
                    .id("rule-dialog-card")
                    .w(px(672.))
                    .max_w_full()
                    .h(px(f32::from(window.viewport_size().height) * 0.8))
                    .max_h_full()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(svg().path(icon).size(px(20.)).text_color(palette.muted()))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .child(crate::emoji::text(d.tag.clone())),
                            )
                            .child(
                                dialog::keyboard_action(div()
                                    .id("rule-modal-close")
                                    .cursor_pointer()
                                    .p_2()
                                    .child(
                                        svg()
                                            .path("close-detail")
                                            .size(px(12.))
                                            .text_color(palette.muted()),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.apply(Action::CloseRuleDetail, cx)
                                    })), Action::CloseRuleDetail, cx),
                            ),
                    )
                    .child(div().text_xs().text_color(palette.muted()).child(meta))
                    .when_some(detail_uses, |e, uses| e.child(div().text_xs().child(crate::emoji::text(uses))))
                    .child(d.search.clone())
                    .when(!d.match_status.is_empty(), |e| {
                        e.child(
                            div()
                                .text_xs()
                                .text_color(palette.accent())
                                .child(d.match_status.clone()),
                        )
                    });
                if d.status.is_empty() && d.type_counts.len() > 1 {
                    let mut chips = div().flex().flex_wrap().items_center().gap(px(6.));
                    let all_active = d.active_type.is_none();
                    chips = chips.child(
                        div()
                            .id("rule-detail-type-all")
                            .h(crate::theme::text_px(24.))
                            .px(px(10.))
                            .rounded_full()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_xs()
                            .when(all_active, |el| {
                                el.bg(palette.primary_soft())
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette.accent())
                            })
                            .when(!all_active, |el| {
                                el.bg(palette.fill())
                                    .text_color(palette.muted())
                                    .hover(|s| s.bg(palette.fill_strong()))
                            })
                            .child("全部")
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(11.))
                                    .opacity(0.75)
                                    .child(format_count(d.entries.len() as u64)),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.select_rule_detail_type(None, cx);
                            })),
                    );
                    for (idx, (rule_type, count)) in d.type_counts.iter().enumerate() {
                        let active = d.active_type.as_deref() == Some(rule_type.as_str());
                        let (pill_bg, pill_fg) = rule_type_badge_colors(rule_type, palette);
                        let label = field_label(rule_type).unwrap_or(rule_type).to_owned();
                        let selected = rule_type.clone();
                        chips = chips.child(
                            div()
                                .id(("rule-detail-type", idx))
                                .h(crate::theme::text_px(24.))
                                .px(px(10.))
                                .rounded_full()
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_xs()
                                .when(active, |el| {
                                    el.bg(pill_bg)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(pill_fg)
                                })
                                .when(!active, |el| {
                                    el.bg(palette.fill())
                                        .text_color(palette.muted())
                                        .hover(|s| s.bg(palette.fill_strong()))
                                })
                                .child(label)
                                .child(
                                    div()
                                        .text_size(crate::theme::text_px(11.))
                                        .opacity(0.75)
                                        .child(format_count(*count as u64)),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select_rule_detail_type(Some(selected.clone()), cx);
                                })),
                        );
                    }
                    card = card.child(chips);
                }
                if !d.status.is_empty() {
                    card = card.child(div().text_sm().child(d.status.clone())).when(
                        d.status != "正在读取…",
                        |e| {
                            e.child(self.button(
                                "rule-retry",
                                "重试",
                                Action::RuleDetail(d.tag.clone()),
                                cx,
                            ))
                        },
                    );
                } else if d.shown.is_empty() {
                    card = card.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(palette.muted())
                            .child(if d.entries.is_empty() {
                                "这个规则提供商没有规则".into()
                            } else if !d.query.is_empty() {
                                format!("没有包含“{}”的条目", d.query)
                            } else {
                                "没有符合条件的条目".into()
                            }),
                    );
                } else {
                    card = card.child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .rounded(px(12.))
                            .bg(palette.fill())
                            .p(px(4.))
                            .overflow_hidden()
                            .child(
                                div()
                                    .relative()
                                    .top(self.overscroll_offset(scrolling::ScrollTarget::RuleDetail))
                                    .size_full()
                                    .child(
                                        uniform_list(
                                    "rule-detail-entries",
                                    d.shown.len(),
                                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                        let p = crate::theme::Palette::get(cx);
                                        let d = match (
                                            &this.rule_browser.detail,
                                            &this.leaving_dialog,
                                        ) {
                                            (Some(d), _) => d,
                                            (None, Some((_, dialog::LeavingDialog::Rule(d)))) => d,
                                            _ => return Vec::new(),
                                        };
                                        range
                                            .filter_map(|pos| {
                                                let item = d.entries.get(*d.shown.get(pos)?)?;
                                                let group_break = pos > 0
                                                    && d.entries[d.shown[pos - 1]].rule_type
                                                        != item.rule_type;
                                                let (pill_bg, pill_fg) =
                                                    rule_type_badge_colors(&item.rule_type, p);
                                                let label = field_label(&item.rule_type)
                                                    .unwrap_or(&item.rule_type)
                                                    .to_owned();
                                                let copy_value = item.value.clone();
                                                Some(
                                                    div()
                                                        .w_full()
                                                        .h(crate::theme::text_px(30.))
                                                        .when(group_break, |el| {
                                                            el.border_t_1().border_color(p.line())
                                                        })
                                                        .child(
                                                            div()
                                                                .id(("rule-detail-row", pos))
                                                                .size_full()
                                                                .px(px(8.))
                                                                .rounded(px(7.))
                                                                .cursor_pointer()
                                                                .hover(|s| s.bg(p.hover()))
                                                                .flex()
                                                                .items_center()
                                                                .gap(px(10.))
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        cx.write_to_clipboard(
                                                                            ClipboardItem::new_string(
                                                                                copy_value.clone(),
                                                                            ),
                                                                        );
                                                                        this.message(
                                                                            format!(
                                                                                "已复制 {copy_value}"
                                                                            ),
                                                                            false,
                                                                            cx,
                                                                        );
                                                                    },
                                                                ))
                                                                .child(
                                                                    div()
                                                                        .w(crate::theme::text_px(72.))
                                                                        .h(crate::theme::text_px(20.))
                                                                        .flex_none()
                                                                        .px(px(6.))
                                                                        .rounded(px(6.))
                                                                        .bg(pill_bg)
                                                                        .flex()
                                                                        .items_center()
                                                                        .justify_center()
                                                                        .text_size(crate::theme::text_px(11.))
                                                                        .font_weight(
                                                                            FontWeight::MEDIUM,
                                                                        )
                                                                        .text_color(pill_fg)
                                                                        .child(
                                                                            div()
                                                                                .truncate()
                                                                                .child(label),
                                                                        ),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .truncate()
                                                                        .font_family("Consolas")
                                                                        .text_xs()
                                                                        .text_color(p.ink())
                                                                        .child(item.value.clone()),
                                                                ),
                                                        ),
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                    }),
                                )
                                .track_scroll(&d.scroll)
                                .size_full(),
                                    ),
                            )
                            .child(self.smooth_scroll_capture(scrolling::ScrollTarget::RuleDetail, cx))
                            .vertical_scrollbar(&d.scroll),
                    );
                }
                if d.status.is_empty() {
                    card = card.child(div().text_xs().text_color(palette.muted()).child(
                        if d.query.is_empty() && d.active_type.is_none() {
                            format!("共 {} 条", format_count(d.entries.len() as u64))
                        } else {
                            format!(
                                "显示 {} / 共 {} 条",
                                format_count(d.shown.len() as u64),
                                format_count(d.entries.len() as u64)
                            )
                        },
                    ));
                }
                el.child(
                    dialog::surface("rule-details-pop", self.page_dialog("rules-detail", Action::CloseRuleDetail, cx), card, 672., leaving, cx),
                )
            },
        ))
    }
}
