use super::{Overview, visibility, workspace::PAGES};
use gpui::{prelude::*, *};
use std::borrow::Cow;

pub struct Assets;
const ICONS: &[(&str, &[u8])] = &[
    ("profile-active", include_bytes!("../../android/assets/icons/profile-active.svg")),
    ("select-caret", include_bytes!("../../android/assets/icons/select-caret.svg")),
    ("glass-clear", include_bytes!("../../android/assets/icons/glass-clear.svg")),
    ("glass-tinted", include_bytes!("../../android/assets/icons/glass-tinted.svg")),
    ("overview", include_bytes!("../assets/overview.svg")),
    ("proxies", include_bytes!("../assets/proxies.svg")),
    ("connections", include_bytes!("../assets/connections.svg")),
    ("logs", include_bytes!("../assets/logs.svg")),
    ("rules", include_bytes!("../assets/rules.svg")),
    ("config", include_bytes!("../assets/config.svg")),
    ("settings", include_bytes!("../assets/settings.svg")),
    ("topology-pause", include_bytes!("../assets/topology-pause.svg")),
    ("topology-play", include_bytes!("../assets/topology-play.svg")),
    ("pause", include_bytes!("../assets/pause.svg")),
    ("play", include_bytes!("../assets/play.svg")),
    ("arrow-down", include_bytes!("../assets/arrow-down.svg")),
    ("toast-info", include_bytes!("../../android/assets/icons/toast-info.svg")),
    ("toast-error", include_bytes!("../../android/assets/icons/toast-error.svg")),
    ("x-small", include_bytes!("../assets/x-small.svg")),
    ("power", include_bytes!("../assets/power.svg")),
    ("search", include_bytes!("../assets/search.svg")),
    ("sort", include_bytes!("../assets/sort.svg")),
    ("minimize", include_bytes!("../assets/minimize.svg")),
    ("maximize", include_bytes!("../assets/maximize.svg")),
    ("close", include_bytes!("../assets/close.svg")),
    ("refresh", include_bytes!("../../android/assets/icons/refresh.svg")),
    ("chevron", include_bytes!("../../android/assets/icons/chevron.svg")),
    ("arrow-right", include_bytes!("../../android/assets/icons/arrow-right.svg")),
    ("dot-failed", include_bytes!("../../android/assets/icons/dot-failed.svg")),
    ("bolt", include_bytes!("../../android/assets/icons/bolt.svg")),
    ("layers", include_bytes!("../../android/assets/icons/layers.svg")),
    ("reject", include_bytes!("../../android/assets/icons/reject.svg")),
    ("hijack", include_bytes!("../../android/assets/icons/hijack.svg")),
    ("sniff", include_bytes!("../../android/assets/icons/sniff.svg")),
    ("resolve", include_bytes!("../../android/assets/icons/resolve.svg")),
    ("tune", include_bytes!("../../android/assets/icons/tune.svg")),
    ("remote", include_bytes!("../../android/assets/icons/remote.svg")),
    ("inline", include_bytes!("../../android/assets/icons/inline.svg")),
    ("file", include_bytes!("../../android/assets/icons/file.svg")),
    ("check", include_bytes!("../../android/assets/icons/check.svg")),
    ("close-detail", include_bytes!("../../android/assets/icons/close-detail.svg")),
    ("chevron-down", include_bytes!("../../android/assets/icons/chevron-down.svg")),
    ("edit", include_bytes!("../../android/assets/icons/edit.svg")),
    ("plus", include_bytes!("../../android/assets/icons/plus.svg")),
    ("check-settings", include_bytes!("../../android/assets/icons/check-settings.svg")),
    ("cancel", include_bytes!("../../android/assets/icons/cancel.svg")),
    ("trash", include_bytes!("../../android/assets/icons/trash.svg")),
    ("profile-remote", include_bytes!("../../android/assets/icons/profile-remote.svg")),
    ("folder", include_bytes!("../../android/assets/icons/folder.svg")),
    ("hero-next", include_bytes!("../../android/assets/icons/hero-next.svg")),
    ("hero-restart", include_bytes!("../../android/assets/icons/hero-restart.svg")),
    ("hero-power", include_bytes!("../../android/assets/icons/hero-power.svg")),
    ("download", include_bytes!("../../android/assets/icons/download.svg")),
    ("upload", include_bytes!("../../android/assets/icons/upload.svg")),
    ("rotate-left", include_bytes!("../../android/assets/icons/rotate-left.svg")),
    ("rotate-right", include_bytes!("../../android/assets/icons/rotate-right.svg")),
    ("flip-x", include_bytes!("../../android/assets/icons/flip-x.svg")),
    ("flip-y", include_bytes!("../../android/assets/icons/flip-y.svg")),
    ("back", include_bytes!("../../android/assets/icons/back.svg")),
    ("tray-power", include_bytes!("../../android/assets/icons/tray-power.svg")),
    ("tray-restart", include_bytes!("../../android/assets/icons/tray-restart.svg")),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, data)) = ICONS.iter().find(|(key, _)| *key == path) {
            return Ok(Some(Cow::Borrowed(*data)));
        }
        gpui_kit_assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(
            ICONS
                .iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
        Ok(paths)
    }
}

