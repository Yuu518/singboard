use super::*;
use gpui_component::scroll::ScrollableElement;
use singboard_core::commands::srs::{RuleEntry, srs_list_provider, srs_match_provider};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

#[derive(Default)]
pub(super) struct RuleBrowser {
    pub(super) matched: HashSet<String>,
    pub(super) toggling: HashSet<String>,
    pub(super) searching: bool,
    pub(super) pre_expanded: bool,
    criteria: Option<(String, Vec<String>, bool)>,
    status: String,
    search_task: Option<Task<()>>,
    pub(super) detail: Option<RuleDetail>,
    pub(super) inspect: Option<RuleInspect>,
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

pub(super) struct RuleInspect {
    pub(super) index: usize,
    pub(super) rule: Value,
    pub(super) scroll: ScrollHandle,
}

fn rule_paths() -> Result<(String, String, String), String> {
    let prefs = crate::maintenance::preferences()?;
    let config = crate::service::snapshot()?
        .map(|s| s.config.to_string_lossy().into_owned())
        .unwrap_or(prefs.config_path);
    Ok((prefs.working_dir, config, prefs.singbox_path))
}

pub(super) fn lookup_query(q: &str) -> bool {
    let (host, prefix) = q.split_once('/').map_or((q, None), |(h, p)| (h, Some(p)));
    if let Ok(addr) = host.parse::<IpAddr>() {
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum ClausePart {
    Op(&'static str),
    RuleSets {
        negated: bool,
        sets: Vec<(String, String)>,
    },
    Item {
        negated: bool,
        label: Option<&'static str>,
        values: Vec<String>,
        mono: bool,
    },
    Final,
    AllTraffic,
}

#[derive(Clone)]
enum RuleAction {
    Route {
        outbound: String,
        final_node: Option<String>,
        direct: bool,
        reject_final: bool,
    },
    Reject(&'static str),
    Stop(&'static str),
    Pass(&'static str),
}

impl RuleAction {
    fn is_preprocessing(&self) -> bool {
        matches!(self, Self::Pass(_) | Self::Stop(_))
    }

    fn summary_label(&self) -> String {
        match self {
            Self::Route { outbound, .. } => outbound.clone(),
            Self::Reject(l) | Self::Stop(l) | Self::Pass(l) => (*l).to_owned(),
        }
    }
}

struct RuleTrace {
    query: String,
    winner: Option<usize>,
    passed: HashSet<usize>,
    shadowed: HashSet<usize>,
    pending: bool,
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

fn split_raw_values(raw: &str) -> Vec<&str> {
    if raw.starts_with('[') && raw.ends_with(']') {
        raw[1..raw.len() - 1].split_whitespace().collect()
    } else {
        vec![raw]
    }
}

fn item_to_clause(key: &str, raw: &str, negated: bool) -> ClausePart {
    let list = split_raw_values(raw);
    if key == "rule_set" {
        let sets = list
            .into_iter()
            .map(|full| {
                let short = full.rsplit('/').next().unwrap_or(full).to_owned();
                (short, full.to_owned())
            })
            .collect();
        return ClausePart::RuleSets { negated, sets };
    }
    if key == "ip_is_private" {
        return ClausePart::Item {
            negated,
            label: None,
            values: vec![if raw == "true" { "私有 IP" } else { "非私有 IP" }.into()],
            mono: false,
        };
    }
    if key == "ip_version" {
        return ClausePart::Item {
            negated,
            label: None,
            values: vec![format!("IPv{raw}")],
            mono: false,
        };
    }
    let Some(label) = field_label(key) else {
        return ClausePart::Item {
            negated,
            label: None,
            values: vec![format!("{key}={raw}")],
            mono: true,
        };
    };
    let values = list
        .into_iter()
        .map(|val| {
            if is_upper_field(key) {
                val.to_uppercase()
            } else {
                val.to_owned()
            }
        })
        .collect();
    ClausePart::Item {
        negated,
        label: Some(label),
        values,
        mono: is_mono_field(key),
    }
}

fn term_to_clauses(term: &str) -> Vec<ClausePart> {
    let mut body = term.trim();
    let mut negated = false;
    if let Some(inner) = body.strip_prefix("!(").and_then(|s| s.strip_suffix(')')) {
        negated = true;
        body = inner;
    } else if let Some(inner) = body.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        body = inner;
    }
    let items = parse_condition_items(body);
    if items.is_empty() {
        return vec![ClausePart::Item {
            negated,
            label: None,
            values: vec![body.to_owned()],
            mono: true,
        }];
    }
    let mut out = Vec::new();
    for (i, (k, v)) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(ClausePart::Op("且"));
        }
        out.push(item_to_clause(&k, &v, negated && i == 0));
    }
    out
}

fn split_payload_terms(payload: &str) -> Vec<(Option<&'static str>, &str)> {
    let mut out = Vec::new();
    let mut rest = payload.trim();
    let mut pending_op: Option<&'static str> = None;
    while !rest.is_empty() {
        let and_pos = rest.find(" && ");
        let or_pos = rest.find(" || ");
        match (and_pos, or_pos) {
            (Some(a), Some(o)) if a < o => {
                out.push((pending_op, &rest[..a]));
                pending_op = Some("且");
                rest = &rest[a + 4..];
            }
            (Some(a), None) => {
                out.push((pending_op, &rest[..a]));
                pending_op = Some("且");
                rest = &rest[a + 4..];
            }
            (_, Some(o)) => {
                out.push((pending_op, &rest[..o]));
                pending_op = Some("或");
                rest = &rest[o + 4..];
            }
            (None, None) => {
                out.push((pending_op, rest));
                break;
            }
        }
    }
    out
}

fn parse_condition_clauses(rule: &Value) -> Vec<ClausePart> {
    if rule["condition"].as_str() == Some("最终回退") {
        return vec![ClausePart::Final];
    }
    let payload = rule["payload"].as_str().unwrap_or("").trim();
    if payload.is_empty() {
        return vec![ClausePart::AllTraffic];
    }
    let mut parts = Vec::new();
    for (op, term) in split_payload_terms(payload) {
        if let Some(op_str) = op {
            parts.push(ClausePart::Op(op_str));
        }
        parts.extend(term_to_clauses(term));
    }
    parts
}

fn is_private_ip(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || (ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000) == 64)
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}

fn ip_in_cidr(addr: IpAddr, cidr: &str) -> bool {
    let Some((net_str, prefix_str)) = cidr.split_once('/') else {
        return cidr.parse::<IpAddr>().is_ok_and(|net| net == addr);
    };
    let Ok(net) = net_str.parse::<IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix_str.parse::<u32>() else {
        return false;
    };
    match (addr, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) if prefix <= 32 => {
            if prefix == 0 {
                true
            } else {
                let mask = !0u32 << (32 - prefix);
                (u32::from(a) & mask) == (u32::from(n) & mask)
            }
        }
        (IpAddr::V6(a), IpAddr::V6(n)) if prefix <= 128 => {
            if prefix == 0 {
                true
            } else {
                let mask = !0u128 << (128 - prefix);
                (u128::from(a) & mask) == (u128::from(n) & mask)
            }
        }
        _ => false,
    }
}

fn eval_item_for_lookup(key: &str, raw: &str, q: &str, ip: Option<IpAddr>, matched_sets: &HashSet<String>) -> Option<bool> {
    let list = split_raw_values(raw);
    match key {
        "rule_set" => Some(list.iter().any(|tag| matched_sets.contains(*tag))),
        "domain" => {
            if ip.is_some() {
                return Some(false);
            }
            let host = q.trim_end_matches('.');
            Some(list.iter().any(|v| host.eq_ignore_ascii_case(v.trim_end_matches('.'))))
        }
        "domain_suffix" => {
            if ip.is_some() {
                return Some(false);
            }
            let host = q.trim_end_matches('.').to_ascii_lowercase();
            Some(list.iter().any(|v| {
                let suffix = v.trim_start_matches('.').trim_end_matches('.').to_ascii_lowercase();
                !suffix.is_empty() && (host == suffix || host.ends_with(&format!(".{suffix}")))
            }))
        }
        "domain_keyword" => {
            if ip.is_some() {
                return Some(false);
            }
            let host = q.to_ascii_lowercase();
            Some(list.iter().any(|v| !v.is_empty() && host.contains(&v.to_ascii_lowercase())))
        }
        "ip_cidr" => {
            let addr = ip?;
            Some(list.iter().any(|cidr| ip_in_cidr(addr, cidr)))
        }
        "ip_is_private" => {
            let addr = ip?;
            let expect = raw == "true";
            Some(is_private_ip(addr) == expect)
        }
        "ip_version" => {
            let addr = ip?;
            Some(matches!(
                (addr, raw),
                (IpAddr::V4(_), "4") | (IpAddr::V6(_), "6")
            ))
        }
        _ => None,
    }
}

fn eval_term_for_lookup(term: &str, q: &str, ip: Option<IpAddr>, matched_sets: &HashSet<String>) -> Option<bool> {
    let mut body = term.trim();
    let mut negated = false;
    if let Some(inner) = body.strip_prefix("!(").and_then(|s| s.strip_suffix(')')) {
        negated = true;
        body = inner;
    } else if let Some(inner) = body.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        body = inner;
    }
    let items = parse_condition_items(body);
    if items.is_empty() {
        return None;
    }
    let mut all_known = true;
    for (k, v) in items {
        match eval_item_for_lookup(&k, &v, q, ip, matched_sets) {
            Some(false) => return Some(negated),
            Some(true) => {}
            None => all_known = false,
        }
    }
    if all_known {
        Some(!negated)
    } else {
        None
    }
}

fn eval_rule_for_lookup(rule: &Value, q: &str, matched_sets: &HashSet<String>) -> bool {
    if rule["condition"].as_str() == Some("最终回退") {
        return true;
    }
    let payload = rule["payload"].as_str().unwrap_or("").trim();
    if payload.is_empty() {
        return true;
    }
    let host = q.split_once('/').map_or(q, |(h, _)| h);
    let ip = host.parse::<IpAddr>().ok();
    let terms = split_payload_terms(payload);
    let mut acc: Option<bool> = None;
    for (op, term) in terms {
        let val = eval_term_for_lookup(term, q, ip, matched_sets);
        acc = match (acc, op, val) {
            (_, None, v) => v,
            (Some(a), Some("且"), Some(b)) => Some(a && b),
            (Some(false), Some("且"), None) | (None, Some("且"), Some(false)) => Some(false),
            (Some(true), Some("且"), None) | (None, Some("且"), Some(true)) => None,
            (Some(a), Some("或"), Some(b)) => Some(a || b),
            (Some(true), Some("或"), None) | (None, Some("或"), Some(true)) => Some(true),
            (Some(false), Some("或"), None) | (None, Some("或"), Some(false)) => None,
            _ => None,
        };
    }
    acc == Some(true)
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

fn is_direct_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    lower == "direct"
        || lower.contains("direct")
        || label.contains("直连")
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
    let reject_final = final_node
        .as_deref()
        .is_some_and(|n| n.to_ascii_lowercase().contains("reject"));
    let direct = !reject_final
        && (final_node.as_deref().is_some_and(is_direct_label)
            || (final_node.is_none() && is_direct_label(&outbound)));
    RuleAction::Route {
        outbound,
        final_node,
        direct,
        reject_final,
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

fn format_compact_count(n: u64) -> String {
    if n >= 10_000 {
        let s = format!("{:.1}", n as f64 / 1000.0);
        format!("{}k", s.strip_suffix(".0").unwrap_or(&s))
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn render_action_capsule(action: &RuleAction, p: crate::theme::Palette) -> AnyElement {
    match action {
        RuleAction::Route {
            outbound,
            final_node,
            direct,
            reject_final,
        } => {
            let (bg, fg) = if *reject_final {
                (p.danger_soft(), p.danger())
            } else if *direct {
                (p.success_soft(), p.success_ink())
            } else {
                (p.primary_soft(), p.accent())
            };
            div()
                .flex_none()
                .max_w(px(320.))
                .h(crate::theme::text_px(24.))
                .px(px(10.))
                .rounded_full()
                .bg(bg)
                .flex()
                .items_center()
                .gap(px(5.))
                .text_xs()
                .when(*direct, |el| {
                    el.child(
                        div()
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(p.success_dot()),
                    )
                })
                .when(*reject_final, |el| {
                    el.child(svg().path("reject").size(px(12.)).text_color(p.danger()))
                })
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg)
                        .child(crate::emoji::text(outbound.clone())),
                )
                .when_some(final_node.clone(), |el, node| {
                    el.child(
                        div()
                            .truncate()
                            .text_color(if *reject_final {
                                p.danger()
                            } else if *direct {
                                p.success_ink().opacity(0.75)
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
            .child(svg().path("reject").size(px(13.)).text_color(p.danger()))
            .child(*label)
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
            .child(svg().path("hijack").size(px(13.)).text_color(p.muted()))
            .child(*label)
            .into_any_element(),
        RuleAction::Pass(label) => div()
            .flex_none()
            .h(crate::theme::text_px(24.))
            .px(px(10.))
            .rounded_full()
            .bg(p.hover())
            .border_1()
            .border_color(p.line())
            .flex()
            .items_center()
            .gap(px(4.))
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.muted())
            .child(
                svg()
                    .path(match *label {
                        "嗅探" => "sniff",
                        "路由选项" => "tune",
                        _ => "resolve",
                    })
                    .size(px(13.))
                    .text_color(p.muted()),
            )
            .child(*label)
            .into_any_element(),
    }
}

fn rule_uuid(rule: &Value) -> Option<&str> {
    rule["uuid"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn is_rule_disabled(rule: &Value) -> bool {
    rule["disabled"].as_bool().unwrap_or(false)
}

fn render_rule_switch(
    id: impl Into<ElementId>,
    enabled: bool,
    toggling: bool,
    uuid: String,
    p: crate::theme::Palette,
    cx: &Context<Workspace>,
) -> AnyElement {
    let label = if enabled {
        "临时禁用此规则"
    } else {
        "重新启用此规则"
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(label).build(window, cx))
        .flex_none()
        .w(px(34.))
        .h(px(20.))
        .p(px(2.))
        .rounded_full()
        .bg(if enabled {
            p.primary_fill()
        } else {
            p.toggle_off()
        })
        .flex()
        .items_center()
        .when(enabled, |el| el.justify_end())
        .when(toggling, |el| {
            el.opacity(0.55).on_click(|_, _, cx| cx.stop_propagation())
        })
        .when(!toggling, |el| {
            el.cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.apply(
                        Action::Run(Command::ToggleRule { uuid: uuid.clone() }),
                        cx,
                    );
                }))
        })
        .child(div().size(px(16.)).rounded_full().bg(rgb(0xffffff)).shadow_sm())
        .into_any_element()
}

fn render_rail_dot(
    action: &RuleAction,
    is_final: bool,
    is_disabled: bool,
    p: crate::theme::Palette,
) -> AnyElement {
    if is_disabled {
        return div()
            .size(px(9.))
            .rounded_full()
            .bg(p.card(false))
            .border_2()
            .border_color(p.muted().opacity(0.45))
            .into_any_element();
    }
    if is_final {
        return div()
            .size(px(10.))
            .rounded(px(3.))
            .bg(p.ink())
            .into_any_element();
    }
    match action {
        RuleAction::Reject(_)
        | RuleAction::Route {
            reject_final: true, ..
        } => div()
            .size(px(14.))
            .rounded_full()
            .bg(p.danger_soft())
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(8.)).rounded_full().bg(p.danger_fill()))
            .into_any_element(),
        RuleAction::Route { direct: true, .. } => div()
            .size(px(14.))
            .rounded_full()
            .bg(p.success_soft())
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(8.)).rounded_full().bg(p.success_dot()))
            .into_any_element(),
        RuleAction::Route { .. } => div()
            .size(px(14.))
            .rounded_full()
            .bg(p.primary_soft())
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(8.)).rounded_full().bg(p.accent()))
            .into_any_element(),
        RuleAction::Pass(_) => div()
            .size(px(9.))
            .rounded_full()
            .bg(p.card(false))
            .border_2()
            .border_color(p.muted())
            .into_any_element(),
        RuleAction::Stop(_) => div()
            .size(px(9.))
            .rounded_full()
            .bg(p.muted())
            .into_any_element(),
    }
}

impl Workspace {
    pub(super) fn rule_disabled_by_uuid(&self, uuid: &str) -> Option<bool> {
        self.rules
            .iter()
            .find(|r| rule_uuid(r) == Some(uuid))
            .map(is_rule_disabled)
    }

    pub(super) fn set_rule_disabled(&mut self, uuid: &str, disabled: bool) {
        for r in &mut self.rules {
            if rule_uuid(r) == Some(uuid) {
                r["disabled"] = Value::Bool(disabled);
            }
        }
        if let Some(inspect) = &mut self.rule_browser.inspect
            && rule_uuid(&inspect.rule) == Some(uuid)
        {
            inspect.rule["disabled"] = Value::Bool(disabled);
        }
    }

    pub(super) fn apply_rules(&mut self, mut next: Vec<Value>) {
        if !self.rule_browser.toggling.is_empty() {
            let pending: HashMap<String, bool> = self
                .rules
                .iter()
                .filter_map(|r| {
                    let u = rule_uuid(r)?;
                    self.rule_browser
                        .toggling
                        .contains(u)
                        .then(|| (u.to_owned(), is_rule_disabled(r)))
                })
                .collect();
            for r in &mut next {
                if let Some(u) = rule_uuid(r)
                    && let Some(&disabled) = pending.get(u)
                {
                    r["disabled"] = Value::Bool(disabled);
                }
            }
        }
        self.rules = next;
        if let Some(inspect) = &mut self.rule_browser.inspect {
            let updated = if let Some(u) = rule_uuid(&inspect.rule).map(str::to_owned) {
                self.rules.iter().find(|r| rule_uuid(r) == Some(u.as_str()))
            } else {
                self.rules.get(inspect.index)
            };
            if let Some(rule) = updated {
                inspect.rule = rule.clone();
            }
        }
    }

    pub(super) fn rule_matches_query(&self, idx: usize, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let Some(r) = self.rules.get(idx) else {
            return false;
        };
        if ["payload", "type", "proxy", "condition", "route", "chain"]
            .iter()
            .any(|key| !r[key].is_null() && r[key].to_string().to_lowercase().contains(query))
        {
            return true;
        }
        if rule_set_tags_from_payload(r["payload"].as_str().unwrap_or(""))
            .iter()
            .any(|tag| self.rule_browser.matched.contains(tag))
        {
            return true;
        }
        if lookup_query(query) {
            return eval_rule_for_lookup(r, query, &self.rule_browser.matched);
        }
        false
    }

    fn compute_rule_trace(&self, query: &str) -> Option<RuleTrace> {
        if self.show_providers || !lookup_query(query) {
            return None;
        }
        let mut winner = None;
        let mut passed = HashSet::new();
        let mut shadowed = HashSet::new();
        for (i, r) in self.rules.iter().enumerate() {
            if is_rule_disabled(r) {
                continue;
            }
            if !eval_rule_for_lookup(r, query, &self.rule_browser.matched) {
                continue;
            }
            let action = parse_rule_action(r);
            if matches!(action, RuleAction::Pass(_)) {
                if winner.is_none() {
                    passed.insert(i);
                }
            } else if winner.is_none() {
                winner = Some(i);
            } else {
                shadowed.insert(i);
            }
        }
        Some(RuleTrace {
            query: query.to_owned(),
            winner,
            passed,
            shadowed,
            pending: self.rule_browser.searching,
        })
    }

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
        self.rule_browser.searching = false;
        self.rule_browser.status.clear();
        self.rule_browser.matched.clear();
        if q.is_empty() || tags.is_empty() || (!self.show_providers && !lookup_query(&q)) {
            return;
        }
        self.rule_browser.searching = true;
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
                this.rule_browser.searching = false;
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

    pub(super) fn open_rule_inspect(&mut self, index: usize) {
        let Some(rule) = self.rules.get(index).cloned() else {
            return;
        };
        self.leaving_dialog = None;
        self.rule_browser.inspect = Some(RuleInspect {
            index,
            rule,
            scroll: ScrollHandle::new(),
        });
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
        let show_providers = self.show_providers;
        let trace = self.compute_rule_trace(&query);

        let leading_pre = if !show_providers && query.is_empty() {
            rows.iter()
                .take_while(|&&i| {
                    self.rules
                        .get(i)
                        .is_some_and(|r| parse_rule_action(r).is_preprocessing())
                })
                .count()
        } else {
            0
        };
        let has_pre_collapse = leading_pre > 0 && leading_pre < rows.len();
        let visible_rows: Vec<usize> = if has_pre_collapse && !self.rule_browser.pre_expanded {
            rows[leading_pre..].to_vec()
        } else {
            rows.clone()
        };
        let count = visible_rows.len();

        let pre_summary = if has_pre_collapse {
            let mut seen = Vec::new();
            for &idx in &rows[..leading_pre] {
                if let Some(r) = self.rules.get(idx) {
                    let lbl = parse_rule_action(r).summary_label();
                    if !seen.contains(&lbl) {
                        seen.push(lbl);
                    }
                }
            }
            seen.join(" · ")
        } else {
            String::new()
        };

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

        let mut provider_usage: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        for (idx, r) in self.rules.iter().enumerate() {
            let payload = r["payload"].as_str().unwrap_or("");
            let tags = rule_set_tags_from_payload(payload);
            if tags.is_empty() {
                continue;
            }
            let target = parse_rule_action(r).summary_label();
            let mut seen = HashSet::new();
            for tag in tags {
                if seen.insert(tag.clone()) {
                    provider_usage
                        .entry(tag)
                        .or_default()
                        .push((idx + 1, target.clone()));
                }
            }
        }
        let provider_counts: HashMap<String, u64> = self
            .providers
            .iter()
            .map(|(name, item)| (name.clone(), item["ruleCount"].as_u64().unwrap_or(0)))
            .collect();

        let trace_banner = trace.as_ref().and_then(|tr| {
            let winner_idx = tr.winner?;
            let winner_rule = self.rules.get(winner_idx)?;
            let is_final = winner_rule["condition"].as_str() == Some("最终回退");
            let winner_action = parse_rule_action(winner_rule);
            let winner_label = if is_final {
                "最终回退".to_owned()
            } else {
                format!("第 #{} 条", winner_idx + 1)
            };
            Some(
                div()
                    .w_full()
                    .flex_none()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(14.))
                    .bg(palette.primary_soft())
                    .border_1()
                    .border_color(palette.selection_border())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_xs()
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.accent())
                                    .child("路由试算"),
                            )
                            .child(
                                div()
                                    .font_family("Consolas")
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette.ink())
                                    .truncate()
                                    .child(tr.query.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(palette.muted())
                                    .child("→ 命中"),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.ink())
                                    .child(winner_label),
                            )
                            .when(!tr.passed.is_empty(), |el| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .text_color(palette.muted())
                                        .child(format!("（途经 {} 条预处理）", tr.passed.len())),
                                )
                            })
                            .when(tr.pending, |el| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .text_color(palette.muted())
                                        .child("· 正在核验规则提供商…"),
                                )
                            }),
                    )
                    .child(render_action_capsule(&winner_action, palette)),
            )
        });

        let has_trace_banner = trace_banner.is_some();
        let list_card = glass_card(palette, translucent, tint)
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .p(px(6.))
            .flex()
            .flex_col()
            .when(has_pre_collapse, |el| {
                let expanded = self.rule_browser.pre_expanded;
                el.child(
                    div()
                        .id("rules-pre-collapse")
                        .flex_none()
                        .mb(px(4.))
                        .px(px(12.))
                        .h(crate::theme::text_px(32.))
                        .rounded(px(10.))
                        .bg(palette.hover())
                        .border_1()
                        .border_color(palette.line())
                        .hover(|s| s.bg(palette.fill()))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.rule_browser.pre_expanded = !this.rule_browser.pre_expanded;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_xs()
                                .child(
                                    svg()
                                        .path(if expanded { "chevron-down" } else { "chevron" })
                                        .size(px(13.))
                                        .text_color(palette.muted()),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.ink())
                                        .child(format!("预处理规则 ({leading_pre})")),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_color(palette.muted())
                                        .child(pre_summary.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(crate::theme::text_px(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.accent())
                                .child(if expanded { "收起" } else { "展开" }),
                        ),
                )
            })
            .child(if count == 0 {
                div()
                    .flex_1()
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
                    .flex_1()
                    .min_h_0()
                    .w_full()
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
                                            .filter_map(|idx| visible_rows.get(idx).copied())
                                            .filter(|&i| i < this.providers.len())
                                            .map(|i| {
                                                let (name, item) = &this.providers[i];
                                                let rule_count =
                                                    item["ruleCount"].as_u64().unwrap_or(0);
                                                let updated = super::format_rfc3339_relative(
                                                    item["updatedAt"].as_str().unwrap_or(""),
                                                );
                                                let vehicle =
                                                    item["vehicleType"].as_str().unwrap_or("");
                                                let behavior =
                                                    item["behavior"].as_str().unwrap_or("");
                                                let is_inline =
                                                    vehicle.eq_ignore_ascii_case("inline");
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
                                                    meta_parts.push(format!("更新于 {updated}"));
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
                                                let icon_glyph = if vehicle
                                                    .eq_ignore_ascii_case("remote")
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

                                                let rp_row_h = (36.
                                                    * crate::theme::effective_text_scale(cx)
                                                    + 18.)
                                                    .ceil();
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
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.apply(
                                                                Action::RuleDetail(
                                                                    inspect_name.clone(),
                                                                ),
                                                                cx,
                                                            );
                                                        },
                                                    ))
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
                                                                    .text_size(
                                                                        crate::theme::text_px(13.),
                                                                    )
                                                                    .when(!prefix.is_empty(), |el| {
                                                                        el.child(
                                                                            div()
                                                                                .text_color(
                                                                                    p.muted(),
                                                                                )
                                                                                .child(
                                                                                    prefix
                                                                                        .to_owned(),
                                                                                ),
                                                                        )
                                                                    })
                                                                    .child(
                                                                        div()
                                                                            .font_weight(
                                                                                FontWeight::SEMIBOLD,
                                                                            )
                                                                            .truncate()
                                                                            .child(
                                                                                crate::emoji::text(
                                                                                    base.to_owned(),
                                                                                ),
                                                                            ),
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
                                                                    .text_size(
                                                                        crate::theme::text_px(13.),
                                                                    )
                                                                    .font_weight(
                                                                        FontWeight::SEMIBOLD,
                                                                    )
                                                                    .child(format_count(
                                                                        rule_count,
                                                                    )),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(p.muted())
                                                                    .child("条"),
                                                            ),
                                                    );
                                                if !is_inline {
                                                    line = line.child(
                                                        div()
                                                            .id(("rp-update", i))
                                                            .role(Role::Button)
                                                            .aria_label("更新")
                                                            .tooltip(|window, cx| {
                                                                gpui_component::tooltip::Tooltip::new("更新")
                                                                    .build(window, cx)
                                                            })
                                                            .cursor_pointer()
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
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    cx.stop_propagation();
                                                                    this.apply(
                                                                        Action::Run(
                                                                            Command::UpdateRuleProvider {
                                                                                name: update_name.clone(),
                                                                            },
                                                                        ),
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
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
                    .vertical_scrollbar(&self.page_list_scroll)
                    .into_any_element()
            } else {
                let total_rows = count;
                let any_toggleable = self.rules.iter().any(|r| rule_uuid(r).is_some());
                let counts_map = std::sync::Arc::new(provider_counts);
                let trace_arc = std::sync::Arc::new(trace);
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
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
                                            .filter_map(|pos| {
                                                visible_rows.get(pos).copied().map(|i| (pos, i))
                                            })
                                            .filter(|&(_, i)| i < this.rules.len())
                                            .map(|(pos, i)| {
                                                let r = &this.rules[i];
                                                let clauses = parse_condition_clauses(r);
                                                let action = parse_rule_action(r);
                                                let is_first = pos == 0;
                                                let is_last = pos + 1 == total_rows;
                                                let is_final =
                                                    r["condition"].as_str() == Some("最终回退");
                                                let uuid = rule_uuid(r).map(str::to_owned);
                                                let is_disabled = is_rule_disabled(r);
                                                let is_toggling = uuid.as_deref().is_some_and(|u| {
                                                    this.rule_browser.toggling.contains(u)
                                                });

                                                let is_winner = trace_arc
                                                    .as_ref()
                                                    .as_ref()
                                                    .and_then(|t| t.winner)
                                                    == Some(i);
                                                let is_passed = trace_arc
                                                    .as_ref()
                                                    .as_ref()
                                                    .is_some_and(|t| t.passed.contains(&i));
                                                let is_shadowed = trace_arc
                                                    .as_ref()
                                                    .as_ref()
                                                    .is_some_and(|t| t.shadowed.contains(&i));
                                                let winner_num = trace_arc
                                                    .as_ref()
                                                    .as_ref()
                                                    .and_then(|t| t.winner)
                                                    .map(|w| w + 1);

                                                let rule_row_h = (28.
                                                    * crate::theme::effective_text_scale(cx)
                                                    + 16.)
                                                    .ceil();
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
                                                            .when(!is_first, |el| {
                                                                el.bg(p.ink().opacity(0.18))
                                                            }),
                                                    )
                                                    .child(render_rail_dot(
                                                        &action,
                                                        is_final,
                                                        is_disabled,
                                                        p,
                                                    ))
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .w(px(1.5))
                                                            .when(!is_last, |el| {
                                                                el.bg(p.ink().opacity(0.18))
                                                            }),
                                                    );

                                                let mut chips: Vec<AnyElement> = Vec::new();
                                                let mut pill_idx = 0usize;
                                                for part in clauses {
                                                    match part {
                                                        ClausePart::Op(op) => {
                                                            chips.push(
                                                                div()
                                                                    .flex_none()
                                                                    .px(px(2.))
                                                                    .text_size(
                                                                        crate::theme::text_px(11.),
                                                                    )
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(p.muted())
                                                                    .child(op)
                                                                    .into_any_element(),
                                                            );
                                                        }
                                                        ClausePart::Final => {
                                                            chips.push(
                                                                div()
                                                                    .flex_none()
                                                                    .text_xs()
                                                                    .text_color(p.muted())
                                                                    .child("以上条件均未命中")
                                                                    .into_any_element(),
                                                            );
                                                        }
                                                        ClausePart::AllTraffic => {
                                                            chips.push(
                                                                div()
                                                                    .flex_none()
                                                                    .text_xs()
                                                                    .text_color(p.muted())
                                                                    .child("所有流量")
                                                                    .into_any_element(),
                                                            );
                                                        }
                                                        ClausePart::RuleSets { negated, sets } => {
                                                            if negated {
                                                                chips.push(
                                                                    div()
                                                                        .flex_none()
                                                                        .h(crate::theme::text_px(20.))
                                                                        .px(px(6.))
                                                                        .rounded(px(6.))
                                                                        .bg(p.danger_soft())
                                                                        .flex()
                                                                        .items_center()
                                                                        .text_size(
                                                                            crate::theme::text_px(
                                                                                11.,
                                                                            ),
                                                                        )
                                                                        .font_weight(
                                                                            FontWeight::SEMIBOLD,
                                                                        )
                                                                        .text_color(p.danger())
                                                                        .child("非")
                                                                        .into_any_element(),
                                                                );
                                                            }
                                                            for (short, full) in sets {
                                                                let id_n = pill_idx;
                                                                pill_idx += 1;
                                                                let matched = this
                                                                    .rule_browser
                                                                    .matched
                                                                    .contains(&full);
                                                                let prov_count =
                                                                    counts_map.get(&full).copied();
                                                                let clickable =
                                                                    prov_count.is_some();
                                                                let full_clone = full.clone();
                                                                chips.push(
                                                                    div()
                                                                        .id(SharedString::from(
                                                                            format!(
                                                                                "rset-{i}-{id_n}"
                                                                            ),
                                                                        ))
                                                                        .flex_none()
                                                                        .h(crate::theme::text_px(
                                                                            22.,
                                                                        ))
                                                                        .px(px(9.))
                                                                        .rounded_full()
                                                                        .border_1()
                                                                        .border_color(if matched {
                                                                            p.selection_border()
                                                                        } else {
                                                                            rgba(0x00000000)
                                                                        })
                                                                        .bg(if matched {
                                                                            p.primary_soft()
                                                                        } else {
                                                                            p.fill()
                                                                        })
                                                                        .when(clickable, |el| {
                                                                            el.cursor_pointer()
                                                                                .hover(|s| {
                                                                                    s.bg(
                                                                                        p.fill_strong(),
                                                                                    )
                                                                                })
                                                                                .on_click(cx.listener(
                                                                                    move |this,
                                                                                          _,
                                                                                          _,
                                                                                          cx| {
                                                                                        cx.stop_propagation();
                                                                                        this.apply(
                                                                                            Action::RuleDetail(
                                                                                                full_clone
                                                                                                    .clone(),
                                                                                            ),
                                                                                            cx,
                                                                                        );
                                                                                    },
                                                                                ))
                                                                        })
                                                                        .flex()
                                                                        .items_center()
                                                                        .gap(px(4.))
                                                                        .text_xs()
                                                                        .font_weight(
                                                                            FontWeight::MEDIUM,
                                                                        )
                                                                        .text_color(if matched {
                                                                            p.accent()
                                                                        } else {
                                                                            p.ink()
                                                                        })
                                                                        .child(
                                                                            svg()
                                                                                .path("layers")
                                                                                .size(px(12.))
                                                                                .text_color(
                                                                                    if matched {
                                                                                        p.accent()
                                                                                    } else {
                                                                                        p.muted()
                                                                                    },
                                                                                ),
                                                                        )
                                                                        .child(short)
                                                                        .when_some(
                                                                            prov_count
                                                                                .filter(|&c| c > 0),
                                                                            |el, cnt| {
                                                                                el.child(
                                                                                    div()
                                                                                        .text_size(
                                                                                            crate::theme::text_px(
                                                                                                11.,
                                                                                            ),
                                                                                        )
                                                                                        .text_color(
                                                                                            if matched {
                                                                                                p.accent()
                                                                                                    .opacity(0.8)
                                                                                            } else {
                                                                                                p.muted()
                                                                                            },
                                                                                        )
                                                                                        .child(
                                                                                            format_compact_count(
                                                                                                cnt,
                                                                                            ),
                                                                                        ),
                                                                                )
                                                                            },
                                                                        )
                                                                        .into_any_element(),
                                                                );
                                                            }
                                                        }
                                                        ClausePart::Item {
                                                            negated,
                                                            label,
                                                            values,
                                                            mono,
                                                        } => {
                                                            if negated {
                                                                chips.push(
                                                                    div()
                                                                        .flex_none()
                                                                        .h(crate::theme::text_px(20.))
                                                                        .px(px(6.))
                                                                        .rounded(px(6.))
                                                                        .bg(p.danger_soft())
                                                                        .flex()
                                                                        .items_center()
                                                                        .text_size(
                                                                            crate::theme::text_px(
                                                                                11.,
                                                                            ),
                                                                        )
                                                                        .font_weight(
                                                                            FontWeight::SEMIBOLD,
                                                                        )
                                                                        .text_color(p.danger())
                                                                        .child("非")
                                                                        .into_any_element(),
                                                                );
                                                            }
                                                            let shown_vals: Vec<&str> = values
                                                                .iter()
                                                                .take(2)
                                                                .map(String::as_str)
                                                                .collect();
                                                            let hidden =
                                                                values.len().saturating_sub(2);
                                                            let mut chip = div()
                                                                .flex_none()
                                                                .max_w(px(340.))
                                                                .h(crate::theme::text_px(22.))
                                                                .px(px(8.))
                                                                .rounded(px(7.))
                                                                .bg(p.hover())
                                                                .border_1()
                                                                .border_color(p.line())
                                                                .flex()
                                                                .items_center()
                                                                .gap(px(5.))
                                                                .text_xs();
                                                            if let Some(lbl) = label {
                                                                chip = chip.child(
                                                                    div()
                                                                        .flex_none()
                                                                        .text_color(p.muted())
                                                                        .child(lbl),
                                                                );
                                                            }
                                                            let mut val_el = div()
                                                                .min_w_0()
                                                                .truncate()
                                                                .text_color(p.ink());
                                                            if mono {
                                                                val_el =
                                                                    val_el.font_family("Consolas");
                                                            }
                                                            chip = chip
                                                                .child(val_el.child(shown_vals.join(", ")));
                                                            if hidden > 0 {
                                                                chip = chip.child(
                                                                    div()
                                                                        .flex_none()
                                                                        .px(px(4.))
                                                                        .rounded(px(4.))
                                                                        .bg(p.fill())
                                                                        .text_size(
                                                                            crate::theme::text_px(
                                                                                10.,
                                                                            ),
                                                                        )
                                                                        .text_color(p.muted())
                                                                        .child(format!("+{hidden}")),
                                                                );
                                                            }
                                                            chips.push(chip.into_any_element());
                                                        }
                                                    }
                                                }

                                                const MAX_CHIPS: usize = 6;
                                                let overflow = chips.len().saturating_sub(MAX_CHIPS);
                                                if overflow > 0 {
                                                    chips.truncate(MAX_CHIPS);
                                                    chips.push(
                                                        div()
                                                            .flex_none()
                                                            .h(crate::theme::text_px(20.))
                                                            .px(px(6.))
                                                            .rounded(px(6.))
                                                            .bg(p.fill())
                                                            .flex()
                                                            .items_center()
                                                            .text_size(crate::theme::text_px(11.))
                                                            .text_color(p.muted())
                                                            .child(format!("+{overflow} 项"))
                                                            .into_any_element(),
                                                    );
                                                }

                                                let cond_row = div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(5.))
                                                    .children(chips);

                                                let trace_badge = if is_winner {
                                                    Some(
                                                        div()
                                                            .flex_none()
                                                            .h(crate::theme::text_px(20.))
                                                            .px(px(7.))
                                                            .rounded_full()
                                                            .bg(p.success_soft())
                                                            .flex()
                                                            .items_center()
                                                            .text_size(crate::theme::text_px(11.))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(p.success_ink())
                                                            .child("生效命中")
                                                            .into_any_element(),
                                                    )
                                                } else if is_passed {
                                                    Some(
                                                        div()
                                                            .flex_none()
                                                            .h(crate::theme::text_px(20.))
                                                            .px(px(7.))
                                                            .rounded_full()
                                                            .bg(p.fill())
                                                            .flex()
                                                            .items_center()
                                                            .text_size(crate::theme::text_px(11.))
                                                            .text_color(p.muted())
                                                            .child("路过")
                                                            .into_any_element(),
                                                    )
                                                } else if is_shadowed {
                                                    let lbl = winner_num
                                                        .map(|w| format!("已被 #{w} 截断"))
                                                        .unwrap_or_else(|| "已截断".into());
                                                    Some(
                                                        div()
                                                            .flex_none()
                                                            .h(crate::theme::text_px(20.))
                                                            .px(px(7.))
                                                            .rounded_full()
                                                            .bg(p.fill())
                                                            .flex()
                                                            .items_center()
                                                            .text_size(crate::theme::text_px(11.))
                                                            .text_color(p.muted())
                                                            .child(lbl)
                                                            .into_any_element(),
                                                    )
                                                } else {
                                                    None
                                                };

                                                div()
                                                    .id(("rule-entry", i))
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.apply(Action::InspectRule(i), cx);
                                                        },
                                                    ))
                                                    .w_full()
                                                    .h(px(rule_row_h))
                                                    .pl(px(10.))
                                                    .pr(px(12.))
                                                    .rounded(px(12.))
                                                    .border_1()
                                                    .border_color(if is_winner {
                                                        p.selection_border()
                                                    } else {
                                                        rgba(0x00000000)
                                                    })
                                                    .when(is_winner, |el| el.bg(p.primary_soft()))
                                                    .when(!is_winner && is_final, |el| {
                                                        el.bg(p.fill())
                                                    })
                                                    .when(!is_disabled && is_shadowed, |el| {
                                                        el.opacity(0.58)
                                                    })
                                                    .hover(|s| {
                                                        s.bg(if is_winner {
                                                            p.primary_soft()
                                                        } else {
                                                            p.hover()
                                                        })
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(10.))
                                                    .child(rail)
                                                    .child(
                                                        div()
                                                            .w(px(28.))
                                                            .flex_none()
                                                            .flex()
                                                            .justify_end()
                                                            .text_size(crate::theme::text_px(11.))
                                                            .text_color(p.muted())
                                                            .when(is_disabled, |el| el.opacity(0.45))
                                                            .child(if is_final {
                                                                "回退".to_owned()
                                                            } else {
                                                                format!("{:02}", i + 1)
                                                            }),
                                                    )
                                                    .child(
                                                        cond_row
                                                            .when(is_disabled, |el| el.opacity(0.45)),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_none()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(6.))
                                                            .when(is_disabled, |el| el.opacity(0.45))
                                                            .children(trace_badge)
                                                            .child(render_action_capsule(
                                                                &action, p,
                                                            )),
                                                    )
                                                    .when(any_toggleable, |el| {
                                                        el.child(match uuid {
                                                            Some(toggle_uuid) => render_rule_switch(
                                                                ("rule-toggle", i),
                                                                !is_disabled,
                                                                is_toggling,
                                                                toggle_uuid,
                                                                p,
                                                                cx,
                                                            ),
                                                            None => div()
                                                                .w(px(34.))
                                                                .flex_none()
                                                                .into_any_element(),
                                                        })
                                                    })
                                            })
                                            .collect()
                                    }),
                                )
                                .track_scroll(&self.page_list_scroll)
                                .size_full(),
                            ),
                    )
                    .child(self.smooth_scroll_capture(scrolling::ScrollTarget::PageList, cx))
                    .vertical_scrollbar(&self.page_list_scroll)
                    .into_any_element()
            });

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(header)
            .child(div().w_full().children(self.filter.clone()))
            .children(trace_banner)
            .when(!self.rule_browser.status.is_empty() && !has_trace_banner, |el| {
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
        if let Some(el) = self.rule_provider_overlay(window, cx) {
            return Some(el);
        }
        self.rule_inspect_overlay(window, cx)
    }

    fn rule_inspect_overlay(&self, window: &Window, cx: &Context<Self>) -> Option<Div> {
        let (inspect, leaving) = match (&self.rule_browser.inspect, &self.leaving_dialog) {
            (Some(inspect), _) => (inspect, false),
            (None, Some((_, dialog::LeavingDialog::RuleInspect(inspect)))) => (inspect, true),
            _ => return None,
        };
        let tint = self.glass_tint;
        let palette = crate::theme::Palette::get(cx);
        let r = &inspect.rule;
        let is_final = r["condition"].as_str() == Some("最终回退");
        let inspect_uuid = rule_uuid(r).map(str::to_owned);
        let inspect_disabled = is_rule_disabled(r);
        let inspect_toggling = inspect_uuid
            .as_deref()
            .is_some_and(|u| self.rule_browser.toggling.contains(u));
        let clauses = parse_condition_clauses(r);
        let action = parse_rule_action(r);
        let payload = r["payload"].as_str().unwrap_or("").trim().to_owned();
        let chain: Vec<String> = r["chain"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        let mut related_sets: Vec<String> = Vec::new();
        for tag in rule_set_tags_from_payload(&payload) {
            if !related_sets.contains(&tag) {
                related_sets.push(tag);
            }
        }

        let mut clause_rows: Vec<AnyElement> = Vec::new();
        for part in clauses {
            match part {
                ClausePart::Op(op) => {
                    clause_rows.push(
                        div()
                            .pl(px(4.))
                            .text_size(crate::theme::text_px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.accent())
                            .child(if op == "且" { "AND · 且" } else { "OR · 或" })
                            .into_any_element(),
                    );
                }
                ClausePart::Final => {
                    clause_rows.push(
                        div()
                            .text_xs()
                            .text_color(palette.ink())
                            .child("以上所有规则均未命中时进入此回退规则")
                            .into_any_element(),
                    );
                }
                ClausePart::AllTraffic => {
                    clause_rows.push(
                        div()
                            .text_xs()
                            .text_color(palette.ink())
                            .child("匹配所有流量")
                            .into_any_element(),
                    );
                }
                ClausePart::RuleSets { negated, sets } => {
                    let mut row = div().flex().flex_wrap().items_center().gap(px(6.));
                    if negated {
                        row = row.child(
                            div()
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(px(6.))
                                .bg(palette.danger_soft())
                                .text_size(crate::theme::text_px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette.danger())
                                .child("不满足"),
                        );
                    }
                    row = row.child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.muted())
                            .child("规则集"),
                    );
                    for (_, full) in sets {
                        row = row.child(
                            div()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded(px(6.))
                                .bg(palette.card(false))
                                .font_family("Consolas")
                                .text_xs()
                                .text_color(palette.ink())
                                .child(full),
                        );
                    }
                    clause_rows.push(row.into_any_element());
                }
                ClausePart::Item {
                    negated,
                    label,
                    values,
                    mono,
                } => {
                    let mut row = div().flex().flex_wrap().items_center().gap(px(6.));
                    if negated {
                        row = row.child(
                            div()
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(px(6.))
                                .bg(palette.danger_soft())
                                .text_size(crate::theme::text_px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette.danger())
                                .child("不满足"),
                        );
                    }
                    if let Some(lbl) = label {
                        row = row.child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.muted())
                                .child(lbl),
                        );
                    }
                    for val in values {
                        let mut chip = div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(6.))
                            .bg(palette.card(false))
                            .text_xs()
                            .text_color(palette.ink());
                        if mono {
                            chip = chip.font_family("Consolas");
                        }
                        row = row.child(chip.child(val));
                    }
                    clause_rows.push(row.into_any_element());
                }
            }
        }

        let copy_payload = if payload.is_empty() {
            r["condition"].as_str().unwrap_or("final").to_owned()
        } else {
            payload.clone()
        };

        let mut body = div()
            .id("rule-inspect-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&inspect.scroll)
            .flex()
            .flex_col()
            .gap_3();

        if let Some(toggle_uuid) = inspect_uuid {
            let toggle = pill_switch("rule-inspect-toggle", !inspect_disabled, palette)
                .role(Role::Button)
                .aria_label(if inspect_disabled {
                    "重新启用此规则"
                } else {
                    "临时禁用此规则"
                })
                .when(inspect_toggling, |el| el.opacity(0.55))
                .when(!inspect_toggling, |el| {
                    el.on_click(cx.listener(move |this, _, _, cx| {
                        this.apply(
                            Action::Run(Command::ToggleRule {
                                uuid: toggle_uuid.clone(),
                            }),
                            cx,
                        );
                    }))
                });
            body = body.child(
                div()
                    .rounded(px(12.))
                    .bg(palette.fill())
                    .px_3()
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.ink())
                                    .child("启用此规则"),
                            )
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(11.))
                                    .text_color(palette.muted())
                                    .child(if inspect_disabled {
                                        "已临时禁用，重启核心或重载配置后恢复"
                                    } else {
                                        "关闭后将临时跳过此规则，不修改配置文件"
                                    }),
                            ),
                    )
                    .child(toggle),
            );
        }

        body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.muted())
                            .child("匹配条件"),
                    )
                    .child(
                        div()
                            .rounded(px(12.))
                            .bg(palette.fill())
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .children(clause_rows),
                    ),
            );

        if !related_sets.is_empty() {
            let mut sets_box = div().flex().flex_wrap().gap(px(6.));
            for (idx, tag) in related_sets.into_iter().enumerate() {
                let prov = self.providers.iter().find(|(n, _)| n == &tag);
                let count_text = prov
                    .map(|(_, v)| format!("{} 条", format_count(v["ruleCount"].as_u64().unwrap_or(0))))
                    .unwrap_or_else(|| "未加载".into());
                let clickable = prov.is_some();
                let open_tag = tag.clone();
                sets_box = sets_box.child(
                    div()
                        .id(("inspect-rset", idx))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(10.))
                        .bg(palette.fill())
                        .when(clickable, |el| {
                            el.cursor_pointer()
                                .hover(|s| s.bg(palette.fill_strong()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.apply(Action::RuleDetail(open_tag.clone()), cx);
                                }))
                        })
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_xs()
                        .child(svg().path("layers").size(px(13.)).text_color(palette.muted()))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.ink())
                                .child(tag),
                        )
                        .child(div().text_color(palette.muted()).child(count_text))
                        .when(clickable, |el| {
                            el.child(
                                svg()
                                    .path("chevron")
                                    .size(px(12.))
                                    .text_color(palette.muted()),
                            )
                        }),
                );
            }
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.muted())
                            .child("关联规则提供商"),
                    )
                    .child(sets_box),
            );
        }

        let chain_el = if chain.len() > 1 {
            let mut row = div().flex().flex_wrap().items_center().gap(px(6.));
            for (ci, node) in chain.into_iter().enumerate() {
                if ci > 0 {
                    row = row.child(
                        svg()
                            .path("arrow-right")
                            .size(px(12.))
                            .text_color(palette.muted()),
                    );
                }
                row = row.child(
                    div()
                        .h(crate::theme::text_px(24.))
                        .px(px(10.))
                        .rounded_full()
                        .bg(if ci == 0 {
                            palette.primary_soft()
                        } else {
                            palette.fill()
                        })
                        .flex()
                        .items_center()
                        .text_xs()
                        .font_weight(if ci == 0 {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if ci == 0 {
                            palette.accent()
                        } else {
                            palette.ink()
                        })
                        .child(crate::emoji::text(node)),
                );
            }
            row.into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .child(render_action_capsule(&action, palette))
                .into_any_element()
        };

        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.muted())
                        .child("执行动作与代理链"),
                )
                .child(chain_el),
        );

        let copy_str = copy_payload.clone();
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.muted())
                                .child("原始规则"),
                        )
                        .child(
                            div()
                                .id("rule-inspect-copy")
                                .cursor_pointer()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded(px(6.))
                                .hover(|s| s.bg(palette.fill()))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.accent())
                                .child("复制")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_str.clone(),
                                    ));
                                    this.message("已复制原始规则".into(), false, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .rounded(px(10.))
                        .bg(palette.fill())
                        .px(px(10.))
                        .py(px(8.))
                        .font_family("Consolas")
                        .text_xs()
                        .text_color(palette.muted())
                        .child(copy_payload),
                ),
        );

        let max_h = (f32::from(window.viewport_size().height) * 0.8).max(260.);
        let card = popover_card(palette, tint)
            .id("rule-inspect-card")
            .w(px(580.))
            .max_w_full()
            .max_h(px(max_h))
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .h(crate::theme::text_px(22.))
                                    .px(px(8.))
                                    .rounded(px(6.))
                                    .bg(palette.fill())
                                    .flex()
                                    .items_center()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.muted())
                                    .child(if is_final {
                                        "最终回退".to_owned()
                                    } else {
                                        format!("#{:02}", inspect.index + 1)
                                    }),
                            )
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(palette.ink())
                                    .child("规则详情"),
                            )
                            .when(inspect_disabled, |el| {
                                el.child(
                                    div()
                                        .h(crate::theme::text_px(20.))
                                        .px(px(7.))
                                        .rounded_full()
                                        .bg(palette.fill())
                                        .flex()
                                        .items_center()
                                        .text_size(crate::theme::text_px(11.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.muted())
                                        .child("已禁用"),
                                )
                            }),
                    )
                    .child(dialog::keyboard_action(
                        div()
                            .id("rule-inspect-close")
                            .cursor_pointer()
                            .p_2()
                            .child(
                                svg()
                                    .path("close-detail")
                                    .size(px(12.))
                                    .text_color(palette.muted()),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply(Action::CloseRuleInspect, cx);
                            })),
                        Action::CloseRuleInspect,
                        cx,
                    )),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(body)
                    .vertical_scrollbar(&inspect.scroll),
            );

        Some(div().absolute().inset_0().size_full().child(dialog::surface(
            "rule-inspect-pop",
            self.page_dialog("rules-inspect", Action::CloseRuleInspect, cx),
            card,
            580.,
            leaving,
            cx,
        )))
    }

    fn rule_provider_overlay(&self, window: &Window, cx: &Context<Self>) -> Option<Div> {
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
            .map(|(i, r)| (i + 1, parse_rule_action(r).summary_label()))
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
                        let updated = super::format_rfc3339_relative(
                            p["updatedAt"].as_str().unwrap_or(""),
                        );
                        [
                            vehicle_label(vehicle),
                            format_label(p["behavior"].as_str().unwrap_or("")),
                            format!("{} 条", p["ruleCount"].as_u64().unwrap_or(0)),
                            if updated.is_empty() {
                                String::new()
                            } else {
                                format!("更新于 {updated}")
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
                            .child(dialog::keyboard_action(
                                div()
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
                                    })),
                                Action::CloseRuleDetail,
                                cx,
                            )),
                    )
                    .child(div().text_xs().text_color(palette.muted()).child(meta))
                    .when_some(detail_uses, |e, uses| {
                        e.child(div().text_xs().child(crate::emoji::text(uses)))
                    })
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
                    card = card.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(palette.muted())
                            .child(d.status.clone()),
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
                                                    (
                                                        None,
                                                        Some((_, dialog::LeavingDialog::Rule(d))),
                                                    ) => d,
                                                    _ => return Vec::new(),
                                                };
                                                range
                                                    .filter_map(|pos| {
                                                        let item =
                                                            d.entries.get(*d.shown.get(pos)?)?;
                                                        let group_break = pos > 0
                                                            && d.entries[d.shown[pos - 1]].rule_type
                                                                != item.rule_type;
                                                        let (pill_bg, pill_fg) =
                                                            rule_type_badge_colors(
                                                                &item.rule_type,
                                                                p,
                                                            );
                                                        let label = field_label(&item.rule_type)
                                                            .unwrap_or(&item.rule_type)
                                                            .to_owned();
                                                        let copy_value = item.value.clone();
                                                        Some(
                                                            div()
                                                                .w_full()
                                                                .h(crate::theme::text_px(30.))
                                                                .when(group_break, |el| {
                                                                    el.border_t_1()
                                                                        .border_color(p.line())
                                                                })
                                                                .child(
                                                                    div()
                                                                        .id((
                                                                            "rule-detail-row",
                                                                            pos,
                                                                        ))
                                                                        .size_full()
                                                                        .px(px(8.))
                                                                        .rounded(px(7.))
                                                                        .cursor_pointer()
                                                                        .hover(|s| s.bg(p.hover()))
                                                                        .flex()
                                                                        .items_center()
                                                                        .gap(px(10.))
                                                                        .on_click(cx.listener(
                                                                            move |this,
                                                                                  _,
                                                                                  _,
                                                                                  cx| {
                                                                                cx.write_to_clipboard(
                                                                                    ClipboardItem::new_string(
                                                                                        copy_value
                                                                                            .clone(),
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
                                                                                .w(crate::theme::text_px(
                                                                                    72.,
                                                                                ))
                                                                                .h(crate::theme::text_px(
                                                                                    20.,
                                                                                ))
                                                                                .flex_none()
                                                                                .px(px(6.))
                                                                                .rounded(px(6.))
                                                                                .bg(pill_bg)
                                                                                .flex()
                                                                                .items_center()
                                                                                .justify_center()
                                                                                .text_size(
                                                                                    crate::theme::text_px(
                                                                                        11.,
                                                                                    ),
                                                                                )
                                                                                .font_weight(
                                                                                    FontWeight::MEDIUM,
                                                                                )
                                                                                .text_color(pill_fg)
                                                                                .child(
                                                                                    div()
                                                                                        .truncate()
                                                                                        .child(
                                                                                            label,
                                                                                        ),
                                                                                ),
                                                                        )
                                                                        .child(
                                                                            div()
                                                                                .flex_1()
                                                                                .min_w_0()
                                                                                .truncate()
                                                                                .font_family(
                                                                                    "Consolas",
                                                                                )
                                                                                .text_xs()
                                                                                .text_color(p.ink())
                                                                                .child(
                                                                                    item.value
                                                                                        .clone(),
                                                                                ),
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
                            .child(
                                self.smooth_scroll_capture(scrolling::ScrollTarget::RuleDetail, cx),
                            )
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
                el.child(dialog::surface(
                    "rule-details-pop",
                    self.page_dialog("rules-detail", Action::CloseRuleDetail, cx),
                    card,
                    672.,
                    leaving,
                    cx,
                ))
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClausePart, eval_rule_for_lookup, format_compact_count, is_rule_disabled,
        parse_condition_clauses, rule_uuid,
    };
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn parses_compound_clauses_and_negation() {
        let rule = json!({
            "payload": "!(rule_set=geosite-cn) && port=[80 443] && network=tcp"
        });
        let clauses = parse_condition_clauses(&rule);
        assert_eq!(
            clauses,
            vec![
                ClausePart::RuleSets {
                    negated: true,
                    sets: vec![("geosite-cn".into(), "geosite-cn".into())],
                },
                ClausePart::Op("且"),
                ClausePart::Item {
                    negated: false,
                    label: Some("端口"),
                    values: vec!["80".into(), "443".into()],
                    mono: true,
                },
                ClausePart::Op("且"),
                ClausePart::Item {
                    negated: false,
                    label: Some("网络"),
                    values: vec!["TCP".into()],
                    mono: false,
                },
            ]
        );
    }

    #[test]
    fn evaluates_domain_ip_and_rule_set_lookup() {
        let mut matched = HashSet::new();
        matched.insert("geosite-cn".to_owned());

        let suffix_rule = json!({ "payload": "domain_suffix=[openai.com claude.ai]" });
        assert!(eval_rule_for_lookup(&suffix_rule, "api.openai.com", &matched));
        assert!(!eval_rule_for_lookup(&suffix_rule, "evilopenai.com", &matched));

        let cidr_rule = json!({ "payload": "ip_cidr=[10.0.0.0/8 192.168.1.0/24]" });
        assert!(eval_rule_for_lookup(&cidr_rule, "192.168.1.42", &matched));
        assert!(!eval_rule_for_lookup(&cidr_rule, "192.168.2.1", &matched));

        let neg_rule = json!({ "payload": "!(rule_set=geosite-cn)" });
        assert!(!eval_rule_for_lookup(&neg_rule, "www.baidu.com", &matched));
        assert!(eval_rule_for_lookup(&neg_rule, "www.google.com", &HashSet::new()));
    }

    #[test]
    fn formats_compact_provider_counts() {
        assert_eq!(format_compact_count(248), "248");
        assert_eq!(format_compact_count(1420), "1.4k");
        assert_eq!(format_compact_count(38420), "38.4k");
        assert_eq!(format_compact_count(12000), "12k");
    }

    #[test]
    fn detects_rule_uuid_and_disabled_state() {
        let standard = json!({ "type": "default", "payload": "domain=example.com", "proxy": "direct" });
        assert_eq!(rule_uuid(&standard), None);
        assert!(!is_rule_disabled(&standard));

        let blank_uuid = json!({ "uuid": "   ", "disabled": false });
        assert_eq!(rule_uuid(&blank_uuid), None);
        assert!(!is_rule_disabled(&blank_uuid));

        let ref1nd_enabled = json!({ "uuid": "550e8400-e29b-41d4-a716-446655440000" });
        assert_eq!(rule_uuid(&ref1nd_enabled), Some("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!is_rule_disabled(&ref1nd_enabled));

        let ref1nd_disabled = json!({ "uuid": "550e8400-e29b-41d4-a716-446655440000", "disabled": true });
        assert_eq!(rule_uuid(&ref1nd_disabled), Some("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_rule_disabled(&ref1nd_disabled));
    }
}
