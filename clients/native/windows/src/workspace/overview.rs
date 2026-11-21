use super::*;
use singboard_stream::Traffic;

fn node_label(text: impl Into<SharedString>) -> StyledText {
    crate::emoji::text(text)
}

fn mode_phrase(mode: &str) -> String {
    match mode.to_lowercase().as_str() {
        "rule" => "规则模式".into(),
        "global" => "全局模式".into(),
        "direct" => "直连模式".into(),
        other if !other.is_empty() => format!("模式 {other}"),
        _ => String::new(),
    }
}

fn exit_heading(mode: &str) -> &'static str {
    match mode.to_lowercase().as_str() {
        "global" | "direct" => "当前出口",
        _ => "默认代理",
    }
}

fn latency_dot(ms: Option<u64>, palette: crate::theme::Palette) -> Rgba {
    match ms {
        None => palette.muted(),
        Some(0) => palette.danger_fill(),
        Some(v) if v < 300 => palette.success_dot(),
        Some(v) if v < 800 => palette.warning_dot(),
        Some(_) => palette.danger_fill(),
    }
}

fn format_latency(ms: u64) -> String {
    if ms == 0 {
        "失败".into()
    } else {
        format!("{ms}ms")
    }
}

fn split_route_fallback(route: &str) -> (String, String, Option<u64>) {
    let (chain_part, ms) = match route.rsplit_once(" · ") {
        Some((left, right)) => {
            let parsed = right
                .trim()
                .strip_suffix("ms")
                .and_then(|s| s.trim().parse::<u64>().ok());
            if parsed.is_some() {
                (left, parsed)
            } else {
                (route, None)
            }
        }
        None => (route, None),
    };
    let parts: Vec<&str> = chain_part
        .split(" → ")
        .flat_map(|s| s.split(" › "))
        .flat_map(|s| s.split(" > "))
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("GLOBAL"))
        .collect();
    match parts.as_slice() {
        [] => (String::new(), String::new(), ms),
        [single] => ((*single).to_owned(), String::new(), ms),
        [head @ .., tail] => ((*tail).to_owned(), head.join(" › "), ms),
    }
}

fn quantity_parts(value: u64, units: &[&'static str], precision: usize) -> (String, &'static str) {
    let mut n = value as f64;
    let mut index = 0;
    while n >= 1024. && index + 1 < units.len() {
        n /= 1024.;
        index += 1;
    }
    let formatted = format!("{n:.precision$}");
    (formatted.trim_end_matches('0').trim_end_matches('.').to_owned(), units[index])
}

fn speed_parts(value: u64) -> (String, &'static str) {
    quantity_parts(value, &["B/s", "KB/s", "MB/s", "GB/s"], 1)
}

fn bytes_parts(value: u64) -> (String, &'static str) {
    quantity_parts(value, &["B", "KB", "MB", "GB", "TB"], 2)
}

fn total_bytes(value: u64) -> String {
    let (n, unit) = bytes_parts(value);
    format!("{n} {unit}")
}

fn history_x(index: usize, count: usize) -> f32 {
    (OVERVIEW_SAMPLES - count + index) as f32 / (OVERVIEW_SAMPLES - 1) as f32
}

fn spark_curve(path: &mut PathBuilder, points: &[Point<Pixels>]) {
    path.move_to(points[0]);
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let mid = (a.x + b.x) * 0.5;
        path.cubic_bezier_to(b, point(mid, a.y), point(mid, b.y));
    }
}

fn spark_area(points: &[Point<Pixels>], base: Pixels) -> PathBuilder {
    let mut path = PathBuilder::fill();
    spark_curve(&mut path, points);
    path.line_to(point(points[points.len() - 1].x, base));
    path.line_to(point(points[0].x, base));
    path.close();
    path
}

fn traffic_wave(
    samples: Vec<Traffic>,
    live: bool,
    narrow: bool,
    palette: crate::theme::Palette,
) -> Div {
    let height = if narrow { 92. } else { 104. };
    let drawn = live && samples.len() >= 2;
    let down_peak = samples.iter().map(|s| s.down).max().unwrap_or(0).max(32768) as f32;
    let up_peak = samples.iter().map(|s| s.up).max().unwrap_or(0).max(32768) as f32;
    let head_top = drawn.then(|| {
        let last_down = samples.last().map(|s| s.down).unwrap_or(0) as f32;
        let axis = 0.62_f32;
        let extent = 0.58_f32;
        let y_ratio = (axis - extent * (last_down / down_peak).clamp(0., 1.)).clamp(0.03, 0.97);
        y_ratio * height
    });

    div()
        .relative()
        .w_full()
        .h(px(height))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let axis_y = bounds.origin.y + bounds.size.height * 0.62;
                    if !drawn {
                        let mut x = bounds.origin.x;
                        let end_x = bounds.origin.x + bounds.size.width;
                        while x < end_x {
                            let seg_end = (x + px(3.)).min(end_x);
                            let mut dash = PathBuilder::stroke(px(1.25));
                            dash.move_to(point(x, axis_y));
                            dash.line_to(point(seg_end, axis_y));
                            if let Ok(path) = dash.build() {
                                let faded = f32::from(x - bounds.origin.x) / (f32::from(bounds.size.width) * 0.12).max(1.);
                                window.paint_path(path, palette.chart_ink((46. * faded.min(1.)) as u8));
                            }
                            x += px(8.);
                        }
                        return;
                    }

                    let count = samples.len();
                    let down_extent = bounds.size.height * 0.58;
                    let up_extent = bounds.size.height * 0.35;

                    let mut down_pts = Vec::with_capacity(count);
                    let mut up_pts = Vec::with_capacity(count);
                    for (i, sample) in samples.iter().enumerate() {
                        let t = history_x(i, count);
                        let x = bounds.origin.x + bounds.size.width * t;
                        let dy = axis_y - down_extent * (sample.down as f32 / down_peak).clamp(0., 1.);
                        let uy = axis_y + up_extent * (sample.up as f32 / up_peak).clamp(0., 1.);
                        down_pts.push(point(x, dy));
                        up_pts.push(point(x, uy));
                    }

                    let faded = |alpha: u8, start: f32| {
                        let opacity = (start / 0.12).clamp(0., 1.);
                        let stop = ((0.12 - start) / (1. - start).max(0.001)).max(0.001);
                        linear_gradient(90.,
                            linear_color_stop(palette.chart_ink((alpha as f32 * opacity).round() as u8), 0.),
                            linear_color_stop(palette.chart_ink(alpha), stop),
                        )
                    };
                    if let Ok(path) = spark_area(&down_pts, axis_y).build() {
                        window.paint_path(path, linear_gradient(180.,
                            linear_color_stop(palette.chart_ink(0x29), 0.),
                            linear_color_stop(palette.chart_ink(0x08), 1.),
                        ));
                    }

                    if let Ok(path) = spark_area(&up_pts, axis_y).build() {
                        window.paint_path(path, linear_gradient(180.,
                            linear_color_stop(palette.chart_ink(0x05), 0.),
                            linear_color_stop(palette.chart_ink(0x14), 1.),
                        ));
                    }

                    let mut axis = PathBuilder::stroke(px(1.));
                    axis.move_to(point(bounds.origin.x, axis_y));
                    axis.line_to(point(bounds.origin.x + bounds.size.width, axis_y));
                    if let Ok(path) = axis.build() {
                        window.paint_path(path, faded(0x1f, 0.));
                    }

                    let mut down_line = PathBuilder::stroke(px(1.5));
                    spark_curve(&mut down_line, &down_pts);
                    if let Ok(path) = down_line.build() {
                        window.paint_path(path, faded(0x8c, history_x(0, count)));
                    }

                    let mut up_line = PathBuilder::stroke(px(1.25));
                    spark_curve(&mut up_line, &up_pts);
                    if let Ok(path) = up_line.build() {
                        window.paint_path(path, faded(0x4d, history_x(0, count)));
                    }
                },
            )
            .size_full(),
        )
        .when_some(head_top, |el, top| {
            el.child(
                div()
                    .absolute()
                    .right(px(-3.))
                    .top(px(top - 3.))
                    .size(px(6.))
                    .rounded_full()
                    .bg(palette.chart_ink(0xbf)),
            )
        })
}

