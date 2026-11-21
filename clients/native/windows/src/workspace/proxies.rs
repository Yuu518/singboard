use super::*;
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Instant,
};

#[derive(Default)]
pub(super) struct ProxyUi {
    query: String,
    collapsed: HashSet<String>,
    orders: HashMap<String, HashMap<String, usize>>,
    hovered: Option<String>,
    hover_motion: HashMap<(bool, String), (Instant, f32, bool)>,
    pointer: Option<Point<Pixels>>,
    pub(super) testing: Option<String>,
    pub(super) testing_nodes: HashSet<String>,
    motion: HashMap<(bool, String), (Instant, f32, bool)>,
    reveal: Option<(String, Rc<Cell<Option<Bounds<Pixels>>>>)>,
}

impl ProxyUi {
    fn set_hover(&mut self, provider: bool, name: &str, hovered: bool) {
        let key = (provider, name.to_owned());
        let current = self.hover_value(&key, None);
        if self.hover_motion.get(&key).is_some_and(|(_, _, t)| *t == hovered) {
            return;
        }
        self.hover_motion.insert(key, (Instant::now(), current, hovered));
    }

    fn hover_value(&self, key: &(bool, String), window: Option<&Window>) -> f32 {
        let Some((started, from, target)) = self.hover_motion.get(key) else {
            return 0.;
        };
        let end = if *target { 1. } else { 0. };
        let elapsed = started.elapsed().as_secs_f32();
        if elapsed >= 0.20 {
            return end;
        }
        if let Some(window) = window {
            window.request_animation_frame();
        }
        let u = (elapsed / 0.20).clamp(0., 1.);
        let ease = 1. - (1. - u) * (1. - u);
        (from + (end - from) * ease).clamp(0., 1.)
    }

    fn hover_level(&self, provider: bool, name: &str, window: &Window) -> f32 {
        self.hover_value(&(provider, name.to_owned()), Some(window))
    }

    pub(super) fn cancel_reveal(&mut self) {
        self.reveal = None;
    }

    pub(super) fn search_changed(&mut self, q: String) {
        if q != self.query {
            self.query = q;
            self.collapsed.clear();
            self.motion.clear();
            self.reveal = None;
        }
    }

    pub(super) fn reset_order(&mut self) {
        self.orders.clear();
    }

    fn fraction(
        &self,
        provider: bool,
        name: &str,
        open: bool,
        dot_count: usize,
        window: Option<&Window>,
    ) -> (f32, f32, f32, Option<f32>) {
        let end = if open { 1. } else { 0. };
        let Some((started, from, target)) = self.motion.get(&(provider, name.to_owned())) else {
            return (end, end, end, None);
        };
        if *target != open {
            return (end, end, end, None);
        }
        let elapsed = started.elapsed().as_secs_f32();
        let total = if open {
            0.27
        } else {
            0.06 + dot_count.min(30) as f32 * 0.012 + 0.35
        };
        if elapsed >= total {
            return (end, end, end, None);
        }
        if let Some(window) = window {
            window.request_animation_frame();
        }
        let h_t = super::scrolling::ease_ios((elapsed / 0.26).clamp(0., 1.));
        let height = end + (from - end) * (1. - h_t);
        let opacity = if open {
            let ot = super::scrolling::ease_standard(((elapsed - 0.04) / 0.22).clamp(0., 1.));
            from + (1. - from) * ot
        } else {
            let ot = super::scrolling::ease_standard((elapsed / 0.18).clamp(0., 1.));
            from * (1. - ot)
        };
        let chev_t = super::scrolling::ease_standard((elapsed / 0.20).clamp(0., 1.));
        let chev = end + (from - end) * (1. - chev_t);
        (height, opacity.clamp(0., 1.), chev, Some(elapsed))
    }
}

fn rank(delay: Option<u64>) -> u64 {
    match delay {
        Some(0) => u64::MAX - 1,
        Some(d) => d,
        None => u64::MAX,
    }
}

fn dot_motion(elapsed: Option<f32>, open: bool, i: usize) -> (f32, f32) {
    elapsed.map_or(
        if open { (0., 0.3) } else { (1., 1.) },
        |t| {
            if open {
                let op = (1. - t / 0.10).clamp(0., 1.);
                let sc = 1. - 0.7 * (t / 0.12).clamp(0., 1.);
                (op, sc)
            } else {
                let delay = 0.06 + i.min(30) as f32 * 0.012;
                let op = ((t - delay) / 0.20).clamp(0., 1.);
                let st = ((t - delay) / 0.34).clamp(0., 1.);
                let sc = 0.3 + 0.7 * super::scrolling::ease_spring_pop(st);
                (op, sc)
            }
        },
    )
}

fn chevron(progress: f32, p: crate::theme::Palette) -> Svg {
    svg()
        .path("chevron")
        .size(px(14.))
        .text_color(p.muted())
        .with_transformation(Transformation::rotate(radians(
            std::f32::consts::FRAC_PI_2 * progress,
        )))
}