impl Overview {
    pub fn navigation(&mut self, window: &mut Window, cx: &Context<Self>) -> Div {
        let palette = crate::theme::Palette::get(cx);
        let ws = self.workspace.read(cx);
        let active = ws.page;
        let tint = ws.glass_tint;
        let target = if ws.header_scrolled() { 1. } else { 0. };
        let dt = self.title_tick.elapsed().as_secs_f32().min(0.033);
        self.title_tick = std::time::Instant::now();
        self.title_opacity += (target - self.title_opacity).clamp(-5. * dt, 5. * dt);
        if (target - self.title_opacity).abs() > 0.001 { window.request_animation_frame(); }
        let opacity = self.title_opacity;
        let page_title = PAGES.iter().find(|(page, _)| *page == active).map(|(_, label)| *label).unwrap_or("");
        let connected = ws.overview_live();
        let local_running = ws.local_state.contains("运行中");
        let local_stopped = ws.local_state.contains("已停止");
        let uptime = ws.uptime();
        let status_label = if local_running {
            uptime.as_deref().unwrap_or("运行中")
        } else if connected {
            "已连接"
        } else if local_stopped {
            "已停止"
        } else {
            ws.traffic_status
        };
        let (dot_color, halo_color) = if local_running || connected {
            (palette.success_dot(), palette.success_soft())
        } else if local_stopped {
            (palette.danger_fill(), palette.danger_soft())
        } else {
            (palette.warning_dot(), palette.warning_soft())
        };
        let mut sidebar = div()
            .absolute()
            .top(px(8.))
            .bottom(px(8.))
            .left(px(8.))
            .w(px(208.))
            .rounded(px(22.))
            .overflow_hidden()
            .bg(palette.floating(tint))
            .border_1()
            .border_color(palette.rim())
            .shadow(crate::glass::float_shadows(palette.0))
            .flex()
            .flex_col()
            .text_color(palette.ink())
            .child(crate::glass::card_layer(palette.floating(tint), px(21.), 0.55))
            .child(crate::glass::grain_layer(px(21.)))
            .child(
                div()
                    .h(px(48.))
                    .pl_4()
                    .gap(px(10.))
                    .flex()
                    .items_center()
                    .window_control_area(WindowControlArea::Drag)
                    .child(img(self.brand.clone()).size(px(24.)).rounded_md())
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("singboard"),
                    ),
            );
        let mut nav = div().px_2().py_1().flex().flex_col().gap(px(2.)).flex_1();
        for (i, (page, label)) in PAGES.into_iter().enumerate() {
            let selected = page == active;
            nav = nav.child(
                div()
                    .id(("nav", i))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px_3()
                    .h(px(35.))
                    .rounded(px(12.))
                    .text_size(px(13.))
                    .cursor_pointer()
                    .when(selected, |item| {
                        item.bg(palette.primary_soft())
                            .border_1()
                            .border_color(palette.selection_border().opacity(0.45))
                            .text_color(palette.accent())
                            .font_weight(FontWeight::SEMIBOLD)
                    })
                    .when(!selected, |item| item.hover(|s| s.bg(palette.hover())))
                    .child(
                        svg()
                            .path(match page {
                                super::workspace::Page::Overview => "overview",
                                super::workspace::Page::Proxies => "proxies",
                                super::workspace::Page::Connections => "connections",
                                super::workspace::Page::Logs => "logs",
                                super::workspace::Page::Rules => "rules",
                                super::workspace::Page::Config => "config",
                                super::workspace::Page::Settings => "settings",
                            })
                            .size(px(18.))
                            .text_color(if selected {
                                palette.accent()
                            } else {
                                palette.muted()
                            }),
                    )
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.workspace.update(cx, |state, cx| state.open(page, cx));
                        cx.notify();
                    })),
            );
        }
        sidebar = sidebar.child(nav).child(
            div().p_3().flex().child(
                div()
                    .id("sidebar-status-pill")
                    .relative()
                    .overflow_hidden()
                    .rounded_full()
                    .bg(palette.fill())
                    .border_1()
                    .border_color(palette.rim())
                    .shadow_sm()
                    .cursor_pointer()
                    .child(crate::glass::control_layer(palette, px(999.), false))
                    .child(
                        div()
                            .relative()
                            .flex()
                            .gap_2()
                            .items_center()
                            .px_3()
                            .py(px(5.))
                            .rounded_full()
                            .hover(|s| s.bg(palette.fill()))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .child(
                                div()
                                    .size(px(14.))
                                    .rounded_full()
                                    .bg(halo_color)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(div().size(px(8.)).rounded_full().bg(dot_color)),
                            )
                            .child(div().text_color(palette.ink()).child(status_label.to_owned())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let action = if local_running { "stop" } else { "start" };
                        this.workspace.update(cx, |state, cx| {
                            if local_running || local_stopped {
                                state.tray_service(action, cx);
                            } else {
                                state.open(super::workspace::Page::Settings, cx);
                            }
                        });
                    })),
            ),
        );
        let mut bar = div()
            .absolute()
            .left(px(216.))
            .right_0()
            .top_0()
            .h(px(48.))
            .pl(px(20.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .mt(px(4. * (1. - opacity)))
                            .opacity(opacity)
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                            .child(page_title),
                    ),
            );
        for (i, glyph) in ["minimize", "maximize", "close"].into_iter().enumerate() {
            bar = bar.child(
                div()
                    .id(("caption", i))
                    .relative()
                    .size(px(28.))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(palette.fill())
                    .border_1()
                    .border_color(palette.rim())
                    .cursor_pointer()
                    .child(crate::glass::control_layer(palette, px(13.), false))
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .group(SharedString::from(format!("caption-btn-{i}")))
                            .hover(move |style| {
                                if i == 2 {
                                    style.bg(rgba(0xf45d56ff))
                                } else {
                                    style.bg(palette.fill_strong())
                                }
                            })
                            .child(
                                svg()
                                    .path(glyph)
                                    .size(px(12.))
                                    .text_color(palette.muted())
                                    .group_hover(
                                        SharedString::from(format!("caption-btn-{i}")),
                                        move |s| {
                                            s.text_color(if i == 2 {
                                                rgb(0xffffff)
                                            } else {
                                                palette.ink()
                                            })
                                        },
                                    ),
                            ),
                    )
                    .on_click(move |_, win, cx| match i {
                        0 => win.minimize_window(),
                        1 if win.is_maximized() => visibility(win, true),
                        1 => win.zoom_window(),
                        _ => super::close_window(win, cx),
                    }),
            );
        }
        div()
            .absolute()
            .inset_0()
            .size_full()
            .when(opacity > 0.01, |root| {
                root.child(
                    div()
                        .absolute()
                        .left(px(216.))
                        .right_0()
                        .top_0()
                        .h(px(66.))
                        .child(crate::glass::bar_layer(palette, tint, opacity)),
                )
            })
            .child(sidebar)
            .child(bar)
    }
}