fn mini_line_ink(
    samples: Vec<u64>,
    floor: u64,
    live: bool,
    palette: crate::theme::Palette,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if !live || samples.len() < 2 {
                return;
            }
            let peak = samples.iter().copied().max().unwrap_or(0).max(floor) as f32;
            let count = samples.len();
            let mut pts = Vec::with_capacity(samples.len());
            for (i, v) in samples.into_iter().enumerate() {
                let x = bounds.origin.x + bounds.size.width * history_x(i, count);
                let y = bounds.origin.y + bounds.size.height
                    - (bounds.size.height - px(4.)) * (v as f32 / peak);
                pts.push(point(x, y));
            }
            if let Ok(built) = spark_area(&pts, bounds.origin.y + bounds.size.height).build() {
                window.paint_path(built, palette.chart_ink(0x0d));
            }
            let mut stroke = PathBuilder::stroke(px(1.25));
            spark_curve(&mut stroke, &pts);
            if let Ok(built) = stroke.build() {
                window.paint_path(built, palette.chart_ink(0x61));
            }
        },
    )
    .w_full()
    .h(px(30.))
}

struct TopologyItem {
    name: String,
    count: u64,
    merged: u64,
}

fn topology_ranking(nodes: &[Value], layer: u64) -> Vec<TopologyItem> {
    let mut rows: Vec<TopologyItem> = nodes
        .iter()
        .filter(|n| n["layer"].as_u64() == Some(layer))
        .map(|n| TopologyItem {
            name: n["name"].as_str().unwrap_or("").to_owned(),
            count: n["count"].as_u64().unwrap_or(0),
            merged: n["merged"].as_u64().unwrap_or(0),
        })
        .collect();
    rows.sort_by(|a, b| {
        (a.merged > 0)
            .cmp(&(b.merged > 0))
            .then(b.count.cmp(&a.count))
            .then(a.name.cmp(&b.name))
    });
    rows
}

fn topology_summary(nodes: &[Value], conn_count: u64) -> String {
    if conn_count == 0 {
        return "暂无连接".into();
    }
    let exits = topology_ranking(nodes, 3);
    let exit_count: u64 = exits.iter().map(|n| n.merged.max(1)).sum();
    let lead = exits
        .first()
        .map(|top| format!("，最多的是 {}（{} 条）", top.name, top.count))
        .unwrap_or_default();
    format!("{conn_count} 条连接经 {exit_count} 个出口{lead}")
}

fn topology_height(nodes: &[Value]) -> f32 {
    let mut counts = [0usize; 4];
    for node in nodes {
        if let Some(layer) = node["layer"].as_u64()
            && (layer as usize) < 4
        {
            counts[layer as usize] += 1;
        }
    }
    let tallest = counts.into_iter().max().unwrap_or(0) as f32;
    (tallest * 28. + 24.).clamp(160., 320.)
}

fn animate_flow(nodes: &mut [Value], edges: &mut [Value], old: &Value, t: f64) {
    let (Some(old_nodes), Some(old_edges)) = (old["nodes"].as_array(), old["edges"].as_array()) else {
        return;
    };
    let node_key = |node: &Value| (node["layer"].as_u64().unwrap_or(0), node["name"].as_str().unwrap_or("").to_owned());
    let positions: std::collections::HashMap<_, _> = old_nodes.iter().map(|node| (node_key(node), node)).collect();
    let link_key = |edge: &Value, rows: &[Value]| {
        Some((node_key(rows.get(edge[5].as_u64()? as usize)?), node_key(rows.get(edge[6].as_u64()? as usize)?)))
    };
    let links: std::collections::HashMap<_, _> = old_edges.iter().filter_map(|e| Some((link_key(e, old_nodes)?, e))).collect();
    let mix = |a: &Value, b: &Value| {
        let next = b.as_f64().unwrap_or(0.);
        let prior = a.as_f64().unwrap_or(next);
        Value::from(prior + (next - prior) * t)
    };
    for edge in edges {
        if let Some(prior) = link_key(edge, nodes).and_then(|id| links.get(&id)) {
            for i in [1, 3, 4] {
                edge[i] = mix(&prior[i], &edge[i]);
            }
        }
    }
    for node in nodes {
        if let Some(prior) = positions.get(&node_key(node)) {
            for key in ["y", "h"] {
                node[key] = mix(&prior[key], &node[key]);
            }
        }
    }
}