fn format_latency(delay: Option<u64>) -> String {
    match delay {
        None => "未测".into(),
        Some(0) => "失败".into(),
        Some(d) => format!("{d}ms"),
    }
}

fn latency_badge_style(delay: Option<u64>, p: crate::theme::Palette) -> (Rgba, Rgba) {
    match delay {
        None => (p.fill(), p.muted()),
        Some(0) => (p.danger_soft(), p.danger()),
        Some(1..=299) => (p.success_soft(), p.success_ink()),
        Some(300..=799) => (p.warning_soft(), p.warning_ink()),
        Some(_) => (p.danger_soft(), p.danger()),
    }
}

fn latency_badge(
    spin_id: SharedString,
    delay: Option<u64>,
    testing: bool,
    palette: crate::theme::Palette,
    medium: bool,
) -> Div {
    let (bg, fg) = latency_badge_style(delay, palette);
    div()
        .relative()
        .flex_none()
        .px(px(7.))
        .py(px(1.))
        .rounded_full()
        .bg(bg)
        .text_xs()
        .when(medium, |el| el.font_weight(FontWeight::MEDIUM))
        .text_color(fg)
        .child(
            div()
                .when(testing, |el| el.opacity(0.))
                .child(format_latency(delay)),
        )
        .when(testing, |el| {
            el.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(super::settings::loading_spinner(spin_id, px(12.), fg)),
            )
        })
}

fn dot_color(delay: Option<u64>, p: crate::theme::Palette) -> Option<Rgba> {
    match delay {
        None => None,
        Some(0) => Some(p.danger_fill()),
        Some(1..=299) => Some(p.success_dot()),
        Some(300..=799) => Some(p.warning_dot()),
        Some(_) => Some(p.danger_fill()),
    }
}

fn format_type(kind: &str) -> String {
    kind.to_lowercase()
        .replace("shadowsocks", "ss")
        .replace("hysteria", "hy")
        .replace("wireguard", "wg")
}

fn node_type_desc(node: &singboard_stream::clash::Node, ipv6: bool) -> String {
    let mut parts = Vec::new();
    let t = format_type(&node.kind);
    if !t.is_empty() {
        parts.push(t);
    }
    if node.udp {
        parts.push(if node.xudp { "xudp" } else { "udp" }.into());
    }
    if ipv6 {
        parts.push("IPv6".into());
    }
    parts.join(" / ")
}

fn format_speed_compact(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B/s".into();
    }
    let v = bytes as f64;
    if v < 1024. {
        format!("{bytes} B/s")
    } else if v < 1048576. {
        format!("{:.1} KB/s", v / 1024.)
    } else if v < 1073741824. {
        format!("{:.1} MB/s", v / 1048576.)
    } else {
        format!("{:.1} GB/s", v / 1073741824.)
    }
}

fn relative_date(iso: &str) -> String {
    if iso.is_empty() {
        return String::new();
    }
    iso.split('T').next().unwrap_or(iso).to_owned()
}

fn format_expire(ts: i64) -> String {
    if ts <= 0 {
        return "无期限".into();
    }
    let secs = ts as u64;
    let days = secs / 86400;
    let mut y = 1970u64;
    let mut rem = days;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let dy = if leap { 366 } else { 365 };
        if rem < dy {
            break;
        }
        rem -= dy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let md = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 1u64;
    for &d in &md {
        if rem < d {
            break;
        }
        rem -= d;
        m += 1;
    }
    format!("{y:04}-{m:02}-{:02}", rem + 1)
}

impl Workspace {
    pub(super) fn freeze_proxy_order(&mut self, name: &str) {
        if !self.sort || self.proxy_ui.orders.contains_key(name) {
            return;
        }
        if let Some(g) = self.groups.iter().find(|g| g.name == name) {
            let mut nodes = g.nodes.iter().collect::<Vec<_>>();
            nodes.sort_by_key(|n| rank(n.delay));
            self.proxy_ui.orders.insert(
                name.into(),
                nodes
                    .into_iter()
                    .enumerate()
                    .map(|(i, n)| (n.name.clone(), i))
                    .collect(),
            );
        }
    }

    pub(super) fn release_proxy_order(&mut self) {
        let state = &mut self.proxy_ui;
        state.orders.retain(|name, _| {
            state.hovered.as_ref() == Some(name) || state.testing.as_ref() == Some(name)
        });
    }