fn topology_focus(nodes: &[Value], edges: &[Value], width: f32, height: f32, pointer: (f32, f32)) -> Option<(std::collections::HashSet<usize>, String)> {
    let (x, y) = pointer;
    let label_width = ((width - 32.) / 4. - 8.).floor().clamp(56., 180.);
    let x_of = |v: &Value| v.as_f64().unwrap_or(0.) as f32 * (width - label_width - 14.).max(0.) / 3.;
    for (i, node) in nodes.iter().enumerate() {
        let left = x_of(&node["layer"]);
        let top = node["y"].as_f64().unwrap_or(0.) as f32 * height;
        let h = node["h"].as_f64().unwrap_or(0.) as f32 * height;
        if x < left || x > left + 13. + label_width || y < top || y > top + h { continue; }
        let mut trajectory = std::collections::HashSet::new();
        for (from, to) in [(5, 6), (6, 5)] {
            let mut seen = std::collections::HashSet::from([i]);
            for _ in 0..3 {
                for edge in edges {
                    if seen.contains(&(edge[from].as_u64().unwrap_or(0) as usize)) {
                        seen.insert(edge[to].as_u64().unwrap_or(0) as usize);
                    }
                }
            }
            trajectory.extend(seen);
        }
        let kind = ["来源 IP", "规则", "策略组", "出口"][node["layer"].as_u64().unwrap_or(0).min(3) as usize];
        return Some((trajectory, format!("{}\n类型: {kind}\n连接数: {}", node["name"].as_str().unwrap_or(""), node["count"])));
    }
    for edge in edges {
        let left = x_of(&edge[0]) + 8.;
        let right = x_of(&edge[2]);
        if x < left || x > right || left >= right { continue; }
        let mid = (left + right) / 2.;
        let (mut lo, mut hi) = (0_f32, 1_f32);
        for _ in 0..10 {
            let t = (lo + hi) / 2.;
            let u = 1. - t;
            let projected = u*u*u*left + 3.*u*t*mid + t*t*t*right;
            if projected < x { lo = t; } else { hi = t; }
        }
        let t = (lo + hi) / 2.;
        let f = t*t*(3.-2.*t);
        let start = edge[1].as_f64().unwrap_or(0.) as f32;
        let end = edge[3].as_f64().unwrap_or(0.) as f32;
        let thickness = edge[4].as_f64().unwrap_or(0.) as f32 * height;
        if (y - (start + (end - start) * f) * height).abs() > (thickness / 2.).max(2.) { continue; }
        let (a, b) = (edge[5].as_u64()? as usize, edge[6].as_u64()? as usize);
        let (src, dst) = (nodes.get(a)?["name"].as_str()?, nodes.get(b)?["name"].as_str()?);
        return Some((std::collections::HashSet::from([a, b]), format!("{src} → {dst}\n连接数: {}", edge[7])));
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverviewLink {
    Offline,
    Connecting,
    Live,
}

fn overview_link(running: bool, traffic: (&str, u32), connections: (&str, u32)) -> OverviewLink {
    if !running {
        OverviewLink::Offline
    } else if traffic.0 == "已连接" && connections.0 == "已连接" {
        OverviewLink::Live
    } else if traffic.1.max(connections.1) >= 3
        || [traffic.0, connections.0].iter().any(|s| s.contains("认证失败"))
    {
        OverviewLink::Offline
    } else {
        OverviewLink::Connecting
    }
}

impl Workspace {
    fn overview_link(&self) -> OverviewLink {
        overview_link(self.api_enabled && self.local_state.contains("运行中"),
            (self.traffic_status, self.traffic_failures), (self.conn_status, self.conn_failures))
    }

    pub fn overview_live(&self) -> bool {
        self.overview_link() == OverviewLink::Live
    }

    pub fn overview_connecting(&self) -> bool {
        self.overview_link() == OverviewLink::Connecting
    }

    pub(super) fn overview_page(&self, window: &mut Window, cx: &Context<Self>) -> Div {
        let palette = crate::theme::Palette::get(cx);
        let translucent = true;
        let tint = self.glass_tint;
        let vp_w = f32::from(window.viewport_size().width);
        let content_w = (vp_w - 256.).max(320.);
        let narrow = content_w < 640.;

        let metrics = self.metrics();
        let active_metrics = self.frozen_topology.as_ref().or(self.hovered_topology.as_ref()).unwrap_or(&metrics);
        let local_running = self.local_state.contains("运行中");
        let traffic_live = local_running && self.traffic_status == "已连接";
        let connections_open = local_running && self.conn_status == "已连接";
        let connections_live = connections_open && self.snapshot.is_some();
        let live = self.overview_live();
        let local_stopped = self.local_state.contains("已停止");
        let local_not_installed = self.local_state.contains("未安装");
        let local_starting = self.local_state.contains("启动中");
        let local_stopping = self.local_state.contains("停止中");
        let restarting = self.settings_layout.service_action == Some("restart");
        let service_transition = !local_running
            && (self.service_syncing || restarting || local_starting || local_stopping);
        let connecting = self.overview_connecting()
            && !local_stopped
            && !local_not_installed
            && !service_transition;

        let notice: Option<(&'static str, &'static str, &'static str, bool)> = if live || connecting
        {
            None
        } else if service_transition {
            if self.service_syncing {
                Some((
                    "正在更新服务组件…",
                    "核心重启后自动显示实时数据。",
                    "",
                    false,
                ))
            } else if restarting || local_starting {
                Some((
                    if restarting { "正在重启 sing-box…" } else { "正在启动 sing-box…" },
                    "核心启动后自动显示实时数据。",
                    "",
                    false,
                ))
            } else {
                Some((
                    "正在停止 sing-box…",
                    "停止后可以在这里重新启动。",
                    "",
                    false,
                ))
            }
        } else if [self.traffic_status, self.conn_status].iter().any(|s| s.contains("认证失败")) {
            Some(("Clash API 密钥不匹配", "请在设置里修改 Clash API 密钥。", "api", true))
        } else if local_not_installed {
            Some((
                "尚未安装 singboard 服务",
                "在设置中安装服务后，即可启动 sing-box。",
                "settings",
                false,
            ))
        } else if local_stopped {
            Some((
                "sing-box 未运行",
                "启动服务后显示实时流量、出口和内存。",
                "start",
                false,
            ))
        } else {
            Some((
                "连不上 sing-box",
                "没有收到 Clash API 的响应，正在自动重试。连上之前这里不显示数字。",
                "api",
                true,
            ))
        };

        let state_text = if connecting {
            "正在连接 sing-box…".to_owned()
        } else if local_running {
            self.uptime().map_or_else(|| "运行中".to_owned(), |time| format!("运行中 {time}"))
        } else if live {
            "已连接".to_owned()
        } else if local_stopped {
            "已停止".to_owned()
        } else {
            "连不上 sing-box".to_owned()
        };
        let (dot_color, halo_color, hollow_dot) = if let Some((_, _, _, alert)) = notice {
            if alert || local_stopped {
                (palette.danger_fill(), palette.danger_soft(), false)
            } else {
                (palette.muted(), rgba(0x00000000).into(), true)
            }
        } else if live {
            (palette.success_dot(), palette.success_soft(), false)
        } else {
            (palette.muted(), rgba(0x00000000).into(), true)
        };

        let version_raw = self.summary["version"].as_str().unwrap_or("").trim();
        let version_text = if version_raw.is_empty() {
            String::new()
        } else if version_raw.to_ascii_lowercase().starts_with("sing-box") {
            version_raw.to_owned()
        } else {
            format!("sing-box {version_raw}")
        };

        let mode_raw = self.summary["config"]["mode"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let mode_text = mode_phrase(&mode_raw);

        let (exit_node, via_chain, exit_direct, exit_latency) = if live {
            let exit_val = &self.summary["exit"];
            if exit_val.is_object() {
                let node = exit_val["node"].as_str().unwrap_or("").to_owned();
                let direct = exit_val["direct"].as_bool().unwrap_or(false);
                let via = exit_val["via"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" › ")
                    })
                    .unwrap_or_default();
                let latency = exit_val["latency"].as_u64();
                (node, via, direct, latency)
            } else {
                let route = self.summary["route"].as_str().unwrap_or("");
                let (node, via, latency) = split_route_fallback(route);
                let direct = node.eq_ignore_ascii_case("DIRECT");
                (node, via, direct, if direct { None } else { latency })
            }
        } else {
            (String::new(), String::new(), false, None)
        };
        let has_exit = live && !exit_node.is_empty();
        let exit_display = if !has_exit {
            "—".to_owned()
        } else if exit_direct {
            "直连".to_owned()
        } else {
            exit_node.clone()
        };
        let via_display = if !has_exit {
            String::new()
        } else if !via_chain.is_empty() {
            via_chain
        } else if exit_direct {
            "不经过代理".into()
        } else {
            String::new()
        };

        let profile_label = if self.active_profile.is_empty() || self.active_profile == "—" {
            "未选择配置".to_owned()
        } else {
            self.active_profile.clone()
        };

        let latest = self.traffic_samples.back().filter(|_| traffic_live && self.has_traffic);
        let (dl_num, dl_unit) = latest.map_or_else(|| ("—".into(), ""), |v| speed_parts(v.down));
        let (ul_num, ul_unit) = latest.map_or_else(|| ("—".into(), ""), |v| speed_parts(v.up));
        let dl_total = connections_live.then(|| metrics["download"].as_u64()).flatten();
        let ul_total = connections_live.then(|| metrics["upload"].as_u64()).flatten();

        let status_dot = if hollow_dot {
            div()
                .size(px(8.))
                .rounded_full()
                .border_1()
                .border_color(palette.muted())
        } else {
            div()
                .size(px(14.))
                .rounded_full()
                .bg(halo_color)
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(px(8.)).rounded_full().bg(dot_color))
        };

        let hero_top_left = if let Some((title, _, action, _)) = notice {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .child(if action.is_empty() {
                    super::settings::loading_spinner(
                        "overview-notice-spinner",
                        px(14.),
                        palette.muted(),
                    )
                } else {
                    status_dot.into_any_element()
                })
                .child(
                    div()
                        .text_size(crate::theme::text_px(13.))
                        .line_height(px(19.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.ink())
                        .child(title),
                )
        } else {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(status_dot)
                        .child(
                            div()
                                .text_size(crate::theme::text_px(13.))
                                .line_height(px(19.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette.ink())
                                .child(state_text),
                        )
                        .when(live && !mode_text.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_size(crate::theme::text_px(13.))
                                    .line_height(px(19.5))
                                    .text_color(palette.muted())
                                    .child("·"),
                            )
                            .child(
                                div()
                                    .id("hero-mode-link")
                                    .cursor_pointer()
                                    .rounded(px(6.))
                                    .px(px(4.))
                                    .hover(|s| s.bg(palette.hover()))
                                    .text_size(crate::theme::text_px(13.))
                                    .line_height(px(19.5))
                                    .text_color(palette.muted())
                                    .child(mode_text)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open(Page::Settings, cx);
                                    })),
                            )
                        }),
                )
                .when(!version_text.is_empty(), |el| {
                    el.child(
                        div()
                            .pl(px(22.))
                            .text_size(crate::theme::text_px(12.))
                            .line_height(px(18.))
                            .text_color(palette.muted())
                            .truncate()
                            .child(version_text),
                    )
                })
        };

        let hero_top = div()
            .when(narrow, |d| d.px(px(20.)).pt(px(18.)))
            .when(!narrow, |d| d.px(px(24.)).pt(px(20.)))
            .min_h(px(if narrow { 52. } else { 54. }))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(hero_top_left)
            .child(
                div()
                    .id("hero-config-chip")
                    .cursor_pointer()
                    .max_w(px(220.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(10.))
                    .pr(px(6.))
                    .py(px(4.))
                    .rounded_full()
                    .hover(|s| s.bg(palette.fill()))
                    .text_size(crate::theme::text_px(13.))
                    .line_height(px(19.5))
                    .text_color(palette.muted())
                    .child(div().truncate().child(node_label(profile_label)))
                    .when(self.active_profile_failed, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(palette.danger())
                                .child("更新失败"),
                        )
                    })
                    .child(
                        svg().path("hero-next").size(px(14.)).flex_none().text_color(palette.muted()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open(Page::Config, cx);
                    })),
            );

        let hero_left = if let Some((_, detail, action, _)) = notice {
            let detail_text = if action == "api" && !self.endpoint.is_empty() {
                format!("{detail} 地址 {}", self.endpoint)
            } else {
                detail.to_owned()
            };
            div()
                .id("hero-notice")
                .when(!narrow, |d| d.flex_1())
                .min_w_0()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(14.))
                .child(
                    div()
                        .text_size(crate::theme::text_px(13.))
                        .line_height(px(21.))
                        .text_color(palette.muted())
                        .child(detail_text),
                )
                .when(!action.is_empty(), |el| {
                    el.child(match action {
                        "start" => div()
                            .id("hero-start-service")
                            .cursor_pointer()
                            .px(px(20.))
                            .py(px(7.))
                            .rounded_lg()
                            .bg(palette.primary_fill())
                            .text_size(crate::theme::text_px(13.))
                            .line_height(px(19.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(0xffffff))
                            .child("启动服务")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tray_service("start", cx);
                            })),
                        "settings" => div()
                            .id("hero-open-settings")
                            .cursor_pointer()
                            .px(px(20.))
                            .py(px(7.))
                            .rounded_lg()
                            .bg(palette.fill())
                            .hover(|s| s.bg(palette.fill_strong()))
                            .text_size(crate::theme::text_px(13.))
                            .line_height(px(19.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.ink())
                            .child("前往设置")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open(Page::Settings, cx);
                            })),
                        _ => div()
                            .id("hero-check-api")
                            .cursor_pointer()
                            .px(px(20.))
                            .py(px(7.))
                            .rounded_lg()
                            .bg(palette.fill())
                            .hover(|s| s.bg(palette.fill_strong()))
                            .text_size(crate::theme::text_px(13.))
                            .line_height(px(19.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.ink())
                            .child("检查 API 配置")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open(Page::Settings, cx);
                            })),
                    })
                })
        } else {
            div()
                .id("hero-exit-node")
                .when(!narrow, |d| d.flex_1())
                .min_w_0()
                .cursor_pointer()
                .rounded(px(12.))
                .px(px(10.))
                .py(px(6.))
                .mx(px(-10.))
                .my(px(-6.))
                .hover(|s| s.bg(palette.fill()))
                .flex()
                .flex_col()
                .gap(px(3.))
                .on_click(cx.listener(|this, _, _, cx| this.open(Page::Proxies, cx)))
                .child(
                    div()
                        .text_size(crate::theme::text_px(12.))
                        .line_height(px(18.))
                        .text_color(palette.muted())
                        .child(exit_heading(&mode_raw)),
                )
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap_3()
                        .min_w_0()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(crate::theme::text_px(if narrow { 26. } else { 28. }))
                                .line_height(px(if narrow { 31. } else { 34. }))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if has_exit {
                                    palette.ink()
                                } else {
                                    palette.muted()
                                })
                                .child(node_label(exit_display)),
                        )
                        .when_some(exit_latency, |el, ms| {
                            el.child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .pb(px(2.))
                                    .text_size(crate::theme::text_px(13.))
                                    .line_height(px(19.5))
                                    .text_color(palette.muted())
                                    .child(
                                        div()
                                            .size(px(7.))
                                            .rounded_full()
                                            .bg(latency_dot(Some(ms), palette)),
                                    )
                                    .child(format_latency(ms)),
                            )
                        }),
                )
                .when(!via_display.is_empty(), |el| {
                    el.child(
                        div()
                            .mt(px(1.))
                            .text_size(crate::theme::text_px(13.))
                            .line_height(px(19.5))
                            .text_color(palette.muted())
                            .truncate()
                            .child(node_label(via_display)),
                    )
                })
        };

        let rate_size = if narrow { 28. } else { 32. };
        let rate_lh = if narrow { 33. } else { 38. };
        let hero_rates = div()
            .id("hero-rates")
            .cursor_pointer()
            .rounded(px(12.))
            .px(px(10.))
            .py(px(6.))
            .mx(px(-10.))
            .my(px(-6.))
            .hover(|s| s.bg(palette.fill()))
            .flex()
            .items_end()
            .when(narrow, |d| d.w_full().gap(px(28.)))
            .when(!narrow, |d| d.gap(px(40.)))
            .on_click(cx.listener(|this, _, _, cx| this.open(Page::Connections, cx)))
            .child(
                div()
                    .when(narrow, |d| d.flex_1())
                    .min_w(px(120.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(crate::theme::text_px(12.))
                            .line_height(px(18.))
                            .text_color(palette.muted())
                            .flex().items_center().gap(px(4.))
                            .child(svg().path("download").size(px(12.)).text_color(palette.muted())).child("下载"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(rate_size))
                                    .line_height(px(rate_lh))
                                    .font_weight(FontWeight::LIGHT)
                                    .text_color(if latest.is_some() {
                                        palette.ink()
                                    } else {
                                        palette.muted()
                                    })
                                    .child(dl_num),
                            )
                            .when(!dl_unit.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_size(crate::theme::text_px(14.))
                                        .line_height(px(21.))
                                        .text_color(palette.muted())
                                        .child(dl_unit),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(crate::theme::text_px(12.))
                            .line_height(px(18.))
                            .text_color(palette.muted())
                            .child(format!(
                                "总计 {}",
                                dl_total.map(total_bytes).unwrap_or_else(|| "—".into())
                            )),
                    ),
            )
            .child(
                div()
                    .when(narrow, |d| d.flex_1())
                    .min_w(px(120.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(crate::theme::text_px(12.))
                            .line_height(px(18.))
                            .text_color(palette.muted())
                            .flex().items_center().gap(px(4.))
                            .child(svg().path("upload").size(px(12.)).text_color(palette.muted())).child("上传"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(rate_size))
                                    .line_height(px(rate_lh))
                                    .font_weight(FontWeight::LIGHT)
                                    .text_color(if latest.is_some() {
                                        palette.ink()
                                    } else {
                                        palette.muted()
                                    })
                                    .child(ul_num),
                            )
                            .when(!ul_unit.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_size(crate::theme::text_px(14.))
                                        .line_height(px(21.))
                                        .text_color(palette.muted())
                                        .child(ul_unit),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(crate::theme::text_px(12.))
                            .line_height(px(18.))
                            .text_color(palette.muted())
                            .child(format!(
                                "总计 {}",
                                ul_total.map(total_bytes).unwrap_or_else(|| "—".into())
                            )),
                    ),
            );

        let hero_main = if narrow {
            div()
                .px(px(20.))
                .mt(px(20.))
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(hero_left)
                .child(div().h(px(0.5)).w_full().bg(palette.line()))
                .child(hero_rates)
        } else {
            div()
                .px(px(24.))
                .mt(px(24.))
                .flex()
                .items_end()
                .justify_between()
                .gap(px(32.))
                .child(hero_left)
                .child(hero_rates)
        };

        let hero_card = glass_card(palette, translucent, tint)
            .w_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(hero_top)
            .child(hero_main)
            .child(traffic_wave(
                self.traffic_samples.iter().copied().collect(),
                latest.is_some(),
                narrow,
                palette,
            ));

        let tcp = metrics["tcp"].as_u64().unwrap_or(0);
        let udp = metrics["udp"].as_u64().unwrap_or(0);
        let conn_total = metrics["connectionCount"].as_u64().unwrap_or_else(|| self.active().len() as u64);
        let mem_bytes = self.memory.filter(|_| traffic_live);
        let (mem_num, mem_unit) = mem_bytes.map_or_else(|| ("—".into(), ""), bytes_parts);
        let count_series: Vec<u64> = self.history_counts.iter().rev().copied().collect();
        let mem_series: Vec<u64> = self.history_memory.iter().rev().copied().collect();

        let stat_px = if narrow { px(14.) } else { px(18.) };
        let stat_pt = if narrow { px(12.) } else { px(14.) };
        let stat_val_size = crate::theme::text_px(if narrow { 18. } else { 19. });

        let stats_card = glass_card(palette, translucent, tint)
            .mt(px(12.))
            .w_full()
            .overflow_hidden()
            .flex()
            .child(
                div()
                    .id("stat-connections")
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .cursor_pointer()
                    .hover(|s| s.bg(palette.fill()))
                    .on_click(cx.listener(|this, _, _, cx| this.open(Page::Connections, cx)))
                    .child(
                        div()
                            .pt(stat_pt)
                            .px(stat_px)
                            .pb(px(8.))
                            .flex()
                            .flex_col()
                            .child(
                                div().text_size(crate::theme::text_px(12.)).line_height(px(18.))
                                    .text_color(palette.muted()).child("连接"),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .line_height(stat_val_size * 1.5)
                                    .text_size(stat_val_size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if connections_live {
                                        palette.ink()
                                    } else {
                                        palette.muted()
                                    })
                                    .child(if connections_live {
                                        conn_total.to_string()
                                    } else {
                                        "—".into()
                                    }),
                            )
                            .child(
                                div()
                                    .min_h(px(if narrow && connections_live { 34. } else { 17.4 }))
                                    .line_height(px(17.4))
                                    .text_size(crate::theme::text_px(12.))
                                    .text_color(palette.muted())
                                    .when(connections_live, |d| if narrow { d.flex().flex_col().child(format!("TCP {tcp}")).child(format!("UDP {udp}")) } else { d.child(format!("TCP {tcp} · UDP {udp}")) }),
                            ),
                    )
                    .child(mini_line_ink(count_series, 10, connections_live, palette)),
            )
            .child(div().w(px(1.)).my(px(14.)).bg(palette.line()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .pt(stat_pt)
                            .px(stat_px)
                            .pb(px(8.))
                            .flex()
                            .flex_col()
                            .child(
                                div().text_size(crate::theme::text_px(12.)).line_height(px(18.))
                                    .text_color(palette.muted()).child("内存"),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .line_height(stat_val_size * 1.5)
                                    .flex()
                                    .items_baseline()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .text_size(stat_val_size)
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if mem_bytes.is_some() {
                                                palette.ink()
                                            } else {
                                                palette.muted()
                                            })
                                            .child(mem_num),
                                    )
                                    .when(!mem_unit.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .text_size(crate::theme::text_px(12.5))
                                                .text_color(palette.muted())
                                                .child(mem_unit),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .min_h(px(if narrow && connections_live { 34. } else { 17.4 }))
                                    .line_height(px(17.4))
                                    .text_size(crate::theme::text_px(12.))
                                    .text_color(palette.muted())
                                    .when(mem_bytes.is_some(), |d| d.child("运行时占用")),
                            ),
                    )
                    .child(mini_line_ink(
                        mem_series,
                        16 * 1024 * 1024,
                        mem_bytes.is_some(),
                        palette,
                    )),
            );

        let (mut dom_loc, dom_ip, dom_err) = format_ip_row(
            self.network_info.as_ref().filter(|_| !self.network_busy).map(|v| &v["domestic"]),
            self.network_busy,
            self.show_ip,
        );
        let (mut abr_loc, abr_ip, abr_err) = format_ip_row(
            self.network_info.as_ref().filter(|_| !self.network_busy).map(|v| &v["abroad"]),
            self.network_busy,
            self.show_ip,
        );
        if self.network_due.is_some() {
            dom_loc = "即将自动检测".into();
            abr_loc = "即将自动检测".into();
        }
        let sites_val = self
            .network_info
            .as_ref()
            .and_then(|v| v["sites"].as_array());

        let ip_row_ui = |label: &'static str,
                         source: &'static str,
                         loc: String,
                         ip: String,
                         err: bool| {
            let pad_x = if narrow { px(16.) } else { px(18.) };
            let pad_y = if narrow { px(10.) } else { px(11.) };
            let pending = matches!(loc.as_str(), "未检测" | "检测中…" | "即将自动检测");
            let location_size = if err { 12. } else if pending { 13. } else { 14. };
            div()
                .px(pad_x)
                .py(pad_y)
                .flex()
                .items_baseline()
                .gap_3()
                .child(
                    div()
                        .w(px(76.))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(crate::theme::text_px(13.))
                                .line_height(px(19.5))
                                .text_color(palette.muted())
                                .child(label),
                        )
                        .child(
                            div()
                                .text_size(crate::theme::text_px(11.))
                                .line_height(px(16.5))
                                .text_color(palette.muted())
                                .child(source),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_wrap()
                        .items_baseline()
                        .justify_between()
                        .gap_x_3()
                        .gap_y(px(2.))
                        .child(
                            div()
                                .text_size(crate::theme::text_px(location_size))
                                .line_height(px(location_size * 1.5))
                                .font_weight(if err || pending {
                                    FontWeight::NORMAL
                                } else {
                                    FontWeight::MEDIUM
                                })
                                .text_color(if err {
                                    palette.danger()
                                } else if pending {
                                    palette.muted()
                                } else {
                                    palette.ink()
                                })
                                .child(loc),
                        )
                        .when(!ip.is_empty(), |el| {
                            el.child(
                                div()
                                    .font_family("Consolas")
                                    .text_size(crate::theme::text_px(12.))
                                    .text_color(palette.muted())
                                    .child(ip),
                            )
                        }),
                )
        };

        let ip_col = div()
            .when(!narrow, |d| d.w(relative(1.1 / 2.1)).flex_none())
            .min_w_0()
            .py_1()
            .flex()
            .flex_col()
            .justify_center()
            .child(ip_row_ui("直连出口", "ipip.net", dom_loc, dom_ip, dom_err))
            .child(div().h(px(1.)).ml_4().bg(palette.line()))
            .child(ip_row_ui("代理出口", "ip.sb", abr_loc, abr_ip, abr_err));

        let mut site_cells = Vec::new();
        for (idx, (_, label, _)) in singboard_stream::tools::OVERVIEW_SITES
            .into_iter()
            .enumerate()
        {
            let item = sites_val.and_then(|arr| arr.get(idx));
            let reason = item.filter(|s| s["status"] == "error").and_then(|s| s["error"].as_str()).unwrap_or_default().to_owned();
            let (dot_color, badge_text) = if self.network_busy || self.network_due.is_some() {
                (palette.muted(), "检测中".to_owned())
            } else if let Some(s) = item {
                match s["status"].as_str().unwrap_or("") {
                    "done" => {
                        let ms = s["ms"].as_u64().unwrap_or(0);
                        (latency_dot(Some(ms), palette), if ms == 0 { "失败".to_owned() } else { format!("{ms}ms") })
                    }
                    "error" => (palette.danger_fill(), "失败".to_owned()),
                    _ => (palette.muted(), "未测".to_owned()),
                }
            } else {
                (palette.muted(), "未测".to_owned())
            };
            site_cells.push(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .px(if narrow { px(16.) } else { px(18.) })
                    .py(if narrow { px(8.) } else { px(7.) })
                    .text_size(crate::theme::text_px(13.))
                    .line_height(px(19.5))
                    .child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .text_color(palette.ink())
                            .child(label),
                    )
                    .when(!reason.is_empty() && !self.network_busy && self.network_due.is_none(), |d| d.child(
                        div().id(("site-error", idx)).flex_1().min_w_0().truncate().text_right()
                            .text_size(crate::theme::text_px(12.)).line_height(px(19.5)).text_color(palette.muted()).child(reason.clone())
                            .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(reason.clone()).build(window, cx)),
                    ))
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().flex_none().size(px(7.)).rounded_full().bg(dot_color))
                            .child(
                                div()
                                    .min_w(crate::theme::text_px(48.))
                                    .whitespace_nowrap()
                                    .text_color(palette.ink())
                                    .child(badge_text),
                            ),
                    ),
            );
        }

        let site_grid = if narrow {
            let mut col = div().w_full().py_1().flex().flex_col();
            for cell in site_cells {
                col = col.child(cell);
            }
            col
        } else {
            let mut grid = div().w_full().grid().grid_cols(2);
            for cell in site_cells {
                grid = grid.child(cell);
            }
            div()
                .flex_1()
                .min_w_0()
                .py_2()
                .px_1()
                .flex()
                .flex_col()
                .justify_center()
                .child(grid)
        };

        let running_net = live || local_running;
        let network_section = div()
            .mt(px(26.))
            .flex()
            .flex_col()
            .child(
                div()
                    .mx(px(6.))
                    .mb(px(9.))
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_3()
                            .min_w_0()
                            .child(
                                crate::glass::backdrop_heading("网络", palette, cx)
                                    .flex_none()
                                    .text_size(crate::theme::text_px(15.))
                                    .line_height(px(22.5))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .when(!running_net, |el| {
                                el.child(
                                    crate::glass::backdrop_text(
                                        "sing-box 未运行，启动后自动检测",
                                        true,
                                        palette,
                                        cx,
                                    )
                                    .text_size(crate::theme::text_px(12.))
                                    .truncate(),
                                )
                            }),
                    )
                    .when(running_net, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .id("toggle-ip")
                                        .relative()
                                        .overflow_hidden()
                                        .rounded_full()
                                        .bg(if self.show_ip {
                                            palette.primary_soft()
                                        } else {
                                            palette.fill()
                                        })
                                        .border_1()
                                        .border_color(if self.show_ip {
                                            palette.selection_border()
                                        } else {
                                            palette.control_rim()
                                        })
                                        .shadow_sm()
                                        .cursor_pointer()
                                        .child(crate::glass::control_layer(
                                            palette,
                                            px(999.),
                                            !self.show_ip,
                                        ))
                                        .child(
                                            div()
                                                .relative()
                                                .px(px(10.))
                                                .py(px(3.))
                                                .rounded_full()
                                                .when(self.show_ip, |d| d.bg(palette.primary_soft()))
                                                .hover(|s| s.bg(palette.fill_strong()))
                                                .text_size(crate::theme::text_px(13.))
                                                .line_height(rems(18. / 16.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(palette.accent())
                                                .child("显示 IP"),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.show_ip = !this.show_ip;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("recheck-net")
                                        .relative()
                                        .overflow_hidden()
                                        .rounded_full()
                                        .bg(palette.fill())
                                        .border_1()
                                        .border_color(palette.control_rim())
                                        .shadow_sm()
                                        .cursor_pointer()
                                        .child(crate::glass::control_layer(palette, px(999.), true))
                                        .child(
                                            div()
                                                .relative()
                                                .px(px(10.))
                                                .py(px(3.))
                                                .rounded_full()
                                                .hover(|s| s.bg(palette.fill_strong()))
                                                .text_size(crate::theme::text_px(13.))
                                                .line_height(rems(18. / 16.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(if self.network_busy {
                                                    palette.muted()
                                                } else {
                                                    palette.accent()
                                                })
                                                .child("重新检测"),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.refresh_network(cx)
                                        })),
                                ),
                        )
                    }),
            )
            .child(if narrow {
                glass_card(palette, translucent, tint)
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(ip_col)
                    .child(div().h(px(0.5)).ml_4().bg(palette.line()))
                    .child(site_grid)
            } else {
                glass_card(palette, translucent, tint)
                    .w_full()
                    .flex()
                    .child(ip_col)
                    .child(div().w(px(1.)).my_4().bg(palette.line()))
                    .child(site_grid)
            });

        let flow = &active_metrics["flow"];
        let mut flow_nodes = flow["nodes"].as_array().cloned().unwrap_or_default();
        let mut flow_edges = flow["edges"].as_array().cloned().unwrap_or_default();
        if self.chart && self.frozen_topology.is_none() && self.hovered_topology.is_none()
            && let Some((previous, changed)) = &self.topology_previous
        {
            let t = changed.elapsed().as_secs_f64() / 0.24;
            if (0. ..1.).contains(&t) {
                animate_flow(&mut flow_nodes, &mut flow_edges, previous, 1. - (1. - t).powi(3));
                window.request_animation_frame();
            }
        }
        let flow_conn_count = flow["connectionCount"]
            .as_u64()
            .unwrap_or(if connections_live { conn_total } else { 0 });
        let topo_paused = self.frozen_topology.is_some();
        let topo_summary_text = {
            let base = topology_summary(&flow_nodes, flow_conn_count);
            if topo_paused {
                format!("已暂停更新 · {base}")
            } else {
                base
            }
        };

        let view_seg = div()
            .relative()
            .overflow_hidden()
            .flex()
            .gap(px(2.))
            .p(px(2.))
            .rounded_full()
            .bg(palette.fill())
            .border_1()
            .border_color(palette.control_rim())
            .child(crate::glass::control_layer(palette, px(999.), true))
            .child(
                div()
                    .id("topo-view-chart")
                    .relative()
                    .px(px(14.))
                    .py(px(3.))
                    .rounded_full()
                    .text_size(crate::theme::text_px(12.))
                    .cursor_pointer()
                    .when(self.chart, |el| {
                        el.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(!self.chart, |el| el.text_color(palette.ink().opacity(0.75)))
                    .child("图")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.chart = true;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("topo-view-list")
                    .relative()
                    .px(px(14.))
                    .py(px(3.))
                    .rounded_full()
                    .text_size(crate::theme::text_px(12.))
                    .cursor_pointer()
                    .when(!self.chart, |el| {
                        el.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(self.chart, |el| el.text_color(palette.ink().opacity(0.75)))
                    .child("列表")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.chart = false;
                        cx.notify();
                    })),
            );

        let pause_btn = playback_button("topology-pause", topo_paused, true, palette, cx)
            .on_click(cx.listener(|this, _, _, cx| {
                this.frozen_topology = if this.frozen_topology.is_some() {
                    None
                } else {
                    Some(this.metrics())
                };
                cx.notify();
            }));

        let topo_header = div()
            .mx(px(6.))
            .mb(px(9.))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap_3()
                    .child(
                        crate::glass::backdrop_heading("连接拓扑", palette, cx)
                            .flex_none()
                            .text_size(crate::theme::text_px(15.))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .when(connections_live, |el| {
                        el.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(crate::theme::text_px(12.))
                                .text_color(crate::glass::backdrop_muted(palette, cx))
                                .child(node_label(topo_summary_text)),
                        )
                    }),
            )
            .when(connections_live, |el| {
                el.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(view_seg)
                        .child(pause_btn),
                )
            });

        let topo_colors = palette.topology_colors();
        let topo_waiting = if local_stopped {
            "sing-box 运行后显示连接拓扑"
        } else if connecting || (connections_open && !connections_live) {
            "正在连接 sing-box…"
        } else {
            "与 sing-box 的连接已中断，恢复后显示连接拓扑"
        };

        let mut topo_card = glass_card(palette, translucent, tint)
            .w_full()
            .pl(px(18.))
            .pr(px(14.))
            .py(px(12.))
            .flex()
            .flex_col();

        if !connections_live {
            topo_card = topo_card.child(
                div()
                    .min_h(px(140.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .px_6()
                    .text_size(crate::theme::text_px(13.))
                    .text_color(palette.muted())
                    .child(topo_waiting),
            );
        } else if self.chart {
            if flow_nodes.is_empty() {
                topo_card = topo_card.child(
                    div()
                        .min_h(px(140.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(crate::theme::text_px(13.))
                        .text_color(palette.muted())
                        .child("暂无连接数据"),
                );
            } else {
                let chart_h = topology_height(&flow_nodes);
                let inner_w = (content_w - 32.).max(320.);
                let label_w = ((inner_w - 32.) / 4. - 8.).clamp(56., 180.);
                let col_span = (inner_w - label_w - 14.).max(120.);
                let col_x = |layer: f32| layer * (col_span / 3.);
                let edges_for_canvas = flow_edges.clone();
                let nodes_for_canvas = flow_nodes.clone();

                let focus = self.topology_pointer.and_then(|p| topology_focus(&flow_nodes, &flow_edges, inner_w, chart_h, p));
                let selected = focus.as_ref().map(|(ids, _)| ids.clone()).unwrap_or_default();
                let bounds_cell = std::rc::Rc::new(std::cell::Cell::new(None::<Bounds<Pixels>>));
                let painted_bounds = bounds_cell.clone();
                let mut label_layer = div().absolute().inset_0().overflow_hidden();
                for (i, node) in flow_nodes.iter().enumerate() {
                    let layer = node["layer"].as_u64().unwrap_or(0) as f32;
                    let y = node["y"].as_f64().unwrap_or(0.) as f32;
                    let h = node["h"].as_f64().unwrap_or(0.) as f32;
                    let merged = node["merged"].as_u64().unwrap_or(0) > 0;
                    let name = node["name"].as_str().unwrap_or("").to_owned();
                    let bar_top = y * chart_h;
                    let bar_h = (h * chart_h).max(6.);
                    let mid_y = (bar_top + bar_h * 0.5 - 8.).clamp(0., chart_h - 16.);
                    let left_x = col_x(layer) + 13.;
                    label_layer = label_layer.child(
                        div()
                            .absolute()
                            .left(px(left_x))
                            .top(px(mid_y))
                            .w(px(label_w))
                            .h(px(16.))
                            .text_size(crate::theme::text_px(12.))
                            .line_height(px(16.))
                            .text_color(if !selected.is_empty() && !selected.contains(&i) {
                                palette.chart_ink(0x33)
                            } else if merged {
                                palette.muted()
                            } else {
                                palette.ink()
                            })
                            .truncate()
                            .child(node_label(name)),
                    );
                }

                if let (Some((_, message)), Some((x, y))) = (&focus, self.topology_pointer) {
                    label_layer = label_layer.child(div().absolute()
                        .left(px(x.min((inner_w - 190.).max(0.))))
                        .top(px((y + 10.).min((chart_h - 67.).max(0.))))
                        .max_w(px(190.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(palette.rim())
                        .shadow_sm()
                        .bg(palette.card(false))
                        .text_color(palette.ink())
                        .px(px(12.))
                        .py(px(8.))
                        .text_size(crate::theme::text_px(12.))
                        .child(node_label(message.clone())));
                }
                topo_card = topo_card.child(
                    div()
                        .id("overview-topology-chart")
                        .relative()
                        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                            this.hovered_topology = hovered.then(|| this.metrics());
                            if !*hovered { this.topology_pointer = None; }
                            cx.notify();
                        }))
                        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                            if let Some(bounds) = bounds_cell.get() {
                                this.topology_pointer = Some((f32::from(event.position.x - bounds.origin.x), f32::from(event.position.y - bounds.origin.y)));
                                cx.notify();
                            }
                        }))
                        .w_full()
                        .h(px(chart_h))
                        .child(
                            canvas(
                                move |bounds, _, _| painted_bounds.set(Some(bounds)),
                                move |bounds, _, w, _| {
                                    let lw =
                                        ((f32::from(bounds.size.width) - 32.) / 4. - 8.)
                                            .clamp(56., 180.);
                                    let span = (f32::from(bounds.size.width) - lw - 14.).max(120.);
                                    let x_of = |layer: f32| bounds.origin.x + px(layer * (span / 3.));
                                    for edge in &edges_for_canvas {
                                        let v = |i: usize| edge[i].as_f64().unwrap_or(0.) as f32;
                                        let a = point(
                                            x_of(v(0)) + px(8.),
                                            bounds.origin.y + bounds.size.height * v(1),
                                        );
                                        let b = point(
                                            x_of(v(2)),
                                            bounds.origin.y + bounds.size.height * v(3),
                                        );
                                        let mid = (a.x + b.x) * 0.5;
                                        let mut path = PathBuilder::stroke(
                                            (bounds.size.height * v(4)).max(px(1.5)),
                                        );
                                        path.move_to(a);
                                        path.cubic_bezier_to(b, point(mid, a.y), point(mid, b.y));
                                        if let Ok(path) = path.build() {
                                            let start = topo_colors[(v(0) as usize).min(3)];
                                            let end = topo_colors[(v(2) as usize).min(3)];
                                            let alpha = if selected.is_empty() { 0x29 } else if selected.contains(&(v(5) as usize)) && selected.contains(&(v(6) as usize)) { 0x80 } else { 0x0d };
                                            w.paint_path(path, linear_gradient(90.,
                                                linear_color_stop(rgba((start << 8) | alpha), 0.),
                                                linear_color_stop(rgba((end << 8) | alpha), 1.),
                                            ));
                                        }
                                    }
                                    for (index, node) in nodes_for_canvas.iter().enumerate() {
                                        let layer = node["layer"].as_u64().unwrap_or(0) as f32;
                                        let y = node["y"].as_f64().unwrap_or(0.) as f32;
                                        let h = node["h"].as_f64().unwrap_or(0.) as f32;
                                        let merged = node["merged"].as_u64().unwrap_or(0) > 0;
                                        let base = topo_colors[(layer as usize).min(3)];
                                        let alpha = if !selected.is_empty() && !selected.contains(&index) { 0x29 } else if merged { 0x59 } else { 0xbf };
                                        w.paint_quad(fill(
                                            Bounds::new(
                                                point(
                                                    x_of(layer),
                                                    bounds.origin.y + bounds.size.height * y,
                                                ),
                                                size(px(8.), (bounds.size.height * h).max(px(6.))),
                                            ),
                                            rgba((base << 8) | alpha),
                                        ));
                                    }
                                },
                            )
                            .size_full(),
                        )
                        .child(label_layer),
                );
            }
        } else if flow_nodes.is_empty() {
            topo_card = topo_card.child(
                div()
                    .py_6()
                    .flex()
                    .justify_center()
                    .text_size(crate::theme::text_px(13.))
                    .text_color(palette.muted())
                    .child("暂无连接数据"),
            );
        } else {
            let total = flow_conn_count.max(1) as f32;
            let render_group = |title: &'static str, rows: Vec<TopologyItem>| {
                let mut col = div().flex_1().min_w_0().flex().flex_col().child(
                    div()
                        .mb(px(6.))
                        .text_size(crate::theme::text_px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.muted())
                        .child(title),
                );
                for item in rows {
                    let ratio = (item.count as f32 / total).clamp(0.02, 1.0);
                    col = col.child(
                        div()
                            .pt(px(5.))
                            .pb(px(9.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .justify_between()
                                    .gap_3()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(crate::theme::text_px(13.))
                                            .text_color(if item.merged > 0 {
                                                palette.muted()
                                            } else {
                                                palette.ink()
                                            })
                                            .child(node_label(item.name)),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(crate::theme::text_px(12.))
                                            .text_color(palette.muted())
                                            .child(format!("{} 条", item.count)),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .h(px(3.))
                                    .rounded_full()
                                    .overflow_hidden()
                                    .bg(palette.fill())
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(ratio))
                                            .rounded_full()
                                            .bg(palette.chart_ink(0x66)),
                                    ),
                            ),
                    );
                }
                col
            };
            let exits = topology_ranking(&flow_nodes, 3);
            let rules = topology_ranking(&flow_nodes, 1);
            topo_card = topo_card.child(
                div()
                    .pt_1()
                    .pr_1()
                    .pb(px(2.))
                    .flex()
                    .when(narrow, |d| d.flex_col().gap(px(14.)))
                    .when(!narrow, |d| d.gap(px(32.)))
                    .child(render_group("按出口", exits))
                    .child(render_group("按规则", rules)),
            );
        }

        let topology_section = div()
            .mt(px(26.))
            .flex()
            .flex_col()
            .child(topo_header)
            .child(topo_card);

        let mut body = div()
            .w_full()
            .flex()
            .flex_col()
            .pb(px(4.))
            .child(
                crate::glass::backdrop_heading("概览", palette, cx)
                    .mb_3()
                    .text_size(crate::theme::text_px(26.))
                    .line_height(px(32.))
                    .font_weight(FontWeight::BOLD),
            )
            .child(hero_card)
            .child(stats_card)
            .child(network_section)
            .child(topology_section);
        if !self.load_error.is_empty() {
            body = body.child(
                div()
                    .mt_3()
                    .px_2()
                    .text_xs()
                    .text_color(crate::glass::backdrop_muted(palette, cx))
                    .child(self.load_error.clone()),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.scroll_pane("overview-scroll", body.pt(px(48.)), cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{OVERVIEW_SAMPLES, OverviewLink, format_uptime, history_x, overview_link};

    #[test]
    fn partial_history_occupies_the_newest_slots() {
        assert_eq!(history_x(0, 2), 58. / 59.);
        assert_eq!(history_x(1, 2), 1.);
        assert_eq!(history_x(0, OVERVIEW_SAMPLES), 0.);
        assert_eq!(history_x(OVERVIEW_SAMPLES - 1, OVERVIEW_SAMPLES), 1.);
    }

    #[test]
    fn uptime_matches_the_status_and_sidebar_clock() {
        for (seconds, label) in [
            (0, "0:00"),
            (61, "1:01"),
            (8596, "2:23:16"),
            (86401, "1天 0:00:01"),
        ] {
            assert_eq!(format_uptime(seconds), label);
        }
    }
    #[test]
    fn overview_requires_a_running_service_and_both_open_sockets() {
        let open = ("已连接", 0);
        let retry = ("连接已断开，正在重连", 2);
        assert_eq!(overview_link(true, open, open), OverviewLink::Live);
        assert_eq!(overview_link(false, open, open), OverviewLink::Offline);
        assert_eq!(overview_link(true, retry, open), OverviewLink::Connecting);
        assert_eq!(overview_link(true, open, retry), OverviewLink::Connecting);
        assert_eq!(overview_link(true, open, ("连接已断开，正在重连", 3)), OverviewLink::Offline);
        assert_eq!(overview_link(true, ("认证失败，请检查密钥", 1), open), OverviewLink::Offline);
    }
}