    fn toggle_proxy(
        &mut self,
        provider: bool,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search = !provider && !self.proxy_ui.query.is_empty();
        let group = if provider {
            &mut self.expanded_providers
        } else if search {
            &mut self.proxy_ui.collapsed
        } else {
            &mut self.expanded
        };
        let inserted = if group.remove(&name) {
            false
        } else {
            group.insert(name.clone());
            true
        };
        let open = if search { !inserted } else { inserted };
        let (from, _, _, _) = self.proxy_ui.fraction(provider, &name, !open, 0, None);
        let now = Instant::now();
        self.proxy_ui
            .motion
            .retain(|_, (started, _, _)| now.duration_since(*started).as_secs_f32() < 1.2);
        self.proxy_ui
            .motion
            .insert((provider, name.clone()), (now, from, open));
        if !provider && !search {
            let mut saved: Vec<String> = self.expanded.iter().cloned().collect();
            saved.sort();
            singboard_stream::runtime().spawn(async move {
                if let Ok(dir) = crate::service::data_dir() {
                    let _ = crate::settings::Settings::edit(&dir, |p| {
                        p.expanded_proxy_groups = saved;
                        Ok(())
                    });
                }
            });
            self.proxy_ui.reveal = None;
            if open {
                let target_bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
                self.proxy_ui.reveal = Some((name.clone(), target_bounds.clone()));
                let popup = window.window_handle();
                cx.spawn(async move |weak, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(280))
                        .await;
                    let _ = popup.update(cx, |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if this.page == Page::Proxies
                                && !this.show_providers
                                && this.expanded.contains(&name)
                                && this
                                    .proxy_ui
                                    .reveal
                                    .as_ref()
                                    .is_some_and(|(g, b)| g == &name && Rc::ptr_eq(b, &target_bounds))
                            {
                                if let Some(bounds) = target_bounds.get() {
                                    this.scroll_bounds_nearest(bounds);
                                    cx.notify();
                                }
                                this.proxy_ui.reveal = None;
                            }
                        });
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }
    fn group_speed(&self, name: &str) -> (u64, u64) {
        let m = self.metrics();
        let entry = &m["chainSpeeds"][name];
        (
            entry[0].as_u64().unwrap_or(0),
            entry[1].as_u64().unwrap_or(0),
        )
    }

    pub(super) fn proxy_page(&self, window: &Window, cx: &Context<Self>) -> Div {
        let palette = crate::theme::Palette::get(cx);
        let translucent = true;
        let filter = self.filter.as_ref().expect("搜索框已初始化");
        let query = filter.read(cx).text.trim().to_lowercase();
        let width = f32::from(window.viewport_size().width) - 256.;
        let cols = (((width - 40. + 8.) / 188.).floor() as usize).max(1);

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
                    .id("proxy-tab-groups")
                    .relative()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .rounded_full()
                    .px_3()
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
                    .child(format!("代理组 ({})", self.groups.len()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.show_providers {
                            this.apply(Action::Providers, cx);
                        }
                    })),
            );
        if !self.providers.is_empty() || self.show_providers {
            tabs = tabs.child(
                div()
                    .id("proxy-tab-providers")
                    .relative()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .rounded_full()
                    .px_3()
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
                    .child(format!("代理提供商 ({})", self.providers.len()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.show_providers {
                            this.apply(Action::Providers, cx);
                        }
                    })),
            );
        }

        let header = div()
            .w_full()
            .px(px(20.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .gap_3()
                    .items_center()
                    .child(
                        crate::glass::backdrop_heading("代理", palette, cx)
                            .text_size(crate::theme::text_px(26.))
                            .line_height(rems(32. / 16.))
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(tabs)
                    .when(self.show_providers, |el| {
                        let disabled = self.busy || self.providers.is_empty();
                        let names: Vec<String> =
                            self.providers.iter().map(|p| p.0.clone()).collect();
                        el.child(
                            div()
                                .id("proxy-prov-update-all")
                                .role(Role::Button)
                                .aria_label("更新全部代理提供商")
                                .tooltip(|window, cx| {
                                    gpui_component::tooltip::Tooltip::new("更新全部代理提供商")
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
                                                    proxies: true,
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
            )
            .when(!self.show_providers, |el| {
                el.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(192.)).flex_none().child(filter.clone()))
                        .child(
                            div()
                                .id("sort-delay")
                                .relative()
                                .overflow_hidden()
                                .flex_none()
                                .rounded_full()
                                .size(px(32.))
                                .border_1()
                                .border_color(if self.sort {
                                    palette.selection_border().opacity(0.6)
                                } else {
                                    palette.control_rim()
                                })
                                .cursor_pointer()
                                .child(crate::glass::control_layer(palette, px(999.), !self.sort))
                                .child(
                                    div()
                                        .relative()
                                        .size_full()
                                        .rounded_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(self.sort, |v| v.bg(palette.primary_soft()))
                                        .when(!self.sort, |v| v.hover(|s| s.bg(palette.fill_strong())))
                                        .child(
                                            svg().path("sort").size(px(16.)).text_color(if self.sort {
                                                palette.accent()
                                            } else {
                                                palette.ink()
                                            }),
                                        ),
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.apply(Action::Sort, cx)),
                                ),
                        ),
                )
            });

        if self.show_providers {
            let mut list = div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(12.))
                .pt(px(2.))
                .pb_4();
            if self.providers.is_empty() {
                list = list.child(
                    div()
                        .py_10()
                        .flex()
                        .justify_center()
                        .child(
                            crate::glass::backdrop_text("暂无代理提供商", true, palette, cx)
                                .text_sm(),
                        ),
                );
            } else {
                for (pi, (name, item)) in self.providers.iter().enumerate() {
                    let open = self.expanded_providers.contains(name);
                    let proxies_arr = item["proxies"].as_array().cloned().unwrap_or_default();
                    let (fraction, grid_opacity, chev_prog, elapsed) =
                        self.proxy_ui.fraction(true, name, open, proxies_arr.len(), Some(window));
                    let tested = proxies_arr
                        .iter()
                        .filter(|n| {
                            n["history"]
                                .as_array()
                                .and_then(|h| h.last())
                                .and_then(|v| v["delay"].as_u64())
                                .is_some()
                        })
                        .count();
                    let updated = relative_date(item["updatedAt"].as_str().unwrap_or(""));
                    let sub = &item["subscriptionInfo"];
                    let total_bytes = sub["Total"].as_u64().unwrap_or(0);
                    let expire = sub["Expire"].as_i64();
                    let usage = if total_bytes > 0 {
                        let used = sub["Download"].as_u64().unwrap_or(0)
                            + sub["Upload"].as_u64().unwrap_or(0);
                        let pct = (used as f64 / total_bytes as f64) * 100.;
                        Some(format!(
                            "{} / {} ({pct:.2}%)",
                            fmt_bytes(used),
                            fmt_bytes(total_bytes)
                        ))
                    } else {
                        None
                    };
                    let toggle_name = name.clone();
                    let provider_hover_name = name.clone();
                    let provider_move_name = name.clone();
                    let hover_t = self.proxy_ui.hover_level(true, &name, window);
                    let vehicle = item["vehicleType"].as_str().unwrap_or("");
                    let mut card = glass_card(palette, translucent, self.glass_tint)
                        .id(("provider-card", pi))
                        .top(px(-1.5 * hover_t))
                        .shadow(crate::glass::card_hover_shadows(palette.0, hover_t))
                        .border_color(crate::glass::card_hover_rim(palette.0, hover_t))
                        .on_hover(cx.listener(move |this, hovered, _, cx| {
                            this.proxy_ui.set_hover(true, &provider_hover_name, *hovered);
                            cx.notify();
                        }))
                        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                            let prev = this.proxy_ui.pointer;
                            this.proxy_ui.pointer = Some(event.position);
                            if this
                                .proxy_ui
                                .hover_motion
                                .get(&(true, provider_move_name.clone()))
                                .is_some_and(|(_, _, active)| *active)
                                && prev.is_none_or(|p| {
                                    (f32::from(p.x - event.position.x)).abs() >= 1.5
                                        || (f32::from(p.y - event.position.y)).abs() >= 1.5
                                })
                            {
                                cx.notify();
                            }
                        }))
                        .child(crate::glass::hover_glow_layer(
                            self.proxy_ui.pointer,
                            hover_t,
                            palette.0,
                            px(17.),
                        ))
                        .w_full()
                        .overflow_hidden()
                        .flex()
                        .flex_col();
                    let mut head_left = div()
                        .id(("pp-toggle", pi))
                        .cursor_pointer()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.toggle_proxy(true, toggle_name.clone(), w, cx)
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .truncate()
                                        .child(crate::emoji::text(name.clone())),
                                )
                                .child(div().text_xs().text_color(palette.muted()).child(format!(
                                    "{} 个节点 ({tested} 已测试)",
                                    proxies_arr.len()
                                )))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(palette.muted())
                                        .child(chevron(chev_prog, palette)),
                                ),
                        );
                    if let Some(exp) = expire.filter(|_| total_bytes > 0) {
                        head_left = head_left.child(
                            div()
                                .text_xs()
                                .text_color(palette.muted())
                                .child(format!("到期: {}", format_expire(exp))),
                        );
                    }
                    if let Some(u) = usage {
                        head_left =
                            head_left.child(div().text_xs().text_color(palette.muted()).child(u));
                    }
                    if !updated.is_empty() {
                        head_left = head_left.child(
                            div()
                                .text_xs()
                                .text_color(palette.muted())
                                .child(format!("更新于 {updated}")),
                        );
                    }
                    if fraction < 1. && !proxies_arr.is_empty() {
                        let dots_per_row = (((width - 170.).max(80.) + 6.) / 20.).floor().max(1.) as usize;
                        let dot_rows = proxies_arr.len().div_ceil(dots_per_row);
                        let dots_full_h = dot_rows as f32 * 20. - 2.;
                        let mut dots = div().flex_none().w_full().flex().flex_wrap().gap(px(6.)).pt(px(4.));
                        for (ni, node) in proxies_arr.iter().enumerate() {
                            let delay = node["history"]
                                .as_array()
                                .and_then(|h| h.last())
                                .and_then(|v| v["delay"].as_u64());
                            let fill = dot_color(delay, palette);
                            let (dot_op, dot_scale) = dot_motion(elapsed, open, ni);
                            dots = dots.child(
                                div()
                                    .size(px(14.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .size(px(14. * dot_scale.max(0.2)))
                                            .opacity(dot_op)
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when_some(fill, |d, c| d.bg(c))
                                            .when(fill.is_none(), |d| {
                                                d.border_1().border_color(palette.line())
                                            })
                                            .when(delay == Some(0), |d| {
                                                d.child(
                                                    svg()
                                                        .path("x-small")
                                                        .size(px(8. * dot_scale.max(0.2)))
                                                        .text_color(rgb(0xffffff)),
                                                )
                                            }),
                                    ),
                            );
                        }
                        let dots_box = div()
                            .w_full()
                            .when(fraction > 0., |el| {
                                el.h(px(dots_full_h * (1. - fraction))).overflow_hidden()
                            })
                            .child(dots);
                        head_left = head_left.child(dots_box);
                    }
                    let mut actions = div().flex().items_center().gap_1().child(
                        div()
                            .id(("pp-hc", pi))
                            .cursor_pointer()
                            .px_2()
                            .py(px(4.))
                            .rounded_full()
                            .hover(|s| s.bg(palette.fill()))
                            .text_xs()
                            .text_color(palette.muted())
                            .child("健康检查")
                            .on_click(cx.listener({
                                let name = name.clone();
                                move |this, _, _, cx| {
                                    this.apply(
                                        Action::Run(Command::HealthCheck { name: name.clone() }),
                                        cx,
                                    );
                                }
                            })),
                    );
                    if !vehicle.eq_ignore_ascii_case("inline") {
                        actions = actions.child(
                            div()
                                .id(("pp-up", pi))
                                .cursor_pointer()
                                .px_2()
                                .py(px(4.))
                                .rounded_full()
                                .hover(|s| s.bg(palette.fill()))
                                .text_xs()
                                .text_color(palette.accent())
                                .font_weight(FontWeight::MEDIUM)
                                .child("更新")
                                .on_click(cx.listener({
                                    let name = name.clone();
                                    move |this, _, _, cx| {
                                        this.apply(
                                            Action::Run(Command::UpdateProxyProvider {
                                                name: name.clone(),
                                            }),
                                            cx,
                                        );
                                    }
                                })),
                        );
                    }
                    card = card.child(
                        div()
                            .px_5()
                            .py_4()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap_3()
                            .child(head_left)
                            .child(actions),
                    );
                    if fraction > 0. && !proxies_arr.is_empty() {
                        let total_rows = proxies_arr.len().div_ceil(cols);
                        let full_height = total_rows as f32 * 84. + 8.;
                        let animating = fraction < 0.999;
                        let clip_height = full_height * fraction;
                        let shown_rows = if animating {
                            ((clip_height / 84.).ceil() as usize + 1).min(total_rows)
                        } else {
                            total_rows
                        };
                        let mut grid = div()
                            .flex_none()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .px_5()
                            .pb_4();
                        for (row_idx, chunk) in proxies_arr.chunks(cols).take(shown_rows).enumerate() {
                            let mut row = div()
                                .w_full()
                                .h(px(76.))
                                .flex_none()
                                .grid()
                                .grid_cols(cols as u16)
                                .gap(px(8.));
                            for (col_idx, node) in chunk.iter().enumerate() {
                                let ni = row_idx * cols + col_idx;
                                let n_name = node["name"].as_str().unwrap_or("").to_owned();
                                let n_kind = format_type(node["type"].as_str().unwrap_or(""));
                                let delay = node["history"]
                                    .as_array()
                                    .and_then(|h| h.last())
                                    .and_then(|v| v["delay"].as_u64());
                                let testing = self.proxy_ui.testing_nodes.contains(&n_name);
                                let test_name = n_name.clone();
                                let url = self.latency_test_url.clone();
                                let badge = latency_badge(
                                    SharedString::from(format!("pp-delay-spin-{pi}-{ni}")),
                                    delay,
                                    testing,
                                    palette,
                                    false,
                                )
                                .id(SharedString::from(format!("pp-delay-{pi}-{ni}")))
                                .when(!testing, |el| {
                                    el.cursor_pointer().on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.run(
                                                Command::Delay {
                                                    name: test_name.clone(),
                                                    url: url.clone(),
                                                    ipv6: this.ipv6,
                                                },
                                                cx,
                                            );
                                        },
                                    ))
                                });
                                row = row.child(
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .h(px(76.))
                                        .p(px(10.))
                                        .rounded(px(12.))
                                        .bg(palette.tile())
                                        .border_1()
                                        .border_color(palette.control_rim())
                                        .flex()
                                        .flex_col()
                                        .justify_between()
                                        .overflow_hidden()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_h_0()
                                                .overflow_hidden()
                                                .text_sm()
                                                .line_height(rems(18. / 16.))
                                                .child(crate::emoji::text(n_name)),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(palette.muted())
                                                        .truncate()
                                                        .child(n_kind),
                                                )
                                                .child(badge),
                                        ),
                                );
                            }
                            grid = grid.child(row);
                        }
                        card = card.child(
                            div()
                                .w_full()
                                .overflow_hidden()
                                .opacity(grid_opacity)
                                .when(animating, |el| el.h(px(clip_height)))
                                .child(grid),
                        );
                    }
                    list = list.child(card);
                }
            }
            return div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(header)
                .child(self.scroll_pane("proxy-providers", list, cx));
        }

        let mut list = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .pt(px(2.))
            .pb_4();
        let mut matched = false;
        for (gi, g) in self.groups.iter().enumerate() {
            let whole = query.is_empty() || g.name.to_lowercase().contains(&query);
            let mut nodes = g
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| whole || n.name.to_lowercase().contains(&query))
                .collect::<Vec<_>>();
            if nodes.is_empty() && !whole {
                continue;
            }
            matched = true;
            if self.sort {
                if let Some(order) = self.proxy_ui.orders.get(&g.name) {
                    nodes.sort_by_key(|(_, n)| order.get(&n.name).copied().unwrap_or(usize::MAX));
                } else {
                    nodes.sort_by_key(|(_, n)| rank(n.delay));
                }
            }
            let open = if query.is_empty() {
                self.expanded.contains(&g.name)
            } else {
                !self.proxy_ui.collapsed.contains(&g.name)
            };
            let (fraction, grid_opacity, chev_prog, elapsed) =
                self.proxy_ui.fraction(false, &g.name, open, nodes.len(), Some(window));
            let selectable = g.kind.eq_ignore_ascii_case("selector");
            let id = g.name.clone();
            let now_node = g.nodes.iter().find(|n| n.name == g.now);
            let now_delay = now_node.and_then(|n| n.delay);
            let show_now_badge = !g.now.is_empty()
                && (now_delay.is_some_and(|d| d > 0)
                    || !now_node.is_some_and(|n| n.kind.eq_ignore_ascii_case("direct")));
            let (dl_spd, ul_spd) = self.group_speed(&g.name);
            let has_speed = dl_spd > 0 || ul_spd > 0;

            let hover_name = g.name.clone();
            let move_name = g.name.clone();
            let hover_t = self.proxy_ui.hover_level(false, &g.name, window);
            let mut card = glass_card(palette, translucent, self.glass_tint)
                .id(("proxy-card", gi))
                .top(px(-1.5 * hover_t))
                .shadow(crate::glass::card_hover_shadows(palette.0, hover_t))
                .border_color(crate::glass::card_hover_rim(palette.0, hover_t))
                .on_hover(cx.listener(move |this, hovered, _, cx| {
                    this.proxy_ui.set_hover(false, &hover_name, *hovered);
                    if *hovered {
                        this.proxy_ui.hovered = Some(hover_name.clone());
                        this.freeze_proxy_order(&hover_name);
                    } else if this.proxy_ui.hovered.as_ref() == Some(&hover_name) {
                        this.proxy_ui.hovered = None;
                    }
                    this.release_proxy_order();
                    cx.notify();
                }))
                .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                    let prev = this.proxy_ui.pointer;
                    this.proxy_ui.pointer = Some(event.position);
                    if this.proxy_ui.hovered.as_ref() == Some(&move_name)
                        && prev.is_none_or(|p| {
                            (f32::from(p.x - event.position.x)).abs() >= 1.5
                                || (f32::from(p.y - event.position.y)).abs() >= 1.5
                        })
                    {
                        cx.notify();
                    }
                }))
                .child(crate::glass::hover_glow_layer(
                    self.proxy_ui.pointer,
                    hover_t,
                    palette.0,
                    px(17.),
                ))
                .flex_none()
                .w_full()
                .overflow_hidden()
                .flex()
                .flex_col();

            let count_text = if !query.is_empty() {
                format!("{} ({}/{})", g.kind, nodes.len(), g.nodes.len())
            } else {
                format!("{} ({})", g.kind, g.nodes.len())
            };

            let mut current = div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_sm()
                .text_color(palette.muted())
                .child(
                    svg()
                        .path("arrow-right")
                        .size(px(14.))
                        .text_color(palette.muted()),
                )
                .child(div().truncate().child(crate::emoji::text(g.now.clone())));
            if show_now_badge {
                current = current.child(latency_badge(
                    SharedString::from(format!("now-delay-spin-{gi}")),
                    now_delay,
                    self.proxy_ui.testing_nodes.contains(&g.now),
                    palette,
                    false,
                ));
            }
            if let Some((group, previous, _)) =
                self.undo.as_ref().filter(|(group, _, _)| group == &g.name)
            {
                let grp = group.clone();
                let prev = previous.clone();
                current = current.child(
                    div()
                        .id(("undo-node", gi))
                        .cursor_pointer()
                        .px_2()
                        .py(px(1.))
                        .rounded_full()
                        .hover(|s| s.bg(palette.fill()))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.accent())
                        .child("撤销")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.apply(
                                Action::Run(Command::Select {
                                    group: grp.clone(),
                                    name: prev.clone(),
                                }),
                                cx,
                            );
                        })),
                );
            }

            let mut header_left = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .truncate()
                                        .child(crate::emoji::text(g.name.clone())),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(palette.muted())
                                        .child(format!(
                                            "{count_text}{}",
                                            if selectable { "" } else { " · 自动" }
                                        )),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(palette.muted())
                                        .child(chevron(chev_prog, palette)),
                                ),
                        )
                        .child(current),
                );

            if fraction < 1. && !nodes.is_empty() {
                let dots_per_row = (((width - 140.).max(80.) + 6.) / 20.).floor().max(1.) as usize;
                let dot_rows = nodes.len().div_ceil(dots_per_row);
                let dots_full_h = dot_rows as f32 * 20. + 2.;
                let mut dots = div().flex_none().w_full().flex().gap(px(6.)).flex_wrap().pt(px(8.));
                for (pos, (ni, node)) in nodes.iter().enumerate() {
                    let (dot_op, dot_scale) = dot_motion(elapsed, open, pos);
                    let selected = node.name == g.now;
                    let fill = dot_color(node.delay, palette);
                    let failed = node.delay == Some(0);
                    let mut slot = div()
                        .id(SharedString::from(format!("dot-{gi}-{ni}")))
                        .size(px(14.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(14. * dot_scale.max(0.2)))
                                .opacity(dot_op)
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when_some(fill, |v, c| v.bg(c))
                                .when(fill.is_none(), |v| {
                                    v.border_1().border_color(palette.line())
                                })
                                .when(selected && failed, |v| {
                                    v.border_2().border_color(palette.ink())
                                })
                                .when(selected && !failed, |v| {
                                    v.child(div().size(px(6. * dot_scale.max(0.2))).rounded_full().bg(if fill.is_some() {
                                        rgb(0xffffff)
                                    } else {
                                        palette.muted()
                                    }))
                                })
                                .when(failed, |v| {
                                    v.child(svg().path("x-small").size(px(8. * dot_scale.max(0.2))).text_color(rgb(0xffffff)))
                                }),
                        );
                    if selectable && !selected {
                        let group = g.name.clone();
                        let name = node.name.clone();
                        slot = slot
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.run(
                                    Command::Select {
                                        group: group.clone(),
                                        name: name.clone(),
                                    },
                                    cx,
                                )
                            }));
                    }
                    dots = dots.child(slot);
                }
                let dots_box = div()
                    .w_full()
                    .when(fraction > 0., |el| {
                        el.h(px(dots_full_h * (1. - fraction))).overflow_hidden()
                    })
                    .child(dots);
                header_left = header_left.child(dots_box);
            }

            let group_test = g.name.clone();
            let group_url = g.test_url.clone();
            let header_right = div()
                .flex_none()
                .flex()
                .flex_col()
                .items_end()
                .gap(px(2.))
                .child(
                    div()
                        .id(("group-test", gi))
                        .cursor_pointer()
                        .px_2()
                        .py(px(3.))
                        .rounded(px(8.))
                        .hover(|s| s.bg(palette.fill()))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.ink())
                        .child(if self.proxy_ui.testing.as_ref() == Some(&g.name) {
                            "测试中…"
                        } else {
                            "延迟测试"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.run(
                                Command::GroupDelay {
                                    name: group_test.clone(),
                                    url: group_url.clone(),
                                    ipv6: this.ipv6,
                                },
                                cx,
                            )
                        })),
                )
                .child(
                    div()
                        .min_w(px(80.))
                        .flex()
                        .flex_col()
                        .items_end()
                        .pr_1()
                        .text_xs()
                        .line_height(px(16.))
                        .whitespace_nowrap()
                        .text_color(palette.muted())
                        .when(!has_speed, |el| el.opacity(0.))
                        .child(format!("↓{}", format_speed_compact(dl_spd)))
                        .child(format!("↑{}", format_speed_compact(ul_spd))),
                );

            card = card.child(
                div()
                    .id(("expand", gi))
                    .cursor_pointer()
                    .px_5()
                    .py_4()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_3()
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.toggle_proxy(false, id.clone(), w, cx)
                    }))
                    .child(header_left)
                    .child(header_right),
            );

            if fraction > 0. && !nodes.is_empty() {
                let total_rows = nodes.len().div_ceil(cols);
                let full_height = total_rows as f32 * 84. + 12.;
                let animating = fraction < 0.999;
                let clip_height = full_height * fraction;
                let shown_rows = if animating {
                    ((clip_height / 84.).ceil() as usize + 1).min(total_rows)
                } else {
                    total_rows
                };
                let mut grid = div()
                    .flex_none()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .px_5()
                    .pb_4()
                    .pt_1();
                for chunk in nodes.chunks(cols).take(shown_rows) {
                    let mut row = div()
                        .w_full()
                        .h(px(76.))
                        .flex_none()
                        .grid()
                        .grid_cols(cols as u16)
                        .gap(px(8.));
                    for &(ni, node) in chunk {
                        let selected = node.name == g.now;
                        let name = node.name.clone();
                        let group = g.name.clone();
                        let is_v6 = self.ipv6 && self.ipv6_nodes.contains(&name);
                        let type_desc = node_type_desc(node, is_v6);
                        let show_lat = node.delay.is_some_and(|d| d > 0)
                            || !node.kind.eq_ignore_ascii_case("direct");
                        let reveal_cell = if selected {
                            self.proxy_ui
                                .reveal
                                .as_ref()
                                .filter(|(g_name, _)| g_name == &g.name)
                                .map(|(_, cell)| cell.clone())
                        } else {
                            None
                        };
                        let mut tile = div()
                            .id(SharedString::from(format!("node-{gi}-{ni}")))
                            .relative()
                            .when_some(reveal_cell, |el, cell| {
                                el.child(
                                    canvas(
                                        |_, _, _| (),
                                        move |bounds, _, _, _| {
                                            cell.set(Some(bounds));
                                        },
                                    )
                                    .absolute()
                                    .inset_0()
                                    .size_full(),
                                )
                            })
                            .w_full()
                            .min_w_0()
                            .h(px(76.))
                            .p(px(10.))
                            .flex()
                            .flex_col()
                            .justify_between()
                            .gap_1()
                            .rounded(px(12.))
                            .overflow_hidden()
                            .border_1()
                            .bg(if selected {
                                palette.primary_soft()
                            } else {
                                palette.tile()
                            })
                            .border_color(if selected {
                                palette.selection_border()
                            } else {
                                palette.control_rim()
                            })
                            .when(!selected && selectable, |el| {
                                el.hover(|s| s.bg(palette.fill_strong()))
                            })
                            .text_color(if selected {
                                palette.accent()
                            } else {
                                palette.ink()
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .text_sm()
                                    .line_height(rems(18. / 16.))
                                    .font_weight(if selected {
                                        FontWeight::MEDIUM
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .child(crate::emoji::text(name.clone())),
                            );
                        if selectable && !selected {
                            tile = tile
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.run(
                                        Command::Select {
                                            group: group.clone(),
                                            name: name.clone(),
                                        },
                                        cx,
                                    )
                                }));
                        }
                        let mut bottom = div()
                            .h(px(18.))
                            .w_full()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(if selected {
                                        palette.accent()
                                    } else {
                                        palette.muted()
                                    })
                                    .truncate()
                                    .child(type_desc),
                            );
                        if show_lat {
                            let test_name = node.name.clone();
                            let url = g.test_url.clone();
                            let testing = self.proxy_ui.testing_nodes.contains(&node.name);
                            bottom = bottom.child(
                                latency_badge(
                                    SharedString::from(format!("delay-spin-{gi}-{ni}")),
                                    node.delay,
                                    testing,
                                    palette,
                                    true,
                                )
                                .id(SharedString::from(format!("delay-{gi}-{ni}")))
                                .when(!testing, |el| {
                                    el.cursor_pointer().on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.run(
                                                Command::Delay {
                                                    name: test_name.clone(),
                                                    url: url.clone(),
                                                    ipv6: this.ipv6,
                                                },
                                                cx,
                                            );
                                        },
                                    ))
                                }),
                            );
                        }
                        row = row.child(tile.child(bottom));
                    }
                    grid = grid.child(row);
                }
                card = card.child(
                    div()
                        .w_full()
                        .overflow_hidden()
                        .opacity(grid_opacity)
                        .when(animating, |el| el.h(px(clip_height)))
                        .child(grid),
                );
            }
            list = list.child(card);
        }
        if !matched && self.load_error.is_empty() {
            list = list.child(
                div()
                    .py_12()
                    .flex()
                    .justify_center()
                    .child(
                        crate::glass::backdrop_text(
                            if self.busy {
                                "正在读取代理组…"
                            } else if query.is_empty() {
                                "当前配置里没有代理组"
                            } else {
                                "没有找到匹配的节点或代理组"
                            },
                            true,
                            palette,
                            cx,
                        )
                        .text_sm(),
                    ),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(header)
            .when(!self.load_error.is_empty(), |v| {
                v.child(
                    div()
                        .px(px(20.))
                        .flex_none()
                        .flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .text_color(crate::glass::backdrop_muted(palette, cx))
                        .child(self.load_error.clone())
                        .child(self.button("retry", "刷新", Action::Refresh, cx)),
                )
            })
            .child(self.scroll_pane("proxy-groups", list, cx))
    }
}
